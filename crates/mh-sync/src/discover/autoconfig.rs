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
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name == "incomingServer" || name == "outgoingServer" {
                    let kind = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == b"type")
                        .map(|a| String::from_utf8_lossy(&a.value).to_ascii_lowercase())
                        .unwrap_or_default();
                    current = Some(Server {
                        incoming: name == "incomingServer",
                        kind,
                        ..Server::default()
                    });
                }
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.decode().ok()?),
            Event::CData(t) => text.push_str(&t.decode().ok()?),
            Event::GeneralRef(r) => {
                if let Some(c) = entity(&r.decode().ok()?) {
                    text.push(c);
                }
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
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

async fn get(client: &reqwest::Client, url: &str) -> Option<String> {
    let res = client.get(url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let body = res.bytes().await.ok()?;
    if body.len() > MAX_BYTES {
        return None;
    }
    String::from_utf8(body.to_vec()).ok()
}

/// İki bilinen adresi sırayla dener.
pub async fn fetch(domain: &str, email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
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
