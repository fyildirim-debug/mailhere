//! Kabuğa özgü işlerin sözleşmesi.
//!
//! Ortak komutlar pencere, tepsi, bildirim kartı ya da dosya penceresine
//! doğrudan dokunmaz; `state.platform` üzerinden kabuğa sorar. Masaüstü
//! (`src-tauri/src/platform.rs`) kartı, tepsiyi ve pencereleri; Android
//! (`mailheremobile`) sistem bildirimini, tek pencereyi ve paylaşımı bağlar.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use mh_core::{AppError, AppResult, ComposePrefill, MessageId, NotifyEvent, Settings};
use tauri::{AppHandle, WebviewWindow};

/// Trait nesnesinden dönen eşzamansız sonuç.
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// İşletim sisteminin kendi penceresinde sorulacak onay (arayüzde çalışan
/// bir betik bu adımı atlayamaz).
#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: &'static str,
    pub message: String,
    /// Onay ve vazgeçme düğmelerinin metni.
    pub ok: &'static str,
    pub cancel: &'static str,
}

pub trait Platform: Send + Sync + 'static {
    /// Yeni posta ya da hatırlatma: masaüstünde köşe kartı ya da sistem
    /// bildirimi (`settings.native_notifications`), Android'de sistem bildirimi.
    fn notify(&self, app: &AppHandle, ev: NotifyEvent, settings: &Settings);

    /// Okunmamış sayısı değişmiş olabilir (tepsi ipucu, rozet).
    fn counts_changed(&self, app: &AppHandle);

    /// Bildirimler sessize alındı: açık kart varsa kapanır.
    fn muted(&self, app: &AppHandle);

    /// Ana pencereyi öne alır; `id` verilmişse o postayı açar.
    fn open_main(&self, app: &AppHandle, id: Option<MessageId>);

    /// Hızlı okuma penceresi (Android: ana pencerede açılır).
    fn open_quick(&self, app: &AppHandle, id: MessageId) -> AppResult<()>;

    /// Yazma penceresi (Android: ana pencerede yazma yığını).
    fn open_compose(&self, app: &AppHandle, prefill: &ComposePrefill) -> AppResult<()>;

    /// Diske yazılmış dosyayı varsayılan uygulamasıyla açar; `direct` değilse
    /// (izin listesi dışı tür) yalnız klasöründe gösterir.
    fn open_file(&self, app: &AppHandle, path: &Path, mime: &str, direct: bool) -> AppResult<()>;

    /// Kaydet penceresi: kullanıcının seçtiği yere yazar ve yolu döner;
    /// vazgeçilirse `None`. `suggested` önerilen dosya adıdır (uzantı korunur).
    fn save_file<'a>(
        &'a self,
        window: &'a WebviewWindow,
        suggested: &'a str,
        bytes: Vec<u8>,
    ) -> BoxFut<'a, AppResult<Option<String>>>;

    /// Dosya seçme penceresi: ek olarak kabul edilecek yerel dosya yolları.
    fn pick_files<'a>(&'a self, window: &'a WebviewWindow) -> BoxFut<'a, AppResult<Vec<PathBuf>>>;

    /// İşletim sisteminin onay penceresi; onaylandıysa `true`.
    fn confirm<'a>(
        &'a self,
        window: &'a WebviewWindow,
        req: Confirm,
    ) -> BoxFut<'a, AppResult<bool>>;

    /// Açılış başarısız: sorunu ve çözümü gösterir, uygulamayı kapatır.
    fn fatal(&self, app: &AppHandle, e: &AppError);
}
