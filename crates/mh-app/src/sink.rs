//! Arka uç olaylarını pencerelere iletir; yeni postada bildirim kararını verir.

use std::time::Duration;

use chrono::Timelike;
use mh_core::{
    events, Account, ChangedEvent, MailboxRole, MessageSummary, NotifyEvent, OutboxEvent,
    SyncStatus,
};
use mh_notify::Decision;
use mh_sync::EventSink;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{now_ms, AppState};

pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

fn emit<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(e) = app.emit(event, payload) {
        tracing::warn!(event, error = %e, "olay gönderilemedi");
    }
}

/// Okunmamış sayısı değişmiş olabilir: kabuk sayacını (tepsi ipucu) yeniler.
fn counts_changed(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.platform.counts_changed(app);
    }
}

/// Kutular değişti: listeler yenilenir, tepsi sayacı güncellenir.
pub fn changed(app: &AppHandle, mailbox_ids: Vec<String>) {
    emit(app, events::CHANGED, ChangedEvent { mailbox_ids });
    counts_changed(app);
}

impl TauriSink {
    /// Yeni posta akışının ilk adımı (`crate::spam::filter_new`): engellenen
    /// göndericinin ve taşınacak şüpheli postaların gelen kutusundan
    /// çıkarılması; kalanlar akışa devam eder.
    fn filter_spam(&self, account: &Account, messages: Vec<MessageSummary>) -> Vec<MessageSummary> {
        let Some(state) = self.app.try_state::<AppState>() else {
            return messages;
        };
        let (kept, touched) = crate::spam::filter_new(&state, account, messages);
        if !touched.is_empty() {
            // Liste eşitlemenin `changed` olayıyla postayı göstermiş olabilir.
            changed(&self.app, touched);
        }
        kept
    }
}

impl EventSink for TauriSink {
    fn sync_status(&self, status: SyncStatus) {
        emit(&self.app, events::SYNC, status);
    }

    fn changed(&self, mailbox_ids: Vec<String>) {
        changed(&self.app, mailbox_ids);
    }

    fn new_mail(&self, account: &Account, messages: Vec<MessageSummary>) {
        // Sıra: (0) engellenen/şüpheli süzgeci → (1) kurallar → (2) elek →
        // (3) bildirim. Süzgeç yalnız ilk girişte çalışır; kurallardan dönen
        // postalar yeniden süzülmez.
        let messages = if crate::rules::after_rules() {
            messages
        } else {
            self.filter_spam(account, messages)
        };
        // Kurallar kart kararından önce uygulanır; eşzamansız iş gerekirse
        // kalan postalar kurallardan sonra bu yönteme yeniden gelir.
        let Some(messages) = crate::rules::before_notify(&self.app, account, messages) else {
            return;
        };
        // Yeni posta bir yanıt olabilir: yanıtlanan hatırlatmalar kapanır.
        crate::followups::on_new_mail(&self.app);
        // Yeni gönderici eleği kurallardan sonra, bildirimden önce: elenen
        // posta gelen kutusunda görünmez ve kart açmaz (`screener.rs`).
        let messages = crate::screener::screen(&self.app, account, messages);
        let Some(state) = self.app.try_state::<AppState>() else {
            return;
        };
        let touched = decide(
            &state,
            account,
            messages,
            // Arayüz kilitliyken kod panoya yazılmaz (kart da içeriksizdir).
            |code| !crate::lock::ui_locked(&self.app) && state.platform.copy_text(&self.app, code),
            |ev, settings| state.platform.notify(&self.app, ev, settings),
        );
        if touched.is_empty() {
            state.platform.counts_changed(&self.app);
        } else {
            // Kodu kopyalanan posta okundu sayıldı: listeler de yenilenir.
            changed(&self.app, touched);
        }
    }

    fn outbox(&self, event: OutboxEvent) {
        emit(&self.app, events::OUTBOX, event);
    }
}

/// Bildirim kararında kullanılacak ayarlar. Ayarlar okunamazsa (depo hatası)
/// yeni posta sessizce yutulmaz: hata günlüğe yazılır (içerik ve adres
/// yazılmaz) ve varsayılan ayarlarla karar verilir. "Posta hızlı gelir":
/// bozuk bir ayar kaydı bildirimi durdurmamalı.
fn settings_or_default(read: mh_core::AppResult<mh_core::Settings>) -> mh_core::Settings {
    read.unwrap_or_else(|e| {
        tracing::warn!(
            code = %e.code,
            "ayarlar okunamadı; yeni posta varsayılan ayarlarla bildirilecek"
        );
        mh_core::Settings::default()
    })
}

