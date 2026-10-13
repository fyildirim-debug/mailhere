//! Pencereler: ana pencere, bildirim kartı, hızlı okuma, yazma.
//!
//! Hepsi kenarlıksızdır; başlık çubuğunu arayüz çizer (`data-tauri-drag-region`).

use std::sync::atomic::Ordering;

use mh_core::{events, CardPosition, ComposeMode, ComposePrefill, MessageId, OpenEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::state::AppState;

pub const MAIN: &str = "main";
pub const NOTIFY: &str = "notify";
const NOTIFY_WIDTH: f64 = 400.0;
/// Kart penceresi köşeye yapışık durur; boşluğu arayüzün saydam dolgusu verir.
const NOTIFY_MARGIN: f64 = 2.0;
/// Hızlı okuma penceresinin ekran kenarına uzaklığı.
const QUICK_MARGIN: f64 = 16.0;

fn url(route: &str) -> WebviewUrl {
    WebviewUrl::App(format!("index.html#/{route}").into())
}

pub fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(MAIN) {
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, MAIN, url("main"))
        .title("Mailhere")
        .inner_size(1280.0, 820.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .decorations(false)
        .build()
}

/// Ana pencereyi gösterir ve öne alır (gerekirse yeniden oluşturur).
pub fn show_main(app: &AppHandle) {
    match create_main(app) {
        Ok(w) => {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        Err(e) => tracing::error!(error = %e, "ana pencere açılamadı"),
    }
}

pub fn open_main(app: &AppHandle, id: Option<MessageId>) {
    show_main(app);
    if let Some(id) = id {
        if let Err(e) = app.emit_to(MAIN, events::OPEN, OpenEvent { id }) {
            tracing::warn!(error = %e, "açma olayı gönderilemedi");
        }
    }
}

/// Ana pencere kapatılınca tepsiye gizlenir (tepsiden "Çık" hariç); bildirim
/// penceresi de kapanmaz, gizlenir.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let quitting = window
            .app_handle()
            .try_state::<AppState>()
            .is_some_and(|s| s.quitting.load(Ordering::SeqCst));
        if quitting {
            return;
        }
        if window.label() == MAIN || window.label() == NOTIFY {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

/// Bildirim penceresi; ilk kartta oluşturulur, görünmez başlar ve odak çalmaz.
pub fn ensure_notify(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(NOTIFY) {
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, NOTIFY, url("notify"))
        .title("Mailhere bildirimi")
        .inner_size(NOTIFY_WIDTH, 160.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .build()
}

/// Kartın gösterileceği ekran: imlecin bulunduğu, yoksa ana pencerenin, yoksa birincil.
fn current_monitor(app: &AppHandle) -> Option<Monitor> {
    if let Ok(p) = app.cursor_position() {
        if let Ok(Some(m)) = app.monitor_from_point(p.x, p.y) {
            return Some(m);
        }
    }
    if let Some(Ok(Some(m))) = app.get_webview_window(MAIN).map(|w| w.current_monitor()) {
        return Some(m);
    }
    app.primary_monitor().ok().flatten()
}

fn card_position(app: &AppHandle) -> CardPosition {
    app.try_state::<AppState>()
        .and_then(|s| s.store.settings().ok())
        .map(|s| s.card_position)
        .unwrap_or_default()
}

/// Çalışma alanı (fiziksel piksel): konum, boyut ve ölçek.
#[derive(Debug, Clone, Copy)]
struct Area {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}

impl From<&Monitor> for Area {
    fn from(m: &Monitor) -> Self {
        let a = m.work_area();
        Self {
            x: a.position.x,
            y: a.position.y,
            width: a.size.width,
            height: a.size.height,
            scale: m.scale_factor(),
        }
    }
}

/// Pencereyi çalışma alanının ayarlı köşesine yerleştirir. Boyutlar mantıksal,
/// sonuç fiziksel pikseldir.
fn corner(
    area: Area,
    corner: CardPosition,
    width: f64,
    height: f64,
    margin: f64,
) -> PhysicalPosition<i32> {
    let scale = area.scale;
    let (w, h, m) = (
        (width * scale).round() as i32,
        (height * scale).round() as i32,
        (margin * scale).round() as i32,
    );
    let left = area.x + m;
    let top = area.y + m;
    let right = area.x + area.width as i32 - w - m;
    let bottom = area.y + area.height as i32 - h - m;
    match corner {
        CardPosition::BottomRight => PhysicalPosition::new(right, bottom),
        CardPosition::TopRight => PhysicalPosition::new(right, top),
        CardPosition::BottomLeft => PhysicalPosition::new(left, bottom),
        CardPosition::TopLeft => PhysicalPosition::new(left, top),
    }
}

/// Kart yığınının yüksekliği belli olunca pencereyi boyutlandırır, köşeye
/// koyar ve odak çalmadan gösterir.
pub fn notify_layout(app: &AppHandle, height: f64) -> tauri::Result<()> {
    tracing::debug!(height, "bildirim penceresi yerleşimi");
    let w = ensure_notify(app)?;
    let height = height.clamp(1.0, 2000.0);
    w.set_size(LogicalSize::new(NOTIFY_WIDTH, height))?;
    if let Some(m) = current_monitor(app) {
        w.set_position(corner(
            Area::from(&m),
            card_position(app),
            NOTIFY_WIDTH,
            height,
            NOTIFY_MARGIN,
        ))?;
    }
    w.set_always_on_top(true)?;
    w.show()?;
    Ok(())
}

pub fn notify_hide(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(NOTIFY) {
        w.hide()?;
    }
    Ok(())
}

/// Hızlı okuma penceresi (aynı posta için açıksa öne alınır).
pub fn open_quick(app: &AppHandle, id: MessageId) -> tauri::Result<()> {
    let label = format!("quick-{id}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let (width, height) = (440.0, 700.0);
    let w = WebviewWindowBuilder::new(app, &label, url(&format!("quick/{id}")))
        .title("Mailhere")
        .inner_size(width, height)
        .min_inner_size(380.0, 480.0)
        .decorations(false)
        .visible(false)
        .build()?;
    match current_monitor(app) {
        Some(m) => {
            // Ekran küçükse pencereyi çalışma alanına sığdır.
            let scale = m.scale_factor();
            let avail = f64::from(m.work_area().size.height) / scale - 2.0 * QUICK_MARGIN;
            let height = height.min(avail.max(480.0));
            w.set_size(LogicalSize::new(width, height))?;
            w.set_position(corner(
                Area::from(&m),
                card_position(app),
                width,
                height,
                QUICK_MARGIN,
            ))?;
        }
        None => w.center()?,
    }
    w.show()?;
    w.set_focus()?;
    Ok(())
}

fn mode_route(mode: ComposeMode) -> &'static str {
    match mode {
        ComposeMode::New => "new",
        ComposeMode::Reply => "reply",
        ComposeMode::ReplyAll => "replyAll",
        ComposeMode::Forward => "forward",
    }
}

/// Yeni bir yazma penceresi.
pub fn open_compose(app: &AppHandle, prefill: &ComposePrefill) -> tauri::Result<()> {
    let n = app
        .try_state::<AppState>()
        .map(|s| s.compose_counter.fetch_add(1, Ordering::SeqCst) + 1)
        .unwrap_or(1);
    let route = format!(
        "compose/{}/{}",
        mode_route(prefill.mode),
        prefill.message_id.unwrap_or(0)
    );
    let w = WebviewWindowBuilder::new(app, format!("compose-{n}"), url(&route))
        .title("Yeni posta · Mailhere")
        .inner_size(820.0, 660.0)
        .min_inner_size(600.0, 480.0)
        .center()
        .decorations(false)
        .build()?;
    w.set_focus()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kart_koseye_yerlesir() {
        // 1920×1040 çalışma alanı (alt görev çubuğu), %150 ölçek, ikinci ekran x=1920'de.
        let area = Area {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1040,
            scale: 1.5,
        };
        let p = corner(area, CardPosition::BottomRight, 400.0, 200.0, 2.0);
        assert_eq!((p.x, p.y), (1920 + 1920 - 600 - 3, 1040 - 300 - 3));
        let p = corner(area, CardPosition::TopLeft, 400.0, 200.0, 2.0);
        assert_eq!((p.x, p.y), (1923, 3));
        let p = corner(area, CardPosition::TopRight, 400.0, 200.0, 16.0);
        assert_eq!((p.x, p.y), (1920 + 1920 - 600 - 24, 24));
        let p = corner(area, CardPosition::BottomLeft, 400.0, 200.0, 0.0);
        assert_eq!((p.x, p.y), (1920, 740));
    }

    #[test]
    fn yazma_rotasi() {
        assert_eq!(mode_route(ComposeMode::ReplyAll), "replyAll");
        assert_eq!(mode_route(ComposeMode::New), "new");
    }
}
