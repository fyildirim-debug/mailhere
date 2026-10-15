//! Uygulama durumu ve açılış.
//!
//! [`Core`] Tauri bilmeyen çekirdektir (depo, eşitleyiciler, giden kutusu);
//! penceresiz çalışma (Android arka plan hizmeti, `crate::headless`) onu
//! Tauri'siz açar. [`setup_core`] bunu Tauri durumuna bağlar ve
//! zamanlayıcıları başlatır.

use std::path::PathBuf;
use std::sync::Arc;

use mh_core::{AppError, AppResult, ErrorCode, Millis};
use mh_jmap::JmapManager;
use mh_notify::Mute;
use mh_send::Outbox;
use mh_store::Store;
use mh_sync::{EventSink, SecretStore, SyncManager};
use tauri::{App, Manager};

use crate::platform::Platform;
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
    /// Açılan eklerin yazıldığı klasör.
    pub cache_dir: PathBuf,
    /// Kabuğa özgü işler (kart, tepsi, pencereler, dosya penceresi).
    pub platform: Arc<dyn Platform>,
    /// QR aktarımı: açık teklif ve uygulanmayı bekleyen paketler.
    pub transfer: crate::transfer::TransferHub,
    /// Bağlantıları baştan kurmanın sıklık sınırı (uyku, arka plandan dönüş).
    pub reconnect: crate::wake::Reconnect,
}

impl AppState {
    /// Açılmış çekirdekten durum: pencereli (`setup_core`) ve penceresiz
    /// (`crate::headless`) kurulum aynı alanları aynı biçimde doldurur.
    pub fn new(core: &Core, cache_dir: PathBuf, platform: Arc<dyn Platform>) -> Self {
        Self {
            store: core.store.clone(),
            secrets: core.secrets.clone(),
            sync: core.sync.clone(),
            jmap: core.jmap.clone(),
            outbox: core.outbox.clone(),
            mute: Mute::new(),
            cache_dir,
            platform,
            transfer: Default::default(),
            reconnect: Default::default(),
        }
    }
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
pub fn dev_env(name: &str) -> Option<String> {
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

/// Tanıtım ve deneme kipi: parolalar ve veritabanı anahtarı bellekte
/// (`MAILHERE_DEMO=1` ya da `MAILHERE_SECRETS=memory`). Kabuk gizli deposunu
/// buna göre seçer (`MemorySecrets` / anahtar zinciri).
pub fn memory_mode() -> bool {
    demo_mode() || dev_flag("MAILHERE_SECRETS", "memory")
}

const DB_FILE: &str = "mailhere.db";

/// Veritabanı ve önbellek klasörleri.
pub struct Dirs {
    pub data: PathBuf,
    pub cache: PathBuf,
    /// Önceki sürümlerin veritabanı yolu (Windows'ta dolaşan profil, Roaming).
    pub legacy_db: Option<PathBuf>,
}

impl Dirs {
    /// Veri ve önbellek tek klasör altında (`<data>/cache`), eski konum yok.
    pub fn under(data: PathBuf) -> Self {
        Self {
            cache: data.join("cache"),
            data,
            legacy_db: None,
        }
    }

    /// Veri klasörü: `MAILHERE_DATA_DIR`; bellek kipinde her açılışta yeni bir
    /// geçici klasör (anahtar süreçle kaybolur, gerçek veriye dokunulmaz);
    /// yoksa kabuğun verdiği işletim sistemi klasörleri (`os`).
    pub fn resolve<E>(memory: bool, os: impl FnOnce() -> Result<Self, E>) -> Result<Self, E> {
        if let Some(dir) = std::env::var_os("MAILHERE_DATA_DIR").filter(|d| !d.is_empty()) {
            return Ok(Self::under(PathBuf::from(dir)));
        }
        if memory {
            let data =
                std::env::temp_dir().join(format!("mailhere-bellek-{}", uuid::Uuid::new_v4()));
            return Ok(Self::under(data));
        }
        os()
    }
}

/// Veritabanı şifreleme anahtarının kasası: parolalarla aynı depo (anahtar
/// zinciri ya da bellek kipinde süreç belleği), kullanıcı adı `db-key`.
pub struct DbKeyVault<'a> {
    secrets: &'a dyn SecretStore,
    persistent: bool,
}

impl<'a> DbKeyVault<'a> {
    pub fn new(secrets: &'a dyn SecretStore, persistent: bool) -> Self {
        Self {
            secrets,
            persistent,
        }
    }
}

impl mh_store::KeyVault for DbKeyVault<'_> {
    fn load(&self) -> AppResult<Option<String>> {
        self.secrets.get(mh_store::DB_KEY_SECRET)
    }

    fn save(&self, secret: &str) -> AppResult<()> {
        self.secrets.set(mh_store::DB_KEY_SECRET, secret)
    }

