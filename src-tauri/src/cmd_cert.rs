//! Sertifika onayı: güvenilmeyen sunucu sertifikasını inceleme ve kullanıcının
//! açık onayıyla parmak izini sabitleme.

use mh_core::{events, AppError, CertInfo, ErrorCode, Security, Settings, TrustedCert};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

fn host_key(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Onaylı sertifika listesini olağan biçime getirir: geçersiz parmak izleri
/// atılır, aynı sunucu + kapı için son onay kalır.
pub fn normalize_trusted(list: Vec<TrustedCert>) -> Vec<TrustedCert> {
    let mut out: Vec<TrustedCert> = Vec::new();
    for c in list {
        let host = host_key(&c.host);
        let Some(sha256) = mh_sync::tls::normalize_fingerprint(&c.sha256) else {
            continue;
        };
        if host.is_empty() || c.port == 0 {
            continue;
        }
        out.retain(|o| !(o.host == host && o.port == c.port));
        out.push(TrustedCert {
            host,
            port: c.port,
            sha256,
        });
    }
    out
}

/// Onayı ayarlara yazar (aynı sunucu + kapının eski onayı değişir).
pub fn with_trust(mut settings: Settings, host: &str, port: u16, sha256: &str) -> Res<Settings> {
    let host = host_key(host);
    if host.is_empty() || port == 0 {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Sunucu adı ve kapı boş olamaz.",
        ));
    }
    let sha256 = mh_sync::tls::normalize_fingerprint(sha256).ok_or_else(|| {
        AppError::new(
            ErrorCode::Invalid,
            "Sertifika parmak izi geçersiz; sertifikayı yeniden incele.",
        )
    })?;
    settings.trusted_certs.push(TrustedCert { host, port, sha256 });
    settings.trusted_certs = normalize_trusted(settings.trusted_certs);
    Ok(settings)
}

/// Sunucunun sertifikasını doğrulamadan alıp inceler.
#[tauri::command]
pub async fn cert_inspect(host: String, port: u16, security: Security) -> Res<CertInfo> {
    mh_sync::tls::inspect(&host, port, security).await
}

/// Kullanıcı sertifikaya güvendi: parmak izi bu sunucu + kapı için kalıcı
/// sabitlenir, ayar olayı yayınlanır ve bu sunucuyu kullanan hesaplar
/// yeniden bağlanır.
#[tauri::command]
pub fn cert_trust(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
    port: u16,
    sha256: String,
) -> Res<()> {
    let settings = with_trust(state.store.settings()?, &host, port, &sha256)?;
    state.store.set_settings(&settings)?;
    mh_sync::tls::set_trusted(&settings.trusted_certs);
    tracing::info!(host = %host_key(&host), port, "sertifika kullanıcı onayıyla sabitlendi");
    if let Err(e) = app.emit(events::SETTINGS, &settings) {
        tracing::warn!(error = %e, "ayar olayı gönderilemedi");
    }
    state.sync.restart_host(&host)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "A1:B2:C3:D4:E5:F6:07:18:29:3A:4B:5C:6D:7E:8F:90:01:12:23:34:45:56:67:78:89:9A:AB:BC:CD:DE:EF:F0";

    #[test]
    fn onay_eklenir_ve_degisir() {
        let s = with_trust(Settings::default(), " Posta.Ornek.com. ", 993, &FP.to_lowercase()).unwrap();
        assert_eq!(
            s.trusted_certs,
            vec![TrustedCert {
                host: "posta.ornek.com".into(),
                port: 993,
                sha256: FP.into()
            }]
        );
        let other = FP.replace("A1", "00");
        let s = with_trust(s, "posta.ornek.com", 993, &other).unwrap();
        assert_eq!(s.trusted_certs.len(), 1);
        assert_eq!(s.trusted_certs[0].sha256, other);
        let s = with_trust(s, "posta.ornek.com", 465, FP).unwrap();
        assert_eq!(s.trusted_certs.len(), 2);

        for (h, p, f) in [("", 993, FP), ("a.b", 0, FP), ("a.b", 993, "A1:B2")] {
            assert_eq!(
                with_trust(Settings::default(), h, p, f).unwrap_err().code,
                ErrorCode::Invalid
            );
        }
    }

    #[test]
    fn bozuk_onaylar_atilir() {
        let list = vec![
            TrustedCert {
                host: "a.b".into(),
                port: 993,
                sha256: "bozuk".into(),
            },
            TrustedCert {
                host: "A.B".into(),
                port: 993,
                sha256: FP.replace(':', ""),
            },
        ];
        assert_eq!(
            normalize_trusted(list),
            vec![TrustedCert {
                host: "a.b".into(),
                port: 993,
                sha256: FP.into()
            }]
        );
    }
}
