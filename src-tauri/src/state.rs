//! Uygulama durumu ve açılış.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mh_core::{AppError, AppResult, ErrorCode, Millis, NotifyEvent};
use mh_jmap::JmapManager;
use mh_notify::Mute;
use mh_send::Outbox;
use mh_store::Store;
use mh_sync::{MemorySecrets, SecretStore, SyncManager};
use tauri::{App, AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::secrets::{DbKeyVault, KeyringSecrets};
use crate::sink::TauriSink;

pub struct AppState {
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
    pub sync: Arc<SyncManager>,
    /// JMAP hesaplarının eşitlemesi (IMAP motoru JMAP hesaplarını atlar).
    pub jmap: Arc<JmapManager>,
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

/// Geliştirme ve deneme anahtarı (`MAILHERE_DEMO`, `MAILHERE_SECRETS`,
/// `MAILHERE_DEMO_NOTIFY`). Yalnız geliştirme derlemesinde okunur; sürüm
/// derlemesi bunları sessizce yok sayar: gerçek veritabanına örnek veri
/// yazılmaz, parolalar ve veritabanı anahtarı belleğe düşmez.
fn dev_env(name: &str) -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var(name).ok()
    } else {
        None
    }
}

fn dev_flag(name: &str, value: &str) -> bool {
    dev_env(name).is_some_and(|v| v.trim().eq_ignore_ascii_case(value))
}

/// Tanıtım kipi (`MAILHERE_DEMO=1`, yalnız geliştirme derlemesi).
pub fn demo_mode() -> bool {
    dev_flag("MAILHERE_DEMO", "1")
}

const DB_FILE: &str = "mailhere.db";

/// Veritabanı ve önbellek klasörleri.
struct Dirs {
    data: PathBuf,
    cache: PathBuf,
    /// Önceki sürümlerin veritabanı yolu (Windows'ta dolaşan profil, Roaming).
    legacy_db: Option<PathBuf>,
}

/// Veri klasörü: `MAILHERE_DATA_DIR`; bellek kipinde her açılışta yeni bir
/// geçici klasör (anahtar süreçle kaybolur, gerçek veriye dokunulmaz);
/// yoksa işletim sisteminin *yerel* uygulama veri klasörü. Windows'ta
/// `app_data_dir` dolaşan profildir (Roaming): posta veritabanı profil
/// sunucusuna kopyalanmasın diye `app_local_data_dir` kullanılır. Linux ve
/// macOS'ta ikisi aynı klasördür.
fn dirs(app: &AppHandle, memory: bool) -> Result<Dirs, Box<dyn std::error::Error>> {
    if let Some(dir) = std::env::var_os("MAILHERE_DATA_DIR").filter(|d| !d.is_empty()) {
        let data = PathBuf::from(dir);
        return Ok(Dirs {
            cache: data.join("cache"),
            data,
            legacy_db: None,
        });
    }
    if memory {
        let data = std::env::temp_dir().join(format!("mailhere-bellek-{}", uuid::Uuid::new_v4()));
        return Ok(Dirs {
            cache: data.join("cache"),
            data,
            legacy_db: None,
        });
    }
    let data = app.path().app_local_data_dir()?;
    Ok(Dirs {
        // Windows'ta da yerel klasör (%LOCALAPPDATA%).
        cache: app
            .path()
            .app_cache_dir()
            .unwrap_or_else(|_| data.join("cache")),
        legacy_db: Some(app.path().app_data_dir()?.join(DB_FILE)),
        data,
    })
}

