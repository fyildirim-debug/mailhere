//! Uygulama içi güncelleme: denetim, indirme + kurma + yeniden başlatma ve
//! arka planda düzenli denetim (`settings.auto_update`).
//!
//! Sürüm bilgisi GitHub sürümündeki imzalı `latest.json` dosyasından okunur
//! (`tauri.conf.json` → `plugins.updater`). Paket imzası kurulmadan önce
//! gömülü açık anahtarla doğrulanır.

use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Duration;

use mh_core::{events, AppError, ErrorCode, UpdateInfo};
use tauri::utils::config::BundleType;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

/// Yeni paketlerin elle indirildiği sayfa.
pub const RELEASES_URL: &str = "https://github.com/fyildirim-debug/mailhere/releases/latest";
/// Açılıştan sonra ilk denetim.
const FIRST_CHECK: Duration = Duration::from_secs(30);
/// Arka plan denetimlerinin aralığı.
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(30);

/// Bildirilen sürümler: her yeni sürüm için `mh:update` yalnız bir kez gider.
#[derive(Default)]
pub struct UpdateState {
    announced: Mutex<Option<String>>,
}

impl UpdateState {
    /// Sürüm ilk kez görülüyorsa işaretler ve `true` döner.
    pub fn first_seen(&self, version: &str) -> bool {
        let mut last = self.announced.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_deref() == Some(version) {
            return false;
        }
        *last = Some(version.to_owned());
        true
    }
}

/// Arayüze giden sürüm bilgisi; tarih `latest.json` içindeki `pub_date`.
fn info_from(version: &str, notes: Option<&str>, raw: &serde_json::Value) -> UpdateInfo {
    UpdateInfo {
        version: version.to_owned(),
        notes: notes
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned),
        date: raw
            .get("pub_date")
            .and_then(|d| d.as_str())
            .filter(|d| !d.trim().is_empty())
            .map(str::to_owned),
    }
}

fn info(u: &Update) -> UpdateInfo {
    info_from(&u.version, u.body.as_deref(), &u.raw_json)
}

fn network_error() -> AppError {
    AppError::new(
        ErrorCode::Network,
        "Güncelleme sunucusuna ulaşılamadı. İnternet bağlantını denetleyip yeniden dene.",
    )
}

/// Kurulumun uygulama içinden güncellenip güncellenemeyeceği; güncellenemiyorsa
/// kullanıcıya gösterilecek neden.
pub fn install_blocker(bundle: Option<BundleType>) -> Option<String> {
    match bundle {
        Some(BundleType::AppImage | BundleType::Nsis | BundleType::Msi) => None,
        Some(BundleType::Deb | BundleType::Rpm) => Some(format!(
            "Bu kurulum (.deb/.rpm paketi) uygulama içinden güncellenemiyor. \
             Yeni sürümün paketini sürümler sayfasından indirip kur: {RELEASES_URL}"
        )),
        _ => Some(format!(
            "Bu Mailhere kurulumu uygulama içinden güncellenemiyor. \
             Yeni sürümü sürümler sayfasından indir: {RELEASES_URL}"
        )),
    }
}

/// Sunucudaki sürüm bilgisini okur; yeni sürüm yoksa `None`.
async fn check(app: &AppHandle) -> Res<Option<Update>> {
    let updater = app
        .updater_builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| {
            tracing::error!(error = %e, "güncelleyici kurulamadı");
            AppError::new(
                ErrorCode::Internal,
                "Güncelleme denetimi başlatılamadı. Uygulamayı yeniden başlatıp dene.",
            )
        })?;
    match updater.check().await {
        Ok(update) => Ok(update),
        // Bu işletim sistemi/mimari için paket yayımlanmamış: güncelleme yok sayılır.
        Err(UpdaterError::TargetNotFound(_) | UpdaterError::TargetsNotFound(_)) => Ok(None),
        // Sunucu yanıt verdi ama sürüm bilgisi yok (ör. son sürümde latest.json
        // yayımlanmamış): yeni sürüm yok.
        Err(UpdaterError::ReleaseNotFound) => {
            tracing::info!("son sürümde güncelleme bilgisi (latest.json) yok");
            Ok(None)
        }
        Err(e @ (UpdaterError::Reqwest(_) | UpdaterError::Network(_) | UpdaterError::Io(_))) => {
            tracing::info!(error = %e, "güncelleme sunucusuna ulaşılamadı");
            Err(network_error())
        }
        Err(e) => {
            tracing::warn!(error = %e, "güncelleme bilgisi okunamadı");
            Err(AppError::new(
                ErrorCode::Internal,
                "Güncelleme bilgisi okunamadı. Biraz sonra yeniden dene.",
            ))
        }
    }
}

