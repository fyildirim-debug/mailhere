//! Pencereler: ana pencere, bildirim kartı, hızlı okuma, yazma.
//!
//! Hepsi kenarlıksızdır; başlık çubuğunu arayüz çizer (`data-tauri-drag-region`).

use std::sync::atomic::Ordering;

use mh_app::routes::{compose_route, is_app_url};
use mh_app::AppState;
use mh_core::{events, AppError, CardPosition, ComposePrefill, LinkEvent, MessageId, OpenEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::platform::DesktopState;

pub const MAIN: &str = "main";
pub const NOTIFY: &str = "notify";
/// Kart 372 px + iki yanda 20 px saydam dolgu (gölge dolgunun içinde biter).
const NOTIFY_WIDTH: f64 = 412.0;
/// Kart penceresi köşeye yapışık durur; boşluğu arayüzün saydam dolgusu verir.
const NOTIFY_MARGIN: f64 = 2.0;
/// Hızlı okuma penceresinin ekran kenarına uzaklığı.
const QUICK_MARGIN: f64 = 16.0;

fn url(route: &str) -> WebviewUrl {
    WebviewUrl::App(format!("index.html#/{route}").into())
}

/// Aynı anda açık olabilecek en çok yazma penceresi.
const MAX_COMPOSE: usize = 12;

/// Geliştirme sunucusunun ana makinesi (`tauri.conf.json` → `devUrl`);
/// `is_app_url` yalnız geliştirme derlemesinde ona izin verir.
fn dev_host(app: &AppHandle) -> Option<String> {
    app.config()
        .build
        .dev_url
        .as_ref()
        .and_then(|u| u.host_str())
        .map(str::to_owned)
}

/// Yönlendirme bekçisi: pencere uygulama dışı adrese gidemez.
fn guard(app: &AppHandle) -> impl Fn(&tauri::Url) -> bool + Send + 'static {
    let dev = dev_host(app);
    move |url| {
        let ok = is_app_url(url, dev.as_deref());
        if !ok {
            tracing::warn!(scheme = url.scheme(), host = ?url.host_str(), "uygulama dışı adrese yönlendirme engellendi");
        }
        ok
    }
}

/// Yeni pencere isteği (postada `target="_blank"` bağlantı): pencere hiç
/// açılmaz; web ve e-posta bağlantısı arayüze `mh:link` olarak verilir,
/// arayüz aldatıcı bağlantı onayını uygulayıp tarayıcıda açar.
fn new_window(
    app: &AppHandle,
    label: &str,
) -> impl Fn(
    tauri::Url,
    tauri::webview::NewWindowFeatures,
) -> tauri::webview::NewWindowResponse<tauri::Wry>
       + Send
       + 'static {
    let (app, label) = (app.clone(), label.to_owned());
    move |url, _| {
        if matches!(url.scheme(), "http" | "https" | "mailto") {
            let ev = LinkEvent {
                url: url.to_string(),
            };
            if let Err(e) = app.emit_to(label.as_str(), events::LINK, ev) {
                tracing::warn!(error = %e, "bağlantı olayı gönderilemedi");
            }
        } else {
            tracing::warn!(scheme = url.scheme(), "desteklenmeyen bağlantı açılmadı");
        }
        tauri::webview::NewWindowResponse::Deny
    }
}

pub fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    build_main(app, true)
}

fn build_main(app: &AppHandle, visible: bool) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(MAIN) {
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, MAIN, url("main"))
        .on_navigation(guard(app))
        .on_new_window(new_window(app, MAIN))
        .title("Mailhere")
        .inner_size(1280.0, 820.0)
        // Dar pencere kipleri (ikon rayı 1120 px altı, liste → okuma yığını
        // 860 px altı) gerçek pencerede de kullanılabilsin.
        .min_inner_size(640.0, 560.0)
        .center()
        .decorations(false)
        .visible(visible)
        .build()
}

/// Ana pencere simge durumunda (görev çubuğunda) açılır: tepside başlandı
/// ama simgeyi gösterecek tepsi yok (`tray::started_hidden`). Pencere zaten
/// varsa dokunulmaz.
pub fn create_main_minimized(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(MAIN).is_some() {
        return Ok(());
    }
    let w = build_main(app, false)?;
    w.minimize()?;
    w.show()
}

/// Ana pencereyi gösterir ve öne alır (gerekirse yeniden oluşturur).
pub fn show_main(app: &AppHandle) {
    // Açılış başarısız (yalnız hata iletişim kutusu açık): ikinci açılış
    // durumsuz bir ana pencere yaratmasın. Parola kilidi beklerken çekirdek
    // yoktur ama pencere (kilit ekranı) açılır.
    if app.try_state::<AppState>().is_none() && !mh_app::lock::is_locked(app) {
        return;
    }
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

fn quitting(app: &AppHandle) -> bool {
    app.try_state::<DesktopState>()
        .is_some_and(|s| s.quitting.load(Ordering::SeqCst))
}

/// Ana pencere kapatılınca tepsiye gizlenir (tepsiden "Çık" hariç; tepsi
/// yoksa küçülür, bkz. `tray::main_closed`); bildirim penceresi de kapanmaz,
/// gizlenir.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if quitting(window.app_handle()) {
            return;
        }
        if window.label() == MAIN {
            api.prevent_close();
            crate::tray::main_closed(window);
            // Parola kilidi "her açışta sor" ise gizlenen pencere kilitlenir;
            // çekirdek ve bildirimler sürer.
            mh_app::lock::window_hidden(window.app_handle());
        } else if window.label() == NOTIFY {
            api.prevent_close();
            let _ = notify_hide(window.app_handle());
        }
    }
}

