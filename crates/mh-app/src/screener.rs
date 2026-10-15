//! Yeni gönderici eleği (isteğe bağlı, `Settings.screener`).
//!
//! Karar `new_mail` akışında, kurallardan SONRA ve bildirim kararından ÖNCE
//! verilir (`sink.rs`). Kurallarla etkileşim:
//!
//! - Kural postayı gelen kutusundan çıkardıysa (taşıma, arşiv, çöp) ya da
//!   susturduysa posta eleğe hiç gelmez; kural kazanır.
//! - Kural postayı gelen kutusunda bıraktıysa (okundu sayma, yıldızlama,
//!   iletme) ve gönderici bilinmiyorsa posta yine eleğe alınır; kuralın
//!   bayrakları postada kalır, iletme yapılmış olur.
//! - Elekteki posta için kart ve sistem bildirimi gösterilmez; tepsi sayacı
//!   onu saymaz.
//!
//! Elenmeyenler: kendi adreslerimizden gelenler, bozuk adresli gönderenler,
//! bilinen göndericiler (gönderdiğimiz postaların alıcıları ve kabul
//! edilenler; hesap başına) ve ilk eşitlemenin postaları (eşitleme motoru
//! ilk eşitlemede `new_mail` çağırmaz).

use std::collections::HashSet;

use mh_core::{
    Account, AppError, ErrorCode, MailboxRole, MessageSummary, MoveTarget, ScreenedSender,
};
use tauri::{AppHandle, Manager, State};

use crate::state::{now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Posta eleğe alınır mı? `own`: hesapların kendi adresleri (küçük harf);
/// `known`: gönderici bu hesapta biliniyor.
pub fn should_screen(enabled: bool, from_email: &str, own: &HashSet<String>, known: bool) -> bool {
    if !enabled || known {
        return false;
    }
    let email = mh_store::normalize_email(from_email);
    mh_store::is_full_address(&email) && !own.contains(&email)
}

/// Kurallardan geçen yeni postaları eler; bildirim kararına gidecekleri döner.
pub fn screen(
    app: &AppHandle,
    account: &Account,
    messages: Vec<MessageSummary>,
) -> Vec<MessageSummary> {
    let Some(state) = app.try_state::<AppState>() else {
        return messages;
    };
    let (kept, touched) = screen_with(&state, account, messages);
    if !touched.is_empty() {
        // Liste eşitlemenin `changed` olayıyla postayı göstermiş olabilir.
        crate::sink::changed(app, touched);
    }
    kept
}

/// Eleğin kendisi (pencere bilmez): kalan postalar ve elenenlerin kutuları.
/// Penceresiz çekirdek (`crate::headless`) de bunu kullanır.
pub(crate) fn screen_with(
    state: &AppState,
    account: &Account,
    messages: Vec<MessageSummary>,
) -> (Vec<MessageSummary>, Vec<String>) {
    let enabled = state.store.settings().map(|s| s.screener).unwrap_or(false);
    if !enabled || messages.is_empty() {
        return (messages, Vec::new());
    }
    let own: HashSet<String> = state
        .store
        .accounts()
        .unwrap_or_default()
        .iter()
        .map(|a| mh_store::normalize_email(&a.email))
        .collect();
    let now = now_ms();
    let mut kept = Vec::with_capacity(messages.len());
    let mut touched: Vec<String> = Vec::new();
    for m in messages {
        // Kural bu arada gelen kutusundan çıkardıysa elenmez.
        let in_inbox = state
            .store
            .location(m.id)
            .ok()
            .flatten()
            .is_some_and(|l| l.role == MailboxRole::Inbox);
        let known = state
            .store
            .sender_known(&account.id, &m.from.email)
            .unwrap_or(true);
        if !in_inbox || !should_screen(true, &m.from.email, &own, known) {
            kept.push(m);
            continue;
        }
        match state
            .store
            .screen_message(m.id, &account.id, &m.from.email, now)
        {
            Ok(()) => {
                tracing::debug!(id = m.id, "ilk kez yazan gönderici elekte bekliyor");
                touched.push(m.mailbox_id.clone());
            }
            Err(e) => {
                tracing::warn!(error = %e.message, "posta eleğe alınamadı");
                kept.push(m);
            }
        }
    }
    touched.sort();
    touched.dedup();
    (kept, touched)
}

/// Açılışta bilinen göndericiler mevcut Gönderilenler'den bir kez doldurulur.
pub fn start(app: &AppHandle, rt: &tokio::runtime::Handle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let store = state.store.clone();
    rt.spawn_blocking(move || match store.known_senders_backfill() {
        Ok(0) => {}
        Ok(n) => tracing::info!(n, "bilinen göndericiler Gönderilenler'den dolduruldu"),
        Err(e) => tracing::warn!(error = %e.message, "bilinen göndericiler doldurulamadı"),
    });
}

/// Elekteki göndericiler.
#[tauri::command]
pub fn screener_list(state: State<'_, AppState>) -> Res<Vec<ScreenedSender>> {
    state.store.screened_senders()
}

/// Elekten karar. Kabul: gönderici bilinen olur, postaları gelen kutusuna
/// döner. Engelle: elekteki postaları çöpe gider (engelleme kuralını arayüz
/// `rule_save` ile kurar).
#[tauri::command]
pub fn screener_decide(
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
    account_id: String,
    allow: bool,
) -> Res<()> {
    let email = mh_store::normalize_email(&email);
    if !mh_store::is_full_address(&email) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Gönderici adresi eksik ya da bozuk.",
        ));
    }
    if state.store.account(&account_id)?.is_none() {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "Hesap bulunamadı; kaldırılmış olabilir.",
        ));
    }
    let ids = state
        .store
        .screener_decide(&account_id, &email, allow, now_ms())?;
    let mut touched: Vec<String> = state
        .store
        .locations(&ids)
        .unwrap_or_default()
        .into_iter()
        .map(|l| l.mailbox_id)
        .collect();
    if !allow && !ids.is_empty() {
        touched.extend(crate::cmd_jmap::move_messages(
            &state,
            &ids,
            MoveTarget::Trash,
        )?);
    }
    touched.sort();
    touched.dedup();
    crate::sink::changed(&app, touched);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elek_karari() {
        let own: HashSet<String> = ["ben@ornek.com".to_owned()].into_iter().collect();
        assert!(should_screen(true, "Yeni@Firma.com", &own, false));
        assert!(
            !should_screen(false, "yeni@firma.com", &own, false),
            "ayar kapalı"
        );
        assert!(
            !should_screen(true, "yeni@firma.com", &own, true),
            "bilinen gönderici"
        );
        assert!(
            !should_screen(true, " BEN@ornek.com ", &own, false),
            "kendi adresimiz"
        );
        assert!(
            !should_screen(true, "com", &own, false),
            "bozuk adres elenmez"
        );
        assert!(!should_screen(true, "", &own, false));
    }
}
