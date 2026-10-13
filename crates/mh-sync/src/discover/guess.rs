//! Yaygın sunucu adlarını kısa bir TCP denemesiyle yoklar.

use std::time::Duration;

use mh_core::{Security, ServerConfig};
use tokio::net::TcpStream;

const PROBE: Duration = Duration::from_secs(3);

async fn open(host: String, port: u16) -> bool {
    matches!(
        tokio::time::timeout(PROBE, TcpStream::connect((host.as_str(), port))).await,
        Ok(Ok(_))
    )
}

/// Adaylardan, öncelik sırasıyla, ilk açık olanı seçer. Denemeler aynı anda yapılır.
async fn first_open(candidates: Vec<(String, u16, Security)>) -> Option<(String, u16, Security)> {
    let probes = candidates.iter().map(|(h, p, _)| open(h.clone(), *p));
    let results = futures::future::join_all(probes).await;
    candidates
        .into_iter()
        .zip(results)
        .find(|(_, ok)| *ok)
        .map(|(c, _)| c)
}

pub fn imap_candidates(domain: &str) -> Vec<(String, u16, Security)> {
    vec![
        (format!("imap.{domain}"), 993, Security::Tls),
        (format!("mail.{domain}"), 993, Security::Tls),
        (format!("imap.{domain}"), 143, Security::Starttls),
        (format!("mail.{domain}"), 143, Security::Starttls),
    ]
}

pub fn smtp_candidates(domain: &str) -> Vec<(String, u16, Security)> {
    vec![
        (format!("smtp.{domain}"), 465, Security::Tls),
        (format!("mail.{domain}"), 465, Security::Tls),
        (format!("smtp.{domain}"), 587, Security::Starttls),
        (format!("mail.{domain}"), 587, Security::Starttls),
    ]
}

pub async fn probe(domain: &str, email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let (imap, smtp) = tokio::join!(
        first_open(imap_candidates(domain)),
        first_open(smtp_candidates(domain))
    );
    let cfg = |(host, port, security): (String, u16, Security)| ServerConfig {
        host,
        port,
        security,
        username: email.to_owned(),
    };
    Some((cfg(imap?), cfg(smtp?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ilk_acik_aday_secilir() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let closed = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let got = first_open(vec![
            ("127.0.0.1".into(), closed, Security::Tls),
            ("127.0.0.1".into(), port, Security::Starttls),
        ])
        .await;
        assert_eq!(got, Some(("127.0.0.1".into(), port, Security::Starttls)));
        assert_eq!(imap_candidates("ornek.com")[0].0, "imap.ornek.com");
    }
}
