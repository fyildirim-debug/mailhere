//! Yanıt hatırlatmaları: komutlar (`followup_set`, `followups_list`),
//! dakikalık denetim ve süresi dolan hatırlatmanın bildirimi.
//!
//! Denetim her dakika ve uygulama açılırken hemen çalışır; böylece uygulama
//! kapalıyken kaçan süreler açılışta değerlendirilir. Konuşmaya kendi
//! adreslerimiz dışından yanıt gelen hatırlatma kendiliğinden kapanır;
//! yanıt gelmeden süre dolan posta gelen kutusunun üstüne "Yanıt bekleniyor"
//! işaretiyle düşer ve (sessiz saat, susturma, tam ekran ve hesabın bildirim
//! ayarına uyarak) kart ya da sistem bildirimi gösterilir.

use std::time::Duration;

use chrono::Timelike;
use mh_core::{
    Account, AppError, FollowUp, Language, MailboxRole, MessageId, MessageSummary, Millis,
    NotifyEvent, NotifyMode, Settings,
};
use mh_notify::Mute;
use mh_store::Store;
use tauri::{AppHandle, Manager, State};

use crate::state::{now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Denetim sıklığı.
const TICK: Duration = Duration::from_secs(60);

/// Hatırlatma kurar (`at`) ya da kaldırır (`None`). Liste yenilenir.
#[tauri::command]
pub fn followup_set(
    app: AppHandle,
    state: State<'_, AppState>,
    id: MessageId,
    at: Option<Millis>,
) -> Res<()> {
    let now = now_ms();
    if at.is_some_and(|at| at <= now) {
        return Err(AppError::new(
            mh_core::ErrorCode::Invalid,
            "Hatırlatma anı ileride olmalı.",
        ));
    }
    state.store.followup_set(id, at, now)?;
    crate::sink::changed(&app, touched_for(&state.store, &[id]));
    Ok(())
}

/// Açık hatırlatmalar, hatırlatma anına göre.
#[tauri::command]
pub fn followups_list(state: State<'_, AppState>) -> Res<Vec<FollowUp>> {
    state.store.followups_list()
}

/// Listelerin yenilenmesi için: tüm gelen kutuları ve postaların kutuları.
pub(crate) fn touched_for(store: &Store, ids: &[MessageId]) -> Vec<String> {
    let mut out: Vec<String> = store
        .mailboxes()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.role == MailboxRole::Inbox)
        .map(|m| m.id)
        .collect();
    out.extend(
        store
            .locations(ids)
            .unwrap_or_default()
            .into_iter()
            .map(|l| l.mailbox_id),
    );
    out.sort();
    out.dedup();
    out
}

/// Hesapların kendi adresleri (küçük harf).
fn own_addresses(store: &Store) -> Vec<String> {
    store
        .accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| mh_store::normalize_email(&a.email))
        .collect()
}

/// Süresi dolan hatırlatma için kart gösterilir mi? Okunmuş bir postayı
/// duyurduğumuz için `mh_notify::decide` yerine aynı kurallar burada:
/// bildirim kapalı, susturma, sessiz saat ya da tam ekranda kart yok.
pub fn card_allowed(
    settings: &Settings,
    mute: &Mute,
    now: Millis,
    minute_of_day: u32,
    fullscreen: bool,
) -> bool {
    settings.notify != NotifyMode::Off
        && !mute.is_muted(now)
        && !mh_notify::in_quiet_hours(settings, minute_of_day)
        && !fullscreen
}

fn awaiting_label(lang: Language) -> &'static str {
    match lang {
        Language::Tr => "Yanıt bekleniyor",
        Language::En => "Awaiting reply",
    }
}

/// Karttaki özet: gönderen yerine beklenen kişi (ilk alıcı), konunun önünde
/// "Yanıt bekleniyor".
pub fn card_event(mut message: MessageSummary, account: &Account, lang: Language) -> NotifyEvent {
    if let Some(to) = message.to.first().cloned() {
        message.from = to;
    }
    let subject = if message.subject.trim().is_empty() {
        awaiting_label(lang).to_owned()
    } else {
        format!("{}: {}", awaiting_label(lang), message.subject.trim())
    };
    message.subject = subject;
    NotifyEvent {
        message,
        account_name: account.name.clone(),
        account_color: account.color.clone(),
        otp: None,
    }
}

/// Yanıt gelen hatırlatmaları kapatır; kapananların kimliklerini döner.
pub(crate) fn close_answered(state: &AppState) -> Vec<MessageId> {
    let own = own_addresses(&state.store);
    match state.store.followups_close_answered(&own) {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(error = %e.message, "yanıt hatırlatmaları denetlenemedi");
            Vec::new()
        }
    }
}

