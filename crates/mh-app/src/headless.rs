//! Penceresiz çekirdek: Tauri `App` olmadan depo, eşitleyiciler ve giden
//! kutusu. Android kabuğu etkinlik yokken (telefon yeniden başladı, arka plan
//! hizmeti süreçsiz uyandı, JobScheduler yoklaması) bunu açar; masaüstü
//! kullanmaz.
//!
//! Bildirim kararı pencereli çekirdekle aynı hattan geçer: kurallar →
//! yanıt hatırlatmaları → yeni gönderici eleği → bildirim kararı
//! ([`crate::sink::decide`]: hesabın bildirimi, kime, sessiz saat, VIP,
//! susturma). Kabuk yalnız [`HeadlessShell`] ile bildirimi çizer ve sayaç
//! değişimini öğrenir; pencere, tepsi ya da `AppHandle` yoktur.
//!
//! Süreç başına tek çekirdek kuralını kabuk uygular: pencereli çekirdek
//! (`setup_core`) açılmadan önce bu çekirdek [`Headless::stop`] ile kapatılır,
//! böylece aynı SQLCipher veritabanını iki `Store` açmaz.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use mh_core::{
    Account, AccountId, AppError, AppResult, ErrorCode, MessageDetail, MessageId, MessageSummary,
    NotifyAction, NotifyEvent, OutboxItem, Settings, SyncState, SyncStatus,
};
use mh_store::CompiledRule;
use mh_sync::{EventSink, SecretStore};

use crate::state::{internal, memory_mode, now_ms, AppState, Core, Dirs};

/// Yanıtlanacak postanın gövdesi inmemişse en çok bu kadar beklenir; sonra
/// önizleme alıntılanır (pencereli yanıtla aynı süre).
const BODY_WAIT: Duration = Duration::from_secs(8);
/// Çalışma ortamı kapatılırken bekleyen engelleyici işlere verilen süre.
const SHUTDOWN: Duration = Duration::from_secs(5);

/// Kabuğun penceresiz çekirdeğe verdiği iki iş: bildirimi göstermek ve
/// okunmamış sayısının değişmiş olabileceğini öğrenmek (bildirimleri
/// uzlaştırmak için).
pub trait HeadlessShell: Send + Sync + 'static {
    /// Kart kararı verilmiş yeni posta (pencereli `Platform::notify` ile aynı).
    fn notify(&self, ev: NotifyEvent, settings: &Settings);
    /// Kutular değişti; okunan postaların bildirimleri kaldırılabilir.
    fn counts_changed(&self);
}

/// Hiçbir pencereye dokunmayan `Platform`: penceresiz çekirdekte
/// `AppState::platform` için. Yöntemleri `AppHandle` ister; penceresiz
/// çekirdekte `AppHandle` hiç olmadığından çağrılmazlar.
pub struct NoopPlatform;

fn no_window() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Bu işlem yalnız uygulama ekrandayken yapılabilir.",
    )
}

impl crate::Platform for NoopPlatform {
    fn notify(&self, _: &tauri::AppHandle, _: NotifyEvent, _: &Settings) {}
    fn counts_changed(&self, _: &tauri::AppHandle) {}
    fn muted(&self, _: &tauri::AppHandle) {}
    fn open_main(&self, _: &tauri::AppHandle, _: Option<MessageId>) {}
    fn open_quick(&self, _: &tauri::AppHandle, _: MessageId) -> AppResult<()> {
        Err(no_window())
    }
    fn open_compose(&self, _: &tauri::AppHandle, _: &mh_core::ComposePrefill) -> AppResult<()> {
        Err(no_window())
    }
    fn open_file(
        &self,
        _: &tauri::AppHandle,
        _: &std::path::Path,
        _: &str,
        _: bool,
    ) -> AppResult<()> {
        Err(no_window())
    }
    fn save_file<'a>(
        &'a self,
        _: &'a tauri::WebviewWindow,
        _: &'a str,
        _: Vec<u8>,
    ) -> crate::BoxFut<'a, AppResult<Option<String>>> {
        Box::pin(async { Err(no_window()) })
    }
    fn pick_files<'a>(
        &'a self,
        _: &'a tauri::WebviewWindow,
    ) -> crate::BoxFut<'a, AppResult<Vec<std::path::PathBuf>>> {
        Box::pin(async { Err(no_window()) })
    }
    fn confirm<'a>(
        &'a self,
        _: &'a tauri::WebviewWindow,
        _: crate::Confirm,
    ) -> crate::BoxFut<'a, AppResult<bool>> {
        Box::pin(async { Err(no_window()) })
    }
    fn fatal(&self, _: &tauri::AppHandle, e: &AppError) {
        tracing::error!(code = %e.code, error = %e.message, "penceresiz çekirdek açılamadı");
    }
    // Penceresiz çekirdek yerel veri sıfırlamaz (kilit komutları arayüzden gelir).
    fn restart(&self, _: &tauri::AppHandle) {}
}

