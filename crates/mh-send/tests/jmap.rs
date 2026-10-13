//! Giden kutusunun JMAP yolu, gerçek bir JMAP sunucusuna (Stalwart) karşı.
//!
//! Yalnız `MH_TEST_JMAP=<oturum adresi>|<kullanıcı>|<parola>` tanımlıysa
//! çalışır. Hesaba kendine ekli bir posta gönderir ve sonunda siler.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mh_core::{
    Account, AccountKind, Address, MessageSummary, OutboxEvent, OutgoingMessage, SyncStatus,
};
use mh_jmap::{Client, Credentials};
use mh_send::Outbox;
use mh_store::Store;
use mh_sync::{EventSink, MemorySecrets, SecretStore, SyncManager};
use serde_json::{json, Value};

const ACC: &str = "stalwart";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn giden_kutusu_jmap_ile_gonderir() {
    let Ok(v) = std::env::var("MH_TEST_JMAP") else {
        eprintln!("MH_TEST_JMAP tanımlı değil; gerçek JMAP gönderim testi atlandı");
        return;
    };
    let p: Vec<&str> = v.splitn(3, '|').collect();
    assert_eq!(
        p.len(),
        3,
        "MH_TEST_JMAP biçimi: <oturum adresi>|<kullanıcı>|<parola>"
    );
    let (url, user, pass) = (p[0], p[1], p[2]);
    let client = Client::connect(url, Credentials::new(user, pass))
        .await
        .unwrap();
    let email = client.session().username.clone();

    let store = Arc::new(Store::open_in_memory().unwrap());
    let account = Account {
        id: ACC.into(),
        name: "Stalwart".into(),
        email: email.clone(),
        display_name: "Mailhere Test".into(),
        color: "#D08A1E".into(),
        kind: AccountKind::Jmap,
        imap: None,
        smtp: None,
        jmap_url: Some(url.into()),
        signature: None,
        created_at: 0,
        auth: Default::default(),
        notify: true,
    };
    store.insert_account(&account).unwrap();
    store.set_jmap_username(ACC, user).unwrap();
    mh_jmap::sync_mailboxes(&client, &store, ACC).await.unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    secrets.set(ACC, pass).unwrap();
    let rec = Arc::new(Rec::default());
    let rt = tokio::runtime::Handle::current();
    let sync = Arc::new(SyncManager::new(
        store.clone(),
        secrets.clone(),
        rec.clone(),
        rt.clone(),
    ));
    let outbox = Outbox::new(store.clone(), secrets, sync, rec.clone());
    outbox.start(&rt);

    let dir = std::env::temp_dir().join(format!("mh-send-jmap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Bütçe 2026.pdf");
    std::fs::write(&file, b"%PDF-1.4 deneme").unwrap();
    let stamp = chrono::Utc::now().timestamp_millis();
    let subject = format!("Mailhere giden kutusu JMAP {stamp}");
    let msg = OutgoingMessage {
        account_id: ACC.into(),
        to: vec![Address::new(Some("Mailhere Test"), &email)],
        cc: vec![],
        bcc: vec![],
        subject: subject.clone(),
        body_text: "Ekli posta, Türkçe karakterlerle: ğüşiöç.".into(),
        in_reply_to: None,
        attachments: vec![file.to_string_lossy().into_owned()],
        send_at: Some(0),
        forward_from: None,
        draft_id: None,
    };
    let item = outbox.enqueue(&msg, 0).unwrap();
    for _ in 0..300 {
        if !rec.outbox.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let ev = rec.outbox.lock().unwrap().clone();
    assert_eq!(ev.len(), 1, "gönderim sonuçlanmalı");
    assert!(ev[0].ok, "gönderim başarılı olmalı: {:?}", ev[0].error);
    assert_eq!(ev[0].id, item.id);

    // Sunucuda: Gönderilenler'de $draft olmadan, ekli; gelen kutusuna da düşer.
    let mut found: Vec<Value> = Vec::new();
    for _ in 0..100 {
        let r = client
            .calls(&[
                (
                    "Email/query".into(),
                    json!({ "accountId": client.account_id(), "sort": [{"property": "receivedAt", "isAscending": false}], "limit": 20 }),
                ),
                (
                    "Email/get".into(),
                    json!({ "accountId": client.account_id(),
                        "#ids": { "resultOf": "0", "name": "Email/query", "path": "/ids" },
                        "properties": ["id", "subject", "keywords", "mailboxIds", "hasAttachment"] }),
                ),
            ])
            .await
            .unwrap();
        found = r[1]["list"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["subject"] == subject.as_str())
            .cloned()
            .collect();
        if found.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(found.len(), 2, "gönderilen ve gelen kopya: {found:?}");
    for e in &found {
        assert_eq!(e["hasAttachment"], true);
        assert!(e["keywords"].get("$draft").is_none());
    }
    let ids: Vec<String> = found
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_owned())
        .collect();
    mh_jmap::destroy_emails(&client, &ids).await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
