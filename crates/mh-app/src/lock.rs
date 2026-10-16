//! Parola kilidi (isteğe bağlı): durum, komutlar ve komut kapısı.
//!
//! Varsayılan kapalıdır; kapalıyken hiçbir akışta parola sorulmaz ve bu
//! modül yalnız "kilit yok" der. Açıkken veritabanı anahtarı anahtar
//! zincirinde parolayla sarılı durur (`mh_store::lock`, kayıt
//! `db-key-kilitli`; düz `db-key` silinir):
//!
//! - **Kilitli açılış.** Kabuk açılışta [`prepare`] çağırır: kayıt varsa
//!   çekirdek (veritabanı, eşitleyiciler) **açılmaz**, açılış bilgisi
//!   ([`Boot`]) bekletilir ve [`LockState`] yönetilir; arayüz kilit ekranını
//!   gösterir. `lock_unlock` doğru parolayla anahtarı açar ve çekirdeği
//!   `setup_core` ile başlatır; uygulama her zamanki gibi sürer.
//! - **Arayüz kilidi.** Çekirdek açıkken pencere gizlenip yeniden açılınca
//!   (masaüstü, [`window_hidden`]) ya da uygulama arka plandan 1 dakikadan
//!   uzun sürede dönünce (telefon, arayüz `lock_ui_lock` çağırır) yalnız
//!   arayüz kilitlenir: anahtar bellekte kalır, eşitleme ve bildirimler
//!   durmaz (ürün ilkesi: posta hızlı gelir). Tercih `relock` kayıttadır
//!   (varsayılan açık).
//! - **Komut kapısı.** [`gate`] kabuğun `invoke_handler`ını sarar: çekirdek
//!   kapalıyken `lock_*` dışındaki her komut, arayüz kilitliyken içerik
//!   taşıyabilecek her komut `locked` hatasıyla reddedilir (panik yok).
//! - **Yanlış parola.** Sayılır; beşinci hatadan sonra üstel bekleme
//!   ([`retry_delay`]). Süreç belleğindedir.
//! - **Unutulan parola.** `lock_reset_local`: kayıt, hesapların anahtar
//!   zincirindeki gizli değerleri (adları kayıtta tutulur) ve yerel
//!   veritabanı silinir; uygulama ilk açılış ekranına döner. Sunucudaki
//!   postalar etkilenmez. Çekirdek açıkken (yalnız arayüz kilitliyken)
//!   veritabanı kapatılamadığından bir işaret bırakılır ve uygulama
//!   yeniden başlar; işaret bir sonraki açılışta ([`prepare`] ya da
//!   etkinliksiz çekirdek, [`headless_check`]) uygulanır.
//!
//! Parola ve anahtar bellekte `Zeroizing` ile tutulur; günlüğe, hata
//! metnine ya da olaylara yazılmaz. Serde'nin komut argümanını çözerken
//! tuttuğu geçici kopya sıfırlanamaz (bilinen sınır).

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use mh_core::{
    events, AppError, AppResult, ErrorCode, LockStatus, LOCK_PASSWORD_MAX, LOCK_PASSWORD_MIN,
};
pub use mh_store::KdfParams;
use mh_store::{DbKey, LockRecord, UnwrapError, DB_KEY_SECRET, LOCK_RECORD_SECRET};
use mh_sync::SecretStore;
use serde::{Deserialize, Serialize};
use tauri::ipc::Invoke;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use zeroize::Zeroizing;

use crate::state::{internal, memory_mode, AppState, Dirs, DB_FILE};
use crate::Platform;

type Res<T> = Result<T, AppError>;

/// Çekirdeği açmak için gereken her şey; kilitli açılışta parola girilene
/// kadar [`LockState`] içinde bekler.
pub struct Boot {
    pub rt: tokio::runtime::Handle,
    pub dirs: Dirs,
    pub secrets: Arc<dyn SecretStore>,
    pub platform: Arc<dyn Platform>,
    /// Bu cihaz için Argon2id parametreleri (masaüstü / telefon).
    pub kdf: KdfParams,
}

/// Yerel veri sıfırlama işareti (veri klasöründe): çekirdek açıkken
/// bırakılır, bir sonraki açılışta uygulanır.
const WIPE_MARKER: &str = "sifirla.json";

/// Hatalı denemelerin bu kadarı beklemesiz; sonrası üstel.
const FREE_ATTEMPTS: u32 = 5;
const MAX_WAIT: Duration = Duration::from_secs(300);

/// Arayüz kilitliyken (çekirdek açık) yine izinli ortak komutlar: içerik
/// taşımazlar; pencere düzeni, eşitleme tetikleme, mobil kabuğun sistem
/// işleri. Kabuğa özgü adlar da burada (ortak kapı tek yerden okur).
const UI_LOCKED_OK: &[&str] = &[
    "notify_hide",
    "notify_layout",
    "notify_focus",
    "open_main",
    "sync_now",
    "sync_reconnect",
    "system_bars",
    "haptic",
    "notification_status",
    "notification_permission",
    "background_mode_get",
    "battery_status",
];

#[derive(Serialize, Deserialize, Default)]
struct WipeMarker {
    secrets: Vec<String>,
}

