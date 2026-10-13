//! E-posta adresinden sunucu ayarlarını bulma. Sıra: gömülü tablo →
//! otomatik yapılandırma → DNS SRV → yaygın adların denenmesi.

mod autoconfig;
mod builtin;
mod guess;
mod srv;

use mh_core::{AppError, AppResult, DiscoveredConfig, DiscoverySource, ErrorCode, ServerConfig};

pub use autoconfig::parse as parse_autoconfig;

/// Adresi doğrular; (küçük harfli adres, alan adı) döner.
pub fn split_email(email: &str) -> AppResult<(String, String)> {
    let email = email.trim();
    let invalid = || {
        AppError::new(
            ErrorCode::Invalid,
            "E-posta adresi geçersiz. ad@alanadi.com biçiminde yaz.",
        )
    };
    let (local, domain) = email.rsplit_once('@').ok_or_else(invalid)?;
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || domain
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
    {
        return Err(invalid());
    }
    Ok((format!("{local}@{domain}"), domain))
}

/// Sunucu adı e-posta alan adının kendisi ya da bir alt alanı mı?
pub fn in_domain(host: &str, domain: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    !domain.is_empty()
        && (host == domain
            || host
                .strip_suffix(domain.as_str())
                .is_some_and(|sub| sub.len() > 1 && sub.ends_with('.')))
}

/// Bulunan ayar alan adı dışına mı gidiyor? Yalnız doğrulanmamış kaynaklarda
/// (DNS SRV yanıtı imzasızdır, tahmin) anlamlıdır: HTTPS ile alan adının
/// kendisinden alınan otomatik yapılandırma ve gömülü tablo başka sunucuyu
/// bilerek gösterebilir.
fn other_domain(
    source: DiscoverySource,
    domain: &str,
    imap: &ServerConfig,
    smtp: &ServerConfig,
) -> bool {
    match source {
        DiscoverySource::Srv | DiscoverySource::Guess => {
            !in_domain(&imap.host, domain) || !in_domain(&smtp.host, domain)
        }
        DiscoverySource::Autoconfig | DiscoverySource::Builtin => false,
    }
}

pub async fn discover(email: &str) -> AppResult<DiscoveredConfig> {
    let (email, domain) = split_email(email)?;
    let found = |(imap, smtp): (ServerConfig, ServerConfig), source| DiscoveredConfig {
        other_domain: other_domain(source, &domain, &imap, &smtp),
        domain: domain.clone(),
        imap,
        smtp,
        source,
    };
    if let Some(c) = builtin::lookup(&domain, &email) {
        return Ok(found(c, DiscoverySource::Builtin));
    }
    if let Some(c) = autoconfig::fetch(&domain, &email).await {
        return Ok(found(c, DiscoverySource::Autoconfig));
    }
    if let Some(c) = srv::lookup(&domain, &email).await {
        return Ok(found(c, DiscoverySource::Srv));
    }
    if let Some(c) = guess::probe(&domain, &email).await {
        return Ok(found(c, DiscoverySource::Guess));
    }
    Err(AppError::new(
        ErrorCode::Discovery,
        "Bu adres için sunucu ayarları bulunamadı. Sunucu adlarını elle girebilirsin.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adres_dogrulama() {
        assert_eq!(
            split_email(" Furkan@Ornek.COM.tr ").unwrap(),
            ("Furkan@ornek.com.tr".into(), "ornek.com.tr".into())
        );
        for bad in ["", "furkan", "@ornek.com", "a@", "a@yerel", "a@ör nek.com"] {
            assert_eq!(
                split_email(bad).unwrap_err().code,
                ErrorCode::Invalid,
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn gomulu_tablo_aga_cikmaz() {
        let c = discover("ali@gmail.com").await.unwrap();
        assert_eq!(c.source, DiscoverySource::Builtin);
        assert_eq!(c.domain, "gmail.com");
        assert_eq!(c.imap.host, "imap.gmail.com");
        assert!(!c.other_domain);
        // Gömülü tablo başka alan adını bilerek gösterir (hotmail → office365).
        let c = discover("ali@hotmail.com").await.unwrap();
        assert_eq!(c.imap.host, "outlook.office365.com");
        assert!(!c.other_domain);
    }

    #[test]
    fn alan_adi_ici() {
        assert!(in_domain("ornek.com", "ornek.com"));
        assert!(in_domain("imap.Ornek.COM.", "ornek.com"));
        assert!(in_domain("a.b.ornek.com", "ornek.com."));
        for host in [
            "kotuornek.com",
            "ornek.com.saldirgan.net",
            "saldirgan.net",
            ".ornek.com",
            "",
        ] {
            assert!(!in_domain(host, "ornek.com"), "{host}");
        }
        assert!(!in_domain("ornek.com", ""));
    }

    #[test]
    fn baska_alan_adi_yalniz_dogrulanmamis_kaynakta() {
        let server = |host: &str| ServerConfig {
            host: host.into(),
            port: 993,
            security: mh_core::Security::Tls,
            username: "a@ornek.com".into(),
        };
        let (own, sub, foreign) = (
            server("ornek.com"),
            server("imap.ornek.com"),
            server("posta.saldirgan.net"),
        );
        for src in [DiscoverySource::Srv, DiscoverySource::Guess] {
            assert!(!other_domain(src, "ornek.com", &own, &sub));
            assert!(
                other_domain(src, "ornek.com", &foreign, &sub),
                "IMAP dışarıda"
            );
            assert!(
                other_domain(src, "ornek.com", &sub, &foreign),
                "SMTP dışarıda"
            );
        }
        for src in [DiscoverySource::Autoconfig, DiscoverySource::Builtin] {
            assert!(!other_domain(src, "ornek.com", &foreign, &foreign));
        }
    }
}
