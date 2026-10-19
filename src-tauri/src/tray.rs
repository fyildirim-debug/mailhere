//! Bildirim alanı (sistem tepsisi) simgesi ve menüsü.
//!
//! Simge her zaman görünür; menü: Mailhere'yi aç, Postaları al, Yeni posta,
//! Bildirimleri 1 saat sessize al, Çık. İpucu okunmamış sayısını söyler.
//! Metinler arayüz diline uyar (ayar değişince yenilenir).
//!
//! Ana pencere kapatılınca tepsiye gizlenir; ilk kez gizlenişte bir kez
//! "arka planda sürüyor" bildirimi gösterilir. Simgeye tıklamak (Windows'ta
//! tek ya da çift tık, Linux'ta tepsinin "etkinleştir"i) ana pencereyi açar;
//! menü sağ tıkla gelir.
//!
//! Linux'ta simge doğrudan D-Bus üzerinden StatusNotifierItem olarak yayımlanır
//! (tray-icon `ksni` arka ucu): AppIndicator kitaplığı gerekmez ve sol tık
//! menü değil pencereyi açar. Simgeyi gösterecek bir StatusNotifier izleyicisi
//! yoksa (ör. eklentisiz GNOME) pencere gizlenmez, simge durumuna küçülür:
//! uygulama kaybolmaz, çıkılmaz.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use mh_app::state::{now_ms, AppState};
use mh_core::{events, ComposeMode, ComposePrefill, Language, Settings};
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager, Window, Wry};
use tauri_plugin_notification::NotificationExt;

use crate::platform::DesktopState;
use crate::prefs::PrefsStore;

const TRAY_ID: &str = "main";

/// Tepsi ve menüsünün metinleri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texts {
    pub open: &'static str,
    pub sync: &'static str,
    pub compose: &'static str,
    pub mute: &'static str,
    pub quit: &'static str,
    pub hint_title: &'static str,
    pub hint_body: &'static str,
}

pub fn texts(lang: Language) -> Texts {
    match lang {
        Language::Tr => Texts {
            open: "Mailhere'yi aç",
            sync: "Postaları al",
            compose: "Yeni posta",
            mute: "Bildirimleri 1 saat sessize al",
            quit: "Çık",
            hint_title: "Mailhere arka planda",
            hint_body: "Mailhere arka planda çalışmayı sürdürüyor; tepsideki simgeden açabilirsin.",
        },
        Language::En => Texts {
            open: "Open Mailhere",
            sync: "Get mail",
            compose: "New message",
            mute: "Mute notifications for 1 hour",
            quit: "Quit",
            hint_title: "Mailhere is still running",
            hint_body: "Mailhere keeps running in the background; open it from the tray icon.",
        },
    }
}

/// İpucu metni.
pub fn tooltip(lang: Language, unread: u32) -> String {
    match (lang, unread) {
        (Language::Tr, 0) => "Mailhere · Hepsi okundu".to_owned(),
        (Language::Tr, n) => format!("Mailhere · {n} okunmamış"),
        (Language::En, 0) => "Mailhere · All read".to_owned(),
        (Language::En, n) => format!("Mailhere · {n} unread"),
    }
}

/// Menü öğeleri (dil değişince yeniden adlandırılır) ve tepsinin kurulup
/// kurulmadığı.
struct TrayState {
    items: Mutex<Option<Items>>,
    lang: Mutex<Language>,
    created: AtomicBool,
}

