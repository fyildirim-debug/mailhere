//! SMTP gönderimi (lettre, rustls).

use std::time::Duration;

use lettre::transport::smtp::authentication::{Credentials, DEFAULT_MECHANISMS};
use lettre::Message;
use mh_core::{AppError, AppResult, ErrorCode, ServerConfig};

pub(crate) fn map_err(e: &lettre::transport::smtp::Error, cfg: &ServerConfig) -> AppError {
    let code = e.status().map(|c| c.to_string()).unwrap_or_default();
    if matches!(code.as_str(), "530" | "534" | "535" | "454") {
        return AppError::new(
            ErrorCode::Auth,
            "Giden posta sunucusu kullanıcı adını ya da parolayı kabul etmedi.",
        );
    }
    if e.is_tls() {
        return AppError::new(
            ErrorCode::Tls,
            "Giden posta sunucusuyla güvenli bağlantı kurulamadı. Sunucu adını ve güvenlik ayarını denetle.",
        );
    }
    if e.is_permanent() {
        return AppError::new(
            ErrorCode::Invalid,
            format!("Sunucu postayı reddetti ({code})."),
        );
    }
    AppError::new(
        ErrorCode::Network,
        format!(
            "Giden posta sunucusuna ({}:{}) ulaşılamadı.",
            cfg.host, cfg.port
        ),
    )
}

/// Postayı gönderir. Şifresiz bağlantıya yalnız yerel test sunucusunda izin
/// verilir; sertifika kuralı `crate::smtp::open` içindedir.
pub async fn send(cfg: &ServerConfig, password: &str, message: Message) -> AppResult<()> {
    let host = cfg.host.trim();
    let mut conn = crate::smtp::open(cfg).await?;
    let raw = message.formatted();
    let work = async {
        if !cfg.username.trim().is_empty() {
            let credentials = Credentials::new(cfg.username.trim().to_owned(), password.to_owned());
            conn.auth(DEFAULT_MECHANISMS, &credentials).await?;
        }
        conn.send(message.envelope(), &raw).await
    };
    let res = tokio::time::timeout(crate::smtp::send_timeout(raw.len()), work).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.quit()).await;
    match res {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => {
            tracing::warn!(host, error = %e, "SMTP gönderimi başarısız");
            Err(map_err(&e, cfg))
        }
        Err(_) => Err(AppError::new(
            ErrorCode::Network,
            "Giden posta sunucusu zamanında yanıt vermedi; posta yeniden denenecek.",
        )),
    }
}