/// Hesapların eşitleme durumu: tek seferlik çalıştırma "ilk eşitleme bitti
/// mi" sorusunu buradan yanıtlar.
#[derive(Default)]
struct Status {
    /// Şu an eşitlenen hesaplar.
    syncing: HashSet<AccountId>,
    /// En az bir kez eşitleme dışı durum (çevrimiçi, çevrimdışı, hata)
    /// bildirmiş hesaplar.
    settled: HashSet<AccountId>,
}

impl Status {
    fn record(&mut self, s: &SyncStatus) {
        if matches!(s.state, SyncState::Syncing { .. }) {
            self.syncing.insert(s.account_id.clone());
        } else {
            self.syncing.remove(&s.account_id);
            self.settled.insert(s.account_id.clone());
        }
    }

    /// Eşitlenen hesap yok ve beklenen her hesap en az bir kez durum
    /// bildirdi.
    fn quiet(&self, expected: usize) -> bool {
        self.syncing.is_empty() && self.settled.len() >= expected
    }
}

/// Eşitleyicilerin olay havuzu: yeni posta bildirim hattına, kutu değişimi
/// kabuğa gider.
struct HeadlessSink {
    shell: Arc<dyn HeadlessShell>,
    handle: tokio::runtime::Handle,
    state: OnceLock<Weak<AppState>>,
    status: Mutex<Status>,
}

impl HeadlessSink {
    fn state(&self) -> Option<Arc<AppState>> {
        self.state.get().and_then(Weak::upgrade)
    }
}

impl EventSink for HeadlessSink {
    fn sync_status(&self, status: SyncStatus) {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(&status);
    }

    fn changed(&self, _mailbox_ids: Vec<String>) {
        self.shell.counts_changed();
    }

    fn new_mail(&self, account: &Account, messages: Vec<MessageSummary>) {
        let Some(state) = self.state() else {
            return;
        };
        let account = account.clone();
        let shell = self.shell.clone();
        // Kurallar gövde indirebilir (eşzamansız); karar kurallardan sonra.
        self.handle
            .spawn(async move { pipeline(&state, &account, messages, shell.as_ref()).await });
    }
}

/// Pencereli `TauriSink::new_mail` ile aynı sıra: kurallar → yanıt
/// hatırlatmaları → elek → bildirim kararı → sayaç.
async fn pipeline(
    state: &AppState,
    account: &Account,
    messages: Vec<MessageSummary>,
    shell: &dyn HeadlessShell,
) {
    let rules: Vec<CompiledRule> = state
        .store
        .rules()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.enabled)
        .map(CompiledRule::new)
        .filter(|r| r.applies_to(&account.id))
        .collect();
    let messages = if rules.is_empty() || messages.is_empty() {
        messages
    } else {
        let (kept, touched) = crate::rules::Engines::of(state)
            .apply_new(&rules, messages)
            .await;
        if !touched.is_empty() {
            shell.counts_changed();
        }
        kept
    };
    // Yeni posta bir yanıt olabilir: yanıtlanan hatırlatmalar kapanır.
    let _ = crate::followups::close_answered(state);
    let (kept, _touched) = crate::screener::screen_with(state, account, messages);
    crate::sink::decide(state, account, kept, |ev, settings| {
        shell.notify(ev, settings)
    });
    shell.counts_changed();
}