/// Yeni sürüm varsa bilgisini döner (arka plan bildirimi bu sürümü tekrar
/// duyurmaz).
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Res<Option<UpdateInfo>> {
    let Some(update) = check(&app).await? else {
        return Ok(None);
    };
    let info = info(&update);
    app.state::<UpdateState>().first_seen(&info.version);
    Ok(Some(info))
}

/// Yeni sürümü indirir, imzasını doğrular, kurar ve uygulamayı yeniden başlatır.
/// Windows'ta kurulum programı uygulamayı kendisi kapatıp yeniden açar.
#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    if let Some(reason) = install_blocker(tauri::utils::platform::bundle_type()) {
        return Err(AppError::new(ErrorCode::Invalid, reason));
    }
    let update = check(&app).await?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Kurulacak yeni sürüm yok; Mailhere güncel.",
        )
    })?;
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| {
        tracing::warn!(error = %e, "güncelleme indirilemedi");
        match e {
            UpdaterError::Minisign(_)
            | UpdaterError::Base64(_)
            | UpdaterError::SignatureUtf8(_)
            | UpdaterError::SignedVersionMismatch { .. }
            | UpdaterError::MissingSignedVersion => signature_error(),
            _ => AppError::new(
                ErrorCode::Network,
                "Güncelleme indirilemedi. Bağlantını denetleyip yeniden dene.",
            ),
        }
    })?;
    update.install(bytes).map_err(|e| {
        tracing::error!(error = %e, "güncelleme kurulamadı");
        AppError::new(
            ErrorCode::Internal,
            format!(
                "Güncelleme kurulamadı. Yeniden dene ya da yeni sürümü sürümler sayfasından indir: {RELEASES_URL}"
            ),
        )
    })?;
    tracing::info!(version = %update.version, "güncelleme kuruldu, yeniden başlatılıyor");
    state.quitting.store(true, Ordering::SeqCst);
    app.request_restart();
    Ok(())
}

fn signature_error() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Güncelleme paketinin imzası doğrulanamadı; paket kurulmadı.",
    )
}

/// Ayar açıksa yeni sürümü denetler ve ilk kez görülen sürümü duyurur.
async fn background_check(app: &AppHandle) {
    let enabled = app
        .try_state::<AppState>()
        .and_then(|s| s.store.settings().ok())
        .is_some_and(|s| s.auto_update);
    if !enabled {
        return;
    }
    match check(app).await {
        Ok(Some(update)) => {
            let info = info(&update);
            if app.state::<UpdateState>().first_seen(&info.version) {
                tracing::info!(version = %info.version, "yeni sürüm bulundu");
                if let Err(e) = app.emit(events::UPDATE, &info) {
                    tracing::warn!(error = %e, "güncelleme olayı gönderilemedi");
                }
            }
        }
        Ok(None) => {}
        Err(e) => tracing::info!(error = %e.message, "arka plan güncelleme denetimi başarısız"),
    }
}

/// Güncelleme durumunu kurar ve arka plan denetimini başlatır (açılıştan 30 sn
/// sonra, ardından 6 saatte bir). Tanıtım kipinde denetim yapılmaz.
pub fn start(app: &AppHandle, rt: &tokio::runtime::Handle) {
    app.manage(UpdateState::default());
    if crate::state::demo_mode() {
        return;
    }
    let app = app.clone();
    rt.spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            background_check(&app).await;
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surum_bir_kez_duyurulur() {
        let s = UpdateState::default();
        assert!(s.first_seen("0.2.0"));
        assert!(!s.first_seen("0.2.0"));
        assert!(s.first_seen("0.3.0"));
        assert!(!s.first_seen("0.3.0"));
    }

    #[test]
    fn surum_bilgisi() {
        let raw = serde_json::json!({
            "version": "0.2.0",
            "notes": "JMAP ve Google girişi.",
            "pub_date": "2026-10-20T12:00:00Z",
        });
        assert_eq!(
            info_from("0.2.0", Some("  JMAP ve Google girişi.\n"), &raw),
            UpdateInfo {
                version: "0.2.0".into(),
                notes: Some("JMAP ve Google girişi.".into()),
                date: Some("2026-10-20T12:00:00Z".into()),
            }
        );
        let bare = info_from("0.2.1", Some("  "), &serde_json::json!({}));
        assert_eq!((bare.notes, bare.date), (None, None));
    }

    #[test]
    fn deb_ve_rpm_elle_guncellenir() {
        assert_eq!(install_blocker(Some(BundleType::AppImage)), None);
        assert_eq!(install_blocker(Some(BundleType::Nsis)), None);
        assert_eq!(install_blocker(Some(BundleType::Msi)), None);
        for b in [Some(BundleType::Deb), Some(BundleType::Rpm)] {
            let m = install_blocker(b).unwrap();
            assert!(m.contains(".deb/.rpm") && m.contains(RELEASES_URL), "{m}");
        }
        assert!(install_blocker(None).unwrap().contains(RELEASES_URL));
    }
}
