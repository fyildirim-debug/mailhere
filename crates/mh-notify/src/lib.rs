//! Mailhere bildirim kuralları.
//!
//! Yeni bir posta geldiğinde köşe kartı mı açılacağını, yoksa yalnız tepsi
//! rozetinin mi güncelleneceğini belirler. Karar saf bir işlevdir; saat ve
//! tam ekran durumu dışarıdan verilir (`is_fullscreen`).

mod fullscreen;

use std::sync::atomic::{AtomicI64, Ordering};

use mh_core::{Category, MessageSummary, Millis, NotifyMode, Settings};

pub use fullscreen::is_fullscreen;

/// Bildirim kararı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Köşe kartı (ya da işletim sistemi bildirimi) göster.
    Card,
    /// Yalnız tepsi rozeti ve sayaç.
    BadgeOnly,
}

/// "HH:MM" metnini gün içi dakikaya çevirir.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Sessiz saat içinde miyiz? Gece yarısını aşan aralıkları (22:00–08:00)
/// destekler. Başlangıç bitişe eşitse sessiz saat kapalı sayılır.
pub fn in_quiet_hours(settings: &Settings, minute_of_day: u32) -> bool {
    let (Some(start), Some(end)) = (
        settings.quiet_start.as_deref().and_then(parse_hhmm),
        settings.quiet_end.as_deref().and_then(parse_hhmm),
    ) else {
        return false;
    };
    let now = minute_of_day % (24 * 60);
    match start.cmp(&end) {
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Less => now >= start && now < end,
        std::cmp::Ordering::Greater => now >= start || now < end,
    }
}

/// Gönderen önemli kişiler listesinde mi? Büyük/küçük harf duyarsız.
pub fn is_vip(settings: &Settings, email: &str) -> bool {
    let email = email.trim();
    settings
        .vip
        .iter()
        .any(|v| v.trim().eq_ignore_ascii_case(email))
}

/// Yeni posta için karar verir.
///
/// - Okunmuş postalar, tam ekran uygulama ve sessiz saat yalnız rozet üretir.
/// - `people`: yalnız Kişiler kategorisi; `all`: her posta; `vip`: yalnız
///   önemli kişiler; `off`: hiçbir zaman kart yok.
pub fn decide(
    settings: &Settings,
    msg: &MessageSummary,
    local_minutes_of_day: u32,
    is_fullscreen: bool,
) -> Decision {
    if !msg.unread || is_fullscreen || in_quiet_hours(settings, local_minutes_of_day) {
        return Decision::BadgeOnly;
    }
    let card = match settings.notify {
        NotifyMode::Off => false,
        NotifyMode::All => true,
        NotifyMode::People => msg.category == Category::People,
        NotifyMode::Vip => is_vip(settings, &msg.from.email),
    };
    if card {
        Decision::Card
    } else {
        Decision::BadgeOnly
    }
}

/// "Bildirimleri N dakika sessize al" durumu. Bellekte tutulur; uygulama
/// yeniden açılınca sıfırlanır.
#[derive(Debug, Default)]
pub struct Mute {
    until: AtomicI64,
}

impl Mute {
    pub fn new() -> Self {
        Self::default()
    }

    /// `now` anından itibaren `minutes` dakika kart gösterme. 0 susturmayı kaldırır.
    pub fn mute_for(&self, now: Millis, minutes: u32) {
        let until = if minutes == 0 {
            0
        } else {
            now + i64::from(minutes) * 60_000
        };
        self.until.store(until, Ordering::Relaxed);
    }

    pub fn is_muted(&self, now: Millis) -> bool {
        now < self.until.load(Ordering::Relaxed)
    }

    /// Susturmanın bittiği an; susturma yoksa `None`.
    pub fn until(&self, now: Millis) -> Option<Millis> {
        let u = self.until.load(Ordering::Relaxed);
        (now < u).then_some(u)
    }