struct Items {
    open: MenuItem<Wry>,
    sync: MenuItem<Wry>,
    compose: MenuItem<Wry>,
    mute: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

fn language(app: &AppHandle) -> Language {
    app.try_state::<AppState>()
        .and_then(|s| s.store.settings().ok())
        .map(|s| s.language)
        .unwrap_or_default()
}

fn tray_state(app: &AppHandle) -> tauri::State<'_, TrayState> {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState {
            items: Mutex::new(None),
            lang: Mutex::new(Language::default()),
            created: AtomicBool::new(false),
        });
    }
    app.state::<TrayState>()
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state = tray_state(app);
    let lang = language(app);
    let t = texts(lang);
    let open = MenuItem::with_id(app, "open", t.open, true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", t.sync, true, None::<&str>)?;
    let compose = MenuItem::with_id(app, "compose", t.compose, true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", t.mute, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sync, &compose, &mute, &sep, &quit])?;
    let icon = Image::from_bytes(include_bytes!("../icons/64x64.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Mailhere")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(on_click)
        .build(app)?;
    *state.lang.lock().unwrap_or_else(|e| e.into_inner()) = lang;
    *state.items.lock().unwrap_or_else(|e| e.into_inner()) = Some(Items {
        open,
        sync,
        compose,
        mute,
        quit,
    });
    state.created.store(true, Ordering::SeqCst);
    // Arayüz dili değişince menü ve ipucu da değişir.
    let handle = app.clone();
    app.listen_any(events::SETTINGS, move |e| {
        if let Ok(s) = serde_json::from_str::<Settings>(e.payload()) {
            relabel(&handle, s.language);
        }
    });
    Ok(())
}

fn relabel(app: &AppHandle, lang: Language) {
    let state = tray_state(app);
    {
        let mut current = state.lang.lock().unwrap_or_else(|e| e.into_inner());
        if *current == lang {
            return;
        }
        *current = lang;
    }
    let t = texts(lang);
    if let Some(items) = state
        .items
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        for (item, text) in [
            (&items.open, t.open),
            (&items.sync, t.sync),
            (&items.compose, t.compose),
            (&items.mute, t.mute),
            (&items.quit, t.quit),
        ] {
            let _ = item.set_text(text);
        }
    }
    refresh(app);
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        "open" => crate::windows::show_main(app),
        "sync" => {
            if let Some(s) = app.try_state::<AppState>() {
                s.sync.sync_now();
                s.jmap.sync_now();
            }
        }
        "compose" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let prefill = ComposePrefill {
                    mode: ComposeMode::New,
                    message_id: None,
                    to: Vec::new(),
                    account_id: None,
                };
                if let Err(e) = crate::windows::open_compose(&app, &prefill) {
                    tracing::warn!(error = %e, "yazma penceresi açılamadı");
                }
            });
        }
        "mute" => {
            if let Some(s) = app.try_state::<AppState>() {
                s.mute.mute_for(now_ms(), 60);
            }
            let _ = crate::windows::notify_hide(app);
        }
        "quit" => {
            if let Some(s) = app.try_state::<DesktopState>() {
                s.quitting.store(true, Ordering::SeqCst);
            }
            app.exit(0);
        }
        _ => {}
    }
}

fn on_click(tray: &TrayIcon, event: TrayIconEvent) {
    let open = matches!(
        event,
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        }
    );
    if open {
        crate::windows::show_main(tray.app_handle());
    }
}

/// Okunmamış sayısını ipucuna yazar.
pub fn refresh(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let unread = state
        .store
        .counts(now_ms())
        .map(|c| c.inbox_unread)
        .unwrap_or(0);
    let lang = *tray_state(app)
        .lang
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip(lang, unread)));
    }
}

/// Tepsi simgesi kullanıcıya görünüyor mu: kuruldu ve (Linux'ta) onu
/// gösterecek bir tepsi var.
pub fn visible(app: &AppHandle) -> bool {
    tray_state(app).created.load(Ordering::SeqCst) && host::present()
}

/// Ana pencere kapatıldı (tepsiden "Çık" değil): tepsi görünüyorsa gizlenir
/// ve ilk seferde bir kez bilgi verilir; tepsi yoksa küçültülür.
pub fn main_closed(window: &Window) {
    let app = window.app_handle();
    if visible(app) {
        let _ = window.hide();
        hint_once(app);
    } else {
        tracing::info!("tepsi yok; ana pencere gizlenmek yerine küçültüldü");
        let _ = window.minimize();
    }
}

