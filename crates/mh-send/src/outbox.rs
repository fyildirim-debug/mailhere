//! Giden kutusu: geri alma süresi, zamanlanmış gönderim, yeniden deneme.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mh_core::{
    Account, AccountKind, AppError, AppResult, ErrorCode, MailboxRole, MessageId, Millis,
    OutboxEvent, OutboxItem, OutgoingMessage,
};
use mh_jmap::JmapManager;
use mh_store::{NewMessage, OutboxRow, Store};
use mh_sync::{credentials_for, EventSink, SecretStore, SyncManager};
use tokio::sync::Notify;

use crate::message::{build_with, validate_with, BuildOptions};
use crate::parts::{forwarded_local, reply_ref, total_size};

/// Kalıcı olmayan hatada en fazla deneme.
const MAX_ATTEMPTS: u32 = 3;

pub struct Outbox {
    pub(crate) store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    pub(crate) sync: Arc<SyncManager>,
    /// JMAP hesaplarının taslakları için; uygulama açılışta bağlar.
    pub(crate) jmap: OnceLock<Arc<JmapManager>>,
    pub(crate) sink: Arc<dyn EventSink>,
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
            jmap: OnceLock::new(),
            sink,
            wake: Notify::new(),
        })
    }

    /// JMAP eşitleyicisini bağlar (taslaklar JMAP hesabında onun kuyruğundan gider).
    pub fn set_jmap(&self, jmap: Arc<JmapManager>) {
        let _ = self.jmap.set(jmap);
    }

    /// Zamanlayıcıyı başlatır.
    pub fn start(self: &Arc<Self>, rt: &tokio::runtime::Handle) {
        let me = self.clone();
        rt.spawn(async move { me.run().await });
    }

    /// Postayı sıraya alır: `send_at` verilmişse o an, yoksa geri alma süresi
    /// sonunda gönderilir. İletilen postanın ekleri (`forward_from`) sınıra
    /// dahildir; içerik yerelde yoksa gönderimde indirilir.
    pub fn enqueue(&self, msg: &OutgoingMessage, now: Millis) -> AppResult<OutboxItem> {
        let forwarded = match msg.forward_from {
            Some(id) => forwarded_local(&self.store, id)?.map_or(0, |list| total_size(&list)),
            None => 0,
        };
        validate_with(msg, forwarded, false)?;
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
            Ok(note) => {
                let _ = self.store.outbox_remove(&row.id);
                if let Some(draft) = row.msg.draft_id {
                    if let Err(e) = self.delete_draft(draft) {
                        tracing::warn!(error = %e, "gönderilen postanın taslağı silinemedi");
                    }
                }
                // Gönderildi; yalnız sonraki adımlarda sorun olduysa not taşır.
                self.sink.outbox(OutboxEvent {
                    id: row.id.clone(),
                    ok: true,
                    error: note,
                });
            }
            Err(e) => {
                let permanent = matches!(
                    e.code,
                    ErrorCode::Auth
                        | ErrorCode::Invalid
                        | ErrorCode::NotFound
                        | ErrorCode::Keyring
                        | ErrorCode::Untrusted
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

    /// Postayı gönderir. SMTP kabul ettikten sonraki adımların (sunucuya
    /// ekleme, yerel kopya) hatası gönderimi başarısız saymaz: yeniden
    /// denenirse alıcıya ikinci kopya gider. Bu durumda kullanıcıya not döner.
    pub(crate) async fn deliver(&self, row: &OutboxRow) -> AppResult<Option<String>> {
        let account = self
            .store
            .account(&row.account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Gönderen hesap bulunamadı."))?;
        let reply = reply_ref(&self.store, &row.msg)?;
        let extra = match row.msg.forward_from {
            Some(id) => self.forwarded(id).await?,
            None => Vec::new(),
        };
        let now = now_ms();
        let opts = BuildOptions {
            reply: reply.as_ref(),
            extra: &extra,
            draft: false,
        };
        let message = build_with(&account, &row.msg, &opts, now)?;
        let raw = message.formatted();
        let mut server_copy = false;
        if account.kind == AccountKind::Jmap {
            // JMAP'te SMTP yok: sunucu gönderir ve Gönderilenler'e kendisi taşır.
            if mh_jmap::is_syncable(&account) {
                let rcpt = mh_jmap::envelope_recipients(&row.msg);
                mh_jmap::send_raw(&self.store, &self.secrets, &account, &raw, &rcpt).await?;
            }
        } else if !is_demo(&account) {
            let cfg = account.smtp.clone().ok_or_else(|| {
                AppError::new(ErrorCode::Invalid, "Hesabın giden posta ayarı yok.")
            })?;
            let creds = credentials_for(&self.secrets, self.sync.tokens(), &account).await?;
            crate::transport::send_with(&cfg, &creds, message).await?;
            server_copy = !crate::transport::server_saves_sent(&cfg);
        }
        // Buradan sonrası gönderimi geri alamaz; hatalar yalnız not olur.
        let mut failed = false;
        if server_copy {
            if let Err(e) = self
                .sync
                .append(&account.id, MailboxRole::Sent, raw.clone())
            {
                tracing::warn!(error = %e, "gönderilen posta sunucuya eklenemedi");
                failed = true;
            }
        }
        if let Err(e) = self.store_local(&account.id, MailboxRole::Sent, raw, now) {
            tracing::warn!(error = %e, "gönderilen postanın yerel kopyası yazılamadı");
            failed = true;
        }
        Ok(failed
            .then(|| "Posta gönderildi ancak Gönderilenler kutusuna kaydedilemedi.".to_owned()))
    }

    /// Postanın yerel kopyası; sunucu kopyası eşitlemede Message-ID ile eşleşir.
    fn store_local(
        &self,
        account_id: &str,
        role: MailboxRole,
        raw: Vec<u8>,
        now: Millis,
    ) -> AppResult<()> {
        // Kutu yoksa sunucunun ad alanında açılacak yolla oluşturulur.
        let mailbox = self.sync.destination(account_id, role)?.mailbox.id;
        let mut m =
            NewMessage::from_raw(raw, false, account_id, &mailbox, now).ok_or_else(|| {
                AppError::new(ErrorCode::Internal, "Gönderilen posta ayrıştırılamadı.")
            })?;
        m.unread = false;
        self.store.upsert_message(&m)?;
        self.sink.changed(vec![mailbox]);
        Ok(())
    }

    /// İletilen postanın ekleri; içerik yerelde yoksa önce indirilir.
    async fn forwarded(&self, id: MessageId) -> AppResult<Vec<mh_mime::AttachmentData>> {
        if let Some(list) = forwarded_local(&self.store, id)? {
            return Ok(list);
        }
        self.sync.fetch_body(id).await?;
        crate::parts::forwarded_required(&self.store, id)
    }

    /// Gönderilemeyen postayı taslak olarak saklar (varsa kendi taslağının yerine).
    fn save_as_draft(&self, row: &OutboxRow) -> AppResult<()> {
        let mut msg = row.msg.clone();
        // Taslak, eksik ek yüzünden yeniden başarısız olmasın.
        msg.attachments.retain(|p| std::path::Path::new(p).exists());
        if self.save_draft(&msg, now_ms()).is_ok() {
            return Ok(());
        }
        // İletilen ekler alınamadıysa en azından metin kalsın.
        msg.forward_from = None;
        self.save_draft(&msg, now_ms()).map(|_| ())
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

    fn view(store: &Store, view: mh_core::View, account: &str) -> Vec<MessageSummary> {
        store
            .list(
                &mh_core::ListQuery {
                    view,
                    account_id: Some(account.into()),
                    category: None,
                    cursor: None,
                    limit: 100,
                },
                now_ms(),
            )
            .unwrap()
            .items
    }

    async fn run_due(store: &Store, outbox: &Outbox) {
        for row in store.outbox_claim_due(now_ms()).unwrap() {
            outbox.process(row).await;
        }
    }

    #[tokio::test]
    async fn taslak_kaydet_yerine_yaz_sil() {
        let (store, _, outbox) = setup();
        let drafts = |s: &Store| view(s, mh_core::View::Drafts, "demo-is");
        let before = drafts(&store).len();

        // Alıcısız taslak kaydedilir; gizli alıcı saklanır.
        let mut m = msg();
        m.to.clear();
        m.bcc = vec![Address::new(None, "gizli@ornek.com.tr")];
        m.subject = "Bütçe planı".into();
        let id = outbox.save_draft(&m, 1_000).unwrap();
        assert_eq!(drafts(&store).len(), before + 1);
        let d = store.detail(id, &[]).unwrap();
        assert_eq!(d.summary.subject, "Bütçe planı");
        assert_eq!(d.bcc, vec![Address::new(None, "gizli@ornek.com.tr")]);
        assert!(!d.summary.unread);
        assert_eq!(d.body_text.as_deref(), Some("Geliyorum!"));

        // Yerine yazınca kimlik ve sayı aynı kalır; metin her kayıtta uzamaz.
        m.draft_id = Some(id);
        m.subject = "Bütçe planı v2".into();
        m.body_text = d.body_text.clone().unwrap();
        m.to = vec![Address::new(Some("Kerem"), "kerem@ornek.com.tr")];
        assert_eq!(outbox.save_draft(&m, 2_000).unwrap(), id);
        assert_eq!(drafts(&store).len(), before + 1);
        let d = store.detail(id, &[]).unwrap();
        assert_eq!(d.summary.subject, "Bütçe planı v2");
        assert_eq!(d.summary.to, m.to);
        assert_eq!(d.body_text.as_deref(), Some(m.body_text.as_str()));

        // Silme yerelden kaldırır; ikinci silme sessizce geçer.
        outbox.delete_draft(id).unwrap();
        assert!(store.summary(id).unwrap().is_none());
        assert_eq!(drafts(&store).len(), before);
        outbox.delete_draft(id).unwrap();

        // Taslak olmayan posta bu yolla silinmez.
        let inbox = store
            .find_by_subject("Cumartesi kahvaltı?")
            .unwrap()
            .unwrap();
        assert_eq!(
            outbox.delete_draft(inbox.id).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert!(store.summary(inbox.id).unwrap().is_some());
    }

    #[tokio::test]
    async fn iletilen_ekler_ve_gonderilen_taslak() {
        let (store, rec, outbox) = setup();
        let original = store
            .find_by_subject("Sözleşme imzaya hazır")
            .unwrap()
            .unwrap();
        let attachment = store.detail(original.id, &[]).unwrap().attachments[0].clone();

        // İletilen posta taslağa ekleriyle kaydedilir.
        let mut m = msg();
        m.account_id = original.account_id.clone();
        m.subject = "Fwd: Sözleşme imzaya hazır".into();
        m.forward_from = Some(original.id);
        let draft = outbox.save_draft(&m, 1_000).unwrap();
        assert_eq!(
            store.detail(draft, &[]).unwrap().attachments,
            vec![attachment.clone()]
        );

        // Taslaktan gönderim: ekler kopyalanır, taslak gönderimden sonra silinir.
        m.draft_id = Some(draft);
        m.send_at = Some(0);
        let item = outbox.enqueue(&m, 0).unwrap();
        assert!(
            store.summary(draft).unwrap().is_some(),
            "geri alma süresinde taslak durur"
        );
        run_due(&store, &outbox).await;
        assert_eq!(
            rec.outbox.lock().unwrap().clone(),
            vec![OutboxEvent {
                id: item.id,
                ok: true,
                error: None
            }]
        );
        assert!(store.summary(draft).unwrap().is_none(), "taslak silindi");
        let sent = view(&store, mh_core::View::Sent, &m.account_id);
        let fwd = sent
            .iter()
            .find(|s| s.subject == "Fwd: Sözleşme imzaya hazır")
            .unwrap();
        assert!(fwd.has_attachments);
        let d = store.detail(fwd.id, &[]).unwrap();
        assert_eq!(d.attachments.len(), 1);
        assert_eq!(
            (d.attachments[0].name.as_str(), d.attachments[0].size),
            (attachment.name.as_str(), attachment.size)
        );
        let a = |id| mh_mime::attachment(&store.raw(id).unwrap().unwrap(), 0).unwrap();
        assert_eq!(a(fwd.id).bytes, a(original.id).bytes, "içerik aynen");
    }

    /// Arayüz, ekli bir taslağı yeniden açınca `forward_from` ve `draft_id`
    /// olarak taslağın kendi kimliğini verir.
    #[tokio::test]
    async fn yeniden_acilan_taslagin_ekleri_korunur() {
        let (store, rec, outbox) = setup();
        let dir = std::env::temp_dir().join(format!("mh-taslak-ek-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Bütçe.xlsx");
        std::fs::write(&path, b"PK\x03\x04 tablo").unwrap();

        let mut m = msg();
        m.attachments = vec![path.to_string_lossy().into_owned()];
        let draft = outbox.save_draft(&m, 1_000).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        // Yeniden açılan taslak: dosya yolu yok, ekler taslağın kendisinden.
        m.attachments.clear();
        m.forward_from = Some(draft);
        m.draft_id = Some(draft);
        m.body_text = "Güncel metin".into();
        assert_eq!(outbox.save_draft(&m, 2_000).unwrap(), draft);
        let d = store.detail(draft, &[]).unwrap();
        assert_eq!(d.attachments.len(), 1);
        assert_eq!(d.attachments[0].name, "Bütçe.xlsx");
        assert_eq!(d.body_text.as_deref(), Some("Güncel metin"));

        m.send_at = Some(0);
        let item = outbox.enqueue(&m, 0).unwrap();
        run_due(&store, &outbox).await;
        assert_eq!(
            rec.outbox.lock().unwrap().clone(),
            vec![OutboxEvent {
                id: item.id,
                ok: true,
                error: None
            }]
        );
        assert!(store.summary(draft).unwrap().is_none());
        let sent = view(&store, mh_core::View::Sent, "demo-is");
        let d = store.detail(sent[0].id, &[]).unwrap();
        assert_eq!(d.attachments.len(), 1);
        assert_eq!(d.attachments[0].name, "Bütçe.xlsx");
        let raw = store.raw(sent[0].id).unwrap().unwrap();
        assert_eq!(
            mh_mime::attachment(&raw, 0).unwrap().bytes,
            b"PK\x03\x04 tablo"
        );
    }

    #[tokio::test]
    async fn olmayan_taslak_yerine_yenisi_acilir() {
        let (store, _, outbox) = setup();
        let mut m = msg();
        m.draft_id = Some(424_242);
        let id = outbox.save_draft(&m, 1_000).unwrap();
        assert_ne!(id, 424_242);
        assert_eq!(store.summary(id).unwrap().unwrap().subject, m.subject);
    }

    #[tokio::test]
    async fn taslaktan_yanit_ayni_konusmada() {
        let (store, _, outbox) = setup();
        let original = store
            .find_by_subject("Cumartesi kahvaltı?")
            .unwrap()
            .unwrap();
        let mut m = msg();
        m.account_id = original.account_id.clone();
        m.in_reply_to = Some(original.id);
        let draft = outbox.save_draft(&m, 1_000).unwrap();
        // Arayüz taslağı yeniden açıp yanıt bağını vermeden gönderir.
        m.in_reply_to = None;
        m.draft_id = Some(draft);
        m.send_at = Some(0);
        outbox.enqueue(&m, 0).unwrap();
        run_due(&store, &outbox).await;
        let sent = view(&store, mh_core::View::Sent, &m.account_id);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].thread_id, original.thread_id);
        let refs = store.reply_refs(sent[0].id).unwrap().unwrap().1;
        assert!(!refs.is_empty(), "References korunur");
    }

    #[tokio::test]
    async fn iletilen_posta_yoksa_reddedilir() {
        let (_, _, outbox) = setup();
        let mut m = msg();
        m.forward_from = Some(987_654);
        assert_eq!(outbox.enqueue(&m, 0).unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(
            outbox.save_draft(&m, 0).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[tokio::test]
    async fn kalici_hatada_kendi_taslagi_guncellenir() {
        let (store, rec, outbox) = setup();
        // Parolası anahtar zincirinde olmayan gerçek (tanıtım dışı) hesap.
        let mut account = store.account("demo-is").unwrap().unwrap();
        account.id = "gercek".into();
        account.imap = None;
        account.smtp = Some(mh_core::ServerConfig {
            host: "smtp.ornek.com.tr".into(),
            port: 465,
            security: mh_core::Security::Tls,
            username: "furkan".into(),
        });
        store.insert_account(&account).unwrap();
        let mut m = msg();
        m.account_id = "gercek".into();
        let draft = outbox.save_draft(&m, 1_000).unwrap();
        m.draft_id = Some(draft);
        m.subject = "Son hali".into();
        m.send_at = Some(0);
        outbox.enqueue(&m, 0).unwrap();
        run_due(&store, &outbox).await;
        let ev = rec.outbox.lock().unwrap().clone();
        assert!(!ev[0].ok);
        assert!(ev[0]
            .error
            .as_deref()
            .unwrap()
            .ends_with("Posta Taslaklar'a kaydedildi."));
        let drafts = view(&store, mh_core::View::Drafts, "gercek");
        assert_eq!(drafts.len(), 1, "ikinci taslak açılmaz");
        assert_eq!(drafts[0].id, draft);
        assert_eq!(drafts[0].subject, "Son hali");
    }

    fn real_account(smtp_port: u16, smtp_host: &str) -> Account {
        Account {
            id: "gercek".into(),
            name: "İş".into(),
            email: "furkan@ornek.test".into(),
            display_name: "Furkan".into(),
            color: "#4C6FD8".into(),
            kind: mh_core::AccountKind::Imap,
            imap: Some(mh_core::ServerConfig {
                host: "imap.ornek.test".into(),
                port: 993,
                security: mh_core::Security::Tls,
                username: "furkan".into(),
            }),
            smtp: Some(mh_core::ServerConfig {
                host: smtp_host.into(),
                port: smtp_port,
                security: mh_core::Security::None,
                username: String::new(),
            }),
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
        }
    }

    #[tokio::test]
    async fn gonderilen_sunucuya_kalici_kuyrukla_eklenir() {
        let (store, rec, outbox) = setup();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            crate::transport::tests::fake_session(sock, true).await
        });
        store
            .insert_account(&real_account(port, "127.0.0.1"))
            .unwrap();
        outbox.secrets.set("gercek", "parola").unwrap();
        let mut m = msg();
        m.account_id = "gercek".into();
        m.send_at = Some(0);
        let item = outbox.enqueue(&m, 0).unwrap();
        for row in store.outbox_claim_due(now_ms()).unwrap() {
            outbox.process(row).await;
        }
        assert!(server
            .await
            .unwrap()
            .data
            .contains("Subject: Re: Cumartesi"));
        assert_eq!(
            rec.outbox.lock().unwrap().clone(),
            vec![OutboxEvent {
                id: item.id,
                ok: true,
                error: None
            }]
        );
        // Sunucu kopyası kalıcı kuyrukta (motor bağlanınca eklenir); yerel
        // kopya Gönderilenler'de.
        let actions = store.actions("gercek").unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0].action,
            mh_store::Action::Append { path, message_id: Some(_) } if path == "Sent"
        ));
        assert!(actions[0].raw.as_ref().is_some_and(|r| !r.is_empty()));
        assert!(store
            .mailbox_by_role("gercek", MailboxRole::Sent)
            .unwrap()
            .is_some_and(|b| b.mailbox.total == 1));
        assert_eq!(store.outbox_next_due().unwrap(), None, "yeniden denenmez");
    }

    #[tokio::test]
    async fn guvenilmeyen_sertifika_yeniden_denenmez() {
        let (store, rec, outbox) = setup();
        let cert = crate::transport::tests::self_signed();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = cert.acceptor.clone();
        tokio::spawn(async move {
            if let Ok((sock, _)) = listener.accept().await {
                let _ = acceptor.accept(sock).await;
            }
        });
        let mut a = real_account(port, "localhost");
        a.smtp.as_mut().unwrap().security = mh_core::Security::Tls;
        store.insert_account(&a).unwrap();
        outbox.secrets.set("gercek", "parola").unwrap();
        let mut m = msg();
        m.account_id = "gercek".into();
        m.send_at = Some(0);
        outbox.enqueue(&m, 0).unwrap();
        for row in store.outbox_claim_due(now_ms()).unwrap() {
            outbox.process(row).await;
        }
        let ev = rec.outbox.lock().unwrap().clone();
        assert!(!ev[0].ok);
        let error = ev[0].error.as_deref().unwrap();
        assert!(error.contains("güvenilir değil"), "{error}");
        assert!(error.contains("Taslaklar"), "{error}");
        assert_eq!(store.outbox_next_due().unwrap(), None, "yeniden denenmez");
        // Gönderilmedi: Gönderilenler'e ekleme yok, yalnız taslak sunucuya gider.
        let actions: Vec<_> = store
            .actions("gercek")
            .unwrap()
            .into_iter()
            .map(|a| a.action)
            .collect();
        assert!(
            matches!(actions.as_slice(), [mh_store::Action::Draft { .. }]),
            "{actions:?}"
        );
    }

    #[tokio::test]
    async fn jmap_taslagi_jmap_esitleyicisinden_gecer() {
        let (store, rec, outbox) = setup();
        let mut m = msg();
        m.account_id = "demo-sunucu".into();
        m.subject = "JMAP taslağı".into();
        // Eşitleyici bağlanmadan JMAP taslağı yazılmaz (sessizce IMAP'e düşmez).
        assert_eq!(
            outbox.save_draft(&m, 1_000).unwrap_err().code,
            ErrorCode::Internal
        );
        outbox.set_jmap(Arc::new(mh_jmap::JmapManager::new(
            store.clone(),
            Arc::new(MemorySecrets::new()),
            rec.clone(),
            tokio::runtime::Handle::current(),
        )));
        let drafts = |s: &Store, acc: &str| view(s, mh_core::View::Drafts, acc);
        let id = outbox.save_draft(&m, 1_000).unwrap();
        assert!(drafts(&store, "demo-sunucu").iter().any(|d| d.id == id));

        // Aynı kimlikle yerine yazılır.
        m.draft_id = Some(id);
        m.subject = "JMAP taslağı 2".into();
        assert_eq!(outbox.save_draft(&m, 2_000).unwrap(), id);
        let list = drafts(&store, "demo-sunucu");
        assert_eq!(
            list.iter()
                .filter(|d| d.subject.starts_with("JMAP taslağı"))
                .count(),
            1
        );

        // Gönderen IMAP hesabına geçince JMAP'teki eski taslak kalkar.
        m.account_id = "demo-is".into();
        let moved = outbox.save_draft(&m, 3_000).unwrap();
        assert!(!drafts(&store, "demo-sunucu")
            .iter()
            .any(|d| d.subject == "JMAP taslağı 2"));
        assert!(drafts(&store, "demo-is").iter().any(|d| d.id == moved));

        outbox.delete_draft(moved).unwrap();
        assert!(!drafts(&store, "demo-is").iter().any(|d| d.id == moved));
    }

    #[tokio::test]
    async fn tanitim_jmap_hesabi_aga_cikmadan_gonderir() {
        let (store, rec, outbox) = setup();
        let mut m = msg();
        m.account_id = "demo-sunucu".into();
        m.subject = "JMAP tanıtım".into();
        m.send_at = Some(0);
        outbox.enqueue(&m, 0).unwrap();
        run_due(&store, &outbox).await;
        assert!(rec.outbox.lock().unwrap()[0].ok);
        let sent = view(&store, mh_core::View::Sent, "demo-sunucu");
        assert!(sent.iter().any(|s| s.subject == "JMAP tanıtım"));
    }
}