struct Inner {
    enabled: bool,
    /// Çekirdek kapalı; parola bekleniyor.
    locked: bool,
    /// Çekirdek açık, yalnız arayüz kilitli.
    ui_locked: bool,
    relock: bool,
    failures: u32,
    retry_after: Option<Instant>,
    /// Kilitli açılışta bekleyen açılış bilgisi.
    boot: Option<Boot>,
    /// Bir kilit işlemi (KDF) sürüyor; ikincisi beklemeli.
    busy: bool,
}

/// Parola kilidinin süreç içi durumu (`app.manage`); çekirdek kapalıyken
/// de vardır.
pub struct LockState {
    inner: Mutex<Inner>,
    secrets: Arc<dyn SecretStore>,
    dirs: Dirs,
    kdf: KdfParams,
    platform: Arc<dyn Platform>,
}

/// Süren işlemin bayrağı; düşerken kalkar.
struct Busy<'a>(&'a LockState);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.lock().busy = false;
    }
}

impl LockState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn busy(&self) -> AppResult<Busy<'_>> {
        let mut g = self.lock();
        if g.busy {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Bir kilit işlemi sürüyor; bitmesini bekleyip yeniden dene.",
            ));
        }
        g.busy = true;
        Ok(Busy(self))
    }

    pub fn status(&self) -> LockStatus {
        let g = self.lock();
        LockStatus {
            enabled: g.enabled,
            locked: g.locked,
            ui_locked: g.ui_locked,
            relock_on_open: g.relock,
            retry_after_ms: g
                .retry_after
                .map(|t| t.saturating_duration_since(Instant::now()).as_millis() as u64)
                .unwrap_or(0),
        }
    }

    /// Bekleme süresi dolmadıysa hata (kalan süreyle).
    fn check_retry(&self) -> AppResult<()> {
        let g = self.lock();
        if let Some(t) = g.retry_after {
            let left = t.saturating_duration_since(Instant::now());
            if !left.is_zero() {
                return Err(wait_error(left));
            }
        }
        Ok(())
    }

    /// Yanlış parola: sayaç artar, bekleme kurulur.
    fn failed(&self) -> Duration {
        let mut g = self.lock();
        g.failures += 1;
        let d = retry_delay(g.failures);
        g.retry_after = (!d.is_zero()).then(|| Instant::now() + d);
        d
    }

    fn succeeded(&self) {
        let mut g = self.lock();
        g.failures = 0;
        g.retry_after = None;
    }
}

// ------------------------------------------------------------------ yardımcılar

/// Beşinci hatadan sonra üstel bekleme: 2, 4, 8 … saniye, en çok 5 dakika.
pub fn retry_delay(failures: u32) -> Duration {
    if failures <= FREE_ATTEMPTS {
        return Duration::ZERO;
    }
    let secs = 2u64.saturating_pow((failures - FREE_ATTEMPTS).min(20));
    Duration::from_secs(secs).min(MAX_WAIT)
}

/// Komut bu durumda çalışabilir mi: `lock_*` her zaman; çekirdek kapalıyken
/// başka hiçbiri; arayüz kilitliyken yalnız içeriksiz liste.
pub fn allows(cmd: &str, core_open: bool, ui_locked: bool) -> bool {
    if cmd.starts_with("lock_") {
        return true;
    }
    if !core_open {
        return false;
    }
    if ui_locked {
        return UI_LOCKED_OK.contains(&cmd);
    }
    true
}

fn locked_error() -> AppError {
    AppError::new(
        ErrorCode::Locked,
        "Mailhere kilitli. Devam etmek için parolanı gir.",
    )
}

fn wait_error(left: Duration) -> AppError {
    let secs = left.as_secs().max(1);
    AppError::new(
        ErrorCode::Locked,
        format!("Çok sayıda yanlış deneme. {secs} saniye sonra yeniden deneyebilirsin."),
    )
}

fn wrong_password(delay: Duration) -> AppError {
    let mut msg = String::from("Parola yanlış. Yeniden dene.");
    if !delay.is_zero() {
        msg.push_str(&format!(
            " Sonraki deneme için {} saniye bekle.",
            delay.as_secs().max(1)
        ));
    }
    AppError::new(ErrorCode::Auth, msg)
}

fn not_enabled() -> AppError {
    AppError::new(ErrorCode::Invalid, "Parola kilidi açık değil.")
}

fn record_missing() -> AppError {
    AppError::new(
        ErrorCode::Keyring,
        "Parola kilidi kaydı anahtar zincirinde bulunamadı. Anahtar zinciri hizmetini denetleyip Mailhere'yi yeniden başlat.",
    )
}

/// Parola kuralları: en az 8 karakter, en çok 256 bayt. Baştaki ve sondaki
/// boşluklar atılmaz (parolanın parçasıdır).
pub fn check_password(p: &str) -> AppResult<()> {
    if p.chars().count() < LOCK_PASSWORD_MIN {
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!("Parola en az {LOCK_PASSWORD_MIN} karakter olmalı."),
        ));
    }
    if p.len() > LOCK_PASSWORD_MAX {
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!("Parola en çok {LOCK_PASSWORD_MAX} bayt olabilir."),
        ));
    }
    Ok(())
}

/// Anahtar zincirindeki kilit kaydı; yoksa `None`, bozuksa `Err`.
fn read_record(secrets: &dyn SecretStore) -> AppResult<Option<LockRecord>> {
    match secrets.get(LOCK_RECORD_SECRET)? {
        Some(json) => LockRecord::parse(&json).map(Some),
        None => Ok(None),
    }
}

