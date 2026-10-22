//! Köşe kartı: kuyruk, pencere ve kart komutları (yalnız masaüstü). Sistem
//! bildirimi ortak katmandadır (`mh_app::notify::native`).

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use mh_app::state::{now_ms, AppState};
use mh_core::{events, AppError, AppResult, Category, MessageSummary, NotifyEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::platform::DesktopState;

type Res<T> = Result<T, AppError>;

/// Kuyrukta tutulacak en fazla kart ve en uzun bekleme.
const QUEUE_MAX: usize = 10;
const QUEUE_TTL: Duration = Duration::from_secs(120);

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// Wayland oturumu mu (XDG_SESSION_TYPE ya da WAYLAND_DISPLAY)?
fn wayland_session(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    session_type.is_some_and(|t| t.trim().eq_ignore_ascii_case("wayland"))
        || wayland_display.is_some()
}

fn gnome(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|d| d.to_ascii_uppercase().contains("GNOME"))
}

/// GTK X11 arka ucuna zorlanmalı mı? Wayland'de uygulama pencere konumu
/// veremez; KWin (KDE Plasma) ve diğer bileşikler kart penceresini ekranın
/// ortasına koyuyordu. XWayland varsa (DISPLAY) GNOME dışı Wayland oturumunda
/// uygulama X11 üzerinden açılır: kart köşesine yerleşir, tepsi D-Bus'la
/// çalışmayı sürdürür. Kullanıcının GDK_BACKEND seçimine dokunulmaz;
/// `MAILHERE_WAYLAND=1` Wayland'de kalmayı seçer. GNOME Wayland'de sistem
/// bildirimi kullanılır (`card_unplaceable`).
pub fn prefer_x11(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    desktop: Option<&str>,
    display: Option<&str>,
    gdk_backend: Option<&str>,
    stay_wayland: Option<&str>,
) -> bool {
    wayland_session(session_type, wayland_display)
        && !gnome(desktop)
        && display.is_some()
        && gdk_backend.is_none()
        && stay_wayland.is_none_or(|v| v.trim() != "1")
}