/// Penceresiz çekirdek: kendi çalışma ortamı, depo, eşitleyiciler, giden
/// kutusu. [`Headless::stop`] her şeyi kapatır.
pub struct Headless {
    core: Core,
    state: Arc<AppState>,
    sink: Arc<HeadlessSink>,
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    handle: tokio::runtime::Handle,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl Headless {
    /// Depoyu açar ve yöneticileri kurar; hiçbir şeyi başlatmaz
    /// ([`Headless::start`]). Veritabanı anahtarı kasada yoksa açmaz ve yeni
    /// anahtar üretmez: ilk açılış uygulamadan yapılmalıdır (bellek kipi
    /// hariç). Parola kilidi açıksa (anahtar parolayla sarılı) `Locked`
    /// hatası döner: çekirdek parola girilene kadar açılmaz.
    pub fn open(
        dirs: &Dirs,
        secrets: Arc<dyn SecretStore>,
        shell: Arc<dyn HeadlessShell>,
    ) -> AppResult<Arc<Self>> {
        if !memory_mode() {
            crate::lock::headless_check(dirs, secrets.as_ref())?;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("mailhere-arka")
            .build()
            .map_err(internal)?;
        let handle = runtime.handle().clone();
        let sink = Arc::new(HeadlessSink {
            shell,
            handle: handle.clone(),
            state: OnceLock::new(),
            status: Mutex::new(Status::default()),
        });
        tracing::info!(dir = %dirs.data.display(), "penceresiz çekirdek: veri klasörü");
        let core = Core::open(dirs, secrets, sink.clone(), &handle, None)?;
        let state = Arc::new(AppState::new(
            &core,
            dirs.cache.clone(),
            Arc::new(NoopPlatform),
        ));
        let _ = sink.state.set(Arc::downgrade(&state));
        Ok(Arc::new(Self {
            core,
            state,
            sink,
            runtime: Mutex::new(Some(runtime)),
            handle,
            tasks: Mutex::new(Vec::new()),
        }))
    }

    pub fn state(&self) -> &Arc<AppState> {
        &self.state
    }

    /// Çekirdeğin çalışma ortamı; kabuk eşzamansız işleri (yanıt, bekleyen
    /// eylemler) buraya verir.
    pub fn handle(&self) -> &tokio::runtime::Handle {
        &self.handle
    }

    /// Eşitlenecek hesap sayısı (IMAP'ta eşitlenebilir olanlar ve JMAP).
    pub fn account_count(&self) -> usize {
        self.state
            .store
            .accounts()
            .unwrap_or_default()
            .iter()
            .filter(|a| mh_sync::is_syncable(a) || a.kind == mh_core::AccountKind::Jmap)
            .count()
    }

    /// Giden kutusunu ve tüm hesapların eşitlemesini başlatır; uykudan
    /// dönüşte bağlantıları yenileyen izleyiciyi kurar.
    pub fn start(&self) -> AppResult<()> {
        self.core.start(&self.handle)?;
        let state = self.state.clone();
        let watch = self.handle.spawn(crate::wake::sleep_watch_with(move || {
            if let Err(e) = crate::wake::reconnect(&state, "uyku") {
                tracing::warn!(error = %e.message, "uykudan sonra bağlantılar yenilenemedi");
            }
        }));
        let push = crate::push::start(&self.state, &self.handle);
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend([watch, push]);
        Ok(())
    }

    /// Tüm hesapları hemen eşitler (uyanma alarmı, yoklama işi).
    pub fn sync_now(&self) {
        self.state.sync.sync_now();
        self.state.jmap.sync_now();
    }

    /// İlk eşitleme bitti mi: hiçbir hesap eşitlenmiyor ve her hesap en az
    /// bir kez durum bildirdi. Tek seferlik çalıştırma bunu bekler.
    pub fn quiet(&self) -> bool {
        self.sink
            .status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .quiet(self.account_count())
    }

    /// Bildirim düğmesi (yanıt hariç): yerelde uygulanır, sunucuya kuyrukla
    /// gider; bildirimler uzlaştırılır.
    pub fn apply(&self, id: MessageId, action: NotifyAction) -> AppResult<()> {
        crate::notify_actions::apply_local(&self.state, id, action)?;
        self.sink.shell.counts_changed();
        Ok(())
    }

    /// Satır içi yanıt: gövde inmemişse kısa süre beklenir (ağ yoksa önizleme
    /// alıntılanır), yanıt giden kutusuna girer ve posta okundu sayılır.
    /// Çekirdeğin çalışma ortamında (`handle`) çalıştırılmalıdır.
    pub async fn reply(&self, id: MessageId, text: &str) -> AppResult<OutboxItem> {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Yanıt boş. Bir şey yaz ya da postayı uygulamada aç.",
            ));
        }
        let state = self.state.clone();
        let detail = match tokio::time::timeout(BODY_WAIT, fetch_detail(&state, id)).await {
            Ok(Ok(d)) => d,
            _ => {
                let store = state.store.clone();
                blocking(move || {
                    let allow = store.settings()?.remote_allow;
                    store.detail(id, &allow)
                })
                .await?
            }
        };
        let s = state.clone();
        let item = blocking(move || {
            let msg = crate::notify_actions::build_reply(&s, &detail, &text)?;
            s.outbox.enqueue(&msg, now_ms())
        })
        .await?;
        // Yanıtlanan posta okunmuş sayılır; olmazsa yanıt yine gider.
        if let Err(e) = crate::notify_actions::apply_local(&state, id, NotifyAction::MarkRead) {
            tracing::warn!(error = %e.message, "yanıtlanan posta okundu sayılamadı");
        }
        self.sink.shell.counts_changed();
        Ok(item)
    }

    /// Eşitlemeyi durdurur, izleyicileri iptal eder, çalışma ortamını kapatır.
    /// Depo son `Arc` düşünce kapanır. İkinci çağrı bir şey yapmaz.
    pub fn stop(&self) {
        self.core.stop();
        for t in self
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            t.abort();
        }
        let Some(rt) = self
            .runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        // Çalışma ortamı kendi iş parçacığından kapatılamaz; eşzamansız
        // bağlamdaysak beklemeden arka planda kapanır.
        if tokio::runtime::Handle::try_current().is_ok() {
            rt.shutdown_background();
        } else {
            rt.shutdown_timeout(SHUTDOWN);
        }
        tracing::info!("penceresiz çekirdek kapatıldı");
    }
}

