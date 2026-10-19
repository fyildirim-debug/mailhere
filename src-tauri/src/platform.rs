//! Masaüstü kabuğunun `Platform` uygulaması: köşe kartı ya da sistem
//! bildirimi, tepsi sayacı, pencereler, dosya pencereleri, açılış hatası.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Mutex;
use std::time::Instant;

use mh_app::state::internal;
use mh_app::{BoxFut, Confirm, Platform};
use mh_core::{AppError, AppResult, ComposePrefill, ErrorCode, MessageId, NotifyEvent, Settings};
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

/// Yalnız masaüstünde anlamlı durum.
#[derive(Default)]
pub struct DesktopState {
    /// Tepsiden "Çık" seçildi; ana pencere artık gizlenmez, kapanır.
    pub quitting: AtomicBool,
    /// Yazma pencerelerinin etiketi için sayaç.
    pub compose_counter: AtomicU32,
    /// Bildirim penceresi henüz dinlemiyorken kaybolmasın diye bekleyen kartlar.
    pub cards: Mutex<Vec<(Instant, NotifyEvent)>>,
}

pub struct DesktopPlatform;

/// Arayüz kilitliyken kartın yerine geçen içeriksiz sistem bildirimi.
fn locked_notification(app: &AppHandle, ev: &NotifyEvent) {
    mh_app::notify::native_text(
        app,
        &format!("Mailhere · {}", ev.account_name),
        "Yeni posta geldi. Görmek için kilidi aç.",
    );
}

/// Kilitli açılış: tepsi ipucu "kilitli" der; pencere gizli açıldıysa
/// (açılışta tepside) bir kez sistem bildirimiyle söylenir, tıklanan tepsi
/// simgesi kilit ekranını açar. Çekirdek parola girilene kadar kapalıdır.
pub fn locked_start(app: &AppHandle, hidden: bool) {
    // Tepsi kimliği `tray.rs` ile aynı ("main"); `tray::refresh` çekirdek
    // yokken ipucuna dokunmaz.
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some("Mailhere · Kilitli"));
    }
    if hidden {
        mh_app::notify::native_text(
            app,
            "Mailhere kilitli",
            "Postaları almak için Mailhere'yi açıp parolanı gir.",
        );
    }
}

impl Platform for DesktopPlatform {
    fn notify(&self, app: &AppHandle, ev: NotifyEvent, settings: &Settings) {
        // Arayüz kilitliyken (pencere gizlenip "her açışta sor" açıkken)
        // kart gönderen, konu ve önizleme göstermez: içeriksiz bildirim.
        if mh_app::lock::ui_locked(app) {
            return locked_notification(app, &ev);
        }
        crate::notify_card::show_card(app, ev, settings.native_notifications);
    }

    /// Çekirdek açıldı (olağan açılış ya da kilit açıldı): tepsi sayacı.
    fn core_ready(&self, app: &AppHandle) {
        crate::tray::refresh(app);
    }

    fn counts_changed(&self, app: &AppHandle) {
        crate::tray::refresh(app);
    }

    fn muted(&self, app: &AppHandle) {
        if let Err(e) = crate::windows::notify_hide(app) {
            tracing::warn!(error = %e, "bildirim penceresi gizlenemedi");
        }
    }

    fn open_main(&self, app: &AppHandle, id: Option<MessageId>) {
        crate::windows::open_main(app, id);
    }

    fn open_quick(&self, app: &AppHandle, id: MessageId) -> AppResult<()> {
        crate::windows::show_quick(app, id).map_err(internal)
    }

    fn open_url(&self, app: &AppHandle, url: &str) -> Result<(), String> {
        crate::links::open_url(app, url)
    }

    fn open_compose(&self, app: &AppHandle, prefill: &ComposePrefill) -> AppResult<()> {
        crate::windows::open_compose(app, prefill).map_err(internal)
    }

    /// Güvenli türler varsayılan uygulamayla açılır; diğerleri yalnız
    /// klasöründe gösterilir, kullanıcı ne yaptığını bilerek açar.
    fn open_file(&self, app: &AppHandle, path: &Path, _mime: &str, direct: bool) -> AppResult<()> {
        let res = if direct {
            crate::links::open_path(app, &path.to_string_lossy())
        } else {
            app.opener()
                .reveal_item_in_dir(path)
                .map_err(|e| e.to_string())
        };
        res.map_err(|e| {
            tracing::warn!(error = %e, "dosya açılamadı");
            AppError::new(ErrorCode::Internal, "Dosya açılamadı.")
        })
    }

    fn save_file<'a>(
        &'a self,
        window: &'a WebviewWindow,
        suggested: &'a str,
        bytes: Vec<u8>,
    ) -> BoxFut<'a, AppResult<Option<String>>> {
        Box::pin(crate::files::save(window, suggested, bytes))
    }

    fn pick_files<'a>(&'a self, window: &'a WebviewWindow) -> BoxFut<'a, AppResult<Vec<PathBuf>>> {
        Box::pin(crate::files::pick(window))
    }

    fn confirm<'a>(
        &'a self,
        window: &'a WebviewWindow,
        req: Confirm,
    ) -> BoxFut<'a, AppResult<bool>> {
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            window
                .dialog()
                .message(req.message)
                .title(req.title)
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    req.ok.into(),
                    req.cancel.into(),
                ))
                .parent(window)
                .show(move |ok| {
                    let _ = tx.send(ok);
                });
            rx.await.map_err(internal)
        })
    }

    /// Açılışta veritabanı açılamadı: pencere açılmadan sorunu ve çözümü
    /// anlatan bir hata iletişim kutusu gösterilir, kapatılınca uygulama
    /// çıkar. (Kurulum hatası Tauri'de paniğe döner; Windows'ta konsol
    /// olmadığından açıklamasız bir çökme olurdu.) Hiçbir dosya silinmez.
    fn fatal(&self, app: &AppHandle, e: &AppError) {
        let exit = app.clone();
        app.dialog()
            .message(e.message.clone())
            .title("Mailhere açılamadı")
            .kind(MessageDialogKind::Error)
            .buttons(MessageDialogButtons::OkCustom("Kapat".into()))
            .show(move |_| exit.exit(1));
    }

    fn restart(&self, app: &AppHandle) {
        app.restart();
    }
}
