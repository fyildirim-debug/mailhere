//! Taslakların sunucu tarafı: ekleme, yerine yazma ve silme.
//!
//! Yalnız `MH_TEST_IMAP=host:port:kullanıcı:parola:güvenlik` tanımlıysa
//! çalışır. Sunucudaki **Drafts** kutusu test başında BOŞALTILIR; yalnız test
//! hesabıyla çalıştır.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::TryStreamExt;
use mh_core::{
    Account, AccountKind, MessageSummary, Security, ServerConfig, SyncState, SyncStatus,
};
use mh_store::Store;
use mh_sync::{connect, EventSink, MemorySecrets, SecretStore, SyncManager};

fn env_config() -> Option<(ServerConfig, String)> {
    let v = std::env::var("MH_TEST_IMAP").ok()?;
    let p: Vec<&str> = v.splitn(5, ':').collect();
    assert_eq!(
        p.len(),
        5,
        "MH_TEST_IMAP biçimi: host:port:kullanıcı:parola:güvenlik"
    );
    let security = match p[4] {
        "tls" => Security::Tls,
        "starttls" => Security::Starttls,
        "none" => Security::None,
        other => panic!("bilinmeyen güvenlik: {other}"),
    };
    Some((
        ServerConfig {
            host: p[0].into(),
            port: p[1].parse().expect("kapı sayı olmalı"),
            security,
            username: p[2].into(),
        },
        p[3].into(),
    ))
}

#[derive(Default)]
struct Recorder {
    statuses: Mutex<Vec<SyncStatus>>,
}

impl EventSink for Recorder {
    fn sync_status(&self, status: SyncStatus) {
        self.statuses.lock().unwrap().push(status);
    }
    fn changed(&self, _: Vec<String>) {}
    fn new_mail(&self, _: &Account, _: Vec<MessageSummary>) {}
}

fn draft(subject: &str, mid: &str) -> Vec<u8> {
    format!(
        "From: Furkan <furkan@ornek.test>\r\nTo: Ayse <ayse@ornek.test>\r\nBcc: gizli@ornek.test\r\n\
         Subject: {subject}\r\nMessage-ID: <{mid}>\r\nDate: Tue, 13 Oct 2026 09:00:00 +0300\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nTaslak metni: {subject}\r\n"
    )
    .into_bytes()
}

async fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "zaman aşımı: {what}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn count(cfg: &ServerConfig, pass: &str, query: &str) -> usize {
    let mut s = connect(cfg, pass).await.unwrap();
    let n = match s.examine("Drafts").await {
        Ok(_) => s.uid_search(query).await.unwrap().len(),
        Err(_) => 0,
    };
    let _ = s.logout().await;
    n
}

async fn wait_server(cfg: &ServerConfig, pass: &str, query: &str, want: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let n = count(cfg, pass, query).await;
        if n == want {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "zaman aşımı: Drafts '{query}' (sunucuda {n}, beklenen {want})"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn taslaklar_sunucuya_yansir() {
    let Some((cfg, pass)) = env_config() else {
        eprintln!("MH_TEST_IMAP tanımlı değil; taslak tümleşik testi atlandı");
        return;
    };

    // Drafts kutusunu boşalt.
    let mut s = connect(&cfg, &pass).await.expect("giriş");
    if s.select("Drafts").await.is_ok() {
        let _: Vec<_> = s
            .uid_store("1:*", "+FLAGS.SILENT (\\Deleted)")
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap_or_default();
        let _: Vec<_> = s.expunge().await.unwrap().try_collect().await.unwrap();
    }
    s.logout().await.unwrap();

    let store = Arc::new(Store::open_in_memory().unwrap());
    let account = Account {
        id: "t".into(),
        name: "Test".into(),
        email: "furkan@ornek.test".into(),
        display_name: "Furkan".into(),
        color: "#4C6FD8".into(),
        kind: AccountKind::Imap,
        imap: Some(cfg.clone()),
        smtp: None,
        jmap_url: None,
        signature: None,
        created_at: 0,
        auth: Default::default(),
    };
    store.insert_account(&account).unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    secrets.set("t", &pass).unwrap();
    let sink = Arc::new(Recorder::default());
    let mgr = SyncManager::new(
        store.clone(),
        secrets,
        sink.clone(),
        tokio::runtime::Handle::current(),
    );
    mgr.start(account);
    wait_for("ilk eşitleme", 30, || {
        sink.statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.state == SyncState::Online)
    })
    .await;
    let local = || store.counts(0).unwrap().drafts;
    let mid = |id| {
        store
            .location(id)
            .unwrap()
            .and_then(|l| l.uid.map(|_| l.message_id_header.unwrap_or_default()))
    };

    // 1) İlk kayıt: \Draft \Seen bayraklarıyla eklenir, yerel satır UID alır.
    let (id, touched) = mgr
        .save_draft("t", draft("Plan v1", "v1@ornek.test"), None, 0)
        .unwrap();
    assert_eq!(touched, vec!["t:Drafts".to_string()]);
    assert_eq!(local(), 1, "yerel kayıt hemen");
    wait_server(&cfg, &pass, "DRAFT SEEN", 1).await;
    wait_for("v1 UID", 20, || mid(id).as_deref() == Some("v1@ornek.test")).await;

    // 2) Yerine yazma: kimlik korunur, eski kopya UID ile silinir.
    let (same, _) = mgr
        .save_draft("t", draft("Plan v2", "v2@ornek.test"), Some(id), 0)
        .unwrap();
    assert_eq!(same, id);
    wait_server(&cfg, &pass, "HEADER Message-ID v2@ornek.test", 1).await;
    wait_server(&cfg, &pass, "ALL", 1).await;
    wait_for("v2 UID", 20, || mid(id).as_deref() == Some("v2@ornek.test")).await;
    assert_eq!(local(), 1);

    // 3) Art arda iki kayıt: v3'ün UID'si bilinmeden v4 gelir; v3 Message-ID
    //    aramasıyla bulunup silinir, sonunda tek kopya kalır.
    mgr.save_draft("t", draft("Plan v3", "v3@ornek.test"), Some(id), 0)
        .unwrap();
    mgr.save_draft("t", draft("Plan v4", "v4@ornek.test"), Some(id), 0)
        .unwrap();
    wait_server(&cfg, &pass, "HEADER Message-ID v4@ornek.test", 1).await;
    wait_server(&cfg, &pass, "ALL", 1).await;
    wait_for("v4 UID ve tek yerel taslak", 20, || {
        mid(id).as_deref() == Some("v4@ornek.test") && local() == 1
    })
    .await;
    let d = store.detail(id, &[]).unwrap();
    assert_eq!(d.summary.subject, "Plan v4");
    assert_eq!(d.bcc.len(), 1, "gizli alıcı taslakta saklanır");

    // 4) Silme: yerelden hemen, sunucudan sırayla.
    assert_eq!(mgr.delete_draft(id).unwrap(), vec!["t:Drafts".to_string()]);
    assert!(store.summary(id).unwrap().is_none());
    wait_server(&cfg, &pass, "ALL", 0).await;
    assert!(
        mgr.delete_draft(id).unwrap().is_empty(),
        "ikinci silme sessiz"
    );

    // Sunucu işlemlerinden hiçbiri hata durumu üretmedi.
    assert!(
        !sink
            .statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| matches!(s.state, SyncState::Error { .. })),
        "{:?}",
        sink.statuses.lock().unwrap()
    );
    mgr.stop("t");
}