/// Gönderilen postayı kendisi "Gönderilmiş" kutusuna saklayan sunucular
/// (Gmail, Outlook / Office 365): IMAP'e ayrıca eklenmez, yoksa iki kopya olur.
pub fn server_saves_sent(cfg: &ServerConfig) -> bool {
    let h = cfg.host.trim().trim_end_matches('.').to_ascii_lowercase();
    const SAVES: &[&str] = &[
        "smtp.office365.com",
        "smtp-mail.outlook.com",
        "outlook.office365.com",
    ];
    h.ends_with("gmail.com") || h.ends_with("googlemail.com") || SAVES.contains(&h.as_str())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mh_core::{Account, AccountKind, Address, OutgoingMessage, Security, TrustedCert};
    use std::sync::Arc;
    use tokio::io::{
        AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
    };
    use tokio::net::{TcpListener, TcpStream};

    /// Sahte sunucunun gördükleri: komutlar ve DATA içeriği.
    #[derive(Default, Debug)]
    pub(crate) struct Seen {
        pub commands: Vec<String>,
        pub data: String,
    }

    /// Yalnız temel komutları bilen sahte SMTP oturumu. `greet` yanlışsa
    /// karşılama gönderilmez (STARTTLS sonrası).
    pub(crate) async fn fake_session<S: AsyncRead + AsyncWrite + Unpin>(s: S, greet: bool) -> Seen {
        let (r, mut w) = tokio::io::split(s);
        let mut lines = BufReader::new(r).lines();
        if greet {
            w.write_all(b"220 sahte ESMTP\r\n").await.unwrap();
            w.flush().await.unwrap();
        }
        let mut seen = Seen::default();
        let mut in_data = false;
        while let Ok(Some(line)) = lines.next_line().await {
            if in_data {
                if line == "." {
                    in_data = false;
                    w.write_all(b"250 kabul\r\n").await.unwrap();
                    w.flush().await.unwrap();
                } else {
                    seen.data.push_str(&line);
                    seen.data.push('\n');
                }
                continue;
            }
            let cmd = line.to_ascii_uppercase();
            seen.commands.push(cmd.clone());
            let reply: &[u8] = if cmd.starts_with("EHLO") {
                b"250-sahte\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n"
            } else if cmd.starts_with("AUTH") {
                b"235 tamam\r\n"
            } else if cmd.starts_with("DATA") {
                in_data = true;
                b"354 devam\r\n"
            } else if cmd.starts_with("QUIT") {
                let _ = w.write_all(b"221 hosca kal\r\n").await;
                let _ = w.flush().await;
                break;
            } else {
                b"250 tamam\r\n"
            };
            w.write_all(reply).await.unwrap();
            w.flush().await.unwrap();
        }
        seen
    }

    async fn fake_smtp(listener: TcpListener) -> Seen {
        let (sock, _) = listener.accept().await.unwrap();
        fake_session(sock, true).await
    }

    pub(crate) struct TestCert {
        pub der: rustls::pki_types::CertificateDer<'static>,
        pub acceptor: tokio_rustls::TlsAcceptor,
    }

    pub(crate) fn self_signed() -> TestCert {
        let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "localhost");
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
        )
        .unwrap();
        TestCert {
            der: cert.der().clone(),
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(cfg)),
        }
    }

    fn message() -> Message {
        let account = Account {
            id: "a".into(),
            name: "İş".into(),
            email: "furkan@ornek.com.tr".into(),
            display_name: "Furkan Yıldırım".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: None,
            smtp: None,
            jmap_url: None,
            signature: None,
            created_at: 0,
            auth: Default::default(),
        };
        let msg = OutgoingMessage {
            account_id: "a".into(),
            to: vec![Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com")],
            cc: vec![],
            bcc: vec![],
            subject: "Cumartesi".into(),
            body_text: "Geliyorum!".into(),
            in_reply_to: None,
            attachments: vec![],
            send_at: None,
            forward_from: None,
            draft_id: None,
        };
        crate::build(&account, &msg, None, 0).unwrap()
    }

    #[tokio::test]
    async fn yerel_sunucuya_gonderim() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(fake_smtp(listener));
        let cfg = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::None,
            username: String::new(),
        };
        send(&cfg, "", message()).await.unwrap();
        let seen = server.await.unwrap();
        assert!(seen.data.contains("Subject: Cumartesi"), "{seen:?}");
        assert!(
            seen.data.contains("To: =?utf-8?b?") || seen.data.contains("To: \"Ay"),
            "{seen:?}"
        );
        assert!(!seen.commands.iter().any(|c| c.starts_with("AUTH")));
    }

    #[tokio::test]
    async fn tls_sertifikasi_onaylanmadan_gonderilmez() {
        let cert = self_signed();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = cert.acceptor.clone();
        let server = tokio::spawn(async move {
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                // İlk bağlantı onaysız: el sıkışma istemcide reddedilir.
                if let Ok(tls) = acceptor.accept(sock).await {
                    return fake_session(tls, true).await;
                }
            }
        });
        let cfg = ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::Tls,
            username: "furkan".into(),
        };
        let err = send(&cfg, "gizli-parola", message()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Untrusted, "{err:?}");
        assert!(err.message.contains(&format!("localhost:{port}")));
        assert!(!err.message.contains("gizli-parola"));

        mh_sync::tls::trust(TrustedCert {
            host: "localhost".into(),
            port,
            sha256: mh_sync::tls::fingerprint(&cert.der),
        });
        send(&cfg, "gizli-parola", message()).await.unwrap();
        let seen = server.await.unwrap();
        assert!(seen.commands.iter().any(|c| c.starts_with("AUTH PLAIN")));
        assert!(seen.data.contains("Subject: Cumartesi"));
    }

    /// Düz metin satırı (arabelleğe fazladan bayt almadan).
    async fn plain_line(s: &mut TcpStream) -> String {
        let mut out = Vec::new();
        loop {
            let b = s.read_u8().await.unwrap();
            if b == b'\n' {
                return String::from_utf8_lossy(&out).trim_end().to_owned();
            }
            out.push(b);
        }
    }

    #[tokio::test]
    async fn starttls_ile_gonderim() {
        let cert = self_signed();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = cert.acceptor.clone();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            sock.write_all(b"220 sahte ESMTP\r\n").await.unwrap();
            assert!(plain_line(&mut sock).await.starts_with("EHLO "));
            sock.write_all(b"250-sahte\r\n250 STARTTLS\r\n")
                .await
                .unwrap();
            assert_eq!(plain_line(&mut sock).await, "STARTTLS");
            sock.write_all(b"220 basla\r\n").await.unwrap();
            let tls = acceptor.accept(sock).await.unwrap();
            fake_session(tls, false).await
        });
        mh_sync::tls::trust(TrustedCert {
            host: "localhost".into(),
            port,
            sha256: mh_sync::tls::fingerprint(&cert.der),
        });
        let cfg = ServerConfig {
            host: "localhost".into(),
            port,
            security: Security::Starttls,
            username: "furkan".into(),
        };
        send(&cfg, "parola", message()).await.unwrap();
        let seen = server.await.unwrap();
        assert!(seen.commands[0].starts_with("EHLO"), "TLS sonrası yeniden EHLO");
        assert!(seen.commands.iter().any(|c| c.starts_with("AUTH PLAIN")));
        assert!(seen.data.contains("Subject: Cumartesi"));
    }

    #[tokio::test]
    async fn starttls_yoksa_duz_metne_dusulmez() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            sock.write_all(b"220 sahte ESMTP\r\n").await.unwrap();
            plain_line(&mut sock).await;
            sock.write_all(b"250-sahte\r\n250 AUTH PLAIN\r\n")
                .await
                .unwrap();
            // İstemci kimlik bilgisi göndermeden kapatmalı.
            let mut rest = Vec::new();
            let _ = sock.read_to_end(&mut rest).await;
            String::from_utf8_lossy(&rest).into_owned()
        });
        let cfg = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::Starttls,
            username: "furkan".into(),
        };
        let err = send(&cfg, "gizli-parola", message()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Tls);
        let rest = server.await.unwrap();
        assert!(!rest.contains("AUTH"), "{rest}");
    }

    #[tokio::test]
    async fn sifresiz_uzak_gonderim_reddedilir() {
        let cfg = ServerConfig {
            host: "smtp.ornek.com".into(),
            port: 25,
            security: Security::None,
            username: "u".into(),
        };
        let message = lettre::Message::builder()
            .from("a@b.co".parse().unwrap())
            .to("c@d.co".parse().unwrap())
            .body(String::from("x"))
            .unwrap();
        assert_eq!(
            send(&cfg, "p", message).await.unwrap_err().code,
            ErrorCode::Tls
        );
    }

    #[test]
    fn sunucu_gonderileni_kendisi_saklar() {
        let mut cfg = ServerConfig {
            host: "smtp.gmail.com".into(),
            port: 465,
            security: Security::Tls,
            username: "u".into(),
        };
        assert!(server_saves_sent(&cfg));
        for h in [
            "smtp.office365.com",
            "SMTP-MAIL.OUTLOOK.COM",
            "outlook.office365.com.",
        ] {
            cfg.host = h.into();
            assert!(server_saves_sent(&cfg), "{h}");
        }
        for h in [
            "smtp.ornek.com",
            "smtp.outlook.com.tr",
            "office365.com.ornek.net",
        ] {
            cfg.host = h.into();
            assert!(!server_saves_sent(&cfg), "{h}");
        }
    }
}
