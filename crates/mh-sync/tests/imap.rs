//! Gerçek bir IMAP sunucusuna karşı uçtan uca eşitleme testi.
//!
//! Yalnız `MH_TEST_IMAP=host:port:kullanıcı:parola:güvenlik` tanımlıysa
//! çalışır (güvenlik: tls | starttls | none; none yalnız localhost). Sunucudaki
//! INBOX, Sent, Trash ve Archive kutuları test başında BOŞALTILIR; yalnız test
//! hesabıyla çalıştır.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::TryStreamExt;
use mh_core::{
    Account, AccountKind, Category, MailboxRole, MessageSummary, MoveTarget, Security,
    ServerConfig, SyncState, SyncStatus,
};
use mh_store::Store;
use mh_sync::{connect, EventSink, MemorySecrets, SecretStore, SyncManager};

fn env_config() -> Option<(ServerConfig, String)> {
    let v = std::env::var("MH_TEST_IMAP").ok()?;
    let p: Vec<&str> = v.splitn(5, ':').collect();
    if p.len() != 5 {
        panic!("MH_TEST_IMAP biçimi: host:port:kullanıcı:parola:güvenlik");
    }
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
    changed: Mutex<Vec<String>>,
    new_mail: Mutex<Vec<MessageSummary>>,
}

impl EventSink for Recorder {
    fn sync_status(&self, status: SyncStatus) {
        self.statuses.lock().unwrap().push(status);
    }
    fn changed(&self, mailbox_ids: Vec<String>) {
        self.changed.lock().unwrap().extend(mailbox_ids);
    }
    fn new_mail(&self, _account: &Account, messages: Vec<MessageSummary>) {
        self.new_mail.lock().unwrap().extend(messages);
    }
}

fn mail(subject: &str, from: &str, extra_headers: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: Furkan <furkan@ornek.test>\r\nSubject: {subject}\r\n\
         Message-ID: <{}@ornek.test>\r\nDate: Tue, 13 Oct 2026 09:00:00 +0300\r\n{extra_headers}\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n",
        subject.replace(' ', "-").to_lowercase()
    )
    .into_bytes()
}

