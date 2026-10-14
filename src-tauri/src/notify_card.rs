//! Köşe kartı: kuyruk, pencere ve kart komutları (yalnız masaüstü). Sistem
//! bildirimi ortak katmandadır (`mh_app::notify::native`).

use std::time::{Duration, Instant};

use mh_app::state::{now_ms, AppState};
use mh_core::{events, AppError, AppResult, Category, MessageSummary, NotifyEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::platform::DesktopState;

type Res<T> = Result<T, AppError>;

/// Kuyrukta tutulacak en fazla kart ve en uzun bekleme.
const QUEUE_MAX: usize = 10;
const QUEUE_TTL: Duration = Duration::from_secs(120);

/// GNOME Wayland her yeni pencereyi öne alamaz ve konum vermeye izin
/// vermez; orada sistem bildirimi kullanılır.
pub fn is_gnome_wayland() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let wayland =
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"));
    let gnome = std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|v| v.to_ascii_uppercase().contains("GNOME"));
    wayland && gnome
}

/// Kartı gösterir. `native` ayarı ya da GNOME Wayland'de sistem bildirimi.
pub fn show_card(app: &AppHandle, ev: NotifyEvent, native_setting: bool) {
    if native_setting || is_gnome_wayland() {
        mh_app::notify::native(app, &ev);
        return;
    }
    if let Some(state) = app.try_state::<DesktopState>() {
        let mut q = state.cards.lock().unwrap_or_else(|e| e.into_inner());
        q.retain(|(at, e)| at.elapsed() < QUEUE_TTL && e.message.id != ev.message.id);
        q.push((Instant::now(), ev.clone()));
        let excess = q.len().saturating_sub(QUEUE_MAX);
        q.drain(..excess);
    }
    if let Err(e) = crate::windows::ensure_notify(app) {
        tracing::warn!(error = %e, "bildirim penceresi açılamadı");
        mh_app::notify::native(app, &ev);
        return;
    }
    tracing::debug!(id = ev.message.id, "bildirim kartı gönderiliyor");
    if let Err(e) = app.emit_to(crate::windows::NOTIFY, events::NOTIFY, ev) {
        tracing::warn!(error = %e, "bildirim olayı gönderilemedi");
    }
}

/// Bildirim penceresi açıldığında bekleyen kartları verir ve kuyruğu boşaltır.
pub fn take_pending(app: &AppHandle) -> Vec<NotifyEvent> {
    let Some(state) = app.try_state::<DesktopState>() else {
        return Vec::new();
    };
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
        message,
    };
    show_card(app, ev, false);
    Ok(())
}