/// Bildirim kararı: hesabın bildirimi, kullanıcı ayarları (kime: herkes /
/// kişiler / VIP, sessiz saat), susturma ve tam ekran bir arada; kart
/// gösterilecek her posta için `notify` çağrılır. Pencereli (`TauriSink`) ve
/// penceresiz (`crate::headless`) çekirdek aynı kararı verir.
///
/// Karardan önce iki iş yapılır: şüpheli postalar kartsız geçer
/// (`crate::spam::notifiable`) ve en taze doğrulama kodu ayar açıksa `copy`
/// ile panoya yazılır (`crate::otp::auto_copy`; kart yine gösterilir). Kod
/// kopyalanınca okundu sayılan postanın kutularını döner.
pub(crate) fn decide(
    state: &AppState,
    account: &Account,
    messages: Vec<MessageSummary>,
    copy: impl FnOnce(&str) -> bool,
    mut notify: impl FnMut(NotifyEvent, &mh_core::Settings),
) -> Vec<String> {
    let settings = settings_or_default(state.store.settings());
    // Şüpheli posta (uyar kipi ya da taşınamayan) kart ve bildirim almaz;
    // kodu da kopyalanmaz.
    let (messages, suspicious): (Vec<_>, Vec<_>) = messages
        .into_iter()
        .partition(|m| crate::spam::notifiable(&settings, m));
    for m in &suspicious {
        tracing::info!(id = m.id, kart = false, supheli = true, "yeni posta");
    }
    let now = now_ms();
    let touched = crate::otp::auto_copy(state, &settings, &messages, now, copy);
    // Kartta hesabın güncel adı ve rengi görünsün (`account_update` sonrası).
    let fresh = state.store.account(&account.id).ok().flatten();
    let account = fresh.as_ref().unwrap_or(account);
    // Bu hesap için bildirim kapalı: kart ve sistem bildirimi yok, yalnız
    // sayaç güncellenir (çağıran yapar).
    if !account.notify {
        return touched;
    }
    let minutes = {
        let t = chrono::Local::now();
        t.hour() * 60 + t.minute()
    };
    // Tam ekran (oyun, sunum, video) sırasında kart yok; bir kez sorulur.
    let fullscreen = !messages.is_empty() && mh_notify::is_fullscreen();
    for message in messages {
        let decision = state
            .mute
            .decide(&settings, &message, now, minutes, fullscreen);
        // Kaçan bildirimi izlemek için (içerik yazılmaz).
        tracing::info!(
            id = message.id,
            kart = decision == Decision::Card,
            okunmamis = message.unread,
            sessiz = state.mute.is_muted(now),
            tam_ekran = fullscreen,
            "yeni posta"
        );
        if decision == Decision::Card {
            // Doğrulama kodu kartta "Kodu kopyala" olarak sunulur. Özet
            // kurallardan önce okunmuş olabilir; gövde bu arada indiyse
            // (gövdeye bakan kural) kod depodaki güncel satırdan alınır.
            let otp = message.otp.clone().or_else(|| {
                let fresh = state.store.summary(message.id).ok().flatten();
                fresh.and_then(|m| m.otp)
            });
            notify(
                NotifyEvent {
                    message,
                    account_name: account.name.clone(),
                    account_color: account.color.clone(),
                    otp,
                },
                &settings,
            );
        }
    }
    touched
}

