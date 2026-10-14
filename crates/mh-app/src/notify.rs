//! Bildirimlerin ortak parçası: metin kaçışlama ve işletim sistemi bildirimi.
//! Köşe kartı penceresi ve kuyruğu masaüstü kabuğundadır (`notify_card.rs`);
//! hangisinin gösterileceğine `Platform::notify` karar verir.

use mh_core::NotifyEvent;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Bildirim sunucuları gövdede basit işaretleme (`<b>`, `<a href>`)
/// yorumlayabilir; yabancı konu düz metin kalsın.
pub fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// İşletim sistemi bildirimi: başlık "gönderen · hesap", gövde konu. Gövde
/// metni ve ek adı taşınmaz.
pub fn native(app: &AppHandle, ev: &NotifyEvent) {
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
