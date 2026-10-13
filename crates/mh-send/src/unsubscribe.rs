//! Abonelikten çıkma: tek tık isteği (RFC 8058) ve `mailto:` çıkma postası.
//!
//! Tek tık isteği postayı gönderenin seçtiği adrese gider; bu yüzden yalnız
//! genel internetteki bir HTTPS adresine, çerezsiz, Referer'siz ve
//! yönlendirme izlenmeden gönderilir. Sunucu adı önce çözülür; adreslerden
//! biri bile yerel ağı ya da bu bilgisayarı gösteriyorsa istek hiç gönderilmez
//! (SSRF). Bağlantı yalnız denetlenen adreslere kurulur (DNS yeniden
//! bağlamaya karşı) ve sistem vekil sunucusu kullanılmaz.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use mh_core::{Address, AppError, AppResult, ErrorCode, OutgoingMessage};
use mh_store::OutboxRow;
use reqwest::header::CONTENT_TYPE;
use reqwest::Url;

use crate::message::validate_with;
use crate::outbox::Outbox;

/// Tek tık isteğinin üst süresi (ad çözümleme dahil).
const ONE_CLICK_TIMEOUT: Duration = Duration::from_secs(10);
/// Çıkma postasının gönderim üst süresi.
const MAIL_TIMEOUT: Duration = Duration::from_secs(60);
/// RFC 8058'in istediği gövde.
const ONE_CLICK_BODY: &str = "List-Unsubscribe=One-Click";
/// Konu ya da gövde verilmemişse kullanılan metin.
const DEFAULT_TEXT: &str = "unsubscribe";

/// Tek tık isteğinin sonucu (başarı dışında).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneClickError {
    /// Sunucu yanıt verdi ama isteği kabul etmedi (2xx dışı durum kodu).
    /// Çıkma sayfası tarayıcıda açılarak tamamlanabilir.
    Refused(u16),
    /// İstek gönderilmedi (güvenlik, geçersiz adres) ya da yanıt alınamadı.
    Failed(AppError),
}

impl OneClickError {
    /// Kullanıcıya gösterilecek hata.
    pub fn to_app(&self) -> AppError {
        match self {
            Self::Refused(status) => AppError::new(
                ErrorCode::Network,
                format!(
                    "Abonelikten çıkma sunucusu isteği kabul etmedi (HTTP {status}). Çıkma sayfasını tarayıcıda açarak dene."
                ),
            ),
            Self::Failed(e) => e.clone(),
        }
    }
}

fn invalid_url() -> OneClickError {
    OneClickError::Failed(AppError::new(
        ErrorCode::Invalid,
        "Abonelikten çıkma adresi geçersiz; tek tık isteği yalnız https adresine gönderilir. Gönderenin sitesinden aboneliği kapatmayı dene.",
    ))
}

fn blocked() -> OneClickError {
    OneClickError::Failed(AppError::new(
        ErrorCode::Invalid,
        "Abonelikten çıkma adresi yerel ağdaki ya da bu bilgisayardaki bir sunucuyu gösteriyor; güvenlik için istek gönderilmedi. Gönderenin sitesinden aboneliği kapatmayı dene.",
    ))
}

fn unreachable_server() -> OneClickError {
    OneClickError::Failed(AppError::new(
        ErrorCode::Network,
        "Abonelikten çıkma sunucusuna ulaşılamadı. İnternet bağlantını denetleyip yeniden dene.",
    ))
}

fn not_resolved() -> OneClickError {
    OneClickError::Failed(AppError::new(
        ErrorCode::Network,
        "Abonelikten çıkma sunucusu bulunamadı. İnternet bağlantını denetleyip yeniden dene.",
    ))
}

fn timed_out() -> OneClickError {
    OneClickError::Failed(AppError::new(
        ErrorCode::Network,
        "Abonelikten çıkma sunucusu 10 saniyede yanıt vermedi. Biraz sonra yeniden dene.",
    ))
}

