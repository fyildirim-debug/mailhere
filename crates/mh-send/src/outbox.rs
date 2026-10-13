//! Giden kutusu: geri alma süresi, zamanlanmış gönderim, yeniden deneme.

use std::sync::Arc;
use std::time::Duration;

use mh_core::{
    Account, AppError, AppResult, ErrorCode, MailboxRole, Millis, OutboxEvent, OutboxItem,
    OutgoingMessage,
};
use mh_store::{NewMessage, OutboxRow, Store};
use mh_sync::{credentials_for, EventSink, SecretStore, SyncManager};
use tokio::sync::Notify;

use crate::message::{build, validate, ReplyRef};

/// Kalıcı olmayan hatada en fazla deneme.
const MAX_ATTEMPTS: u32 = 3;

pub struct Outbox {
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    sync: Arc<SyncManager>,
    sink: Arc<dyn EventSink>,
    wake: Notify,
}

fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

/// Tanıtım hesabı: ağa çıkmadan "gönderilmiş" sayılır.
fn is_demo(account: &Account) -> bool {
    account
        .smtp
        .as_ref()
        .is_none_or(|s| s.host.trim().to_ascii_lowercase().ends_with(".invalid"))
}

impl Outbox {
    pub fn new(
        store: Arc<Store>,
        secrets: Arc<dyn SecretStore>,
        sync: Arc<SyncManager>,
        sink: Arc<dyn EventSink>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            secrets,
            sync,
            sink,
            wake: Notify::new(),
        })
    }

    /// Zamanlayıcıyı başlatır.
    pub fn start(self: &Arc<Self>, rt: &tokio::runtime::Handle) {
        let me = self.clone();
        rt.spawn(async move { me.run().await });
    }

    /// Postayı sıraya alır: `send_at` verilmişse o an, yoksa geri alma süresi
    /// sonunda gönderilir.
    pub fn enqueue(&self, msg: &OutgoingMessage, now: Millis) -> AppResult<OutboxItem> {
        validate(msg)?;
        if self.store.account(&msg.account_id)?.is_none() {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "Gönderen hesap bulunamadı.",
            ));
        }
        let undo = i64::from(self.store.settings()?.undo_send_seconds) * 1000;
        let send_at = msg.send_at.unwrap_or(now + undo);
        let id = uuid::Uuid::new_v4().to_string();
        self.store.outbox_add(&id, msg, send_at, now)?;
        self.wake.notify_one();
        Ok(OutboxItem { id, send_at })
    }

    /// Gönderim başlamadan geri alır; geri alındıysa `true`.
    pub fn cancel(&self, id: &str) -> AppResult<bool> {
        let removed = self.store.outbox_cancel(id)?;
        if removed {
            self.wake.notify_one();
        }
        Ok(removed)
    }

    async fn run(self: Arc<Self>) {
        loop {
            let now = now_ms();
            match self.store.outbox_claim_due(now) {
                Ok(due) => {
                    for row in due {
                        self.process(row).await;
                    }
                }
                Err(e) => tracing::error!(error = %e, "giden kutusu okunamadı"),
            }
            let wait = match self.store.outbox_next_due() {
                Ok(Some(at)) => Duration::from_millis((at - now_ms()).clamp(50, 30_000) as u64),
                _ => Duration::from_secs(30),
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.wake.notified() => {}
            }
        }
    }

    fn emit(&self, id: &str, error: Option<String>) {
        self.sink.outbox(OutboxEvent {
            id: id.to_owned(),
            ok: error.is_none(),
            error,
        });
    }

    async fn process(&self, row: OutboxRow) {
        match self.deliver(&row).await {
            Ok(()) => {
                let _ = self.store.outbox_remove(&row.id);
                self.emit(&row.id, None);
            }
            Err(e) => {
                let permanent = matches!(
                    e.code,
                    ErrorCode::Auth | ErrorCode::Invalid | ErrorCode::NotFound | ErrorCode::Keyring
                );
                let attempts = row.attempts + 1;
                if !permanent && attempts < MAX_ATTEMPTS {
                    let retry_at = now_ms() + 60_000 * i64::from(attempts);
                    tracing::info!(id = %row.id, attempts, "gönderim yeniden denenecek");
                    let _ = self.store.outbox_retry(&row.id, retry_at, &e.message);
                    return;
                }
                let saved = self.save_as_draft(&row).is_ok();
                let _ = self.store.outbox_remove(&row.id);
                let msg = if saved {
                    format!("{} Posta Taslaklar'a kaydedildi.", e.message)
                } else {
                    e.message.clone()
                };
                self.emit(&row.id, Some(msg));
            }
        }
    }

    async fn deliver(&self, row: &OutboxRow) -> AppResult<()> {
        let account = self
            .store
            .account(&row.account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Gönderen hesap bulunamadı."))?;
        let reply = match row.msg.in_reply_to {
            Some(id) => self
                .store
                .reply_refs(id)?
                .map(|(message_id, references)| ReplyRef {
                    message_id,
                    references,
                }),
            None => None,
        };
        let now = now_ms();
        let message = build(&account, &row.msg, reply.as_ref(), now)?;
        let raw = message.formatted();
        if !is_demo(&account) {
            let cfg = account.smtp.clone().ok_or_else(|| {
                AppError::new(ErrorCode::Invalid, "Hesabın giden posta ayarı yok.")
            })?;
            let creds = credentials_for(&self.secrets, self.sync.tokens(), &account).await?;
            crate::transport::send_with(&cfg, &creds, message).await?;
            if !crate::transport::server_saves_sent(&cfg) {
                if let Err(e) = self
                    .sync
                    .append(&account.id, MailboxRole::Sent, raw.clone())
                {
                    tracing::warn!(error = %e, "gönderilen posta sunucuya eklenemedi");
                }
            }
        }
        self.store_local(&account.id, MailboxRole::Sent, raw, now)
    }

    /// Postanın yerel kopyası; sunucu kopyası eşitlemede Message-ID ile eşleşir.
    fn store_local(
        &self,
        account_id: &str,
        role: MailboxRole,
        raw: Vec<u8>,
        now: Millis,
    ) -> AppResult<()> {
        let mailbox = match self.store.mailbox_by_role(account_id, role)? {
            Some(m) => m.mailbox.id,
            None => {
                let path = if role == MailboxRole::Sent {
                    "Sent"
                } else {
                    "Drafts"
                };
                self.store.upsert_mailbox(account_id, path, path, role)?
            }
        };
        let mut m =
            NewMessage::from_raw(raw, false, account_id, &mailbox, now).ok_or_else(|| {
                AppError::new(ErrorCode::Internal, "Gönderilen posta ayrıştırılamadı.")
            })?;
        m.unread = false;
        self.store.upsert_message(&m)?;
        self.sink.changed(vec![mailbox]);
        Ok(())
    }

    fn save_as_draft(&self, row: &OutboxRow) -> AppResult<()> {
        let account = self
            .store
            .account(&row.account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Hesap bulunamadı."))?;
        let mut msg = row.msg.clone();
        // Taslak, eksik ek yüzünden yeniden başarısız olmasın.
        msg.attachments.retain(|p| std::path::Path::new(p).exists());
        let raw = build(&account, &msg, None, now_ms())?.formatted();
        self.store_local(&account.id, MailboxRole::Drafts, raw, now_ms())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::{Address, MessageSummary, SyncStatus};
    use mh_sync::MemorySecrets;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Rec {
        outbox: Mutex<Vec<OutboxEvent>>,
    }

    impl EventSink for Rec {
        fn sync_status(&self, _: SyncStatus) {}
        fn changed(&self, _: Vec<String>) {}
        fn new_mail(&self, _: &Account, _: Vec<MessageSummary>) {}
        fn outbox(&self, e: OutboxEvent) {
            self.outbox.lock().unwrap().push(e);
        }
    }

    fn msg() -> OutgoingMessage {
        OutgoingMessage {
            account_id: "demo-is".into(),
            to: vec![Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com")],
            cc: vec![],
            bcc: vec![],
            subject: "Re: Cumartesi kahvaltı?".into(),
            body_text: "Geliyorum!".into(),
            in_reply_to: None,
            attachments: vec![],
            send_at: None,
            forward_from: None,
            draft_id: None,
        }
    }

    fn setup() -> (Arc<Store>, Arc<Rec>, Arc<Outbox>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        mh_store::seed_demo(&store, chrono_now()).unwrap();
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
        let rec = Arc::new(Rec::default());
        let sync = Arc::new(SyncManager::new(
            store.clone(),
            secrets.clone(),
            rec.clone(),
            tokio::runtime::Handle::current(),
        ));
        let outbox = Outbox::new(store.clone(), secrets, sync, rec.clone());
        (store, rec, outbox)
    }

    fn chrono_now() -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339("2026-10-13T10:30:00+03:00").unwrap()
    }

    #[tokio::test]
    async fn geri_alma_suresi_ve_iptal() {
        let (store, _, outbox) = setup();
        let item = outbox.enqueue(&msg(), 1_000).unwrap();
        assert_eq!(item.send_at, 1_000 + 10_000, "varsayılan 10 sn geri alma");
        assert!(outbox.cancel(&item.id).unwrap());
        assert!(!outbox.cancel(&item.id).unwrap());
        let mut timed = msg();
        timed.send_at = Some(99_000);
        assert_eq!(outbox.enqueue(&timed, 1_000).unwrap().send_at, 99_000);
        let mut bad = msg();
        bad.account_id = "yok".into();
        assert_eq!(
            outbox.enqueue(&bad, 0).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(store.outbox_next_due().unwrap(), Some(99_000));
    }

    #[tokio::test]
    async fn tanitim_hesabi_yerelde_gonderir() {
        let (store, rec, outbox) = setup();
        let mut m = msg();
        let reply_to = store
            .find_by_subject("Cumartesi kahvaltı?")
            .unwrap()
            .unwrap();
        m.account_id = reply_to.account_id.clone();
        m.in_reply_to = Some(reply_to.id);
        m.send_at = Some(0);
        let item = outbox.enqueue(&m, 0).unwrap();
        for row in store.outbox_claim_due(now_ms()).unwrap() {
            outbox.process(row).await;
        }
        let ev = rec.outbox.lock().unwrap().clone();
        assert_eq!(
            ev,
            vec![OutboxEvent {
                id: item.id,
                ok: true,
                error: None
            }]
        );
        let sent = store
            .list(
                &mh_core::ListQuery {
                    view: mh_core::View::Sent,
                    account_id: Some(m.account_id.clone()),
                    category: None,
                    cursor: None,
                    limit: 10,
                },
                now_ms(),
            )
            .unwrap()
            .items;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].subject, "Re: Cumartesi kahvaltı?");
        assert!(!sent[0].unread);
        assert_eq!(
            sent[0].thread_id, reply_to.thread_id,
            "yanıt aynı konuşmada"
        );
        assert_eq!(store.outbox_next_due().unwrap(), None);
    }
}