/// Veri klasörünü hazırlar, eski konumdaki veritabanını taşır ve şifreli
/// veritabanını açar (gerekirse eski şifresiz veritabanını dönüştürür).
fn open_store(dirs: &Dirs, secrets: &dyn SecretStore, memory: bool) -> AppResult<Store> {
    mh_store::private_dir(&dirs.data).map_err(|e| {
        tracing::error!(error = %e, "veri klasörü oluşturulamadı");
        AppError::new(
            ErrorCode::Storage,
            format!(
                "Veri klasörü ({}) oluşturulamadı. Klasörün yazılabilir olduğunu denetleyip Mailhere'yi yeniden başlat.",
                dirs.data.display()
            ),
        )
    })?;
    // Açılan ekler buraya yazılır; yalnız kullanıcıya açık olsun.
    if let Err(e) = mh_store::private_dir(&dirs.cache) {
        tracing::warn!(error = %e, "önbellek klasörü hazırlanamadı");
    }
    let db = dirs.data.join(DB_FILE);
    if let Some(legacy) = &dirs.legacy_db {
        mh_store::relocate(legacy, &db)?;
    }
    Store::open_with_vault(&db, &DbKeyVault::new(secrets, !memory))
}

/// Açılışta veritabanı açılamadı: pencere açılmadan sorunu ve çözümü anlatan
/// bir hata iletişim kutusu gösterilir, kapatılınca uygulama çıkar. (Kurulum
/// hatası Tauri'de paniğe döner; Windows'ta konsol olmadığından açıklamasız
/// bir çökme olurdu.) Hiçbir dosya silinmez.
fn startup_failed(app: &AppHandle, e: &AppError) {
    tracing::error!(code = %e.code, error = %e.message, "Mailhere açılamadı");
    let exit = app.clone();
    app.dialog()
        .message(e.message.clone())
        .title("Mailhere açılamadı")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCustom("Kapat".into()))
        .show(move |_| exit.exit(1));
}

pub fn setup(app: &mut App, rt: tokio::runtime::Handle) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let demo = demo_mode();
    // Tanıtım ve deneme kipi: parolalar ve veritabanı anahtarı bellekte.
    let memory = demo || dev_flag("MAILHERE_SECRETS", "memory");
    let secrets: Arc<dyn SecretStore> = if memory {
        tracing::info!("parolalar ve veritabanı anahtarı bellekte (tanıtım ya da test kipi)");
        Arc::new(MemorySecrets::new())
    } else {
        Arc::new(KeyringSecrets)
    };
    let dirs = dirs(&handle, memory)?;
    tracing::info!(dir = %dirs.data.display(), "veri klasörü");
    let store = match open_store(&dirs, secrets.as_ref(), memory) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            startup_failed(&handle, &e);
            return Ok(());
        }
    };
    let cache_dir = dirs.cache;

    // Kullanıcının onayladığı sertifikalar ilk bağlantıdan önce yüklenir.
    mh_sync::tls::set_trusted(&store.settings().map_err(|e| e.message)?.trusted_certs);
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
    let jmap = Arc::new(JmapManager::new(
        store.clone(),
        secrets.clone(),
        sink.clone(),
        rt.clone(),
    ));
    let outbox = Outbox::new(store.clone(), secrets.clone(), sync.clone(), sink);
    outbox.set_jmap(jmap.clone());
    app.manage(AppState {
        store: store.clone(),
        secrets,
        sync: sync.clone(),
        jmap: jmap.clone(),
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
    jmap.start_all().map_err(|e| e.message)?;
    rt.spawn(crate::sink::snooze_ticker(handle.clone()));
    // Yanıt hatırlatmaları: açılışta kaçan süreler değerlendirilir, sonra dakikada bir.
    rt.spawn(crate::followups::ticker(handle.clone()));
    crate::screener::start(&handle, &rt);
    // Önceki sürümlerde HTML kaynağıyla kaydedilmiş önizlemeler onarılır.
    {
        let (store, app) = (store.clone(), handle.clone());
        rt.spawn_blocking(move || match store.repair_html_snippets() {
            Ok(0) => {}
            Ok(n) => {
                tracing::info!(n, "HTML kaynaklı önizlemeler onarıldı");
                crate::sink::changed(&app, Vec::new());
            }
            Err(e) => tracing::warn!(error = %e.message, "önizlemeler onarılamadı"),
        });
    }
    crate::tray::refresh(&handle);

    // Tanıtım kipinde ekran görüntüsü otomasyonu: N saniye sonra örnek kart.
    let demo_card = dev_env("MAILHERE_DEMO_NOTIFY").and_then(|v| v.trim().parse::<u64>().ok());
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