/// Yeni posta geldi: yanıtlanan hatırlatmalar hemen kapansın.
pub fn on_new_mail(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let closed = close_answered(&state);
    if !closed.is_empty() {
        crate::sink::changed(app, touched_for(&state.store, &closed));
    }
}

/// Bir denetim turu: yanıtlananları kapatır, süresi dolanları ateşler ve
/// bildirir.
pub fn check(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let now = now_ms();
    let mut changed = close_answered(&state);
    let fired = match state.store.followups_fire_due(now) {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!(error = %e.message, "süresi dolan hatırlatmalar okunamadı");
            Vec::new()
        }
    };
    if fired.is_empty() {
        if !changed.is_empty() {
            crate::sink::changed(app, touched_for(&state.store, &changed));
        }
        return;
    }
    changed.extend(fired.iter().map(|m| m.id));
    crate::sink::changed(app, touched_for(&state.store, &changed));

    let Ok(settings) = state.store.settings() else {
        return;
    };
    let minutes = {
        let t = chrono::Local::now();
        t.hour() * 60 + t.minute()
    };
    let fullscreen = mh_notify::is_fullscreen();
    if !card_allowed(&settings, &state.mute, now, minutes, fullscreen) {
        return;
    }
    for message in fired {
        let Ok(Some(account)) = state.store.account(&message.account_id) else {
            continue;
        };
        if !account.notify {
            continue;
        }
        state.platform.notify(
            app,
            card_event(message, &account, settings.language),
            &settings,
        );
    }
}

/// Açılışta hemen, sonra dakikada bir denetler.
pub async fn ticker(app: AppHandle) {
    loop {
        check(&app);
        tokio::time::sleep(TICK).await;
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{AccountKind, Address, Category};

    use super::*;

    fn account() -> Account {
        Account {
            id: "a".into(),
            name: "İş".into(),
            email: "ben@ornek.com".into(),
            display_name: "Ben".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: None,
            smtp: None,
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
        }
    }

    fn summary(subject: &str) -> MessageSummary {
        MessageSummary {
            id: 3,
            account_id: "a".into(),
            mailbox_id: "a:Sent".into(),
            thread_id: "t".into(),
            from: Address::new(Some("Ben"), "ben@ornek.com"),
            to: vec![Address::new(Some("Kerem Aydın"), "kerem@ornek.com")],
            subject: subject.into(),
            snippet: "Taslağı görebilir miyim?".into(),
            date: 0,
            unread: false,
            starred: false,
            has_attachments: false,
            category: Category::People,
            snoozed_until: None,
            follow_up: Some(10),
            pinned: false,
            tags: Vec::new(),
        }
    }

    #[test]
    fn kartta_beklenen_kisi_ve_isaret() {
        let ev = card_event(summary("Teklif"), &account(), Language::Tr);
        assert_eq!(ev.message.from.email, "kerem@ornek.com");
        assert_eq!(ev.message.subject, "Yanıt bekleniyor: Teklif");
        assert_eq!(ev.account_name, "İş");
        assert_eq!(ev.message.id, 3, "kart gönderilmiş postayı açar");
        let ev = card_event(summary("  "), &account(), Language::En);
        assert_eq!(ev.message.subject, "Awaiting reply");
        // Alıcısı olmayan posta gönderenle kalır.
        let mut s = summary("x");
        s.to.clear();
        assert_eq!(
            card_event(s, &account(), Language::Tr).message.from.email,
            "ben@ornek.com"
        );
    }

    #[test]
    fn kart_sessiz_saat_susturma_ve_kapali_bildirime_uyar() {
        let mute = Mute::new();
        let now = 1_000_000;
        let mut s = Settings {
            quiet_start: None,
            quiet_end: None,
            ..Settings::default()
        };
        assert!(card_allowed(&s, &mute, now, 12 * 60, false));
        assert!(!card_allowed(&s, &mute, now, 12 * 60, true), "tam ekran");
        s.notify = NotifyMode::Off;
        assert!(!card_allowed(&s, &mute, now, 12 * 60, false));
        // Yalnız Kişiler ya da VIP ayarı da hatırlatma kartını engellemez.
        s.notify = NotifyMode::Vip;
        assert!(card_allowed(&s, &mute, now, 12 * 60, false));
        s.quiet_start = Some("22:00".into());
        s.quiet_end = Some("08:00".into());
        assert!(!card_allowed(&s, &mute, now, 23 * 60, false), "sessiz saat");
        assert!(card_allowed(&s, &mute, now, 9 * 60, false));
        mute.mute_for(now, 30);
        assert!(
            !card_allowed(&s, &mute, now + 1_000, 9 * 60, false),
            "susturma"
        );
    }
}
