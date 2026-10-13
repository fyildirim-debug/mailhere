//! E-posta adresinden JMAP oturum adresini bulma (RFC 8620 §2.2).
//!
//! Sıra: `https://<alan>/.well-known/jmap`, `mail.` ve `jmap.` alt alanları,
//! sonra DNS SRV `_jmap._tcp`. Kimliksiz istekte JMAP oturumu ya da 401
//! dönen ilk aday kabul edilir. SRV hedefi e-posta alan adı ya da alt alanı
//! değilse kullanılmaz (DNS yanıtı doğrulanmaz; parola saldırgana gitmesin).

use std::time::Duration;

use hickory_resolver::proto::rr::RData;
use hickory_resolver::TokioResolver;
use mh_core::{AppError, AppResult, ErrorCode};
use reqwest::{StatusCode, Url};

use crate::client::{get_following, read_limited, MAX_JSON};
use crate::types::{Session, CORE};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const SRV_TIMEOUT: Duration = Duration::from_secs(5);

/// Adreste bir JMAP oturumu var mı? 401 de kabul edilir (giriş isteyen JMAP).
pub async fn probe(url: &str) -> bool {
    probe_final(url).await.is_some()
}

/// Adreste bir JMAP oturumu varsa yönlendirmelerden sonraki adresi döner.
/// Kimliksiz istektir; başka kökene yönlendirme izlenebilir (https'ten
/// http'ye ya da bu bilgisayara inilmez). Dönen adres kullanıcıya gösterilir
/// ve parola yalnız oraya gider.
async fn probe_final(url: &str) -> Option<Url> {
    let url = Url::parse(url).ok()?;
    let work = async {
        let (resp, final_url) = get_following(url, None, "application/json", PROBE_TIMEOUT)
            .await
            .ok()?;
        match resp.status() {
            StatusCode::UNAUTHORIZED => Some(final_url),
            s if s.is_success() => {
                let bytes = read_limited(resp, MAX_JSON).await.ok()?;
                let s: Session = serde_json::from_slice(&bytes).ok()?;
                s.capabilities.contains_key(CORE).then_some(final_url)
            }
            _ => None,
        }
    };
    tokio::time::timeout(PROBE_TIMEOUT * 2, work)
        .await
        .ok()
        .flatten()
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
    let host = s.target.to_ascii().trim_end_matches('.').to_owned();
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

/// SRV hedefi alan adı dışında: DNS yanıtı imzasız olduğundan (DNSSEC yok)
/// kendiliğinden kullanılmaz; kullanıcı adresi bilerek yazabilir.
fn foreign_srv(host: &str, port: u16) -> AppError {
    AppError::new(
        ErrorCode::Discovery,
        format!(
            "DNS kaydı JMAP sunucusunu başka bir alan adında gösteriyor ({host}); güvenlik için kendiliğinden kullanılmadı. Bu sunucu doğruysa oturum adresini elle yaz (ör. {}).",
            srv_url(host, port)
        ),
    )
}

/// SRV hedefinden denenecek oturum adresi; hedef alan adı dışındaysa hata.
fn srv_candidate(host: &str, port: u16, domain: &str) -> AppResult<String> {
    if !mh_sync::discover::in_domain(host, domain) {
        return Err(foreign_srv(host, port));
    }
    Ok(srv_url(host, port))
}

/// Adresin alan adında JMAP oturum adresini arar. Yönlendirme izlendiyse son
/// adres döner.
pub async fn discover(email: &str) -> AppResult<String> {
    let (_, domain) = mh_sync::discover::split_email(email)?;
    let list = candidates(&domain);
    let (results, srv) = tokio::join!(
        futures::future::join_all(list.iter().map(|u| probe_final(u))),
        srv(&domain)
    );
    if let Some(url) = results.into_iter().flatten().next() {
        return Ok(url.to_string());
    }
    if let Some((host, port)) = srv {
        if let Some(url) = probe_final(&srv_candidate(&host, port, &domain)?).await {
            return Ok(url.to_string());
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
    async fn yoklama_yonlendirme_sonrasi_adresi_doner() {
        use crate::client::tests::raw_server;
        let (target, _) = raw_server(
            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
        )
        .await;
        let (entry, seen) = raw_server(format!(
            "HTTP/1.1 302 Found\r\nlocation: {target}/jmap/session\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        ))
        .await;
        // Kimliksiz yoklama başka kökene geçer; kullanıcıya son adres gösterilir
        // ve parola yalnız oraya gider.
        let got = probe_final(&format!("{entry}/.well-known/jmap"))
            .await
            .unwrap();
        assert_eq!(got.as_str(), format!("{target}/jmap/session"));
        assert!(!seen.lock().unwrap()[0]
            .to_ascii_lowercase()
            .contains("authorization"));
        // JMAP olmayan yanıt kabul edilmez.
        let (other, _) = raw_server(
            "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}".into(),
        )
        .await;
        assert!(probe_final(&format!("{other}/x")).await.is_none());
    }

    #[test]
    fn alan_disi_srv_hedefi_kesif_hatasidir() {
        assert_eq!(
            srv_candidate("jmap.ornek.com", 443, "ornek.com").unwrap(),
            "https://jmap.ornek.com/.well-known/jmap"
        );
        assert_eq!(
            srv_candidate("Ornek.com", 8443, "ornek.com").unwrap(),
            "https://Ornek.com:8443/.well-known/jmap"
        );
        for host in [
            "posta.saldirgan.com",
            "kotuornek.com",
            "ornek.com.saldirgan.com",
        ] {
            let e = srv_candidate(host, 443, "ornek.com").unwrap_err();
            assert_eq!(e.code, ErrorCode::Discovery, "{host}");
            assert!(e.message.contains(host), "{}", e.message);
            assert!(
                e.message
                    .contains(&format!("https://{host}/.well-known/jmap")),
                "{}",
                e.message
            );
        }
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
