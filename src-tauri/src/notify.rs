//! Köşe kartı ya da işletim sistemi bildirimi.

use std::time::{Duration, Instant};

use mh_core::{events, NotifyEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;

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

/// Bildirim sunucuları gövdede basit işaretleme (`<b>`, `<a href>`)
/// yorumlayabilir; yabancı konu düz metin kalsın.
pub fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn native(app: &AppHandle, ev: &NotifyEvent) {
    let title = escape_markup(&format!(
        "{} · {}",
        ev.message.from.label(),
        ev.account_name
    ));
    let body = if ev.message.subject.trim().is_empty() {
        "(Konu yok)".to_owned()
    } else {
        escape_markup(&ev.message.subject)
    };
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(error = %e, "sistem bildirimi gösterilemedi");
    }
}

/// Kartı gösterir. `native` ayarı ya da GNOME Wayland'de sistem bildirimi.
pub fn show_card(app: &AppHandle, ev: NotifyEvent, native_setting: bool) {
    if native_setting || is_gnome_wayland() {
        native(app, &ev);
        return;
    }
    if let Some(state) = app.try_state::<AppState>() {
        let mut q = state.notify_queue.lock().unwrap_or_else(|e| e.into_inner());
        q.retain(|(at, e)| at.elapsed() < QUEUE_TTL && e.message.id != ev.message.id);
        q.push((Instant::now(), ev.clone()));
        let excess = q.len().saturating_sub(QUEUE_MAX);
        q.drain(..excess);
    }
    if let Err(e) = crate::windows::ensure_notify(app) {
        tracing::warn!(error = %e, "bildirim penceresi açılamadı");
        native(app, &ev);
        return;
    }
    tracing::debug!(id = ev.message.id, "bildirim kartı gönderiliyor");
    if let Err(e) = app.emit_to(crate::windows::NOTIFY, events::NOTIFY, ev) {
        tracing::warn!(error = %e, "bildirim olayı gönderilemedi");
    }
}

/// Bildirim penceresi açıldığında bekleyen kartları verir ve kuyruğu boşaltır.
pub fn take_pending(app: &AppHandle) -> Vec<NotifyEvent> {
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    let mut q = state.notify_queue.lock().unwrap_or_else(|e| e.into_inner());
    tracing::debug!(count = q.len(), "bekleyen kartlar istendi");
    q.drain(..)
        .filter(|(at, _)| at.elapsed() < QUEUE_TTL)
        .map(|(_, e)| e)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bildirim_metni_isaretleme_tasimaz() {
        assert_eq!(
            escape_markup("<a href='x'>Tıkla</a> & <b>"),
            "&lt;a href='x'&gt;Tıkla&lt;/a&gt; &amp; &lt;b&gt;"
        );
    }
}
