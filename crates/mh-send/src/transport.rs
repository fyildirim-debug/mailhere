//! SMTP gönderimi (lettre, rustls).

use std::time::Duration;

use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use mh_core::{AppError, AppResult, ErrorCode, Security, ServerConfig};

fn map_err(e: &lettre::transport::smtp::Error, cfg: &ServerConfig) -> AppError {
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
/// verilir.
pub async fn send(cfg: &ServerConfig, password: &str, message: Message) -> AppResult<()> {
    let host = cfg.host.trim();
    let builder = match cfg.security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        Security::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
        Security::None if mh_sync::is_local(host) => Ok(
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
        ),
        Security::None => {
            return Err(AppError::new(
                ErrorCode::Tls,
                "Şifresiz gönderime yalnız yerel test sunucularında izin verilir.",
            ))
        }
    }
    .map_err(|e| map_err(&e, cfg))?;
    let mut builder = builder
        .port(cfg.port)
        .timeout(Some(Duration::from_secs(60)));
    if !cfg.username.trim().is_empty() {
        builder = builder.credentials(Credentials::new(
            cfg.username.trim().to_owned(),
            password.to_owned(),
        ));
    }
    let transport = builder.build();
    match transport.send(message).await {
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::warn!(host, error = %e, "SMTP gönderimi başarısız");
            Err(map_err(&e, cfg))
        }
    }
}

/// Gmail gönderilen postayı kendisi saklar; IMAP'e ayrıca eklenmez.
pub fn server_saves_sent(cfg: &ServerConfig) -> bool {
    let h = cfg.host.trim().to_ascii_lowercase();
    h.ends_with("gmail.com") || h.ends_with("googlemail.com")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::{Account, AccountKind, Address, OutgoingMessage};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// Yalnız temel komutları bilen sahte SMTP sunucusu; DATA içeriğini döner.
    async fn fake_smtp(listener: tokio::net::TcpListener) -> String {
        let (sock, _) = listener.accept().await.unwrap();
        let (r, mut w) = sock.into_split();
        let mut lines = BufReader::new(r).lines();
        w.write_all(b"220 sahte ESMTP\r\n").await.unwrap();
        let mut data = String::new();
        let mut in_data = false;
        while let Ok(Some(line)) = lines.next_line().await {
            if in_data {
                if line == "." {
                    in_data = false;
                    w.write_all(b"250 kabul\r\n").await.unwrap();
                } else {
                    data.push_str(&line);
                    data.push('\n');
                }
                continue;
            }
            let cmd = line.to_ascii_uppercase();
            let reply: &[u8] = if cmd.starts_with("EHLO") {
                b"250-sahte\r\n250 8BITMIME\r\n"
            } else if cmd.starts_with("DATA") {
                in_data = true;
                b"354 devam\r\n"
            } else if cmd.starts_with("QUIT") {
                w.write_all(b"221 hosca kal\r\n").await.unwrap();
                break;
            } else {
                b"250 tamam\r\n"
            };
            w.write_all(reply).await.unwrap();
        }
        data
    }

    #[tokio::test]
    async fn yerel_sunucuya_gonderim() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(fake_smtp(listener));
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
        };
        let message = crate::build(&account, &msg, None, 0).unwrap();
        let cfg = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::None,
            username: String::new(),
        };
        send(&cfg, "", message).await.unwrap();
        let data = server.await.unwrap();
        assert!(data.contains("Subject: Cumartesi"), "{data}");
        assert!(
            data.contains("To: =?utf-8?b?") || data.contains("To: \"Ay"),
            "{data}"
        );
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
    fn gmail_gonderileni_saklar() {
        let mut cfg = ServerConfig {
            host: "smtp.gmail.com".into(),
            port: 465,
            security: Security::Tls,
            username: "u".into(),
        };
        assert!(server_saves_sent(&cfg));
        cfg.host = "smtp.ornek.com".into();
        assert!(!server_saves_sent(&cfg));
    }
}
