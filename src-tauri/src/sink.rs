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

/// Kutular değişti: listeler yenilenir, tepsi sayacı güncellenir.
pub fn changed(app: &AppHandle, mailbox_ids: Vec<String>) {
    emit(app, events::CHANGED, ChangedEvent { mailbox_ids });
    crate::tray::refresh(app);
}

impl EventSink for TauriSink {
    fn sync_status(&self, status: SyncStatus) {
        emit(&self.app, events::SYNC, status);
    }

    fn changed(&self, mailbox_ids: Vec<String>) {
        changed(&self.app, mailbox_ids);
    }

    fn new_mail(&self, account: &Account, messages: Vec<MessageSummary>) {
        // Kurallar kart kararından önce uygulanır; eşzamansız iş gerekirse
        // kalan postalar kurallardan sonra bu yönteme yeniden gelir.
        let Some(messages) = crate::rules::before_notify(&self.app, account, messages) else {
            return;
        };
        let Some(state) = self.app.try_state::<AppState>() else {
            return;
        };
        // Kartta hesabın güncel adı ve rengi görünsün (`account_update` sonrası).
        let fresh = state.store.account(&account.id).ok().flatten();
        let account = fresh.as_ref().unwrap_or(account);
        let settings = match state.store.settings() {
            Ok(s) => s,
            Err(_) => return,
        };
        let now = now_ms();
        let minutes = {
            let t = chrono::Local::now();
            t.hour() * 60 + t.minute()
        };
        // Tam ekran (oyun, sunum, video) sırasında kart yok; bir kez sorulur.
        let fullscreen = !messages.is_empty() && mh_notify::is_fullscreen();
        for message in messages {
            if state
                .mute
                .decide(&settings, &message, now, minutes, fullscreen)
                == Decision::Card
            {
                crate::notify::show_card(
                    &self.app,
                    NotifyEvent {
                        message,
                        account_name: account.name.clone(),
                        account_color: account.color.clone(),
                    },
                    settings.native_notifications,
                );
            }
        }
        crate::tray::refresh(&self.app);
    }

    fn outbox(&self, event: OutboxEvent) {
        emit(&self.app, events::OUTBOX, event);
    }
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
