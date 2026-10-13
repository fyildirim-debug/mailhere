//! E-posta adresinden JMAP oturum adresini bulma (RFC 8620 §2.2).
//!
//! Sıra: `https://<alan>/.well-known/jmap`, `mail.` ve `jmap.` alt alanları,
//! sonra DNS SRV `_jmap._tcp`. Kimliksiz istekte JMAP oturumu ya da 401
//! dönen ilk aday kabul edilir.

use std::time::Duration;

use hickory_resolver::proto::rr::RData;
use hickory_resolver::TokioResolver;
use mh_core::{AppError, AppResult, ErrorCode};
use reqwest::{StatusCode, Url};

use crate::client::get_following;
use crate::types::{Session, CORE};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const SRV_TIMEOUT: Duration = Duration::from_secs(5);

/// Adreste bir JMAP oturumu var mı? 401 de kabul edilir (giriş isteyen JMAP).
pub async fn probe(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let work = async {
        let (resp, _) = get_following(url, None, "application/json", PROBE_TIMEOUT)
            .await
            .ok()?;
        match resp.status() {
            StatusCode::UNAUTHORIZED => Some(true),
            s if s.is_success() => {
                let bytes = resp.bytes().await.ok()?;
                let s: Session = serde_json::from_slice(&bytes).ok()?;
                Some(s.capabilities.contains_key(CORE))
            }
            _ => Some(false),
        }
    };
    matches!(
        tokio::time::timeout(PROBE_TIMEOUT * 2, work).await,
        Ok(Some(true))
    )
}

/// `_jmap._tcp.<alan>` SRV kaydının hedefi: (sunucu, kapı).
async fn srv(domain: &str) -> Option<(String, u16)> {
    let resolver = TokioResolver::builder_tokio().ok()?.build().ok()?;
    let lookup = tokio::time::timeout(
        SRV_TIMEOUT,
        resolver.srv_lookup(format!("_jmap._tcp.{domain}.")),
    )
    .await
    .ok()?
    .ok()?;
    let mut records: Vec<_> = lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::SRV(srv) => Some(srv.clone()),
            _ => None,
        })
        // "." hedefi hizmetin bilerek kapalı olduğunu söyler.
        .filter(|s| !s.target.is_root() && s.port != 0)
        .collect();
    records.sort_by_key(|s| (s.priority, std::cmp::Reverse(s.weight)));
    let s = records.first()?;
    let host = s.target.to_utf8().trim_end_matches('.').to_owned();
    Some((host, s.port))
}

/// Alan adı için denenecek adresler (öncelik sırasıyla).
pub fn candidates(domain: &str) -> Vec<String> {
    ["", "mail.", "jmap."]
        .iter()
        .map(|p| format!("https://{p}{domain}/.well-known/jmap"))
        .collect()
}

fn srv_url(host: &str, port: u16) -> String {
    if port == 443 {
        format!("https://{host}/.well-known/jmap")
    } else {
        format!("https://{host}:{port}/.well-known/jmap")
    }
}

/// Adresin alan adında JMAP oturum adresini arar.
pub async fn discover(email: &str) -> AppResult<String> {
    let (_, domain) = mh_sync::discover::split_email(email)?;
    let list = candidates(&domain);
    let (results, srv) = tokio::join!(
        futures::future::join_all(list.iter().map(|u| probe(u))),
        srv(&domain)
    );
    if let Some((url, _)) = list.iter().zip(&results).find(|(_, ok)| **ok) {
        return Ok(url.clone());
    }
    if let Some((host, port)) = srv {
        let url = srv_url(&host, port);
        if probe(&url).await {
            return Ok(url);
        }
    }
    Err(AppError::new(
        ErrorCode::Discovery,
        "Bu adres için JMAP sunucusu bulunamadı. Oturum adresini elle yazabilirsin (ör. https://posta.alanadi.com/.well-known/jmap).",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aday_adresler() {
        assert_eq!(
            candidates("ornek.com"),
            vec![
                "https://ornek.com/.well-known/jmap",
                "https://mail.ornek.com/.well-known/jmap",
                "https://jmap.ornek.com/.well-known/jmap",
            ]
        );
        assert_eq!(
            srv_url("posta.ornek.com", 443),
            "https://posta.ornek.com/.well-known/jmap"
        );
        assert_eq!(
            srv_url("posta.ornek.com", 8443),
            "https://posta.ornek.com:8443/.well-known/jmap"
        );
    }

    #[tokio::test]
    async fn gecersiz_adres_aga_cikmadan_reddedilir() {
        assert_eq!(
            discover("gecersiz").await.unwrap_err().code,
            ErrorCode::Invalid
        );
        assert!(!probe("bozuk adres").await);
        assert!(!probe("http://uzak.ornek.com/.well-known/jmap").await);
    }
}
