//! Gerçek bir JMAP sunucusuna (Stalwart) karşı uçtan uca test.
//!
//! Yalnız `MH_TEST_JMAP=<oturum adresi>|<kullanıcı>|<parola>` tanımlıysa
//! çalışır. Hesaba kendine posta gönderir, bayrak/taşıma/taslak işlemlerini
//! dener ve oluşturduğu postaları sonunda siler; yalnız test hesabıyla çalıştır.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mh_core::{
    Account, AccountKind, ErrorCode, ListQuery, MailboxRole, MessageSummary, MoveTarget, SyncState,
    SyncStatus, View,
};
use mh_jmap::{Client, Credentials, JmapManager, Timing};
use mh_store::Store;
use mh_sync::{EventSink, MemorySecrets, SecretStore};
use serde_json::{json, Value};

const ACC: &str = "stalwart";

fn env_config() -> Option<(String, String, String)> {
    let v = std::env::var("MH_TEST_JMAP").ok()?;
    let p: Vec<&str> = v.splitn(3, '|').collect();
    if p.len() != 3 {
        panic!("MH_TEST_JMAP biçimi: <oturum adresi>|<kullanıcı>|<parola>");
    }
    Some((p[0].into(), p[1].into(), p[2].into()))
}

#[derive(Default)]
struct Rec {
    statuses: Mutex<Vec<SyncStatus>>,
    new_mail: Mutex<Vec<MessageSummary>>,
}

impl EventSink for Rec {
    fn sync_status(&self, status: SyncStatus) {
        self.statuses.lock().unwrap().push(status);
    }
    fn changed(&self, _: Vec<String>) {}
    fn new_mail(&self, _: &Account, messages: Vec<MessageSummary>) {
        self.new_mail.lock().unwrap().extend(messages);
    }
}

async fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    for _ in 0..secs * 10 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("beklenen olmadı: {what}");
}

fn list(store: &Store, view: View) -> Vec<MessageSummary> {
    store
        .list(
            &ListQuery {
                view,
                account_id: Some(ACC.into()),
                category: None,
                only: None,
                cursor: None,
                limit: 500,
            },
            0,
        )
        .unwrap()
        .items
}

fn raw(from: &str, to: &str, subject: &str, msgid: &str, body: &str) -> Vec<u8> {
    format!(
        "From: Mailhere Test <{from}>\r\nTo: <{to}>\r\nSubject: {subject}\r\nMessage-ID: <{msgid}>\r\nDate: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n",
        chrono::Utc::now().to_rfc2822()
    )
    .into_bytes()
}