/// "Arka planda sürüyor" bilgisini bir kez gösterir (sistem bildirimi).
fn hint_once(app: &AppHandle) {
    let Some(prefs) = app.try_state::<PrefsStore>() else {
        return;
    };
    if prefs.get().tray_hint_shown {
        return;
    }
    if let Err(e) = prefs.update(|p| p.tray_hint_shown = true) {
        tracing::warn!(error = %e, "tepsi bilgisi seçimi kaydedilemedi");
    }
    let t = texts(language(app));
    if let Err(e) = app
        .notification()
        .builder()
        .title(t.hint_title)
        .body(t.hint_body)
        .show()
    {
        tracing::warn!(error = %e, "tepsi bilgisi gösterilemedi");
    }
}

/// Açılış kaydından (`--tepside`) başlandı: pencere açılmadı. Simgeyi
/// gösterecek tepsi yoksa (ya da oturum açılırken kısa sürede gelmezse)
/// pencere simge durumunda açılır ki uygulama kaybolmasın.
pub fn started_hidden(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("mailhere-tepsi".into())
        .spawn(move || {
            // Oturum açılırken masaüstünün tepsisi Mailhere'den sonra gelebilir.
            for _ in 0..HOST_WAIT_SECS {
                if visible(&app) {
                    return;
                }
                if !tray_state(&app).created.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            if visible(&app) {
                return;
            }
            tracing::info!("tepsi yok; ana pencere simge durumunda açılıyor");
            if let Err(e) = crate::windows::create_main_minimized(&app) {
                tracing::error!(error = %e, "ana pencere açılamadı");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "tepsi denetimi başlatılamadı");
    }
}

/// Oturum açılışında tepsinin gelmesi için beklenen en uzun süre.
const HOST_WAIT_SECS: u32 = 20;

/// Tepsinin kendisi: Windows'ta her zaman var; Linux'ta masaüstünün
/// StatusNotifier izleyicisi (KDE, GNOME + AppIndicator eklentisi, XFCE,
/// Cinnamon, MATE, Budgie...).
#[cfg(not(target_os = "linux"))]
mod host {
    pub fn present() -> bool {
        true
    }
}

#[cfg(target_os = "linux")]
mod host {
    use std::time::Duration;

    /// Simgeyi gösterecek bir tepsi var mı. Yanıt vermeyen oturum veriyolu
    /// çağıranı (pencere kapatma) bekletmesin: kısa süre sınırıyla ayrı iş
    /// parçacığında sorulur; yanıt yoksa "var" sayılır (pencere gizlenir).
    pub fn present() -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("mailhere-tepsi-sor".into())
            .spawn(move || {
                let _ = tx.send(status_notifier());
            });
        if spawned.is_err() {
            return true;
        }
        rx.recv_timeout(Duration::from_millis(600)).unwrap_or(true)
    }

    /// StatusNotifier izleyicisi oturum veriyolunda mı.
    fn status_notifier() -> bool {
        let Ok(conn) = dbus::blocking::Connection::new_session() else {
            return false;
        };
        let proxy = conn.with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_millis(400),
        );
        let owned: Result<(bool,), dbus::Error> = proxy.method_call(
            "org.freedesktop.DBus",
            "NameHasOwner",
            ("org.kde.StatusNotifierWatcher",),
        );
        owned.is_ok_and(|(owned,)| owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipucu() {
        assert_eq!(tooltip(Language::Tr, 0), "Mailhere · Hepsi okundu");
        assert_eq!(tooltip(Language::Tr, 3), "Mailhere · 3 okunmamış");
        assert_eq!(tooltip(Language::En, 0), "Mailhere · All read");
        assert_eq!(tooltip(Language::En, 3), "Mailhere · 3 unread");
    }

    #[test]
    fn menu_metinleri_iki_dilde() {
        let tr = texts(Language::Tr);
        assert_eq!(
            [tr.open, tr.sync, tr.quit],
            ["Mailhere'yi aç", "Postaları al", "Çık"]
        );
        assert_eq!(
            tr.hint_body,
            "Mailhere arka planda çalışmayı sürdürüyor; tepsideki simgeden açabilirsin."
        );
        let en = texts(Language::En);
        assert_eq!(
            [en.open, en.sync, en.quit],
            ["Open Mailhere", "Get mail", "Quit"]
        );
        assert_ne!(tr, en);
    }
}