/// Açılışta, iş parçacıkları başlamadan çağrılır (`main`).
pub fn choose_backend() {
    if !cfg!(target_os = "linux") {
        return;
    }
    if prefer_x11(
        env("XDG_SESSION_TYPE").as_deref(),
        env("WAYLAND_DISPLAY").as_deref(),
        env("XDG_CURRENT_DESKTOP").as_deref(),
        env("DISPLAY").as_deref(),
        env("GDK_BACKEND").as_deref(),
        env("MAILHERE_WAYLAND").as_deref(),
    ) {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

/// Kart penceresi konumlanamaz: GTK Wayland arka ucunda (GNOME Wayland ya da
/// XWayland'siz Wayland). Orada sistem bildirimi kullanılır.
pub fn card_unplaceable() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let session = wayland_session(
        env("XDG_SESSION_TYPE").as_deref(),
        env("WAYLAND_DISPLAY").as_deref(),
    );
    let x11 = env("GDK_BACKEND").is_some_and(|b| b.trim().to_ascii_lowercase().starts_with("x11"));
    session && !x11
}

/// Kartı gösterir. `native` ayarı ya da konumlanamayan oturumda sistem bildirimi.
pub fn show_card(app: &AppHandle, ev: NotifyEvent, native_setting: bool) {
    if native_setting || card_unplaceable() {
        mh_app::notify::native(app, &ev);
        return;
    }
    // Kart penceresi kartlar bitince kapanır (`windows::notify_hide`); gizli
    // bekleyen web görünümü (WebView2, WebKitGTK) askıya alınıp sonraki
    // kartları kaçırıyordu. Pencere açık ve dinliyorsa kart olayla gider;
    // değilse kuyruğa girer, yeni pencere açılınca kuyruktan alır.
    let ready = app
        .try_state::<DesktopState>()
        .is_some_and(|s| s.notify_ready.load(Ordering::SeqCst))
        && app.get_webview_window(crate::windows::NOTIFY).is_some();
    if !ready {
        if let Some(state) = app.try_state::<DesktopState>() {
            let mut q = state.cards.lock().unwrap_or_else(|e| e.into_inner());
            q.retain(|(at, e)| at.elapsed() < QUEUE_TTL && e.message.id != ev.message.id);
            q.push((Instant::now(), ev.clone()));
            let excess = q.len().saturating_sub(QUEUE_MAX);
            q.drain(..excess);
        }
    }
    if let Err(e) = crate::windows::ensure_notify(app) {
        tracing::warn!(error = %e, "bildirim penceresi açılamadı");
        mh_app::notify::native(app, &ev);
        return;
    }
    tracing::info!(id = ev.message.id, ready, "bildirim kartı gönderiliyor");
    if ready {
        if let Err(e) = app.emit_to(crate::windows::NOTIFY, events::NOTIFY, ev) {
            tracing::warn!(error = %e, "bildirim olayı gönderilemedi");
        }
    }
}

/// Bildirim penceresi açıldığında bekleyen kartları verir ve kuyruğu boşaltır.
pub fn take_pending(app: &AppHandle) -> Vec<NotifyEvent> {
    let Some(state) = app.try_state::<DesktopState>() else {
        return Vec::new();
    };
    state.notify_ready.store(true, Ordering::SeqCst);
    let mut q = state.cards.lock().unwrap_or_else(|e| e.into_inner());
    tracing::debug!(count = q.len(), "bekleyen kartlar istendi");
    q.drain(..)
        .filter(|(at, _)| at.elapsed() < QUEUE_TTL)
        .map(|(_, e)| e)
        .collect()
}

// Pencere açan komutlar `async`tır: eşzamanlı komutta pencere oluşturmak
// Windows'ta (WebView2) kilitlenmeye yol açar.

#[tauri::command]
pub async fn notify_layout(app: AppHandle, height: f64) -> Res<()> {
    crate::windows::notify_layout(&app, height).map_err(mh_app::state::internal)
}

/// Yanıt alanı açılınca kart penceresi klavye odağı alır.
#[tauri::command]
pub async fn notify_focus(app: AppHandle) -> Res<()> {
    crate::windows::notify_focus(&app).map_err(mh_app::state::internal)
}

#[tauri::command]
pub async fn notify_hide(app: AppHandle) -> Res<()> {
    crate::windows::notify_hide(&app).map_err(mh_app::state::internal)
}

#[tauri::command]
pub fn notify_pending(app: AppHandle) -> Res<Vec<NotifyEvent>> {
    Ok(take_pending(&app))
}

/// Ekran görüntüsü için: "Cumartesi kahvaltı?" postasıyla bir kart açar.
#[tauri::command]
pub async fn demo_notify(app: AppHandle) -> Res<()> {
    show_demo_card(&app)
}

pub fn show_demo_card(app: &AppHandle) -> AppResult<()> {
    const SUBJECT: &str = "Cumartesi kahvaltı?";
    let state = app.state::<AppState>();
    let message = match state.store.find_by_subject(SUBJECT)? {
        Some(m) => m,
        None => MessageSummary {
            id: 0,
            account_id: String::new(),
            mailbox_id: String::new(),
            thread_id: String::new(),
            from: mh_core::Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com"),
            to: vec![],
            subject: SUBJECT.into(),
            snippet: "Moda'daki yeni yere gidelim mi? Saat 10 gibi buluşursak kalabalığa kalmayız."
                .into(),
            date: now_ms(),
            unread: true,
            starred: false,
            has_attachments: false,
            category: Category::People,
            snoozed_until: None,
            follow_up: None,
            pinned: false,
            tags: Vec::new(),
        },
    };
    let account = state.store.account(&message.account_id)?;
    let ev = NotifyEvent {
        account_name: account
            .as_ref()
            .map_or_else(|| "Kişisel".into(), |a| a.name.clone()),
        account_color: account.map_or_else(|| "#2E9A61".into(), |a| a.color),
        otp: None,
        message,
    };
    show_card(app, ev, false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_wayland_x11e_gecer_gnome_ve_kullanici_secimi_korunur() {
        let w = Some("wayland");
        // KDE Plasma Wayland, XWayland var: X11 arka ucu.
        assert!(prefer_x11(
            w,
            Some("wayland-0"),
            Some("KDE"),
            Some(":0"),
            None,
            None
        ));
        // GNOME Wayland: sistem bildirimi yolu, arka uç değişmez.
        assert!(!prefer_x11(
            w,
            Some("wayland-0"),
            Some("ubuntu:GNOME"),
            Some(":0"),
            None,
            None
        ));
        // XWayland yok, kullanıcı GDK_BACKEND seçmiş ya da Wayland'de kalmak istiyor.
        assert!(!prefer_x11(
            w,
            Some("wayland-0"),
            Some("KDE"),
            None,
            None,
            None
        ));
        assert!(!prefer_x11(
            w,
            Some("wayland-0"),
            Some("KDE"),
            Some(":0"),
            Some("wayland"),
            None
        ));
        assert!(!prefer_x11(
            w,
            Some("wayland-0"),
            Some("KDE"),
            Some(":0"),
            None,
            Some("1")
        ));
        // X11 oturumu: dokunulmaz.
        assert!(!prefer_x11(
            Some("x11"),
            None,
            Some("KDE"),
            Some(":0"),
            None,
            None
        ));
    }
}