async fn keywords(client: &Client, id: &str) -> Value {
    client
        .call(
            "Email/get",
            json!({ "accountId": client.account_id(), "ids": [id], "properties": ["keywords", "mailboxIds"] }),
        )
        .await
        .unwrap()["list"][0]
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gercek_sunucu_uctan_uca() {
    let Some((url, user, pass)) = env_config() else {
        eprintln!("MH_TEST_JMAP tanımlı değil; gerçek JMAP testi atlandı");
        return;
    };
    let stamp = chrono::Utc::now().timestamp_millis();

    // Keşif ve giriş
    assert!(
        mh_jmap::probe(&url).await,
        "oturum adresi JMAP olarak tanınmalı"
    );
    assert_eq!(
        mh_jmap::test_login(&url, &user, "yanlis-parola")
            .await
            .unwrap_err()
            .code,
        ErrorCode::Auth
    );
    let client = Client::connect(&url, Credentials::new(user.clone(), pass.clone()))
        .await
        .unwrap();
    assert!(client.can_submit());
    let email: String = client.session().username.clone();
    assert!(
        email.contains('@'),
        "Stalwart kullanıcı adı e-posta adresidir: {email}"
    );

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
        jmap_url: Some(url.clone()),
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: 0,
        auth: Default::default(),
        notify: true,
    };
    store.insert_account(&account).unwrap();
    store.set_jmap_username(ACC, &user).unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    secrets.set(ACC, &pass).unwrap();

    let rec = Arc::new(Rec::default());
    let mgr = JmapManager::with_timing(
        store.clone(),
        secrets.clone(),
        rec.clone(),
        tokio::runtime::Handle::current(),
        Timing {
            // Anlık bildirimi sınamak için yoklama uzun tutulur.
            poll: Duration::from_secs(600),
            ping_secs: 30,
            ..Timing::default()
        },
    );
    mgr.start(account.clone());
    wait_until("ilk eşitleme", 60, || {
        rec.statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.state == SyncState::Online)
    })
    .await;
    let roles: Vec<MailboxRole> = store
        .mailboxes_for(ACC)
        .unwrap()
        .into_iter()
        .map(|m| m.mailbox.role)
        .collect();
    assert!(roles.contains(&MailboxRole::Inbox));
    assert!(roles.contains(&MailboxRole::Sent));
    assert!(roles.contains(&MailboxRole::Drafts));
    // Durum akışına bağlanması için kısa bir süre tanı.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Kendine gönderim: EmailSubmission + anlık bildirim (EventSource).
    let subject = format!("Mailhere JMAP testi {stamp}");
    let msgid = format!("mh-{stamp}@mailhere.test");
    mh_jmap::send_raw(
        &store,
        &secrets,
        &account,
        &raw(
            &email,
            &email,
            &subject,
            &msgid,
            "Merhaba, bu bir deneme postasıdır. Çağrı ğüşiöç.",
        ),
        std::slice::from_ref(&email),
    )
    .await
    .unwrap();
    wait_until("gelen kutusuna anlık bildirim", 60, || {
        rec.new_mail
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.subject == subject)
    })
    .await;
    wait_until("gönderilen yerelde", 30, || {
        list(&store, View::Sent)
            .iter()
            .any(|m| m.subject == subject)
    })
    .await;
    let inbox_msg = list(&store, View::Inbox)
        .into_iter()
        .find(|m| m.subject == subject)
        .expect("gelen kutusunda");
    assert!(inbox_msg.unread);
    let loc = store.jmap_locations(&[inbox_msg.id]).unwrap().remove(0);
    let jmap_id = loc.jmap_id.clone().unwrap();

    // Gövde (blob indirme) ve okuma alanı.
    mgr.fetch_body(inbox_msg.id).await.unwrap();
    let detail = store.detail(inbox_msg.id, &[]).unwrap();
    assert!(
        detail
            .body_text
            .unwrap_or_default()
            .contains("Çağrı ğüşiöç"),
        "gövde UTF-8 korunur"
    );

    // Bayraklar
    mgr.set_flags(&[inbox_msg.id], Some(false), Some(true))
        .unwrap();
    for _ in 0..100 {
        let k = keywords(&client, &jmap_id).await;
        if k["keywords"]["$seen"] == true && k["keywords"]["$flagged"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let k = keywords(&client, &jmap_id).await;
    assert_eq!(k["keywords"]["$seen"], true);
    assert_eq!(k["keywords"]["$flagged"], true);

    // Arşive (gerekirse sunucuda oluşturulur), sonra çöpe.
    mgr.move_messages(&[inbox_msg.id], MoveTarget::Archive)
        .unwrap();
    wait_until("yerelde arşivde", 30, || {
        store
            .jmap_locations(&[inbox_msg.id])
            .unwrap()
            .first()
            .is_some_and(|l| l.role == MailboxRole::Archive)
    })
    .await;
    mgr.move_messages(&[inbox_msg.id], MoveTarget::Trash)
        .unwrap();
    let trash = store
        .mailbox_by_role(ACC, MailboxRole::Trash)
        .unwrap()
        .unwrap()
        .mailbox
        .path;
    for _ in 0..100 {
        let k = keywords(&client, &jmap_id).await;
        if k["mailboxIds"]
            .as_object()
            .is_some_and(|m| m.len() == 1 && m.contains_key(&trash))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let k = keywords(&client, &jmap_id).await;
    assert_eq!(k["mailboxIds"], json!({ trash.clone(): true }));

    // Taslaklar: kaydet, yerine yaz (aynı yerel kimlik), sil.
    let server_id = |id| {
        store
            .jmap_locations(&[id])
            .unwrap()
            .first()
            .and_then(|l| l.jmap_id.clone())
    };
    let now = chrono::Utc::now().timestamp_millis();
    let (d1, _) = mgr
        .save_draft(
            ACC,
            raw(
                &email,
                "ayse@ornek.test",
                "Taslak 1",
                &format!("t1-{stamp}@mailhere.test"),
                "Yarım",
            ),
            None,
            now,
        )
        .unwrap();
    wait_until("taslak sunucuda", 30, || server_id(d1).is_some()).await;
    let j1 = server_id(d1).unwrap();
    assert_eq!(keywords(&client, &j1).await["keywords"]["$draft"], true);
    let (d2, _) = mgr
        .save_draft(
            ACC,
            raw(
                &email,
                "ayse@ornek.test",
                "Taslak 2",
                &format!("t2-{stamp}@mailhere.test"),
                "Daha az yarım",
            ),
            Some(d1),
            now,
        )
        .unwrap();
    assert_eq!(d2, d1, "yerine yazınca kimlik korunur");
    wait_until("yeni sürüm sunucuda", 30, || {
        server_id(d1).is_some_and(|j| j != j1)
    })
    .await;
    let j2 = server_id(d1).unwrap();
    let mut gone = Value::Null;
    for _ in 0..100 {
        gone = client
            .call(
                "Email/get",
                json!({ "accountId": client.account_id(), "ids": [j1], "properties": ["id"] }),
            )
            .await
            .unwrap();
        if gone["notFound"] == json!([j1]) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        gone["notFound"],
        json!([j1]),
        "önceki taslak sunucudan silinir"
    );
    mgr.delete_draft(d1).unwrap();
    assert!(store.summary(d1).unwrap().is_none());
    for _ in 0..100 {
        gone = client
            .call(
                "Email/get",
                json!({ "accountId": client.account_id(), "ids": [j2], "properties": ["id"] }),
            )
            .await
            .unwrap();
        if gone["notFound"] == json!([j2]) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        gone["notFound"],
        json!([j2]),
        "silinen taslak sunucudan kalkar"
    );

    // Temizlik: testin oluşturduğu postalar.
    let mut ids = vec![jmap_id];
    if let Some(sent) = list(&store, View::Sent)
        .into_iter()
        .find(|m| m.subject == subject)
    {
        if let Some(j) = store.jmap_locations(&[sent.id]).unwrap()[0].jmap_id.clone() {
            ids.push(j);
        }
    }
    mh_jmap::destroy_emails(&client, &ids).await.unwrap();
    mgr.stop(ACC);
}