/// Bildirim penceresi; ilk kartta oluşturulur, görünmez başlar ve odak çalmaz.
pub fn ensure_notify(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(NOTIFY) {
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, NOTIFY, url("notify"))
        .on_navigation(guard(app))
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
    let pos = card_position(app);
    let at = current_monitor(app)
        .map(|m| corner(Area::from(&m), pos, NOTIFY_WIDTH, height, NOTIFY_MARGIN));
    #[cfg(target_os = "linux")]
    gtk_place(&w, pos, height, at)?;
    #[cfg(not(target_os = "linux"))]
    {
        w.set_size(LogicalSize::new(NOTIFY_WIDTH, height))?;
        if let Some(at) = at {
            w.set_position(at)?;
        }
    }
    w.set_always_on_top(true)?;
    w.show()?;
    Ok(())
}

/// Linux: kart penceresini GTK'da boyutlandırıp yerleştirir. Pencere
/// yöneticisi (Mutter) odak almış pencerenin uygulama tarafından taşınmasını
/// yok sayar; "Yanıtla"dan sonra pencere büyüyünce aşağı taşar, düğmeler
/// ekran dışında kalırdı. Kart köşesine göre "gravity" verilir: yönetici
/// boyut değişince o köşeyi sabit tutar; konum da o köşeye göre verilir.
#[cfg(target_os = "linux")]
fn gtk_place(
    w: &WebviewWindow,
    pos: CardPosition,
    height: f64,
    at: Option<PhysicalPosition<i32>>,
) -> tauri::Result<()> {
    use gtk::gdk::Gravity;
    use gtk::prelude::GtkWindowExt;

    let scale = w.scale_factor().unwrap_or(1.0).max(0.1);
    let (gravity, east, south) = match pos {
        CardPosition::BottomRight => (Gravity::SouthEast, true, true),
        CardPosition::TopRight => (Gravity::NorthEast, true, false),
        CardPosition::BottomLeft => (Gravity::SouthWest, false, true),
        CardPosition::TopLeft => (Gravity::NorthWest, false, false),
    };
    let (lw, lh) = (NOTIFY_WIDTH.round() as i32, height.round() as i32);
    // GTK konumu mantıksal piksel ve gravity'nin gösterdiği köşe için ister.
    let anchor = at.map(|p| {
        let x = (f64::from(p.x) / scale).round() as i32;
        let y = (f64::from(p.y) / scale).round() as i32;
        (
            x + if east { lw } else { 0 },
            y + if south { lh } else { 0 },
        )
    });
    let win = w.clone();
    w.run_on_main_thread(move || {
        let Ok(gw) = win.gtk_window() else { return };
        gw.set_gravity(gravity);
        gw.resize(lw, lh);
        if let Some((x, y)) = anchor {
            gw.move_(x, y);
        }
    })
}

/// Kart penceresine klavye odağı verir. Pencere odak çalmadan açılır
/// (`focused(false)`); karttaki yanıt alanına yazılabilmesi için kullanıcı
/// "Yanıtla"ya basınca arayüz odak ister. Görünmeyen pencere öne alınmaz.
pub fn notify_focus(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(NOTIFY) {
        if w.is_visible()? {
            w.set_focus()?;
        }
    }
    Ok(())
}

/// Kartlar bitti (ya da sessize alındı): pencere kapanır. Gizli bekletilen
/// web görünümünü işletim sistemi askıya alabiliyor, sonraki kartlar
/// gelmiyordu; yeni kart yeni pencere açar (bkz. `notify_card::show_card`).
pub fn notify_hide(app: &AppHandle) -> tauri::Result<()> {
    if let Some(s) = app.try_state::<DesktopState>() {
        s.notify_ready.store(false, Ordering::SeqCst);
    }
    if let Some(w) = app.get_webview_window(NOTIFY) {
        w.destroy()?;
    }
    Ok(())
}

/// Hızlı okuma penceresi (aynı posta için açıksa öne alınır).
pub fn show_quick(app: &AppHandle, id: MessageId) -> tauri::Result<()> {
    let label = format!("quick-{id}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let (width, height) = (440.0, 700.0);
    let w = WebviewWindowBuilder::new(app, &label, url(&format!("quick/{id}")))
        .on_navigation(guard(app))
        .on_new_window(new_window(app, &label))
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

/// Hızlı okuma penceresini açan komut (yalnız masaüstü). Pencere açan
/// komutlar `async`tır: eşzamanlı komutta pencere oluşturmak Windows'ta
/// (WebView2) kilitlenmeye yol açar.
#[tauri::command]
pub async fn open_quick(app: AppHandle, id: MessageId) -> Result<(), AppError> {
    show_quick(&app, id).map_err(mh_app::state::internal)
}

/// Yeni bir yazma penceresi.
pub fn open_compose(app: &AppHandle, prefill: &ComposePrefill) -> tauri::Result<()> {
    let n = app
        .try_state::<DesktopState>()
        .map(|s| s.compose_counter.fetch_add(1, Ordering::SeqCst) + 1)
        .unwrap_or(1);
    let route = compose_route(prefill);
    let open = app
        .webview_windows()
        .keys()
        .filter(|l| l.starts_with("compose-"))
        .count();
    if open >= MAX_COMPOSE {
        tracing::warn!(open, "yazma penceresi sınırına ulaşıldı");
        return Ok(());
    }
    let label = format!("compose-{n}");
    let w = WebviewWindowBuilder::new(app, &label, url(&route))
        .on_navigation(guard(app))
        .on_new_window(new_window(app, &label))
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
}