/// Hata zincirinde TLS (sertifika) hatası var mı?
fn is_tls(e: &(dyn std::error::Error + 'static)) -> bool {
    let mut cur = Some(e);
    while let Some(err) = cur {
        if err.downcast_ref::<rustls::Error>().is_some() {
            return true;
        }
        let mut io = err.downcast_ref::<std::io::Error>();
        while let Some(inner) = io.and_then(|e| e.get_ref()) {
            if inner.downcast_ref::<rustls::Error>().is_some() {
                return true;
            }
            io = inner.downcast_ref::<std::io::Error>();
        }
        cur = err.source();
    }
    false
}

fn request_error(e: &reqwest::Error) -> OneClickError {
    if is_tls(e) {
        return OneClickError::Failed(AppError::new(
            ErrorCode::Tls,
            "Abonelikten çıkma sunucusunun sertifikası doğrulanamadı; istek gönderilmedi. Gönderenin sitesinden aboneliği kapatmayı dene.",
        ));
    }
    if e.is_timeout() {
        return timed_out();
    }
    unreachable_server()
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(o[0] == 0 // "bu ağ" ve belirtilmemiş (0.0.0.0/8)
        || ip.is_loopback() // 127.0.0.0/8
        || ip.is_private() // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local() // 169.254/16
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64/10
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF 192.0.0/24
        || ip.is_documentation() // 192.0.2/24, 198.51.100/24, 203.0.113/24
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // ölçüm 198.18/15
        || ip.is_multicast() // 224/4
        || o[0] >= 240) // ayrılmış 240/4 ve yayın
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let s = ip.segments();
    let embedded =
        |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    // IPv4 taşıyan biçimler gömülü adrese göre değerlendirilir.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        return is_public_v4(embedded(s[6], s[7])); // NAT64 64:ff9b::/96
    }
    if s[0] == 0x2002 {
        return is_public_v4(embedded(s[1], s[2])); // 6to4 2002::/16
    }
    !(s[0..6] == [0, 0, 0, 0, 0, 0] // ::/96: belirtilmemiş, loopback, eski IPv4 uyumlu
        || ip.is_multicast() // ff00::/8
        || (s[0] & 0xfe00) == 0xfc00 // ULA fc00::/7
        || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        || (s[0] & 0xffc0) == 0xfec0 // eski site-local fec0::/10
        || (s[0] == 0x2001 && s[1] < 0x0200) // IETF 2001::/23 (Teredo dahil)
        || (s[0] == 0x2001 && s[1] == 0x0db8) // belgeleme 2001:db8::/32
        || (s[0] == 0x0100 && s[1..4] == [0, 0, 0])) // atılacak 100::/64
}

/// Adres genel internette mi? Loopback, özel, link-local, ULA, CGNAT,
/// çoklu yayın, belirtilmemiş, belgeleme ve ayrılmış bloklar değildir.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

/// İstek kuralları (testler yerel sunucuya izin verir).
pub(crate) struct Policy {
    pub allow: fn(IpAddr) -> bool,
    pub extra_root: Option<reqwest::Certificate>,
}

impl Policy {
    fn public() -> Self {
        Self {
            allow: is_public_ip,
            extra_root: None,
        }
    }
}

/// Sunucu adının adresleri; IP yazılmışsa kendisi.
async fn resolve(host: &str, port: u16) -> Result<(Vec<SocketAddr>, bool), OneClickError> {
    let literal = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = literal.parse::<IpAddr>() {
        return Ok((vec![SocketAddr::new(ip, port)], true));
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| not_resolved())?
        .collect();
    if addrs.is_empty() {
        return Err(not_resolved());
    }
    Ok((addrs, false))
}

/// RFC 8058 tek tık isteği: `List-Unsubscribe=One-Click` gövdeli HTTPS POST.
/// 2xx yanıt başarıdır; yönlendirme izlenmez.
pub async fn one_click(url: &str) -> Result<(), OneClickError> {
    one_click_with(url, &Policy::public()).await
}