/// Kaydı yazar ve geri okuyarak saklandığını doğrular (bazı anahtar zinciri
/// arka uçları yazmayı kabul edip saklamaz).
fn write_record(secrets: &dyn SecretStore, rec: &LockRecord) -> AppResult<()> {
    let json = rec.to_json();
    secrets.set(LOCK_RECORD_SECRET, &json)?;
    match secrets.get(LOCK_RECORD_SECRET)? {
        Some(back) if back == json => Ok(()),
        _ => {
            tracing::error!("kilit kaydı anahtar zincirinden geri okunamadı");
            Err(AppError::new(
                ErrorCode::Keyring,
                "Kilit kaydı anahtar zincirine yazılamadı. Anahtar zinciri hizmetini denetleyip yeniden dene.",
            ))
        }
    }
}

/// Hesapların anahtar zincirindeki gizli değer adları (parola ve OAuth
/// jetonu; her hesap için ikisi de denenir, olmayanı silmek hata değildir).
fn secret_names(state: &AppState) -> Vec<String> {
    state
        .store
        .accounts()
        .unwrap_or_default()
        .iter()
        .flat_map(|a| [a.id.clone(), mh_sync::oauth_secret_key(&a.id)])
        .collect()
}

fn emit(app: &AppHandle, status: LockStatus) {
    if let Err(e) = app.emit(events::LOCK, status) {
        tracing::warn!(error = %e, "kilit olayı gönderilemedi");
    }
}

fn lock_state(app: &AppHandle) -> AppResult<tauri::State<'_, LockState>> {
    app.try_state::<LockState>().ok_or_else(|| {
        tracing::error!("kilit durumu kurulmamış (prepare çağrılmadı)");
        internal("kilit durumu yok")
    })
}

// -------------------------------------------------------------- yerel veri

/// Yerel veriyi siler: hesapların gizli değerleri, kilit kaydı, düz anahtar,
/// veritabanı (tüm yan dosyalarıyla), açılan ve seçilen ekler, işaret.
fn wipe_local(dirs: &Dirs, secrets: &dyn SecretStore, names: &[String]) -> AppResult<()> {
    for n in names {
        if let Err(e) = secrets.delete(n) {
            tracing::warn!(error = %e.message, "hesabın gizli değeri silinemedi");
        }
    }
    let _ = secrets.delete(LOCK_RECORD_SECRET);
    let _ = secrets.delete(DB_KEY_SECRET);
    let db = dirs.data.join(DB_FILE);
    mh_store::remove_database(&db).map_err(|e| {
        tracing::error!(error = %e, "veritabanı silinemedi");
        AppError::new(
            ErrorCode::Storage,
            format!(
                "Yerel veritabanı ({}) silinemedi. Dosyanın başka bir program tarafından açık olmadığını denetleyip yeniden dene.",
                db.display()
            ),
        )
    })?;
    for sub in ["ekler", "giden"] {
        let _ = std::fs::remove_dir_all(dirs.cache.join(sub));
    }
    let _ = std::fs::remove_file(dirs.data.join(WIPE_MARKER));
    tracing::info!("yerel veri sıfırlandı");
    Ok(())
}

fn write_wipe_marker(dirs: &Dirs, names: &[String]) -> AppResult<()> {
    let marker = WipeMarker {
        secrets: names.to_vec(),
    };
    let json = serde_json::to_string(&marker).map_err(internal)?;
    std::fs::write(dirs.data.join(WIPE_MARKER), json).map_err(|e| {
        tracing::error!(error = %e, "sıfırlama işareti yazılamadı");
        AppError::new(
            ErrorCode::Storage,
            "Sıfırlama işareti yazılamadı. Veri klasörünün yazılabilir olduğunu denetle.",
        )
    })
}

/// Bir önceki oturumdan kalan sıfırlama işareti varsa uygular.
pub fn apply_pending_wipe(dirs: &Dirs, secrets: &dyn SecretStore) -> AppResult<()> {
    let path = dirs.data.join(WIPE_MARKER);
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            tracing::warn!(error = %e, "sıfırlama işareti okunamadı");
            return Ok(());
        }
    };
    let marker: WipeMarker = serde_json::from_str(&json).unwrap_or_default();
    tracing::warn!("bekleyen sıfırlama uygulanıyor (parola unutuldu)");
    wipe_local(dirs, secrets, &marker.secrets)
}

// ------------------------------------------------------------------- açılış

/// Açılışın başı: bekleyen sıfırlama uygulanır, kilit kaydına bakılır ve
/// [`LockState`] yönetilir. Kayıt yoksa `Some(boot)` döner, kabuk
/// `setup_core` çağırır. Kayıt varsa `boot` bekletilir ve `None` döner:
/// çekirdek `lock_unlock` ile açılacaktır; kabuk pencereyi (kilit ekranı)
/// açar. `Err` yalnız anahtar zincirine ulaşılamazsa.
pub fn prepare(app: &AppHandle, boot: Boot) -> AppResult<Option<Boot>> {
    if !memory_mode() {
        apply_pending_wipe(&boot.dirs, boot.secrets.as_ref())?;
    }
    let record = boot.secrets.get(LOCK_RECORD_SECRET)?;
    let locked = record.is_some();
    // Bozuk kayıt da kilit sayılır: kilit ekranı hatayı ve sıfırlama yolunu gösterir.
    let relock = record
        .as_deref()
        .map(|json| LockRecord::parse(json).map(|r| r.relock).unwrap_or(true))
        .unwrap_or(true);
    if locked && boot.secrets.get(DB_KEY_SECRET)?.is_some() {
        // Kilit açılırken düz anahtar silinememişti; şimdi silinir.
        tracing::warn!("düz veritabanı anahtarı kilitle birlikte duruyordu; siliniyor");
        if let Err(e) = boot.secrets.delete(DB_KEY_SECRET) {
            tracing::warn!(error = %e.message, "düz anahtar silinemedi");
        }
    }
    let state = LockState {
        inner: Mutex::new(Inner {
            enabled: locked,
            locked,
            ui_locked: false,
            relock,
            failures: 0,
            retry_after: None,
            boot: None,
            busy: false,
        }),
        secrets: boot.secrets.clone(),
        dirs: boot.dirs.clone(),
        kdf: boot.kdf,
        platform: boot.platform.clone(),
    };
    if locked {
        state.lock().boot = Some(boot);
        app.manage(state);
        tracing::info!("parola kilidi açık; çekirdek parola bekliyor");
        return Ok(None);
    }
    app.manage(state);
    Ok(Some(boot))
}

