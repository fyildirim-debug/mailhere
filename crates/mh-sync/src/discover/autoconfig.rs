//! Alan adının kendi yayımladığı otomatik yapılandırma belgesi
//! (`autoconfig.<alan>` ve `/.well-known/autoconfig`).

use std::time::Duration;

use mh_core::{Security, ServerConfig};
use quick_xml::events::Event;
use quick_xml::Reader;

const TIMEOUT: Duration = Duration::from_secs(6);
const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Default, Clone)]
struct Server {
    incoming: bool,
    kind: String,
    host: String,
    port: Option<u16>,
    socket: String,
    username: String,
}

impl Server {
    fn security(&self) -> Option<Security> {
        match self.socket.to_ascii_uppercase().as_str() {
            "SSL" | "TLS" => Some(Security::Tls),
            "STARTTLS" => Some(Security::Starttls),
            _ => None,
        }
    }
}

fn substitute(s: &str, email: &str) -> String {
    let (local, domain) = email.split_once('@').unwrap_or((email, ""));
    s.replace("%EMAILADDRESS%", email)
        .replace("%EMAILLOCALPART%", local)
        .replace("%EMAILDOMAIN%", domain)
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let n = name.strip_prefix('#')?;
            let code = match n.strip_prefix('x') {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// Belgeden ilk uygun IMAP ve SMTP sunucusunu seçer (şifresizler atlanır,
/// TLS STARTTLS'ten önce gelir).
pub fn parse(xml: &str, email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut servers: Vec<Server> = Vec::new();
    let mut current: Option<Server> = None;
    let mut text = String::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_owned();
                if name == "incomingServer" || name == "outgoingServer" {
                    let kind = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == "type")
                        .map(|a| a.value.to_ascii_lowercase())
                        .unwrap_or_default();
                    current = Some(Server {
                        incoming: name == "incomingServer",
                        kind,
                        ..Server::default()
                    });
                }
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::CData(t) => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) => {
                if let Some(c) = entity(&r) {
                    text.push(c);
                }
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_owned();
                if let Some(cur) = current.as_mut() {
                    let value = text.trim().to_owned();
                    match name.as_str() {
                        "hostname" => cur.host = value,
                        "port" => cur.port = value.parse().ok(),
                        "socketType" => cur.socket = value,
                        "username" => cur.username = value,
                        "incomingServer" | "outgoingServer" => servers.extend(current.take()),
                        _ => {}
                    }
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let pick = |incoming: bool, kind: &str| -> Option<ServerConfig> {
        let mut candidates: Vec<&Server> = servers
            .iter()
            .filter(|s| {
                s.incoming == incoming
                    && s.kind == kind
                    && !s.host.is_empty()
                    && s.security().is_some()
            })
            .collect();
        candidates.sort_by_key(|s| match s.security() {
            Some(Security::Tls) => 0,
            _ => 1,
        });
        let s = candidates.first()?;
        let security = s.security()?;
        Some(ServerConfig {
            host: substitute(&s.host, email),
            port: s.port.unwrap_or(match (incoming, security) {
                (true, Security::Tls) => 993,
                (true, _) => 143,
                (false, Security::Tls) => 465,
                (false, _) => 587,
            }),
            security,
            username: if s.username.is_empty() {
                email.to_owned()
            } else {
                substitute(&s.username, email)
            },
        })
    };
    Some((pick(true, "imap")?, pick(false, "smtp")?))
}

/// Belgeyi indirir; `MAX_BYTES`'ı aşan yanıtın kalanı okunmadan bırakılır.
async fn get(client: &reqwest::Client, url: &str) -> Option<String> {
    let mut res = client.get(url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    if res.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.ok()? {
        if body.len() + chunk.len() > MAX_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).ok()
}

/// Belge yalnız HTTPS ile alınır; yönlendirme de şifresiz adrese inemez.
/// Böylece belge alan adının TLS sertifikasıyla doğrulanmış olur.
fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()
}

/// İki bilinen adresi sırayla dener.
pub async fn fetch(domain: &str, email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let client = client()?;
    let encoded: String = email
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    let urls = [
        format!("https://autoconfig.{domain}/mail/config-v1.1.xml?emailaddress={encoded}"),
        format!(
            "https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml?emailaddress={encoded}"
        ),
    ];
    for url in &urls {
        if let Some(found) = get(&client, url).await.and_then(|xml| parse(&xml, email)) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="ornek.com.tr">
    <domain>ornek.com.tr</domain>
    <displayName>Örnek Posta</displayName>
    <incomingServer type="pop3">
      <hostname>pop.ornek.com.tr</hostname>
      <port>995</port>
      <socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>imap.ornek.com.tr</hostname>
      <port>143</port>
      <socketType>STARTTLS</socketType>
      <username>%EMAILLOCALPART%</username>
    </incomingServer>
    <incomingServer type="imap">
      <hostname>posta.%EMAILDOMAIN%</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.ornek.com.tr</hostname>
      <port>25</port>
      <socketType>plain</socketType>
    </outgoingServer>
    <outgoingServer type="smtp">
      <hostname><![CDATA[gonder.ornek.com.tr]]></hostname>
      <port>587</port>
      <socketType>STARTTLS</socketType>
      <username>%EMAILLOCALPART%&#64;%EMAILDOMAIN%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

    #[test]
    fn belge_cozumlenir() {
        let (imap, smtp) = parse(XML, "furkan@ornek.com.tr").unwrap();
        assert_eq!(imap.host, "posta.ornek.com.tr");
        assert_eq!(imap.port, 993);
        assert_eq!(imap.security, Security::Tls);
        assert_eq!(imap.username, "furkan@ornek.com.tr");
        assert_eq!(smtp.host, "gonder.ornek.com.tr");
        assert_eq!(smtp.port, 587);
        assert_eq!(smtp.security, Security::Starttls);
        assert_eq!(smtp.username, "furkan@ornek.com.tr");
    }

    #[test]
    fn eksik_ya_da_bozuk_belge() {
        assert!(parse("<clientConfig></clientConfig>", "a@b.c").is_none());
        assert!(parse("<clientConfig><incomingServer", "a@b.c").is_none());
        let only_plain = r#"<clientConfig><emailProvider>
            <incomingServer type="imap"><hostname>i</hostname><port>143</port><socketType>plain</socketType></incomingServer>
            <outgoingServer type="smtp"><hostname>s</hostname><socketType>SSL</socketType></outgoingServer>
            </emailProvider></clientConfig>"#;
        assert!(
            parse(only_plain, "a@b.c").is_none(),
            "şifresiz IMAP kabul edilmez"
        );
    }

    /// Her bağlantıya `head` başlığını ve ardından `body` parçalarını yazan,
    /// sonra bağlantıyı açık tutan düz HTTP sunucusu. Gelen istek sayısını
    /// sayar.
    async fn server(
        head: &'static str,
        body: Vec<u8>,
        repeat: usize,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://127.0.0.1:{}/mail/config-v1.1.xml",
            listener.local_addr().unwrap().port()
        );
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                h.fetch_add(1, Ordering::SeqCst);
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock.write_all(head.as_bytes()).await;
                    for _ in 0..repeat {
                        if sock.write_all(&body).await.is_err() {
                            return;
                        }
                    }
                    // Bağlantı açık kalır: tüm gövdeyi bekleyen istemci takılır.
                    tokio::time::sleep(Duration::from_secs(60)).await;
                });
            }
        });
        (url, hits)
    }

    fn plain_client() -> reqwest::Client {
        reqwest::Client::builder().timeout(TIMEOUT).build().unwrap()
    }

    #[tokio::test]
    async fn kucuk_belge_okunur() {
        let body = b"<clientConfig/>".to_vec();
        let (url, _) = server(
            "HTTP/1.1 200 OK\r\ncontent-length: 15\r\nconnection: close\r\n\r\n",
            body,
            1,
        )
        .await;
        assert_eq!(
            get(&plain_client(), &url).await.as_deref(),
            Some("<clientConfig/>")
        );
    }

    #[tokio::test]
    async fn buyuk_belge_akista_kesilir() {
        // Uzunluk bildirmeyen, bitmeyen gövde: sınır aşılınca hemen bırakılır.
        let (url, _) = server(
            "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n",
            vec![b'x'; 64 * 1024],
            MAX_BYTES / (64 * 1024) + 2,
        )
        .await;
        let got = tokio::time::timeout(Duration::from_secs(3), get(&plain_client(), &url))
            .await
            .expect("sınır aşılınca beklemeden döner");
        assert!(got.is_none());

        // Bildirilen uzunluk sınırı aşıyorsa gövde hiç okunmaz.
        let (url, _) = server(
            "HTTP/1.1 200 OK\r\ncontent-length: 999999999\r\n\r\n",
            Vec::new(),
            0,
        )
        .await;
        let got = tokio::time::timeout(Duration::from_secs(3), get(&plain_client(), &url))
            .await
            .expect("gövde beklenmez");
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn sifresiz_adrese_gidilmez() {
        let (url, hits) = server(
            "HTTP/1.1 200 OK\r\ncontent-length: 15\r\nconnection: close\r\n\r\n",
            b"<clientConfig/>".to_vec(),
            1,
        )
        .await;
        assert!(get(&client().unwrap(), &url).await.is_none());
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "http adresine bağlanılmaz"
        );
    }

    #[test]
    fn kapi_yoksa_varsayilan() {
        let xml = r#"<clientConfig><emailProvider>
            <incomingServer type="imap"><hostname>i.b.c</hostname><socketType>SSL</socketType></incomingServer>
            <outgoingServer type="smtp"><hostname>s.b.c</hostname><socketType>SSL</socketType></outgoingServer>
            </emailProvider></clientConfig>"#;
        let (i, s) = parse(xml, "a@b.c").unwrap();
        assert_eq!((i.port, s.port), (993, 465));
        assert_eq!(i.username, "a@b.c");
    }
}
