//! Gerçek bir IMAP sunucusuna karşı uçtan uca eşitleme testi.
//!
//! Yalnız `MH_TEST_IMAP=host:port:kullanıcı:parola:güvenlik` tanımlıysa
//! çalışır (güvenlik: tls | starttls | none; none yalnız localhost). Sunucudaki
//! INBOX, Sent, Trash ve Archive kutuları test başında BOŞALTILIR; yalnız test
//! hesabıyla çalıştır. Testler aynı kutuları kullandığı için sırayla çalışır.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use futures::TryStreamExt;
use mh_core::{
    Account, AccountKind, Category, MailboxRole, MessageSummary, MoveTarget, Security,
    ServerConfig, SyncState, SyncStatus,
};
use mh_store::Store;
use mh_sync::{
    connect, sync_mailbox, EventSink, ImapSession, ImapStream, MemorySecrets, SecretStore,
    SyncManager,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Testler aynı sunucu kutularını boşaltıp doldurur: biri bitmeden öbürü başlamasın.
static SIRA: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

fn view(store: &Store, view: mh_core::View) -> Vec<MessageSummary> {
    store
        .list(
            &mh_core::ListQuery {
                view,
                account_id: None,
                category: None,
                only: None,
                cursor: None,
                limit: 500,
            },
            i64::MAX / 2,
        )
        .unwrap()
        .items
}

fn inbox(store: &Store) -> Vec<MessageSummary> {
    view(store, mh_core::View::Inbox)
}

/// Sunucudaki test kutularını boşaltır (Archive silinir); açık oturumu döner.
async fn reset_server(cfg: &ServerConfig, pass: &str) -> ImapSession {
    let mut s = connect(cfg, pass).await.expect("giriş");
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
    s
}

fn test_account(cfg: &ServerConfig) -> Account {
    Account {
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
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imap_esitleme_uctan_uca() {
    let Some((cfg, pass)) = env_config() else {
        eprintln!("MH_TEST_IMAP tanımlı değil; IMAP tümleşik testi atlandı");
        return;
    };
    let _sira = SIRA.lock().await;

    // Sunucuyu hazırla: kutuları boşalt, örnek postaları yükle.
    let mut s = reset_server(&cfg, &pass).await;
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
    let account = test_account(&cfg);
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

/// N'inci `UID FETCH` komutunu yazarken bağlantıyı koparan akış: yarıda kalan
/// eşitlemeyi (kapanış, kopan ağ, zaman aşımı) belirlenimci biçimde taklit eder.
#[derive(Debug)]
struct Kesik {
    inner: tokio::net::TcpStream,
    fetches: usize,
    fail_at: usize,
}

impl AsyncRead for Kesik {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Kesik {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.windows(9).any(|w| w == b"UID FETCH") {
            self.fetches += 1;
            if self.fetches >= self.fail_at {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "bağlantı kesildi (test)",
                )));
            }
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Şifresiz test sunucusuna, `fail_at`'inci UID FETCH'te kopacak oturum.
async fn kesik_oturum(cfg: &ServerConfig, pass: &str, fail_at: usize) -> ImapSession {
    let tcp = tokio::net::TcpStream::connect((cfg.host.as_str(), cfg.port))
        .await
        .unwrap();
    let stream: ImapStream = Box::new(Kesik {
        inner: tcp,
        fetches: 0,
        fail_at,
    });
    let mut client = async_imap::Client::new(stream);
    client.read_response().await.unwrap().unwrap();
    client
        .login(&cfg.username, pass)
        .await
        .map_err(|(e, _)| e)
        .unwrap()
}

fn toplu(i: usize) -> Vec<u8> {
    mail(
        &format!("Toplu {i:03}"),
        "Kerem Aydın <kerem@ornek.test>",
        "",
        "Toplu gönderim denemesi.",
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imap_yarida_kalan_esitleme_ve_geri_alma() {
    let Some((cfg, pass)) = env_config() else {
        eprintln!("MH_TEST_IMAP tanımlı değil; IMAP tümleşik testi atlandı");
        return;
    };
    let _sira = SIRA.lock().await;
    let plain = cfg.security == Security::None;

    let mut s = reset_server(&cfg, &pass).await;
    for i in 0..120 {
        s.append("INBOX", None, None, toplu(i)).await.unwrap();
    }
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.insert_account(&test_account(&cfg)).unwrap();
    let inbox_id = store
        .upsert_mailbox("t", "INBOX", "INBOX", MailboxRole::Inbox)
        .unwrap();
    let info = || store.mailbox(&inbox_id).unwrap().unwrap();
    let count = || store.local_uids(&inbox_id).unwrap().len();

    // 1) İlk eşitleme ikinci parçada kopar: inenler kalır, filigran ilerlemez;
    //    sonraki eşitleme eksikleri indirir.
    if plain {
        // FETCH (sıra) · UID FETCH boyut #1 · gövde #2 · boyut #3 → kopar
        let mut kesik = kesik_oturum(&cfg, &pass, 3).await;
        let err = sync_mailbox(&mut kesik, &store, &info(), true)
            .await
            .unwrap_err();
        assert!(err.is_connection(), "{err}");
        let got = count();
        assert!(got > 0 && got < 120, "kısmi indirme: {got}");
        assert_eq!(
            info().highest_uid,
            0,
            "yarıda kalan ilk eşitleme filigranı ilerletmez"
        );
    } else {
        eprintln!("şifreli test sunucusu: kopan bağlantı adımı atlandı");
    }
    let mut tam = connect(&cfg, &pass).await.unwrap();
    sync_mailbox(&mut tam, &store, &info(), true).await.unwrap();
    assert_eq!(count(), 120);
    let first_highest = info().highest_uid;
    assert!(first_highest > 0);

    // 2) Çevrimdışıyken 50'den fazla yeni posta, eşitleme yine yarıda kopar:
    //    hiçbiri kalıcı olarak atlanmaz.
    for i in 120..250 {
        s.append("INBOX", None, None, toplu(i)).await.unwrap();
    }
    if plain {
        // UID FETCH yeni UID'ler #1 · boyut #2 · gövde #3 · boyut #4 → kopar
        let mut kesik = kesik_oturum(&cfg, &pass, 4).await;
        assert!(sync_mailbox(&mut kesik, &store, &info(), true)
            .await
            .is_err());
        let got = count();
        assert!(got > 120 && got < 250, "kısmi indirme: {got}");
        assert_eq!(info().highest_uid, first_highest, "filigran yerinde");
    }
    let out = sync_mailbox(&mut tam, &store, &info(), true).await.unwrap();
    assert_eq!(count(), 250, "eksik kalan posta yok");
    assert!(info().highest_uid > first_highest);
    assert!(out.changed);
    let _ = tam.logout().await;
    let _ = s.logout().await;

    // 3) Motor kapalıyken (uygulama kapalı, çevrimdışı) yapılan işlemler
    //    kalıcı kuyrukta bekler; "yeniden açılışta" sırayla uygulanır.
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    secrets.set("t", &pass).unwrap();
    let sink = Arc::new(Recorder::default());
    let manager = || {
        SyncManager::new(
            store.clone(),
            secrets.clone(),
            sink.clone(),
            tokio::runtime::Handle::current(),
        )
    };
    let pick = |subject: &str| {
        inbox(&store)
            .into_iter()
            .find(|m| m.subject == subject)
            .unwrap()
    };
    let (a, b, c) = (pick("Toplu 010"), pick("Toplu 011"), pick("Toplu 012"));
    let a_uid = store.location(a.id).unwrap().unwrap().uid;
    {
        let mgr = manager();
        // a: arşivle ve hemen geri al (sunucuya hiç gitmez).
        mgr.move_messages(&[a.id], MoveTarget::Archive).unwrap();
        mgr.move_messages(&[a.id], MoveTarget::Inbox).unwrap();
        // b: arşivle, hemen yıldızla (yer işlem çalışınca çözülür).
        mgr.move_messages(&[b.id], MoveTarget::Archive).unwrap();
        mgr.set_flags(&[b.id], None, Some(true)).unwrap();
        // c: okundu yap, sonra okunmadı: yalnız son istek kalır.
        mgr.set_flags(&[c.id], Some(false), None).unwrap();
        mgr.set_flags(&[c.id], Some(true), None).unwrap();
    }
    let la = store.location(a.id).unwrap().unwrap();
    assert_eq!(
        (la.role, la.uid, la.pending),
        (MailboxRole::Inbox, a_uid, false),
        "geri alma eski UID'yi geri yazar"
    );
    assert!(!store.actions("t").unwrap().is_empty(), "kuyruk kalıcı");

    let mgr = manager();
    mgr.start(test_account(&cfg));
    wait_for("kuyruk boşalır", 30, || {
        store.actions("t").unwrap().is_empty()
    })
    .await;
    wait_server(&cfg, &pass, "Archive", "ALL", 1, "yalnız b arşivde").await;
    wait_server(
        &cfg,
        &pass,
        "Archive",
        "FLAGGED",
        1,
        "taşımadan sonra yıldız",
    )
    .await;
    wait_server(&cfg, &pass, "INBOX", "ALL", 249, "a gelen kutusunda kaldı").await;
    wait_server(&cfg, &pass, "INBOX", "SEEN", 0, "c okunmadı kaldı").await;
    wait_for("b arşivde UID'li ve yıldızlı", 30, || {
        store
            .location(b.id)
            .unwrap()
            .is_some_and(|l| l.role == MailboxRole::Archive && l.uid.is_some() && !l.pending)
            && store.summary(b.id).unwrap().is_some_and(|m| m.starred)
    })
    .await;

    // 4) Sunucuda yapılmış taşımayı geri almak: posta geri döner, hayalet ya da
    //    kopya kalmaz.
    mgr.move_messages(&[c.id], MoveTarget::Archive).unwrap();
    wait_server(&cfg, &pass, "Archive", "ALL", 2, "c arşivde").await;
    wait_for("c arşivde UID'li", 30, || {
        store
            .location(c.id)
            .unwrap()
            .is_some_and(|l| l.role == MailboxRole::Archive && l.uid.is_some())
    })
    .await;
    mgr.move_messages(&[c.id], MoveTarget::Inbox).unwrap();
    wait_server(&cfg, &pass, "Archive", "ALL", 1, "c arşivden çıktı").await;
    wait_server(&cfg, &pass, "INBOX", "ALL", 249, "c gelen kutusunda").await;
    wait_for("c gelen kutusunda UID'li", 30, || {
        store
            .location(c.id)
            .unwrap()
            .is_some_and(|l| l.role == MailboxRole::Inbox && l.uid.is_some() && !l.pending)
    })
    .await;
    mgr.sync_now();
    wait_for("yerel kutular sunucuyla aynı", 30, || {
        let list = inbox(&store);
        list.len() == 249
            && list.iter().all(|m| {
                store
                    .location(m.id)
                    .unwrap()
                    .is_some_and(|l| l.uid.is_some() && !l.pending)
            })
            && view(&store, mh_core::View::Archive).len() == 1
    })
    .await;

    // 5) Zaten hedefte olanı yeniden taşımak (Çöp'te Del, Arşiv'de E):
    //    sunucuda bir şey değişmez, UID korunur.
    let before = store.location(b.id).unwrap().unwrap();
    mgr.move_messages(&[b.id], MoveTarget::Archive).unwrap();
    assert_eq!(store.location(b.id).unwrap().unwrap(), before);
    assert!(!store.has_pending_moves("t").unwrap());

    mgr.stop("t");
}