/// Etkinliksiz çekirdek (Android) açılırken: bekleyen sıfırlama uygulanır;
/// düz anahtar yoksa kilit kaydı varsa `Locked`, yoksa `Keyring` hatası.
pub fn headless_check(dirs: &Dirs, secrets: &dyn SecretStore) -> AppResult<()> {
    apply_pending_wipe(dirs, secrets)?;
    if secrets.get(DB_KEY_SECRET)?.is_some() {
        return Ok(());
    }
    if secrets.get(LOCK_RECORD_SECRET)?.is_some() {
        return Err(AppError::new(
            ErrorCode::Locked,
            "Mailhere kilitli: postaları almak için uygulamayı açıp parolanı gir.",
        ));
    }
    Err(AppError::new(
        ErrorCode::Keyring,
        "Veritabanı anahtarı yok; Mailhere önce uygulamadan açılmalı.",
    ))
}

/// `setup_core` bitti: çekirdek açık, arayüz açık.
pub fn core_opened(app: &AppHandle) {
    let Some(lock) = app.try_state::<LockState>() else {
        return;
    };
    {
        let mut g = lock.lock();
        g.locked = false;
        g.ui_locked = false;
        g.boot = None;
    }
    emit(app, lock.status());
}

/// Ana pencere gizlendi (masaüstü): tercih açıksa arayüz kilitlenir;
/// çekirdek ve bildirimler sürer.
pub fn window_hidden(app: &AppHandle) {
    let Some(lock) = app.try_state::<LockState>() else {
        return;
    };
    {
        let mut g = lock.lock();
        if !(g.enabled && g.relock) || g.locked || g.ui_locked {
            return;
        }
        g.ui_locked = true;
    }
    tracing::info!("pencere gizlendi; arayüz kilitlendi");
    emit(app, lock.status());
}

pub fn status(app: &AppHandle) -> LockStatus {
    app.try_state::<LockState>()
        .map(|l| l.status())
        .unwrap_or_default()
}

/// Çekirdek kapalı, parola bekleniyor.
pub fn is_locked(app: &AppHandle) -> bool {
    status(app).locked
}

/// Yalnız arayüz kilitli (çekirdek açık).
pub fn ui_locked(app: &AppHandle) -> bool {
    status(app).ui_locked
}