    /// Susturma da hesaba katılarak karar verir.
    pub fn decide(
        &self,
        settings: &Settings,
        msg: &MessageSummary,
        now: Millis,
        local_minutes_of_day: u32,
        is_fullscreen: bool,
    ) -> Decision {
        if self.is_muted(now) {
            return Decision::BadgeOnly;
        }
        decide(settings, msg, local_minutes_of_day, is_fullscreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::Address;

    fn msg(category: Category, from: &str, unread: bool) -> MessageSummary {
        MessageSummary {
            id: 1,
            account_id: "a".into(),
            mailbox_id: "a:INBOX".into(),
            thread_id: "t".into(),
            from: Address::new(Some("Ayşe Demir"), from),
            to: vec![],
            subject: "Cumartesi kahvaltı?".into(),
            snippet: String::new(),
            date: 0,
            unread,
            starred: false,
            has_attachments: false,
            category,
            snoozed_until: None,
        }
    }

    fn settings(mode: NotifyMode) -> Settings {
        Settings {
            notify: mode,
            quiet_start: None,
            quiet_end: None,
            ..Settings::default()
        }
    }

    const NOON: u32 = 12 * 60;

    #[test]
    fn saat_cozumleme() {
        assert_eq!(parse_hhmm("22:00"), Some(1320));
        assert_eq!(parse_hhmm(" 8:05 "), Some(485));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("öğle"), None);
    }

    #[test]
    fn kisiler_modu() {
        let s = settings(NotifyMode::People);
        assert_eq!(
            decide(&s, &msg(Category::People, "a@b.c", true), NOON, false),
            Decision::Card
        );
        assert_eq!(
            decide(&s, &msg(Category::Newsletters, "a@b.c", true), NOON, false),
            Decision::BadgeOnly
        );
        assert_eq!(
            decide(
                &s,
                &msg(Category::Notifications, "a@b.c", true),
                NOON,
                false
            ),
            Decision::BadgeOnly
        );
    }

    #[test]
    fn tumu_ve_kapali_modu() {
        let all = settings(NotifyMode::All);
        for c in [
            Category::People,
            Category::Notifications,
            Category::Newsletters,
        ] {
            assert_eq!(
                decide(&all, &msg(c, "a@b.c", true), NOON, false),
                Decision::Card
            );
        }
        let off = settings(NotifyMode::Off);
        assert_eq!(
            decide(&off, &msg(Category::People, "a@b.c", true), NOON, false),
            Decision::BadgeOnly
        );
    }

    #[test]
    fn vip_modu_adrese_bakar() {
        let mut s = settings(NotifyMode::Vip);
        s.vip = vec![" Ayse.Demir@Eposta.com ".into()];
        assert_eq!(
            decide(
                &s,
                &msg(Category::Newsletters, "ayse.demir@eposta.com", true),
                NOON,
                false
            ),
            Decision::Card
        );
        assert_eq!(
            decide(
                &s,
                &msg(Category::People, "kerem@ornek.com", true),
                NOON,
                false
            ),
            Decision::BadgeOnly
        );
    }

    #[test]
    fn okunmus_ve_tam_ekran_rozet() {
        let s = settings(NotifyMode::All);
        assert_eq!(
            decide(&s, &msg(Category::People, "a@b.c", false), NOON, false),
            Decision::BadgeOnly
        );
        assert_eq!(
            decide(&s, &msg(Category::People, "a@b.c", true), NOON, true),
            Decision::BadgeOnly
        );
    }

    #[test]
    fn sessiz_saat_gece_yarisini_asar() {
        let s = Settings::default(); // 22:00 – 08:00
        let m = msg(Category::People, "a@b.c", true);
        assert_eq!(decide(&s, &m, 23 * 60, false), Decision::BadgeOnly);
        assert_eq!(decide(&s, &m, 2 * 60, false), Decision::BadgeOnly);
        assert_eq!(decide(&s, &m, 22 * 60, false), Decision::BadgeOnly);
        assert_eq!(decide(&s, &m, 8 * 60, false), Decision::Card);
        assert_eq!(decide(&s, &m, 7 * 60 + 59, false), Decision::BadgeOnly);
        assert_eq!(decide(&s, &m, 21 * 60 + 59, false), Decision::Card);
    }

    #[test]
    fn sessiz_saat_ayni_gun_ve_kapali() {
        let mut s = settings(NotifyMode::All);
        s.quiet_start = Some("13:00".into());
        s.quiet_end = Some("14:30".into());
        let m = msg(Category::People, "a@b.c", true);
        assert_eq!(decide(&s, &m, 13 * 60 + 10, false), Decision::BadgeOnly);
        assert_eq!(decide(&s, &m, 14 * 60 + 30, false), Decision::Card);
        assert_eq!(decide(&s, &m, 12 * 60 + 59, false), Decision::Card);
        s.quiet_end = Some("13:00".into());
        assert!(!in_quiet_hours(&s, 13 * 60));
        s.quiet_end = None;
        assert!(!in_quiet_hours(&s, 13 * 60 + 10));
        s.quiet_end = Some("bozuk".into());
        assert!(!in_quiet_hours(&s, 13 * 60 + 10));
    }

    #[test]
    fn susturma() {
        let mute = Mute::new();
        let s = settings(NotifyMode::All);
        let m = msg(Category::People, "a@b.c", true);
        let now = 1_000_000;
        assert!(!mute.is_muted(now));
        mute.mute_for(now, 60);
        assert!(mute.is_muted(now + 59 * 60_000));
        assert_eq!(mute.until(now), Some(now + 3_600_000));
        assert_eq!(
            mute.decide(&s, &m, now + 1000, NOON, false),
            Decision::BadgeOnly
        );
        assert!(!mute.is_muted(now + 60 * 60_000));
        assert_eq!(
            mute.decide(&s, &m, now + 3_600_000, NOON, false),
            Decision::Card
        );
        mute.mute_for(now, 30);
        mute.mute_for(now, 0);
        assert!(!mute.is_muted(now));
    }
}
