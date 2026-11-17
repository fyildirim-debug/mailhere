//! Doğrulama kodu kolaylıkları: yeni postadaki kodun panoya kendiliğinden
//! kopyalanması (`Settings::otp_auto_copy`) ve süresi dolan kod postalarının
//! Çöp'e taşınması (`Settings::otp_expire_hours`).
//!
//! Pano yalnız kabuk üzerinden yazılır (`Platform::copy_text`; penceresiz
//! çekirdekte `HeadlessShell::copy_text`). Günlüğe kodun kendisi yazılmaz.

use std::time::Duration;

use mh_core::{MessageSummary, Millis, MoveTarget, Settings};
use tauri::{AppHandle, Manager};

use crate::state::{now_ms, AppState};

/// Kendiliğinden kopyalanacak kod en çok bu kadar eski postadan alınır:
/// çevrimdışı kalınan sürede birikmiş eski kodlar panoyu ezmesin.
pub const AUTO_COPY_WINDOW: Millis = 15 * 60 * 1000;
/// Süresi dolan kod postaları bu aralıkla (ve açılışta) taranır.
const EXPIRE_EVERY: Duration = Duration::from_secs(3600);
/// `otp_expire_hours` üst sınırı (30 gün).
pub const EXPIRE_HOURS_MAX: u32 = 24 * 30;
const HOUR: Millis = 3600 * 1000;

/// Kodu panoya kopyalanacak posta: ayar açıksa, bildirim kararına gelen
/// postalardan (kuraldan, elekten ve istenmeyen süzgecinden geçmiş) okunmamış,
/// şüpheli olmayan, kod taşıyan ve taze olanların en yenisi.
pub fn auto_copy_candidate<'a>(
    settings: &Settings,
    messages: &'a [MessageSummary],
    now: Millis,
) -> Option<&'a MessageSummary> {
    if !settings.otp_auto_copy {
        return None;
    }
    messages
        .iter()
        .filter(|m| m.unread && !m.spam && m.otp.is_some())
        .filter(|m| (now - AUTO_COPY_WINDOW..=now + AUTO_COPY_WINDOW).contains(&m.date))
        .max_by_key(|m| (m.date, m.id))
}

/// Yeni postalardaki en taze kodu panoya kopyalar (`copy` kabuğun panosu);
/// başarılıysa ve `otp_mark_read` açıksa postayı okundu sayar. İçeriği
/// değişen kutuları döner.
pub(crate) fn auto_copy(
    state: &AppState,
    settings: &Settings,
    messages: &[MessageSummary],
    now: Millis,
    copy: impl FnOnce(&str) -> bool,
) -> Vec<String> {
    let Some(message) = auto_copy_candidate(settings, messages, now) else {
        return Vec::new();
    };
    let Some(code) = message.otp.as_deref() else {
        return Vec::new();
    };
    if !copy(code) {
        tracing::debug!(id = message.id, "doğrulama kodu panoya kopyalanamadı");
        return Vec::new();
    }
    tracing::info!(id = message.id, "doğrulama kodu panoya kopyalandı");
    if !settings.otp_mark_read {
        return Vec::new();
    }
    match crate::cmd_jmap::set_flags(state, &[message.id], Some(false), None) {
        Ok(touched) => touched,
        Err(e) => {
            tracing::warn!(id = message.id, code = %e.code, "kodu kopyalanan posta okundu sayılamadı");
            Vec::new()
        }
    }
}

/// Süresi dolan kod postalarını Çöp'e taşır (ayar kapalıysa bir şey
/// yapmaz); içeriği değişen kutuları döner. Yalnız gelen kutusundaki, kod
/// taşıyan, yıldızsız ve sabitlenmemiş postalar; taşıma geri alınabilir.
pub(crate) fn expire(state: &AppState, now: Millis) -> Vec<String> {
    let hours = match state.store.settings() {
        Ok(s) => s.otp_expire_hours.min(EXPIRE_HOURS_MAX),
        Err(_) => return Vec::new(),
    };
    if hours == 0 {
        return Vec::new();
    }
    let ids = match state
        .store
        .expired_otp_messages(now - Millis::from(hours) * HOUR)
    {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(code = %e.code, "süresi dolan kod postaları okunamadı");
            return Vec::new();
        }
    };
    if ids.is_empty() {
        return Vec::new();
    }
    match crate::cmd_jmap::move_messages(state, &ids, MoveTarget::Trash) {
        Ok(touched) => {
            tracing::info!(n = ids.len(), "süresi dolan kod postaları çöpe taşındı");
            touched
        }
        Err(e) => {
            tracing::warn!(code = %e.code, "süresi dolan kod postaları taşınamadı");
            Vec::new()
        }
    }
}