/// Süresi dolan ertelemeleri gelen kutusuna geri getirir (liste yenilenir).
pub async fn snooze_ticker(app: AppHandle) {
    let mut last = now_ms();
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let now = now_ms();
        let Some(state) = app.try_state::<AppState>() else {
            continue;
        };
        if state.store.snoozes_expired_between(last, now).unwrap_or(0) > 0 {
            let inboxes = state
                .store
                .mailboxes()
                .unwrap_or_default()
                .into_iter()
                .filter(|m| m.role == MailboxRole::Inbox)
                .map(|m| m.id)
                .collect();
            changed(&app, inboxes);
        }
        last = now;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use mh_core::{NotifyMode, Settings, SpamFilter};

    use super::*;
    use crate::testutil::{deliver, raw_mail, state};

    /// Saatten bağımsız ayar: sessiz saat yok, her posta kart açar.
    fn base() -> Settings {
        Settings {
            notify: NotifyMode::All,
            quiet_start: None,
            quiet_end: None,
            ..Settings::default()
        }
    }

    /// Kararı çalıştırır: (kart açan postalar ve kodları, panoya yazılanlar,
    /// değişen kutular).
    #[allow(clippy::type_complexity)]
    fn run(
        st: &AppState,
        messages: Vec<MessageSummary>,
        copy_ok: bool,
    ) -> (Vec<(i64, Option<String>)>, Vec<String>, Vec<String>) {
        let account = st.store.account("a").unwrap().unwrap();
        let cards = RefCell::new(Vec::new());
        let copied = RefCell::new(Vec::new());
        let touched = decide(
            st,
            &account,
            messages,
            |code| {
                copied.borrow_mut().push(code.to_owned());
                copy_ok
            },
            |ev, _| cards.borrow_mut().push((ev.message.id, ev.otp)),
        );
        (cards.into_inner(), copied.into_inner(), touched)
    }

    #[test]
    fn ayarlar_okunamazsa_varsayilanla_bildirilir() {
        // Depo hatası: karar sessizce düşmez, varsayılan ayarlar kullanılır.
        let broken = Err(mh_core::AppError::new(
            mh_core::ErrorCode::Storage,
            "Veritabanı okunamadı.",
        ));
        assert_eq!(settings_or_default(broken), Settings::default());
        let custom = Settings {
            card_seconds: 20,
            ..Settings::default()
        };
        assert_eq!(settings_or_default(Ok(custom.clone())), custom);
        // Varsayılan ayarlar yeni postayı duyurur (bildirim kapalı değildir).
        assert_eq!(Settings::default().notify, NotifyMode::All);
    }

    #[tokio::test]
    async fn supheli_posta_kart_acmaz() {
        let st = state();
        st.store.set_settings(&base()).unwrap();
        let now = now_ms();
        let clean = deliver(
            &st,
            "a",
            raw_mail("Ayşe <ayse@eposta.com>", "Selam", "d1@x", "", "Yarın?"),
            now,
        );
        let spam = deliver(
            &st,
            "a",
            raw_mail(
                "Banka <x@kotu.example>",
                "Hesabınız askıda",
                "d2@x",
                "X-Spam-Flag: YES\r\n",
                "Tıklayın.",
            ),
            now,
        );
        // Uyar (varsayılan): yalnız temiz posta kart açar.
        let (cards, _, _) = run(&st, vec![clean.clone(), spam.clone()], true);
        assert_eq!(cards, [(clean.id, None)]);
        // Taşı (taşınamamış posta buraya gelirse): yine kart yok.
        st.store
            .set_settings(&Settings {
                spam_filter: SpamFilter::Move,
                ..base()
            })
            .unwrap();
        let (cards, _, _) = run(&st, vec![spam.clone()], true);
        assert!(cards.is_empty());
        // Kapalı: olağan akış.
        st.store
            .set_settings(&Settings {
                spam_filter: SpamFilter::Off,
                ..base()
            })
            .unwrap();
        let (cards, _, _) = run(&st, vec![clean.clone(), spam.clone()], true);
        assert_eq!(cards, [(clean.id, None), (spam.id, None)]);
    }

    #[tokio::test]
    async fn kod_kopyalanir_kart_yine_gosterilir() {
        let st = state();
        let now = now_ms();
        let code = |mid: &str| {
            deliver(
                &st,
                "a",
                raw_mail(
                    "Banka <bildirim@banka.example>",
                    "Giriş bildirimi",
                    mid,
                    "",
                    "Doğrulama kodunuz: 604218",
                ),
                now,
            )
        };
        let unread = |id| st.store.summary(id).unwrap().unwrap().unread;

        // Varsayılan: kopyalama kapalı; kart kodu taşır.
        st.store.set_settings(&base()).unwrap();
        let m = code("k1@x");
        let (cards, copied, touched) = run(&st, vec![m.clone()], true);
        assert_eq!(cards, [(m.id, Some("604218".into()))]);
        assert!(copied.is_empty() && touched.is_empty());
        assert!(unread(m.id));

        // Kopyalama açık: panoya yazılır, posta okundu sayılır, kart yine çıkar.
        st.store
            .set_settings(&Settings {
                otp_auto_copy: true,
                ..base()
            })
            .unwrap();
        let (cards, copied, touched) = run(&st, vec![m.clone()], true);
        assert_eq!(cards, [(m.id, Some("604218".into()))]);
        assert_eq!(copied, ["604218"]);
        assert_eq!(touched, ["a:INBOX"]);
        assert!(!unread(m.id));

        // Pano yazılamazsa okundu sayılmaz; kart yine çıkar.
        let m2 = code("k2@x");
        let (cards, copied, touched) = run(&st, vec![m2.clone()], false);
        assert_eq!(cards.len(), 1);
        assert_eq!(copied, ["604218"]);
        assert!(touched.is_empty());
        assert!(unread(m2.id));

        // "Okundu say" kapalı: kopyalanır, okunmamış kalır.
        st.store
            .set_settings(&Settings {
                otp_auto_copy: true,
                otp_mark_read: false,
                ..base()
            })
            .unwrap();
        let (_, copied, touched) = run(&st, vec![m2.clone()], true);
        assert_eq!(copied, ["604218"]);
        assert!(touched.is_empty());
        assert!(unread(m2.id));

        // Hesabın bildirimi kapalı: kart yok ama kod yine kopyalanır.
        let mut account = st.store.account("a").unwrap().unwrap();
        account.notify = false;
        st.store.insert_account(&account).unwrap();
        let (cards, copied, _) = run(&st, vec![m2.clone()], true);
        assert!(cards.is_empty());
        assert_eq!(copied, ["604218"]);
    }
}
