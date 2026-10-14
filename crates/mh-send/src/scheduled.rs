//! Planlanmış gönderimler: liste, hemen gönderme ve taslağa çevirme.

use mh_core::{AppError, AppResult, ErrorCode, MessageId, Millis, ScheduledItem};

use crate::outbox::Outbox;

fn gone() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Bu gönderim artık giden kutusunda değil.",
    )
}

impl Outbox {
    /// Giden kutusundaki gönderimler, gönderim anına göre.
    pub fn scheduled(&self) -> AppResult<Vec<ScheduledItem>> {
        self.store.outbox_list()
    }

    /// Gönderimi hemen başlatır. Zaten gönderiliyorsa bir şey yapmaz.
    pub fn send_now(&self, id: &str, now: Millis) -> AppResult<()> {
        if self.store.outbox_send_now(id, now)? {
            self.wake.notify_one();
            self.sink.changed(Vec::new());
            return Ok(());
        }
        match self.store.outbox_claimed(id)? {
            Some(_) => Ok(()),
            None => Err(gone()),
        }
    }

    /// Gönderimi iptal edip postayı taslağa çevirir (gönderimden kendi
    /// taslağı varsa onun yerine yazılır); taslağın kimliğini döner. Gönderim
    /// başladıysa düzenlenemez. Taslak yazılamazsa gönderim sürer. İletilen
    /// postanın içeriği önceden indirilmiş olmalıdır.
    pub fn to_draft(&self, id: &str, now: Millis) -> AppResult<MessageId> {
        let Some(row) = self.store.outbox_take(id)? else {
            return Err(match self.store.outbox_claimed(id)? {
                Some(_) => AppError::new(
                    ErrorCode::Invalid,
                    "Posta gönderilmek üzere; artık düzenlenemez.",
                ),
                None => gone(),
            });
        };
        self.wake.notify_one();
        let mut msg = row.msg.clone();
        msg.send_at = None;
        // Sonradan silinmiş ek dosyası taslağı engellemesin.
        msg.attachments
            .retain(|p| std::path::Path::new(p).is_file());
        match self.save_draft(&msg, now) {
            Ok(draft) => {
                self.sink.changed(Vec::new());
                Ok(draft)
            }
            Err(e) => {
                if let Err(re) = self.store.outbox_restore(&row, now) {
                    tracing::error!(error = %re, "taslağa çevrilemeyen gönderim geri konamadı");
                }
                self.wake.notify_one();
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mh_core::{
        Account, Address, ErrorCode, MessageSummary, OutboxEvent, OutgoingMessage, SyncStatus, View,
    };
    use mh_store::Store;
    use mh_sync::{EventSink, MemorySecrets, SecretStore, SyncManager};

    use crate::Outbox;

    #[derive(Default)]
    struct Rec {
        changed: Mutex<u32>,
        outbox: Mutex<Vec<OutboxEvent>>,
    }

    impl EventSink for Rec {
        fn sync_status(&self, _: SyncStatus) {}
        fn changed(&self, _: Vec<String>) {
            *self.changed.lock().unwrap() += 1;
        }
        fn new_mail(&self, _: &Account, _: Vec<MessageSummary>) {}
        fn outbox(&self, e: OutboxEvent) {
            self.outbox.lock().unwrap().push(e);
        }
    }

    fn setup() -> (Arc<Store>, Arc<Rec>, Arc<Outbox>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-13T10:30:00+03:00").unwrap();
        mh_store::seed_demo(&store, now).unwrap();
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

    fn msg(subject: &str) -> OutgoingMessage {
        OutgoingMessage {
            account_id: "demo-is".into(),
            to: vec![Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com")],
            cc: vec![],
            bcc: vec![],
            subject: subject.into(),
            body_text: "Planlı posta".into(),
            in_reply_to: None,
            attachments: vec![],
            send_at: Some(9_000_000_000_000),
            forward_from: None,
            draft_id: None,
            follow_up_at: None,
        }
    }

    fn drafts(store: &Store) -> Vec<MessageSummary> {
        store
            .list(
                &mh_core::ListQuery {
                    view: View::Drafts,
                    account_id: Some("demo-is".into()),
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 100,
                },
                0,
            )
            .unwrap()
            .items
    }

    async fn run_due(store: &Store, outbox: &Outbox) {
        for row in store.outbox_claim_due(mh_core::Millis::MAX).unwrap() {
            outbox.process(row).await;
        }
    }

    #[tokio::test]
    async fn liste_hemen_gonder_ve_sayac() {
        let (store, rec, outbox) = setup();
        let item = outbox.enqueue(&msg("Planlı"), 1_000).unwrap();
        assert_eq!(*rec.changed.lock().unwrap(), 1, "ekleme sayacı tazeler");
        assert_eq!(store.counts(0).unwrap().scheduled, 1);
        let list = outbox.scheduled().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, item.id);
        assert_eq!(list[0].subject, "Planlı");
        assert_eq!(list[0].send_at, 9_000_000_000_000);

        outbox.send_now(&item.id, 2_000).unwrap();
        assert_eq!(store.outbox_next_due().unwrap(), Some(2_000));
        run_due(&store, &outbox).await;
        assert!(rec.outbox.lock().unwrap()[0].ok);
        assert_eq!(store.counts(0).unwrap().scheduled, 0);
        assert!(*rec.changed.lock().unwrap() >= 3, "gönderim sayacı tazeler");
        assert_eq!(
            outbox.send_now(&item.id, 3_000).unwrap_err().code,
            ErrorCode::NotFound
        );

        // İptal de sayacı tazeler.
        let before = *rec.changed.lock().unwrap();
        let item = outbox.enqueue(&msg("İptal"), 1_000).unwrap();
        assert!(outbox.cancel(&item.id).unwrap());
        assert_eq!(*rec.changed.lock().unwrap(), before + 2);
    }

    #[tokio::test]
    async fn taslaga_cevirme() {
        let (store, _, outbox) = setup();
        let before = drafts(&store).len();
        let item = outbox.enqueue(&msg("Taslağa dönecek"), 1_000).unwrap();
        let draft = outbox.to_draft(&item.id, 2_000).unwrap();
        assert_eq!(store.counts(0).unwrap().scheduled, 0, "gönderim iptal");
        let list = drafts(&store);
        assert_eq!(list.len(), before + 1);
        let d = store.detail(draft, &[]).unwrap();
        assert_eq!(d.summary.subject, "Taslağa dönecek");
        assert_eq!(d.body_text.as_deref(), Some("Planlı posta"));
        assert_eq!(
            outbox.to_draft(&item.id, 3_000).unwrap_err().code,
            ErrorCode::NotFound
        );

        // Taslaktan planlanan posta kendi taslağına döner (yeni taslak açılmaz).
        let mut m = msg("İkinci sürüm");
        m.draft_id = Some(draft);
        let item = outbox.enqueue(&m, 1_000).unwrap();
        assert_eq!(outbox.to_draft(&item.id, 4_000).unwrap(), draft);
        assert_eq!(drafts(&store).len(), before + 1);
        assert_eq!(
            store.summary(draft).unwrap().unwrap().subject,
            "İkinci sürüm"
        );

        // Gönderim başladıysa düzenlenemez.
        let item = outbox.enqueue(&msg("Gidiyor"), 1_000).unwrap();
        outbox.send_now(&item.id, 1_000).unwrap();
        let claimed = store.outbox_claim_due(1_000).unwrap();
        assert_eq!(claimed.len(), 1);
        let e = outbox.to_draft(&item.id, 5_000).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert_eq!(e.message, "Posta gönderilmek üzere; artık düzenlenemez.");
        outbox.send_now(&item.id, 5_000).unwrap();

        // Taslak yazılamazsa gönderim yerinde kalır.
        let mut bad = msg("İletilen yok");
        bad.forward_from = Some(
            store
                .find_by_subject("Cumartesi kahvaltı?")
                .unwrap()
                .unwrap()
                .id,
        );
        let item = outbox.enqueue(&bad, 1_000).unwrap();
        store.delete_messages(&[bad.forward_from.unwrap()]).unwrap();
        assert_eq!(
            outbox.to_draft(&item.id, 6_000).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(outbox.scheduled().unwrap().iter().any(|i| i.id == item.id));
    }

    #[tokio::test]
    async fn kural_iletmesi_hemen_ve_isaretli_gider() {
        let (store, rec, outbox) = setup();
        let original = store
            .find_by_subject("Sözleşme imzaya hazır")
            .unwrap()
            .unwrap();
        let mut m = msg("Fwd: Sözleşme imzaya hazır");
        m.account_id = original.account_id.clone();
        m.send_at = None;
        m.forward_from = Some(original.id);
        let item = outbox.enqueue_auto(&m, 1_000).unwrap();
        assert_eq!(item.send_at, 1_000, "geri alma süresi yok");
        run_due(&store, &outbox).await;
        assert!(rec.outbox.lock().unwrap()[0].ok);
        let sent = store
            .list(
                &mh_core::ListQuery {
                    view: View::Sent,
                    account_id: Some(m.account_id.clone()),
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 100,
                },
                0,
            )
            .unwrap()
            .items;
        let fwd = sent.iter().find(|s| s.subject == m.subject).unwrap();
        assert!(fwd.has_attachments, "ekler kopyalanır");
        let raw = String::from_utf8(store.raw(fwd.id).unwrap().unwrap()).unwrap();
        assert!(raw.contains("Auto-Submitted: auto-forwarded"), "{raw}");
    }
}
