//! Uygulama durumu ve açılış.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mh_core::{AppError, ErrorCode, Millis, NotifyEvent};
use mh_notify::Mute;
use mh_send::Outbox;
use mh_store::Store;
use mh_sync::{MemorySecrets, SecretStore, SyncManager};
use tauri::{App, AppHandle, Manager};

use crate::secrets::KeyringSecrets;
use crate::sink::TauriSink;

pub struct AppState {
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
    pub sync: Arc<SyncManager>,
    pub outbox: Arc<Outbox>,
    /// "Bildirimleri sessize al" durumu.
    pub mute: Mute,
    /// Bildirim penceresi henüz dinlemiyorken kaybolmasın diye bekleyen kartlar.
    pub notify_queue: Mutex<Vec<(Instant, NotifyEvent)>>,
    pub compose_counter: AtomicU32,
    /// Tepsiden "Çık" seçildi; ana pencere artık gizlenmez, kapanır.
    pub quitting: AtomicBool,
    /// Açılan eklerin yazıldığı klasör.
    pub cache_dir: PathBuf,
}

pub fn now_ms() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

pub fn internal(e: impl std::fmt::Display) -> AppError {
    tracing::error!(error = %e, "iç hata");
    AppError::new(
        ErrorCode::Internal,
        "Beklenmeyen bir hata oluştu. İşlemi yeniden dene.",
    )
}

fn env_flag(name: &str, value: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v.trim().eq_ignore_ascii_case(value))
}

/// Veri klasörü: `MAILHERE_DATA_DIR` ya da işletim sisteminin uygulama veri klasörü.
fn data_dir(app: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(dir) = std::env::var_os("MAILHERE_DATA_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    Ok(app.path().app_data_dir()?)
}

pub fn setup(app: &mut App, rt: tokio::runtime::Handle) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let data = data_dir(&handle)?;
    std::fs::create_dir_all(&data)?;
    let cache_dir = if std::env::var_os("MAILHERE_DATA_DIR").is_some() {
        data.join("cache")
    } else {
        handle
            .path()
            .app_cache_dir()
            .unwrap_or_else(|_| data.join("cache"))
    };
    tracing::info!(dir = %data.display(), "veri klasörü");

    let store = Arc::new(Store::open(&data.join("mailhere.db")).map_err(|e| e.message)?);
    let demo = env_flag("MAILHERE_DEMO", "1");
    let secrets: Arc<dyn SecretStore> = if demo || env_flag("MAILHERE_SECRETS", "memory") {
        tracing::info!("parolalar bellekte tutuluyor (tanıtım ya da test kipi)");
        Arc::new(MemorySecrets::new())
    } else {
        Arc::new(KeyringSecrets)
    };
    if demo
        && mh_store::seed_demo(&store, chrono::Local::now().fixed_offset())
            .map_err(|e| e.message)?
    {
        tracing::info!("tanıtım verisi yüklendi");
    }

    let sink = Arc::new(TauriSink::new(handle.clone()));
    let sync = Arc::new(SyncManager::new(
        store.clone(),
        secrets.clone(),
        sink.clone(),
        rt.clone(),
    ));
    let outbox = Outbox::new(store.clone(), secrets.clone(), sync.clone(), sink);
    app.manage(AppState {
        store: store.clone(),
        secrets,
        sync: sync.clone(),
        outbox: outbox.clone(),
        mute: Mute::new(),
        notify_queue: Mutex::default(),
        compose_counter: AtomicU32::new(0),
        quitting: AtomicBool::new(false),
        cache_dir,
    });

    crate::windows::create_main(&handle)?;
    if let Err(e) = crate::tray::create(&handle) {
        tracing::warn!(error = %e, "tepsi simgesi oluşturulamadı");
    }
    outbox.start(&rt);
    crate::updater::start(&handle, &rt);
    sync.start_all().map_err(|e| e.message)?;
    rt.spawn(crate::sink::snooze_ticker(handle.clone()));
    crate::tray::refresh(&handle);

    // Tanıtım kipinde ekran görüntüsü otomasyonu: N saniye sonra örnek kart.
    let demo_card = std::env::var("MAILHERE_DEMO_NOTIFY")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok());
    if let (true, Some(secs)) = (demo, demo_card) {
        let app = handle.clone();
        rt.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            if let Err(e) = crate::commands::show_demo_card(&app) {
                tracing::warn!(error = %e.message, "tanıtım kartı açılamadı");
            }
        });
    }
    Ok(())
}