/// Penceresiz çekirdeğin kod süresi zamanlayıcısı: açılışta hemen, sonra
/// saatte bir. `changed` kabuğa kutuların değiştiğini bildirir.
pub(crate) async fn expire_loop(state: std::sync::Arc<AppState>, changed: impl Fn()) {
    loop {
        let s = state.clone();
        let touched = tokio::task::spawn_blocking(move || expire(&s, now_ms()))
            .await
            .unwrap_or_default();
        if !touched.is_empty() {
            changed();
        }
        tokio::time::sleep(EXPIRE_EVERY).await;
    }
}

/// Pencereli çekirdeğin kod süresi zamanlayıcısı (`setup_core`): açılışta
/// hemen, sonra saatte bir.
pub async fn expire_ticker(app: AppHandle) {
    loop {
        let handle = app.clone();
        let touched = tokio::task::spawn_blocking(move || {
            handle
                .try_state::<AppState>()
                .map(|state| expire(&state, now_ms()))
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        if !touched.is_empty() {
            crate::sink::changed(&app, touched);
        }
        tokio::time::sleep(EXPIRE_EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::testutil::{deliver, deliver_to, mailbox_of, raw_mail, state};

    const MIN: Millis = 60 * 1000;

    fn code_mail(mid: &str, code: &str) -> Vec<u8> {
        raw_mail(
            "Banka <bildirim@banka.example>",
            "Giriş bildirimi",
            mid,
            "",
            &format!("Doğrulama kodunuz: {code}"),
        )
    }

    fn settings(auto_copy: bool, mark_read: bool) -> Settings {
        Settings {
            otp_auto_copy: auto_copy,
            otp_mark_read: mark_read,
            ..Settings::default()
        }
    }

    #[tokio::test]
    async fn kopyalanacak_kod_secimi() {
        let st = state();
        let now = 1_000 * MIN;
        let old = deliver(&st, "a", code_mail("o1@x", "111111"), now - 30 * MIN);
        let fresh = deliver(&st, "a", code_mail("o2@x", "222222"), now - 5 * MIN);
        let newest = deliver(&st, "a", code_mail("o3@x", "333333"), now - MIN);
        let plain = deliver(
            &st,
            "a",
            raw_mail("Ayşe <ayse@eposta.com>", "Selam", "o4@x", "", "Yarın?"),
            now,
        );
        // Şüpheli postanın kodu kopyalanmaz.
        let spam = deliver(
            &st,
            "a",
            raw_mail(
                "Banka <x@kotu.example>",
                "Giriş",
                "o5@x",
                "X-Spam-Flag: YES\r\n",
                "Doğrulama kodunuz: 444444",
            ),
            now,
        );
        assert_eq!(spam.otp.as_deref(), Some("444444"));
        assert!(spam.spam);
        let all = vec![
            old.clone(),
            fresh.clone(),
            newest.clone(),
            plain.clone(),
            spam.clone(),
        ];
        let on = settings(true, true);
        // En taze kod.
        assert_eq!(
            auto_copy_candidate(&on, &all, now).map(|m| m.id),
            Some(newest.id)
        );
        // Ayar kapalı (varsayılan).
        assert_eq!(auto_copy_candidate(&Settings::default(), &all, now), None);
        // Eski, kodsuz ve şüpheli postalar aday değildir.
        assert_eq!(auto_copy_candidate(&on, &[old, plain, spam], now), None);
        // Okunmuş posta (kod başka cihazda görülmüş) aday değildir.
        let mut read = fresh.clone();
        read.unread = false;
        assert_eq!(auto_copy_candidate(&on, &[read], now), None);
        assert_eq!(
            auto_copy_candidate(&on, std::slice::from_ref(&fresh), now).map(|m| m.id),
            Some(fresh.id)
        );
    }

    #[tokio::test]
    async fn kod_kopyalaninca_okundu_sayilir() {
        let st = state();
        let now = 1_000 * MIN;
        let m = deliver(&st, "a", code_mail("c1@x", "604 218"), now - MIN);
        let unread = |id| st.store.summary(id).unwrap().unwrap().unread;
        let copied = RefCell::new(Vec::<String>::new());
        let copy = |ok: bool| {
            let copied = &copied;
            move |code: &str| {
                copied.borrow_mut().push(code.to_owned());
                ok
            }
        };

        // Pano yazılamadı: okundu sayılmaz.
        let touched = auto_copy(
            &st,
            &settings(true, true),
            std::slice::from_ref(&m),
            now,
            copy(false),
        );
        assert!(touched.is_empty());
        assert!(unread(m.id));
        // Kopyalandı ama "okundu say" kapalı.
        let touched = auto_copy(
            &st,
            &settings(true, false),
            std::slice::from_ref(&m),
            now,
            copy(true),
        );
        assert!(touched.is_empty());
        assert!(unread(m.id));
        // Kopyalandı ve okundu sayıldı; kod boşluksuzdur.
        let touched = auto_copy(
            &st,
            &settings(true, true),
            std::slice::from_ref(&m),
            now,
            copy(true),
        );
        assert_eq!(touched, ["a:INBOX"]);
        assert!(!unread(m.id));
        assert_eq!(*copied.borrow(), ["604218", "604218", "604218"]);
        // Ayar kapalıyken pano hiç çağrılmaz.
        let touched = auto_copy(&st, &settings(false, true), &[m], now, |_: &str| {
            panic!("ayar kapalıyken pano yazılmaz")
        });
        assert!(touched.is_empty());
    }

    #[tokio::test]
    async fn suresi_dolan_kod_postalari_cope_tasinir() {
        let st = state();
        let now = now_ms();
        let old = deliver(&st, "a", code_mail("x1@x", "111111"), now - 2 * HOUR);
        let fresh = deliver(&st, "a", code_mail("x2@x", "222222"), now - 10 * MIN);
        let plain = deliver(
            &st,
            "a",
            raw_mail("Ayşe <ayse@eposta.com>", "Selam", "x3@x", "", "Yarın?"),
            now - 2 * HOUR,
        );
        let archived = deliver_to(
            &st,
            "a",
            "Archive",
            code_mail("x4@x", "333333"),
            now - 2 * HOUR,
        );
        let starred = deliver(&st, "a", code_mail("x5@x", "444444"), now - 2 * HOUR);
        st.store.set_flags(&[starred.id], None, Some(true)).unwrap();

        // Varsayılan kapalı: hiçbir şey taşınmaz.
        assert!(expire(&st, now).is_empty());
        assert_eq!(mailbox_of(&st, old.id), "a:INBOX");

        let mut s = st.store.settings().unwrap();
        s.otp_expire_hours = 1;
        st.store.set_settings(&s).unwrap();
        assert_eq!(expire(&st, now), ["a:INBOX", "a:Trash"]);
        assert_eq!(mailbox_of(&st, old.id), "a:Trash");
        for (id, mailbox) in [
            (fresh.id, "a:INBOX"),
            (plain.id, "a:INBOX"),
            (archived.id, "a:Archive"),
            (starred.id, "a:INBOX"),
        ] {
            assert_eq!(mailbox_of(&st, id), mailbox);
        }
        // İkinci tur: yapılacak iş kalmadı.
        assert!(expire(&st, now).is_empty());
        // 1 gün: 2 saatlik kod postası henüz dolmadı.
        s.otp_expire_hours = 24;
        st.store.set_settings(&s).unwrap();
        st.store
            .set_flags(&[starred.id], None, Some(false))
            .unwrap();
        assert!(expire(&st, now).is_empty());
        assert_eq!(mailbox_of(&st, starred.id), "a:INBOX");
    }
}