pub(crate) async fn one_click_with(url: &str, policy: &Policy) -> Result<(), OneClickError> {
    let url = Url::parse(url).map_err(|_| invalid_url())?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid_url());
    }
    let host = url
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(invalid_url)?
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(443);
    let work = async {
        let (addrs, literal) = resolve(&host, port).await?;
        if !addrs.iter().all(|a| (policy.allow)(a.ip())) {
            tracing::info!("tek tık isteği reddedildi: adres genel değil");
            return Err(blocked());
        }
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .no_proxy()
            .https_only(true)
            .connect_timeout(ONE_CLICK_TIMEOUT)
            .timeout(ONE_CLICK_TIMEOUT)
            .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")));
        if !literal {
            // Bağlantı yalnız denetlenen adreslere kurulur.
            builder = builder.resolve_to_addrs(&host, &addrs);
        }
        if let Some(root) = &policy.extra_root {
            builder = builder.add_root_certificate(root.clone());
        }
        let client = builder.build().map_err(|e| {
            tracing::warn!(error = %e, "tek tık istemcisi kurulamadı");
            unreachable_server()
        })?;
        let resp = client
            .post(url.clone())
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(ONE_CLICK_BODY)
            .send()
            .await
            .map_err(|e| {
                tracing::info!(error = %e, "tek tık isteği gönderilemedi");
                request_error(&e)
            })?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            tracing::info!(status = status.as_u16(), "tek tık isteği kabul edilmedi");
            Err(OneClickError::Refused(status.as_u16()))
        }
    };
    tokio::time::timeout(ONE_CLICK_TIMEOUT, work)
        .await
        .map_err(|_| timed_out())?
}

/// `mailto:` adresinden, `account_id` hesabından gidecek çıkma postası.
/// Konu ya da gövde verilmemişse "unsubscribe" yazılır. Adres geçersizse
/// `None`.
pub fn mailto_message(account_id: &str, uri: &str) -> Option<OutgoingMessage> {
    let m = mh_mime::parse_mailto(uri)?;
    Some(OutgoingMessage {
        account_id: account_id.to_owned(),
        to: vec![Address::new(None, &m.to)],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: m.subject.unwrap_or_else(|| DEFAULT_TEXT.into()),
        body_text: m.body.unwrap_or_else(|| DEFAULT_TEXT.into()),
        in_reply_to: None,
        attachments: Vec::new(),
        send_at: None,
        forward_from: None,
        draft_id: None,
    })
}