/// Hesap eklendi ya da kaldırıldı: kilit açıksa kayıttaki gizli değer adları
/// yenilenir (parola unutulup veri sıfırlanınca hepsi silinebilsin).
pub fn accounts_changed(app: &AppHandle) {
    let Some(lock) = app.try_state::<LockState>() else {
        return;
    };
    if !lock.lock().enabled {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let names = secret_names(&state);
    let result = (|| -> AppResult<()> {
        let Some(mut rec) = read_record(lock.secrets.as_ref())? else {
            return Ok(());
        };
        if rec.secrets != names {
            rec.secrets = names;
            write_record(lock.secrets.as_ref(), &rec)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!(error = %e.message, "kilit kaydındaki hesap adları yenilenemedi");
    }
}

/// Kabuğun `invoke_handler`ını sarar: kilitliyken izinsiz komut `locked`
/// hatasıyla reddedilir, komut gövdesi hiç çalışmaz.
///
/// ```ignore
/// .invoke_handler(mh_app::lock::gate(mh_app::shared_commands![…]))
/// ```
pub fn gate<R, F>(inner: F) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static
where
    R: Runtime,
    F: Fn(Invoke<R>) -> bool + Send + Sync + 'static,
{
    move |invoke: Invoke<R>| {
        let denied = {
            let webview = invoke.message.webview_ref();
            match webview.try_state::<LockState>() {
                Some(lock) => {
                    let core_open = webview.try_state::<AppState>().is_some();
                    let ui_locked = lock.lock().ui_locked;
                    !allows(invoke.message.command(), core_open, ui_locked)
                }
                None => false,
            }
        };
        if denied {
            tracing::debug!(
                command = invoke.message.command(),
                "kilitliyken komut reddedildi"
            );
            invoke.resolver.reject(locked_error());
            return true;
        }
        inner(invoke)
    }
}

// ----------------------------------------------------------------- komutlar

#[tauri::command]
pub fn lock_status(app: AppHandle) -> Res<LockStatus> {
    Ok(lock_state(&app)?.status())
}

/// Kilidi açar (etkinleştirir): düz anahtar parolayla sarılır, kayıt yazılıp
/// doğrulanır, düz anahtar silinir. Yalnız çekirdek açıkken.
#[tauri::command]
pub async fn lock_enable(app: AppHandle, password: String) -> Res<LockStatus> {
    let password = Zeroizing::new(password);
    check_password(&password)?;
    let lock = lock_state(&app)?;
    let state = app.try_state::<AppState>().ok_or_else(locked_error)?;
    if lock.lock().enabled {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Parola kilidi zaten açık. Parolayı değiştirmek için \"Parolayı değiştir\"i kullan.",
        ));
    }
    let _busy = lock.busy()?;
    let names = secret_names(&state);
    let secrets = lock.secrets.clone();
    let kdf = lock.kdf;
    tokio::task::spawn_blocking(move || -> AppResult<()> {
        let plain = secrets.get(DB_KEY_SECRET)?.ok_or_else(|| {
            AppError::new(
                ErrorCode::Keyring,
                "Veritabanı anahtarı anahtar zincirinde bulunamadı; kilit açılamadı. Mailhere'yi yeniden başlatıp dene.",
            )
        })?;
        let key = DbKey::from_secret(plain)?;
        let mut rec = mh_store::lock::wrap(&key, password.as_bytes(), kdf, true)?;
        rec.secrets = names;
        write_record(secrets.as_ref(), &rec)?;
        // Kayıt gerçekten açılabiliyor mu: aynı parolayla geri açılır.
        let back = read_record(secrets.as_ref())?.ok_or_else(record_missing)?;
        match mh_store::lock::unwrap(&back, password.as_bytes()) {
            Ok(k) if k == key => {}
            _ => {
                let _ = secrets.delete(LOCK_RECORD_SECRET);
                tracing::error!("yazılan kilit kaydı açılamadı; kilit kurulmadı");
                return Err(AppError::new(
                    ErrorCode::Keyring,
                    "Kilit kaydı doğrulanamadı; kilit kurulmadı. Yeniden dene.",
                ));
            }
        }
        // Düz anahtar artık gereksiz. Silinemezse kilit yine etkindir
        // (kayıt var); bir sonraki açılış siler.
        if let Err(e) = secrets.delete(DB_KEY_SECRET) {
            tracing::warn!(error = %e.message, "düz veritabanı anahtarı silinemedi");
        }
        Ok(())
    })
    .await
    .map_err(internal)??;
    {
        let mut g = lock.lock();
        g.enabled = true;
        g.relock = true;
        g.locked = false;
        g.ui_locked = false;
        g.failures = 0;
        g.retry_after = None;
    }
    tracing::info!("parola kilidi kuruldu");
    let status = lock.status();
    emit(&app, status.clone());
    Ok(status)
}

/// Kilidi kapatır: parola doğrulanır, düz anahtar geri yazılır, kayıt silinir.
#[tauri::command]
pub async fn lock_disable(app: AppHandle, password: String) -> Res<LockStatus> {
    let password = Zeroizing::new(password);
    let lock = lock_state(&app)?;
    if app.try_state::<AppState>().is_none() {
        return Err(locked_error());
    }
    if !lock.lock().enabled {
        return Err(not_enabled());
    }
    lock.check_retry()?;
    let _busy = lock.busy()?;
    let secrets = lock.secrets.clone();
    let result = tokio::task::spawn_blocking(move || -> AppResult<Result<(), UnwrapError>> {
        let rec = read_record(secrets.as_ref())?.ok_or_else(record_missing)?;
        let key = match mh_store::lock::unwrap(&rec, password.as_bytes()) {
            Ok(k) => k,
            Err(e) => return Ok(Err(e)),
        };
        let hex = key.to_secret();
        secrets.set(DB_KEY_SECRET, &hex)?;
        let back = secrets
            .get(DB_KEY_SECRET)?
            .and_then(|b| DbKey::from_secret(b).ok());
        match back {
            Some(k) if k == key => {}
            _ => {
                let _ = secrets.delete(DB_KEY_SECRET);
                tracing::error!("düz anahtar anahtar zincirinden geri okunamadı; kilit kapatılmadı");
                return Err(AppError::new(
                    ErrorCode::Keyring,
                    "Anahtar zincirine yazılamadı; kilit kapatılmadı. Anahtar zinciri hizmetini denetleyip yeniden dene.",
                ));
            }
        }
        secrets.delete(LOCK_RECORD_SECRET)?;
        Ok(Ok(()))
    })
    .await
    .map_err(internal)??;
    match result {
        Err(UnwrapError::WrongPassword) => {
            let d = lock.failed();
            emit(&app, lock.status());
            Err(wrong_password(d))
        }
        Err(UnwrapError::Corrupt(e)) => Err(e),
        Ok(()) => {
            {
                let mut g = lock.lock();
                g.enabled = false;
                g.relock = true;
                g.locked = false;
                g.ui_locked = false;
            }
            lock.succeeded();
            tracing::info!("parola kilidi kapatıldı");
            let status = lock.status();
            emit(&app, status.clone());
            Ok(status)
        }
    }
}

/// Parolayı değiştirir: mevcut parola doğrulanır, anahtar yeni parolayla
/// (yeni tuz ve nonce) sarılır.
#[tauri::command]
pub async fn lock_change(app: AppHandle, current: String, next: String) -> Res<LockStatus> {
    let current = Zeroizing::new(current);
    let next = Zeroizing::new(next);
    check_password(&next)?;
    let lock = lock_state(&app)?;
    if app.try_state::<AppState>().is_none() {
        return Err(locked_error());
    }
    if !lock.lock().enabled {
        return Err(not_enabled());
    }
    lock.check_retry()?;
    let _busy = lock.busy()?;
    let secrets = lock.secrets.clone();
    let kdf = lock.kdf;
    let result = tokio::task::spawn_blocking(move || -> AppResult<Result<(), UnwrapError>> {
        let old = read_record(secrets.as_ref())?.ok_or_else(record_missing)?;
        let key = match mh_store::lock::unwrap(&old, current.as_bytes()) {
            Ok(k) => k,
            Err(e) => return Ok(Err(e)),
        };
        let mut rec = mh_store::lock::wrap(&key, next.as_bytes(), kdf, old.relock)?;
        rec.secrets = old.secrets.clone();
        write_record(secrets.as_ref(), &rec)?;
        let back = read_record(secrets.as_ref())?.ok_or_else(record_missing)?;
        match mh_store::lock::unwrap(&back, next.as_bytes()) {
            Ok(k) if k == key => Ok(Ok(())),
            _ => {
                // Eski kayıt geri konur; parola değişmemiş olur.
                write_record(secrets.as_ref(), &old)?;
                tracing::error!("yeni kilit kaydı açılamadı; eski parola korundu");
                Err(AppError::new(
                    ErrorCode::Keyring,
                    "Yeni kayıt doğrulanamadı; parola değişmedi. Yeniden dene.",
                ))
            }
        }
    })
    .await
    .map_err(internal)??;
    match result {
        Err(UnwrapError::WrongPassword) => {
            let d = lock.failed();
            emit(&app, lock.status());
            Err(wrong_password(d))
        }
        Err(UnwrapError::Corrupt(e)) => Err(e),
        Ok(()) => {
            lock.succeeded();
            tracing::info!("kilit parolası değiştirildi");
            Ok(lock.status())
        }
    }
}

/// Kilidi açar: parola doğruysa çekirdek kapalıysa açılır (`setup_core`),
/// arayüz kilidi kalkar. Yanlış parola sayılır ve bekleme kurar.
#[tauri::command]
pub async fn lock_unlock(app: AppHandle, password: String) -> Res<LockStatus> {
    let password = Zeroizing::new(password);
    let lock = lock_state(&app)?;
    if !lock.lock().enabled {
        return Err(not_enabled());
    }
    lock.check_retry()?;
    let _busy = lock.busy()?;
    let secrets = lock.secrets.clone();
    let result = tokio::task::spawn_blocking(move || -> AppResult<Result<DbKey, UnwrapError>> {
        let rec = read_record(secrets.as_ref())?.ok_or_else(record_missing)?;
        Ok(mh_store::lock::unwrap(&rec, password.as_bytes()))
    })
    .await
    .map_err(internal)??;
    let key = match result {
        Err(UnwrapError::WrongPassword) => {
            let d = lock.failed();
            tracing::info!(failures = lock.lock().failures, "yanlış kilit parolası");
            emit(&app, lock.status());
            return Err(wrong_password(d));
        }
        Err(UnwrapError::Corrupt(e)) => return Err(e),
        Ok(k) => k,
    };
    lock.succeeded();
    let boot = lock.lock().boot.take();
    if let Some(boot) = boot {
        // Çekirdek kapalıydı: şimdi açılır. Başarısızsa `fatal` gösterildi.
        let app2 = app.clone();
        let key2 = key.clone();
        tokio::task::spawn_blocking(move || crate::state::setup_core(&app2, boot, Some(&key2)))
            .await
            .map_err(internal)??;
        // Kilit açılırken silinememiş düz anahtar kaldıysa şimdi silinir.
        let secrets = lock.secrets.clone();
        if secrets.get(DB_KEY_SECRET).ok().flatten().is_some() {
            let _ = secrets.delete(DB_KEY_SECRET);
        }
        tracing::info!("kilit açıldı; çekirdek başladı");
    } else {
        tracing::info!("arayüz kilidi açıldı");
    }
    drop(key);
    {
        let mut g = lock.lock();
        g.locked = false;
        g.ui_locked = false;
    }
    let status = lock.status();
    emit(&app, status.clone());
    Ok(status)
}

/// Arayüzü kilitler (telefon arka plandan 1 dk'dan uzun sürede döndü);
/// çekirdek sürer. Kilit kapalıysa bir şey yapmaz.
#[tauri::command]
pub fn lock_ui_lock(app: AppHandle) -> Res<LockStatus> {
    let lock = lock_state(&app)?;
    let changed = {
        let mut g = lock.lock();
        if g.enabled && !g.locked && !g.ui_locked && app.try_state::<AppState>().is_some() {
            g.ui_locked = true;
            true
        } else {
            false
        }
    };
    let status = lock.status();
    if changed {
        tracing::info!("arayüz kilitlendi");
        emit(&app, status.clone());
    }
    Ok(status)
}

/// "Pencereyi her açışta parola sor" tercihi (kayıtta saklanır).
#[tauri::command]
pub async fn lock_set_relock(app: AppHandle, on: bool) -> Res<LockStatus> {
    let lock = lock_state(&app)?;
    if !lock.lock().enabled {
        return Err(not_enabled());
    }
    let _busy = lock.busy()?;
    let secrets = lock.secrets.clone();
    tokio::task::spawn_blocking(move || -> AppResult<()> {
        let mut rec = read_record(secrets.as_ref())?.ok_or_else(record_missing)?;
        if rec.relock != on {
            rec.relock = on;
            write_record(secrets.as_ref(), &rec)?;
        }
        Ok(())
    })
    .await
    .map_err(internal)??;
    lock.lock().relock = on;
    let status = lock.status();
    emit(&app, status.clone());
    Ok(status)
}

/// "Parolamı unuttum": yerel veritabanı, kilit kaydı ve hesapların gizli
/// değerleri silinir; uygulama ilk açılış ekranına döner. Sunucudaki
/// postalar etkilenmez. Çekirdek açıkken işaret bırakılır ve uygulama
/// yeniden başlar.
#[tauri::command]
pub async fn lock_reset_local(app: AppHandle) -> Res<LockStatus> {
    let lock = lock_state(&app)?;
    if !lock.lock().enabled {
        return Err(not_enabled());
    }
    let _busy = lock.busy()?;
    if let Some(state) = app.try_state::<AppState>() {
        // Çekirdek açık: açık veritabanı silinemez; işaret + yeniden başlatma.
        let names = secret_names(&state);
        let dirs = lock.dirs.clone();
        tokio::task::spawn_blocking(move || write_wipe_marker(&dirs, &names))
            .await
            .map_err(internal)??;
        tracing::warn!("yerel veri sıfırlanacak; uygulama yeniden başlıyor");
        lock.platform.restart(&app);
        return Ok(lock.status());
    }
    // Çekirdek kapalı: hemen silinir, boş çekirdek açılır.
    let secrets = lock.secrets.clone();
    let dirs = lock.dirs.clone();
    tokio::task::spawn_blocking(move || -> AppResult<()> {
        let names = read_record(secrets.as_ref())
            .ok()
            .flatten()
            .map(|r| r.secrets)
            .unwrap_or_default();
        wipe_local(&dirs, secrets.as_ref(), &names)
    })
    .await
    .map_err(internal)??;
    let boot = lock
        .lock()
        .boot
        .take()
        .ok_or_else(|| internal("açılış bilgisi yok"))?;
    {
        let mut g = lock.lock();
        g.enabled = false;
        g.relock = true;
        g.failures = 0;
        g.retry_after = None;
    }
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || crate::state::setup_core(&app2, boot, None))
        .await
        .map_err(internal)??;
    let status = lock.status();
    emit(&app, status.clone());
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_sync::MemorySecrets;

    fn temp_dirs() -> Dirs {
        let root =
            std::env::temp_dir().join(format!("mailhere-kilit-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("cache").join("ekler")).unwrap();
        Dirs::under(root)
    }

    #[test]
    fn bekleme_besinci_hatadan_sonra_ustel() {
        for n in 0..=5 {
            assert_eq!(retry_delay(n), Duration::ZERO, "{n}. hata beklemesiz");
        }
        assert_eq!(retry_delay(6), Duration::from_secs(2));
        assert_eq!(retry_delay(7), Duration::from_secs(4));
        assert_eq!(retry_delay(10), Duration::from_secs(32));
        assert_eq!(retry_delay(13), Duration::from_secs(256));
        assert_eq!(retry_delay(14), MAX_WAIT);
        assert_eq!(retry_delay(200), MAX_WAIT, "taşma yok");
    }

    #[test]
    fn kapi_kilitliyken_yalniz_kilit_komutlari() {
        // Çekirdek kapalı: yalnız lock_*.
        assert!(allows("lock_status", false, false));
        assert!(allows("lock_unlock", false, false));
        assert!(allows("lock_reset_local", false, false));
        assert!(!allows("messages_list", false, false));
        assert!(!allows("settings_get", false, false));
        assert!(!allows("notify_pending", false, false));
        // Arayüz kilitli: içeriksiz liste.
        assert!(allows("lock_unlock", true, true));
        assert!(allows("notify_hide", true, true));
        assert!(allows("sync_now", true, true));
        assert!(allows("haptic", true, true));
        assert!(!allows("messages_list", true, true));
        assert!(!allows("message_get", true, true));
        assert!(!allows("notify_pending", true, true));
        assert!(!allows("settings_get", true, true));
        assert!(!allows("counts", true, true));
        // Açık: her şey.
        assert!(allows("messages_list", true, false));
        assert!(allows("settings_get", true, false));
    }

    #[test]
    fn parola_kurallari() {
        assert_eq!(check_password("kisa").unwrap_err().code, ErrorCode::Invalid);
        assert!(check_password("sekizkar").is_ok());
        assert!(
            check_password("çğıöşü12").is_ok(),
            "karakter sayılır, bayt değil"
        );
        assert!(check_password(&"a".repeat(256)).is_ok());
        assert_eq!(
            check_password(&"a".repeat(257)).unwrap_err().code,
            ErrorCode::Invalid
        );
    }

    #[test]
    fn hata_metinleri_parola_tasimaz_ve_bekleme_soyler() {
        assert!(wrong_password(Duration::ZERO).message.contains("yanlış"));
        let e = wrong_password(Duration::from_secs(8));
        assert!(e.message.contains("8 saniye"));
        assert_eq!(wait_error(Duration::from_millis(1)).code, ErrorCode::Locked);
        assert!(wait_error(Duration::from_millis(1))
            .message
            .contains("1 saniye"));
    }

    #[test]
    fn etkinliksiz_denetim_kilit_ve_anahtarsizlik_ayrilir() {
        let dirs = temp_dirs();
        let secrets = MemorySecrets::new();
        // Anahtar yok, kayıt yok: ilk açılış uygulamadan.
        assert_eq!(
            headless_check(&dirs, &secrets).unwrap_err().code,
            ErrorCode::Keyring
        );
        // Kayıt var: kilitli.
        secrets.set(LOCK_RECORD_SECRET, "{}").unwrap();
        let e = headless_check(&dirs, &secrets).unwrap_err();
        assert_eq!(e.code, ErrorCode::Locked);
        assert!(e.message.contains("kilitli"));
        // Düz anahtar var: açık.
        secrets.set(DB_KEY_SECRET, &"a".repeat(64)).unwrap();
        assert!(headless_check(&dirs, &secrets).is_ok());
        let _ = std::fs::remove_dir_all(&dirs.data);
    }

    #[test]
    fn kayit_yazilir_geri_okunur_ve_adlar_bozmaz() {
        let secrets = MemorySecrets::new();
        let key = DbKey::generate().unwrap();
        let mut rec = mh_store::lock::wrap(&key, b"parola1234", KdfParams::MIN, true).unwrap();
        rec.secrets = vec!["hesap-1".into(), "oauth:hesap-1".into()];
        write_record(&secrets, &rec).unwrap();
        let back = read_record(&secrets).unwrap().unwrap();
        assert_eq!(back, rec);
        assert_eq!(mh_store::lock::unwrap(&back, b"parola1234").unwrap(), key);
        assert!(read_record(&MemorySecrets::new()).unwrap().is_none());
        secrets.set(LOCK_RECORD_SECRET, "bozuk").unwrap();
        assert_eq!(read_record(&secrets).unwrap_err().code, ErrorCode::Keyring);
    }

    #[test]
    fn yerel_veri_sifirlanir_ve_isaret_uygulanir() {
        let dirs = temp_dirs();
        let secrets = MemorySecrets::new();
        let db = dirs.data.join(DB_FILE);
        for suffix in ["", "-wal", "-shm"] {
            std::fs::write(format!("{}{suffix}", db.display()), b"x").unwrap();
        }
        std::fs::write(dirs.cache.join("ekler").join("a.pdf"), b"x").unwrap();
        secrets.set("hesap-1", "parola").unwrap();
        secrets.set("oauth:hesap-1", "jeton").unwrap();
        secrets.set("hesap-2", "kalir").unwrap();
        secrets.set(LOCK_RECORD_SECRET, "{}").unwrap();
        secrets.set(DB_KEY_SECRET, "anahtar").unwrap();

        // İşaret bırakılmış gibi: bir sonraki açılış uygular.
        write_wipe_marker(&dirs, &["hesap-1".into(), "oauth:hesap-1".into()]).unwrap();
        assert!(dirs.data.join(WIPE_MARKER).exists());
        apply_pending_wipe(&dirs, &secrets).unwrap();

        assert!(!db.exists());
        assert!(!dirs.data.join(format!("{DB_FILE}-wal")).exists());
        assert!(!dirs.cache.join("ekler").exists());
        assert!(!dirs.data.join(WIPE_MARKER).exists());
        assert!(secrets.get("hesap-1").unwrap().is_none());
        assert!(secrets.get("oauth:hesap-1").unwrap().is_none());
        assert!(secrets.get(LOCK_RECORD_SECRET).unwrap().is_none());
        assert!(secrets.get(DB_KEY_SECRET).unwrap().is_none());
        assert_eq!(
            secrets.get("hesap-2").unwrap().as_deref(),
            Some("kalir"),
            "listede olmayan ad silinmez"
        );
        // İşaret yoksa bir şey yapılmaz.
        apply_pending_wipe(&dirs, &secrets).unwrap();
        let _ = std::fs::remove_dir_all(&dirs.data);
    }

    #[test]
    fn durum_bekleme_suresini_verir() {
        let dirs = temp_dirs();
        let state = LockState {
            inner: Mutex::new(Inner {
                enabled: true,
                locked: true,
                ui_locked: false,
                relock: true,
                failures: 0,
                retry_after: None,
                boot: None,
                busy: false,
            }),
            secrets: Arc::new(MemorySecrets::new()),
            dirs: dirs.clone(),
            kdf: KdfParams::MIN,
            platform: Arc::new(crate::headless::NoopPlatform),
        };
        let s = state.status();
        assert!(s.enabled && s.locked && !s.ui_locked && s.relock_on_open);
        assert_eq!(s.retry_after_ms, 0);
        for _ in 0..5 {
            assert_eq!(state.failed(), Duration::ZERO);
        }
        assert!(state.check_retry().is_ok());
        assert_eq!(state.failed(), Duration::from_secs(2));
        let s = state.status();
        assert!(s.retry_after_ms > 1000 && s.retry_after_ms <= 2000);
        assert_eq!(state.check_retry().unwrap_err().code, ErrorCode::Locked);
        state.succeeded();
        assert!(state.check_retry().is_ok());
        assert_eq!(state.status().retry_after_ms, 0);
        // Meşgul bayrağı tek işlem geçirir, düşünce kalkar.
        {
            let _b = state.busy().unwrap();
            assert_eq!(state.busy().err().map(|e| e.code), Some(ErrorCode::Invalid));
        }
        assert!(state.busy().is_ok());
        let _ = std::fs::remove_dir_all(&dirs.data);
    }
}
