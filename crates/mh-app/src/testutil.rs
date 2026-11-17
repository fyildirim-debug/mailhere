//! Birim testleri için ağa çıkmayan `AppState`: bellek içi depo, tanıtım
//! sunuculu (eşitlenmeyen) hesaplar ve hiçbir pencereye dokunmayan kabuk.

use std::sync::{Arc, Mutex};

use mh_core::{
    Account, AccountKind, MailboxRole, MessageSummary, Security, ServerConfig, SyncStatus,
};
use mh_jmap::JmapManager;
use mh_send::Outbox;
use mh_store::{NewMessage, Store};
use mh_sync::{EventSink, MemorySecrets, SecretStore, SyncManager};

use crate::state::AppState;

/// Değişen kutuları kaydeden olay havuzu.
#[derive(Default)]
pub struct Rec {
    pub changed: Mutex<Vec<String>>,
}

impl EventSink for Rec {
    fn sync_status(&self, _: SyncStatus) {}
    fn changed(&self, ids: Vec<String>) {
        self.changed.lock().unwrap().extend(ids);
    }
    fn new_mail(&self, _: &Account, _: Vec<MessageSummary>) {}
}

/// Ağa çıkmayan (tanıtım sunuculu) hesap.
pub fn account(id: &str, email: &str) -> Account {
    let server = |host: &str| ServerConfig {
        host: host.into(),
        port: 993,
        security: Security::Tls,
        username: email.into(),
    };
    Account {
        id: id.into(),
        name: id.into(),
        email: email.into(),
        display_name: "Furkan Yıldırım".into(),
        color: "#4C6FD8".into(),
        kind: AccountKind::Imap,
        imap: Some(server("imap.ornek.invalid")),
        smtp: Some(server("smtp.ornek.invalid")),
        jmap_url: None,
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: 0,
        auth: Default::default(),
        notify: true,
    }
}

/// İki hesaplı durum: `a` (furkan@ornek.com.tr; İstenmeyen kutusu var) ve
/// `b` (furkan@eposta.com; İstenmeyen kutusu yok). Tokio çalışma ortamında
/// çağrılmalıdır.
pub fn state() -> AppState {
    let store = Arc::new(Store::open_in_memory().unwrap());
    for (id, email, junk) in [
        ("a", "furkan@ornek.com.tr", true),
        ("b", "furkan@eposta.com", false),
    ] {
        store.insert_account(&account(id, email)).unwrap();
        let mut boxes = vec![
            ("INBOX", MailboxRole::Inbox),
            ("Archive", MailboxRole::Archive),
            ("Trash", MailboxRole::Trash),
            ("Sent", MailboxRole::Sent),
            ("Drafts", MailboxRole::Drafts),
        ];
        if junk {
            boxes.push(("Junk", MailboxRole::Junk));
        }
        for (path, role) in boxes {
            store
                .upsert_mailbox(id, path, path, role, None, false)
                .unwrap();
        }
    }
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    let rec = Arc::new(Rec::default());
    let rt = tokio::runtime::Handle::current();
    let sync = Arc::new(SyncManager::new(
        store.clone(),
        secrets.clone(),
        rec.clone(),
        rt.clone(),
    ));
    let jmap = Arc::new(JmapManager::new(
        store.clone(),
        secrets.clone(),
        rec.clone(),
        rt,
    ));
    let outbox = Outbox::new(store.clone(), secrets.clone(), sync.clone(), rec);
    outbox.set_jmap(jmap.clone());
    AppState {
        store,
        secrets,
        sync,
        jmap,
        outbox,
        mute: mh_notify::Mute::new(),
        cache_dir: std::env::temp_dir(),
        platform: Arc::new(crate::headless::NoopPlatform),
        transfer: Default::default(),
        reconnect: Default::default(),
    }
}

/// Düz metin posta. `extra` ek başlık satırlarıdır (her biri `\r\n` ile biter).
pub fn raw_mail(from: &str, subject: &str, mid: &str, extra: &str, body: &str) -> Vec<u8> {
    format!(
        "{extra}From: {from}\r\nTo: Furkan <furkan@ornek.com.tr>\r\nSubject: {subject}\r\n\
         Message-ID: <{mid}>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
    )
    .into_bytes()
}

/// Hesabın `mailbox` kutusuna (gövdesiyle) posta yazar; özetini döner.
pub fn deliver_to(
    state: &AppState,
    account: &str,
    mailbox: &str,
    raw: Vec<u8>,
    date: i64,
) -> MessageSummary {
    let mailbox = format!("{account}:{mailbox}");
    let mut m = NewMessage::from_raw(raw, false, account, &mailbox, date).unwrap();
    m.date = date;
    let id = state.store.upsert_message(&m).unwrap().id;
    state.store.summary(id).unwrap().unwrap()
}

/// Gelen kutusuna yeni posta.
pub fn deliver(state: &AppState, account: &str, raw: Vec<u8>, date: i64) -> MessageSummary {
    deliver_to(state, account, "INBOX", raw, date)
}

/// Postanın şu anki kutusu.
pub fn mailbox_of(state: &AppState, id: i64) -> String {
    state.store.location(id).unwrap().unwrap().mailbox_id
}