impl Outbox {
    /// Postayı giden kutusunda bekletmeden, geri alma süresi olmadan hemen
    /// gönderir; hata doğrudan döner (yeniden denenmez, taslağa kaydedilmez).
    /// Abonelikten çıkma postası gibi kısa, kullanıcının açıkça istediği
    /// postalar içindir. Gönderilen posta Gönderilenler'e yazılır.
    pub async fn send_immediately(&self, msg: &OutgoingMessage) -> AppResult<()> {
        validate_with(msg, 0, false)?;
        let row = OutboxRow {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: msg.account_id.clone(),
            send_at: 0,
            msg: msg.clone(),
            attempts: 0,
        };
        match tokio::time::timeout(MAIL_TIMEOUT, self.deliver(&row)).await {
            Ok(Ok(note)) => {
                if let Some(note) = note {
                    tracing::warn!(note = %note, "çıkma postası gönderildi, kopyası yazılamadı");
                }
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AppError::new(
                ErrorCode::Network,
                "Abonelikten çıkma postası bir dakikada gönderilemedi. İnternet bağlantını denetleyip yeniden dene.",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn genel_adres_denetimi() {
        for bad in [
            "0.0.0.0",
            "0.1.2.3",
            "127.0.0.1",
            "127.255.0.9",
            "10.0.0.1",
            "172.16.5.4",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.254",
            "192.0.0.8",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.7",
            "203.0.113.9",
            "224.0.0.251",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::127.0.0.1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
            "64:ff9b::a00:1",
            "2002:c0a8:101::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "2001::1",
            "2001:db8::1",
            "100::1",
        ] {
            assert!(!is_public_ip(bad.parse().unwrap()), "{bad}");
        }
        for good in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.255.255",
            "172.32.0.1",
            "192.169.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
            "2001:4860:4860::8888",
        ] {
            assert!(is_public_ip(good.parse().unwrap()), "{good}");
        }
    }

    #[test]
    fn mailto_postasi_kurulur() {
        let m = mailto_message("a", "mailto:cik@bulten.com?subject=Liste%20iptal").unwrap();
        assert_eq!(m.account_id, "a");
        assert_eq!(m.to, vec![Address::new(None, "cik@bulten.com")]);
        assert_eq!(m.subject, "Liste iptal");
        assert_eq!(m.body_text, "unsubscribe");
        assert!(m.cc.is_empty() && m.bcc.is_empty() && m.attachments.is_empty());
        let m = mailto_message("a", "mailto:cik@bulten.com").unwrap();
        assert_eq!(
            (m.subject.as_str(), m.body_text.as_str()),
            ("unsubscribe", "unsubscribe")
        );
        assert!(mailto_message("a", "mailto:gecersiz").is_none());
        assert!(validate_with(
            &mailto_message("a", "mailto:x@y.com?cc=z@w.com").unwrap(),
            0,
            false
        )
        .is_ok());
    }

    struct NoSink;

    impl mh_sync::EventSink for NoSink {
        fn sync_status(&self, _: mh_core::SyncStatus) {}
        fn changed(&self, _: Vec<String>) {}
        fn new_mail(&self, _: &mh_core::Account, _: Vec<mh_core::MessageSummary>) {}
    }

    #[tokio::test]
    async fn cikma_postasi_bekletmeden_gonderilir() {
        let store = Arc::new(mh_store::Store::open_in_memory().unwrap());
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-13T10:30:00+03:00").unwrap();
        mh_store::seed_demo(&store, now).unwrap();
        let secrets: Arc<dyn mh_sync::SecretStore> = Arc::new(mh_sync::MemorySecrets::new());
        let sync = Arc::new(mh_sync::SyncManager::new(
            store.clone(),
            secrets.clone(),
            Arc::new(NoSink),
            tokio::runtime::Handle::current(),
        ));
        let outbox = Outbox::new(store.clone(), secrets, sync, Arc::new(NoSink));
        // Tanıtım hesabı ağa çıkmadan "gönderir".
        let account = store.accounts().unwrap().remove(0);
        let msg =
            mailto_message(&account.id, "mailto:cik@bulten.com?subject=Liste%20iptal").unwrap();
        outbox.send_immediately(&msg).await.unwrap();
        let sent = store.find_by_subject("Liste iptal").unwrap().unwrap();
        assert_eq!(sent.account_id, account.id);
        assert_eq!(sent.to, vec![Address::new(None, "cik@bulten.com")]);
        assert_eq!(
            store.outbox_next_due().unwrap(),
            None,
            "giden kutusuna girmez"
        );
        // Alıcısız posta gönderilmez.
        let mut bad = msg;
        bad.to.clear();
        assert_eq!(
            outbox.send_immediately(&bad).await.unwrap_err().code,
            ErrorCode::Invalid
        );
    }

    type Seen = Arc<Mutex<Vec<String>>>;

    /// Her bağlantıda isteği okuyup `status` ile yanıtlayan HTTPS sunucusu
    /// (localhost için kendinden imzalı sertifika); (kapı, kök sertifika, istekler).
    async fn https_server(status: &'static str) -> (u16, reqwest::Certificate, Seen) {
        let cert = crate::transport::tests::self_signed();
        let root = reqwest::Certificate::from_der(&cert.der).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let s2 = seen.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let (acceptor, seen) = (cert.acceptor.clone(), s2.clone());
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(sock).await else {
                        return;
                    };
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    // Başlıklar ve (kısa) gövde gelene kadar oku.
                    while !String::from_utf8_lossy(&buf).contains(ONE_CLICK_BODY) {
                        match tls.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        if buf.len() > 64 * 1024 {
                            break;
                        }
                    }
                    seen.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf).into_owned());
                    let reply = format!(
                        "HTTP/1.1 {status}\r\nlocation: https://localhost:1/baska\r\nset-cookie: iz=1\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    );
                    let _ = tls.write_all(reply.as_bytes()).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        (port, root, seen)
    }

    fn local_policy(root: reqwest::Certificate) -> Policy {
        Policy {
            allow: |ip| ip.is_loopback(),
            extra_root: Some(root),
        }
    }

    #[tokio::test]
    async fn tek_tik_istegi_cerezsiz_post_edilir() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, root, seen) = https_server("200 OK").await;
        let url = format!("https://localhost:{port}/cik?u=42");
        one_click_with(&url, &local_policy(root.clone()))
            .await
            .unwrap();
        // Aynı istemci ikinci istekte de çerez göndermez.
        one_click_with(&url, &local_policy(root)).await.unwrap();
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        for req in &seen {
            let lower = req.to_ascii_lowercase();
            assert!(req.starts_with("POST /cik?u=42 HTTP/1.1\r\n"), "{req}");
            assert!(
                lower.contains("content-type: application/x-www-form-urlencoded"),
                "{req}"
            );
            assert!(req.ends_with("\r\n\r\nList-Unsubscribe=One-Click"), "{req}");
            assert!(!lower.contains("\r\ncookie:"), "{req}");
            assert!(!lower.contains("\r\nreferer:"), "{req}");
            assert!(!lower.contains("\r\nauthorization:"), "{req}");
        }
    }

