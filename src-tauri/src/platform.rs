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

impl Platform for DesktopPlatform {
    fn notify(&self, app: &AppHandle, ev: NotifyEvent, settings: &Settings) {
        crate::notify_card::show_card(app, ev, settings.native_notifications);
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

    fn open_compose(&self, app: &AppHandle, prefill: &ComposePrefill) -> AppResult<()> {
        crate::windows::open_compose(app, prefill).map_err(internal)
    }

    /// Güvenli türler varsayılan uygulamayla açılır; diğerleri yalnız
    /// klasöründe gösterilir, kullanıcı ne yaptığını bilerek açar.
    fn open_file(&self, app: &AppHandle, path: &Path, _mime: &str, direct: bool) -> AppResult<()> {
        let opener = app.opener();
        let res = if direct {
            opener.open_path(path.to_string_lossy(), None::<&str>)
        } else {
            opener.reveal_item_in_dir(path)
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
}
