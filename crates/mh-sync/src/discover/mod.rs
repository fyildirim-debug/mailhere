//! E-posta adresinden sunucu ayarlarını bulma. Sıra: gömülü tablo →
//! otomatik yapılandırma → DNS SRV → yaygın adların denenmesi.

mod autoconfig;
mod builtin;
mod guess;
mod srv;

use mh_core::{AppError, AppResult, DiscoveredConfig, DiscoverySource, ErrorCode};

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

pub async fn discover(email: &str) -> AppResult<DiscoveredConfig> {
    let (email, domain) = split_email(email)?;
    let found = |(imap, smtp), source| DiscoveredConfig {
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
    }
}