    #[tokio::test]
    async fn yonlendirme_izlenmez_ve_basarisizlik_bildirilir() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, root, seen) = https_server("302 Found").await;
        let url = format!("https://localhost:{port}/cik");
        assert_eq!(
            one_click_with(&url, &local_policy(root.clone())).await,
            Err(OneClickError::Refused(302))
        );
        assert_eq!(seen.lock().unwrap().len(), 1, "yönlendirme izlenmedi");
        let (port, root, _) = https_server("500 Internal Server Error").await;
        let err = one_click_with(&format!("https://localhost:{port}/"), &local_policy(root))
            .await
            .unwrap_err();
        assert_eq!(err, OneClickError::Refused(500));
        assert!(err.to_app().message.contains("HTTP 500"));
    }

    #[tokio::test]
    async fn yerel_adrese_istek_gonderilmez() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, _, seen) = https_server("200 OK").await;
        for url in [
            format!("https://localhost:{port}/"),
            format!("https://127.0.0.1:{port}/"),
            format!("https://[::1]:{port}/"),
            format!("https://[::ffff:127.0.0.1]:{port}/"),
            "https://169.254.169.254/latest/meta-data".to_owned(),
            "https://10.0.0.1/".to_owned(),
        ] {
            let OneClickError::Failed(e) = one_click(&url).await.unwrap_err() else {
                panic!("{url}");
            };
            assert_eq!(e.code, ErrorCode::Invalid, "{url}");
            assert!(e.message.contains("güvenlik için"), "{url}: {}", e.message);
        }
        assert!(seen.lock().unwrap().is_empty(), "hiç istek gitmedi");
        // Yalnız https; kullanıcı bilgili ve bozuk adres reddedilir.
        for url in [
            "http://ornek.com/cik",
            "https://kullanici:parola@ornek.com/cik",
            "ftp://ornek.com/",
            "bozuk",
        ] {
            let OneClickError::Failed(e) = one_click(url).await.unwrap_err() else {
                panic!("{url}");
            };
            assert_eq!(e.code, ErrorCode::Invalid, "{url}");
            assert!(e.message.contains("https"), "{url}");
        }
    }

    #[tokio::test]
    async fn guvenilmeyen_sertifika_reddedilir() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, _, seen) = https_server("200 OK").await;
        let policy = Policy {
            allow: |ip| ip.is_loopback(),
            extra_root: None,
        };
        let OneClickError::Failed(e) =
            one_click_with(&format!("https://localhost:{port}/"), &policy)
                .await
                .unwrap_err()
        else {
            panic!("sertifika kabul edilmemeliydi");
        };
        assert_eq!(e.code, ErrorCode::Tls, "{}", e.message);
        assert!(seen.lock().unwrap().is_empty());
    }
}