async fn fetch_detail(state: &AppState, id: MessageId) -> AppResult<MessageDetail> {
    crate::commands::ensure_body(state, id).await?;
    let allow = state.store.settings()?.remote_allow;
    let store = state.store.clone();
    blocking(move || store.detail(id, &allow)).await
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(internal)?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(account: &str, state: SyncState) -> SyncStatus {
        SyncStatus {
            account_id: account.into(),
            state,
        }
    }

    #[test]
    fn sessizlik_her_hesap_bir_kez_durum_bildirince() {
        let mut s = Status::default();
        assert!(!s.quiet(2), "hiç durum gelmeden sessiz sayılmaz");
        s.record(&status("is", SyncState::Syncing { done: 0, total: 3 }));
        assert!(!s.quiet(2));
        s.record(&status("is", SyncState::Online));
        assert!(!s.quiet(2), "ikinci hesap bekleniyor");
        s.record(&status("ev", SyncState::Offline));
        assert!(s.quiet(2), "çevrimdışı da bir sonuçtur");
        s.record(&status("ev", SyncState::Syncing { done: 1, total: 2 }));
        assert!(!s.quiet(2), "yeniden eşitlenen hesap sessizliği bozar");
        s.record(&status(
            "ev",
            SyncState::Error {
                message: "x".into(),
            },
        ));
        assert!(s.quiet(2));
        assert!(s.quiet(0), "hesapsız çekirdek hemen sessizdir");
    }

    struct SessizKabuk;

    impl HeadlessShell for SessizKabuk {
        fn notify(&self, _: NotifyEvent, _: &Settings) {}
        fn counts_changed(&self) {}
    }

    /// Parola kilidi açıkken (anahtar sarılı, düz anahtar yok) penceresiz
    /// çekirdek açılmaz: veritabanı, eşitleyiciler ve çalışma ortamı
    /// kurulmaz; `Locked` döner. Anahtar hiç yoksa `Keyring` (ilk açılış).
    #[test]
    fn kilitliyken_penceresiz_cekirdek_acilmaz() {
        let root =
            std::env::temp_dir().join(format!("mailhere-headless-kilit-{}", uuid::Uuid::new_v4()));
        let dirs = Dirs::under(root.clone());
        let secrets = Arc::new(mh_sync::MemorySecrets::new());
        secrets
            .set(mh_store::LOCK_RECORD_SECRET, r#"{"v":1}"#)
            .unwrap();
        let e = Headless::open(&dirs, secrets.clone(), Arc::new(SessizKabuk))
            .err()
            .expect("kilitliyken açılmamalı");
        assert_eq!(e.code, ErrorCode::Locked);
        assert!(
            !root.join("mailhere.db").exists(),
            "veritabanı dosyası yaratılmaz"
        );
        secrets.delete(mh_store::LOCK_RECORD_SECRET).unwrap();
        let e = Headless::open(&dirs, secrets, Arc::new(SessizKabuk))
            .err()
            .expect("anahtarsız açılmamalı");
        assert_eq!(e.code, ErrorCode::Keyring);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn penceresiz_platform_pencere_isteyen_isleri_reddeder() {
        let e = no_window();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("ekranda"));
    }
}