fn big_mail() -> Vec<u8> {
    let filler = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=\r\n".repeat(17_000);
    format!(
        "From: Elif <elif@ornek.test>\r\nTo: furkan@ornek.test\r\nSubject: Buyuk ek\r\n\
         Message-ID: <buyuk-ek@ornek.test>\r\nDate: Tue, 13 Oct 2026 08:00:00 +0300\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"s\"\r\n\r\n--s\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n\
         Sözleşme ekte.\r\n--s\r\nContent-Type: application/pdf; name=\"Sozlesme.pdf\"\r\n\
         Content-Disposition: attachment; filename=\"Sozlesme.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n\
         {filler}--s--\r\n"
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

async fn server_search(cfg: &ServerConfig, pass: &str, mailbox: &str, query: &str) -> usize {
    let mut s = connect(cfg, pass).await.unwrap();
    let n = match s.examine(mailbox).await {
        Ok(_) => s.uid_search(query).await.unwrap().len(),
        Err(_) => 0,
    };
    let _ = s.logout().await;
    n
}

async fn wait_server(
    cfg: &ServerConfig,
    pass: &str,
    mailbox: &str,
    query: &str,
    want: usize,
    what: &str,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let n = server_search(cfg, pass, mailbox, query).await;
        if n == want {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "zaman aşımı: {what} (sunucuda {n}, beklenen {want})"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

fn inbox(store: &Store) -> Vec<MessageSummary> {
    store
        .list(
            &mh_core::ListQuery {
                view: mh_core::View::Inbox,
                account_id: None,
                category: None,
                cursor: None,
                limit: 100,
            },
            i64::MAX / 2,
        )
        .unwrap()
        .items
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imap_esitleme_uctan_uca() {
    let Some((cfg, pass)) = env_config() else {
        eprintln!("MH_TEST_IMAP tanımlı değil; IMAP tümleşik testi atlandı");
        return;
    };

    // Sunucuyu hazırla: kutuları boşalt, örnek postaları yükle.
    let mut s = connect(&cfg, &pass).await.expect("giriş");
    for mb in ["INBOX", "Sent", "Trash", "Archive"] {
        if s.select(mb).await.is_ok() {
            let _: Vec<_> = s
                .uid_store("1:*", "+FLAGS.SILENT (\\Deleted)")
                .await
                .unwrap()
                .try_collect()
                .await
                .unwrap_or_default();
            let _: Vec<_> = s.expunge().await.unwrap().try_collect().await.unwrap();
        }
    }
    // Seçili kutu silinirse sunucu bağlantıyı kapatır; önce başka kutuya geç.
    s.select("INBOX").await.unwrap();
    let _ = s.delete("Archive").await;
    s.append(
        "INBOX",
        None,
        None,
        mail(
            "Butce taslagi",
            "Kerem Aydın <kerem@ornek.test>",
            "",
            "Pazarlama kaleminde düşüş var.",
        ),
    )
    .await
    .unwrap();
    s.append(
        "INBOX",
        Some("(\\Seen)"),
        None,
        mail(
            "Haftalik bulten",
            "Mimar Haftalık <bulten@mimar.test>",
            "List-Id: <haftalik.mimar.test>\r\n",
            "Sayı 142",
        ),
    )
    .await
    .unwrap();
    s.append("INBOX", None, None, big_mail()).await.unwrap();
    s.logout().await.unwrap();

    // Eşitlemeyi başlat.
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
            && inbox(&store).len() == 3
    })
    .await;
    assert!(
        sink.new_mail.lock().unwrap().is_empty(),
        "ilk eşitleme bildirim üretmez"
    );
    let roles: Vec<_> = store.mailboxes().unwrap().iter().map(|m| m.role).collect();
    assert!(
        roles.contains(&MailboxRole::Sent)
            && roles.contains(&MailboxRole::Trash)
            && roles.contains(&MailboxRole::Junk)
    );

    let list = inbox(&store);
    let by = |subj: &str| list.iter().find(|m| m.subject == subj).unwrap().clone();
    let butce = by("Butce taslagi");
    let bulten = by("Haftalik bulten");
    let buyuk = by("Buyuk ek");
    assert!(butce.unread && !bulten.unread);
    assert_eq!(butce.category, Category::People);
    assert_eq!(bulten.category, Category::Newsletters);
    assert_eq!(butce.snippet, "Pazarlama kaleminde düşüş var.");
    assert!(buyuk.has_attachments);
    assert!(
        !store.location(buyuk.id).unwrap().unwrap().has_raw,
        "büyük postanın yalnız başlığı iner"
    );

    // Gövde istek üzerine iner.
    mgr.fetch_body(buyuk.id).await.unwrap();
    let d = store.detail(buyuk.id, &[]).unwrap();
    assert_eq!(d.attachments.len(), 1);
    assert_eq!(d.attachments[0].name, "Sozlesme.pdf");

    // Bayraklar sunucuya gider.
    mgr.set_flags(&[butce.id], Some(false), Some(true)).unwrap();
    wait_server(&cfg, &pass, "INBOX", "SEEN FLAGGED", 1, "bayraklar").await;

    // Arşive taşıma: kutu yoksa oluşturulur, yerel satır yeni UID'yi sahiplenir.
    mgr.move_messages(&[bulten.id], MoveTarget::Archive)
        .unwrap();
    wait_server(&cfg, &pass, "Archive", "ALL", 1, "arşive taşıma").await;
    wait_server(&cfg, &pass, "INBOX", "ALL", 2, "gelen kutusundan çıkış").await;
    wait_for("arşivde UID", 20, || {
        store
            .location(bulten.id)
            .unwrap()
            .is_some_and(|l| l.role == MailboxRole::Archive && l.uid.is_some())
    })
    .await;

    // Başka istemciden gelen yeni posta IDLE ile anında görülür ve bildirilir.
    let mut other = connect(&cfg, &pass).await.unwrap();
    other
        .append(
            "INBOX",
            None,
            None,
            mail(
                "Yeni posta",
                "Ayşe Demir <ayse@ornek.test>",
                "",
                "Cumartesi kahvaltı?",
            ),
        )
        .await
        .unwrap();
    wait_for("IDLE ile yeni posta", 30, || {
        sink.new_mail
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.subject == "Yeni posta")
    })
    .await;

    // Başka istemcide silinen posta yerelden de kalkar.
    other.select("INBOX").await.unwrap();
    let uids = other.uid_search("SUBJECT \"Buyuk ek\"").await.unwrap();
    let set = mh_sync::uid_set(&uids.into_iter().collect::<Vec<_>>());
    let _: Vec<_> = other
        .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    let _: Vec<_> = other.expunge().await.unwrap().try_collect().await.unwrap();
    other.logout().await.unwrap();
    mgr.sync_now();
    wait_for("silinen posta", 30, || {
        store.summary(buyuk.id).unwrap().is_none()
    })
    .await;

    // Gönderilenlere ekleme.
    assert!(mgr
        .append(
            "t",
            MailboxRole::Sent,
            mail("Gonderildi", "Furkan <furkan@ornek.test>", "", "Tamam.")
        )
        .unwrap());
    wait_server(&cfg, &pass, "Sent", "SEEN", 1, "gönderilenlere ekleme").await;

    mgr.stop("t");
}