    fn persistent(&self) -> bool {
        self.persistent
    }
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

/// Tauri'siz çekirdek: depo, eşitleyiciler ve giden kutusu. Olaylar verilen
/// `sink`e gider; pencere ya da platform bilmez.
pub struct Core {
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
    pub sync: Arc<SyncManager>,
    pub jmap: Arc<JmapManager>,
    pub outbox: Arc<Outbox>,
}

impl Core {
    /// Depoyu açar, onaylı sertifikaları yükler, tanıtım verisini (varsa)
    /// yazar ve yöneticileri kurar; hiçbir şeyi başlatmaz.
    pub fn open(
        dirs: &Dirs,
        secrets: Arc<dyn SecretStore>,
        sink: Arc<dyn EventSink>,
        rt: &tokio::runtime::Handle,
    ) -> AppResult<Self> {
        let memory = memory_mode();
        let store = Arc::new(open_store(dirs, secrets.as_ref(), memory)?);
        // Kullanıcının onayladığı sertifikalar ilk bağlantıdan önce yüklenir.
        mh_sync::tls::set_trusted(&store.settings()?.trusted_certs);
        if demo_mode() && mh_store::seed_demo(&store, chrono::Local::now().fixed_offset())? {
            tracing::info!("tanıtım verisi yüklendi");
        }
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
        Ok(Self {
            store,
            secrets,
            sync,
            jmap,
            outbox,
        })
    }

    /// Giden kutusunu ve tüm hesapların eşitlemesini başlatır.
    pub fn start(&self, rt: &tokio::runtime::Handle) -> AppResult<()> {
        self.outbox.start(rt);
        self.sync.start_all()?;
        self.jmap.start_all()
    }

    /// Tüm hesapların eşitleme görevlerini durdurur (IMAP IDLE, JMAP akışı).
    /// Giden kutusunun döngüsü çalışma ortamıyla birlikte kapanır; depo son
    /// `Arc` düşünce kapanır. Penceresiz çekirdek yerini pencereliye
    /// bırakırken kullanılır.
    pub fn stop(&self) {
        let accounts = match self.store.accounts() {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(error = %e.message, "hesaplar okunamadı; eşitleme durdurulamadı");
                return;
            }
        };
        for a in &accounts {
            self.sync.stop(&a.id);
            self.jmap.stop(&a.id);
        }
    }
}

/// Çekirdeği açar, `AppState` olarak Tauri durumuna bağlar ve
/// zamanlayıcıları (erteleme, hatırlatma, elek, önizleme onarımı) başlatır.
/// Kabuk bundan sonra pencerelerini açar; duruma `app.state::<AppState>()`
/// ile erişir.
///
/// Açılış başarısızsa `Platform::fatal` çağrılmıştır (hata iletişim kutusu
/// gösterilir, kapatılınca uygulama çıkar); kabuk `Err` alınca pencere
/// açmadan `Ok(())` dönmelidir — hiçbir dosya silinmez.
pub fn setup_core(
    app: &mut App,
    rt: tokio::runtime::Handle,
    dirs: Dirs,
    secrets: Arc<dyn SecretStore>,
    platform: Arc<dyn Platform>,
) -> AppResult<()> {
    let handle = app.handle().clone();
    // Ortak komutların yan durumları; açılış başarısız olsa da yönetilir.
    app.manage(crate::cmd_oauth::OAuthState::default());
    app.manage(crate::files::Picked::default());
    tracing::info!(dir = %dirs.data.display(), "veri klasörü");
    let sink = Arc::new(TauriSink::new(handle.clone()));
    let core = match Core::open(&dirs, secrets, sink, &rt) {
        Ok(core) => core,
        Err(e) => {
            tracing::error!(code = %e.code, error = %e.message, "Mailhere açılamadı");
            platform.fatal(&handle, &e);
            return Err(e);
        }
    };
    app.manage(AppState::new(&core, dirs.cache, platform.clone()));
    if let Err(e) = core.start(&rt) {
        tracing::error!(code = %e.code, error = %e.message, "eşitleme başlatılamadı");
        platform.fatal(&handle, &e);
        return Err(e);
    }
    rt.spawn(crate::sink::snooze_ticker(handle.clone()));
    // Uykudan dönüşte ölü IDLE bağlantıları beklenmeden yenilenir.
    rt.spawn(crate::wake::sleep_watch(handle.clone()));
    // Yanıt hatırlatmaları: açılışta kaçan süreler değerlendirilir, sonra dakikada bir.
    rt.spawn(crate::followups::ticker(handle.clone()));
    crate::screener::start(&handle, &rt);
    // Önceki sürümlerin kaydettiği HTML kaynaklı, şablon artıklı ya da
    // bağlantılı önizlemeler onarılır.
    {
        let (store, app) = (core.store.clone(), handle.clone());
        rt.spawn_blocking(move || match store.repair_snippets() {
            Ok(0) => {}
            Ok(n) => {
                tracing::info!(n, "önizlemeler onarıldı");
                crate::sink::changed(&app, Vec::new());
            }
            Err(e) => tracing::warn!(error = %e.message, "önizlemeler onarılamadı"),
        });
    }
    Ok(())
}
