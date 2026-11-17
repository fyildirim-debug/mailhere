//! "Daha eski postalar" (`fetch_older`), kalıcı silme (`purge_messages`) ve
//! kutu boşaltma (`empty_mailbox`): süreç içi sahte IMAP sunucusuna karşı.
//! Ağ gerekmez; her test kendi sunucusunu açar.

mod fake_imap;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fake_imap::Server;
use mh_core::{
    Account, AccountKind, ErrorCode, MailboxRole, MessageSummary, MoveTarget, Security,
    ServerConfig, SyncStatus,
};
use mh_store::{Action, MailboxInfo, Store};
use mh_sync::{
    connect, fetch_older, sync_mailbox, EventSink, MemorySecrets, SecretStore, SyncManager,
    OLDER_BATCH,
};

const ACC: &str = "t";
const PASS: &str = "parola";

#[derive(Default)]
struct Recorder {
    new_mail: Mutex<Vec<MessageSummary>>,
}

impl EventSink for Recorder {
    fn sync_status(&self, _: SyncStatus) {}
    fn changed(&self, _: Vec<String>) {}
    fn new_mail(&self, _: &Account, messages: Vec<MessageSummary>) {
        self.new_mail.lock().unwrap().extend(messages);
    }
}

fn cfg(port: u16) -> ServerConfig {
    ServerConfig {
        host: "127.0.0.1".into(),
        port,
        security: Security::None,
        username: "furkan".into(),
    }
}

fn account(port: u16) -> Account {
    Account {
        id: ACC.into(),
        name: "Test".into(),
        email: "furkan@ornek.test".into(),
        display_name: "Furkan".into(),
        color: "#4C6FD8".into(),
        kind: AccountKind::Imap,
        imap: Some(cfg(port)),
        smtp: None,
        jmap_url: None,
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: 0,
        auth: Default::default(),
        notify: true,
    }
}

fn mail(mid: &str) -> Vec<u8> {
    format!(
        "From: Ayse <ayse@ornek.test>\r\nTo: Furkan <furkan@ornek.test>\r\nSubject: Posta {mid}\r\n\
         Message-ID: <{mid}@ornek.test>\r\nDate: Tue, 13 Oct 2026 09:00:00 +0300\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nMerhaba {mid}.\r\n"
    )
    .into_bytes()
}

fn store_with_boxes(port: u16) -> Arc<Store> {
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.insert_account(&account(port)).unwrap();
    for (path, role) in [
        ("INBOX", MailboxRole::Inbox),
        ("Trash", MailboxRole::Trash),
        ("Junk", MailboxRole::Junk),
    ] {
        store
            .upsert_mailbox(ACC, path, path, role, None, false)
            .unwrap();
    }
    store
}

fn info(store: &Store, path: &str) -> MailboxInfo {
    store.mailbox(&format!("{ACC}:{path}")).unwrap().unwrap()
}

fn local_uids(store: &Store, path: &str) -> Vec<u32> {
    store
        .local_uids(&format!("{ACC}:{path}"))
        .unwrap()
        .into_iter()
        .map(|(_, uid, ..)| uid)
        .collect()
}

/// Yereldeki postanın kimliği (kutu + UID).
fn local_id(store: &Store, path: &str, uid: u32) -> i64 {
    store
        .local_uids(&format!("{ACC}:{path}"))
        .unwrap()
        .into_iter()
        .find(|(_, u, ..)| *u == uid)
        .map(|(id, ..)| id)
        .unwrap_or_else(|| panic!("{path} kutusunda UID {uid} yerelde yok"))
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "zaman aşımı: {what}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn manager(store: &Arc<Store>, rec: &Arc<Recorder>) -> SyncManager {
    let secrets = Arc::new(MemorySecrets::new());
    secrets.set(ACC, PASS).unwrap();
    SyncManager::new(
        store.clone(),
        secrets as Arc<dyn SecretStore>,
        rec.clone(),
        tokio::runtime::Handle::current(),
    )
}

#[tokio::test]
async fn daha_eski_postalar_parti_parti_iner_ve_esitleme_onlari_silmez() {
    let server = Server::start().await;
    server.with(|st| {
        for i in 1..=700 {
            st.deliver("INBOX", mail(&format!("g{i}")), &["\\Seen"]);
        }
    });
    let store = store_with_boxes(server.port);
    let mut s = connect(&cfg(server.port), PASS).await.unwrap();

    // İlk eşitleme: yalnız en yeni 300.
    let out = sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    assert!(out.new_unread.is_empty(), "ilk eşitleme bildirim üretmez");
    assert_eq!(local_uids(&store, "INBOX"), (401..=700).collect::<Vec<_>>());

    // Birinci parti: bir sonraki 300; gerisi var.
    let none = HashSet::new();
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (OLDER_BATCH as u32, true));
    assert_eq!(local_uids(&store, "INBOX"), (101..=700).collect::<Vec<_>>());
    assert!(!info(&store, "INBOX").older_done);

    // İkinci parti: kalan 100; kutunun başına ulaşıldı.
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (100, false));
    assert_eq!(local_uids(&store, "INBOX").len(), 700);
    assert!(info(&store, "INBOX").older_done);
    // Alım olağan yoldan geçer: konu, önizleme ve gövde yazılır.
    let oldest = store
        .summary(local_id(&store, "INBOX", 1))
        .unwrap()
        .unwrap();
    assert_eq!(oldest.subject, "Posta g1");
    assert!(!oldest.unread, "sunucudaki okundu bayrağı korunur");
    assert!(store.raw(oldest.id).unwrap().is_some());

    // Sonraki olağan eşitleme eski postaları "en yeni pencerede değil" diye silmez.
    let out = sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    assert!(out.new_unread.is_empty());
    assert_eq!(local_uids(&store, "INBOX").len(), 700);

    // Sunucuda silinen (eski ve yeni) postalar yerelden de kalkar; yalnız onlar.
    server.with(|st| {
        st.mailbox_mut("INBOX")
            .msgs
            .retain(|m| m.uid != 50 && m.uid != 650)
    });
    sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    let uids = local_uids(&store, "INBOX");
    assert_eq!(uids.len(), 698);
    assert!(!uids.contains(&50) && !uids.contains(&650));

    // Yeni posta olağan eşitlemeyle ve bildirimle gelir.
    server.with(|st| st.deliver("INBOX", mail("yeni"), &[]));
    let out = sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    assert_eq!(out.new_unread.len(), 1);
    assert_eq!(local_uids(&store, "INBOX").len(), 699);

    // Yeniden sorulursa: indirilecek bir şey yok.
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (0, false));
    let _ = s.logout().await;
}

#[tokio::test]
async fn daha_eski_postalar_yeni_postaya_ve_bekleyen_silmeye_dokunmaz() {
    let server = Server::start().await;
    server.with(|st| {
        for i in 1..=10 {
            st.deliver("INBOX", mail(&format!("a{i}")), &[]);
        }
        for i in 1..=3 {
            st.deliver("Trash", mail(&format!("c{i}")), &[]);
        }
    });
    let store = store_with_boxes(server.port);
    let mut s = connect(&cfg(server.port), PASS).await.unwrap();
    let none = HashSet::new();

    // Henüz eşitlenmemiş kutu: indirilmez, "daha var" döner (önce olağan eşitleme).
    let o = fetch_older(&mut s, &store, &info(&store, "Trash"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (0, true));
    assert!(local_uids(&store, "Trash").is_empty());
    // Sunucuda boş kutu: daha eskisi yok.
    let o = fetch_older(&mut s, &store, &info(&store, "Junk"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (0, false));

    sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    assert_eq!(local_uids(&store, "INBOX").len(), 10);

    // Filigranın üstündeki (henüz eşitlenmemiş) yeni postalar bu yoldan inmez:
    // onları olağan eşitleme bildirimle indirir.
    server.with(|st| {
        for i in 11..=15 {
            st.deliver("INBOX", mail(&format!("a{i}")), &[]);
        }
    });
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (0, false));
    assert_eq!(local_uids(&store, "INBOX").len(), 10);
    let out = sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    assert_eq!(
        out.new_unread.len(),
        5,
        "yeni postaların bildirimi kaybolmaz"
    );

    // Aradaki boşluk (yarıda kalmış indirme) tamamlanır; kalıcı silinmesi
    // sırada bekleyen UID yeniden indirilmez.
    store.set_mailbox_older_done("t:INBOX", false).unwrap();
    store
        .delete_messages(&[local_id(&store, "INBOX", 3), local_id(&store, "INBOX", 4)])
        .unwrap();
    let skip: HashSet<u32> = [4].into_iter().collect();
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &skip)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (1, false));
    let uids = local_uids(&store, "INBOX");
    assert!(uids.contains(&3) && !uids.contains(&4));

    // Yerelde hiç postası kalmamış (ama eşitlenmiş) kutu da doldurulur.
    let all: Vec<i64> = store
        .local_uids("t:INBOX")
        .unwrap()
        .into_iter()
        .map(|(id, ..)| id)
        .collect();
    store.delete_messages(&all).unwrap();
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (15, false));

    // UIDVALIDITY değişti: UID'ler başka postaları gösterir; hiçbir şey inmez.
    server.with(|st| st.mailbox_mut("INBOX").uidvalidity = 99);
    store
        .delete_messages(&[local_id(&store, "INBOX", 1)])
        .unwrap();
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (0, true));
    assert_eq!(local_uids(&store, "INBOX").len(), 14);
    // Olağan eşitleme kutuyu baştan kurar ve "daha eskisi yok" işaretini kaldırır.
    assert!(info(&store, "INBOX").older_done);
    sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    assert!(!info(&store, "INBOX").older_done);
    assert_eq!(local_uids(&store, "INBOX").len(), 15);
    let _ = s.logout().await;
}

#[tokio::test]
async fn sunucu_aramayi_reddederse_yerel_postalar_silinmez() {
    let server = Server::start().await;
    server.with(|st| {
        for i in 1..=320 {
            st.deliver("INBOX", mail(&format!("g{i}")), &[]);
        }
    });
    let store = store_with_boxes(server.port);
    let mut s = connect(&cfg(server.port), PASS).await.unwrap();
    sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    assert_eq!(local_uids(&store, "INBOX").len(), 300);

    // Sunucu aramaya NO diyor: boş sonuç "kutuda posta kalmadı" diye okunmaz.
    server.with(|st| st.search_fails = true);
    let err = sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap_err();
    assert!(!err.is_connection(), "{err}");
    assert_eq!(
        local_uids(&store, "INBOX").len(),
        300,
        "yerel postalar yerinde"
    );
    // "Daha eski postalar" da hata verir; "daha eskisi yok" diye işaretlenmez.
    let none = HashSet::new();
    assert!(fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .is_err());
    assert!(!info(&store, "INBOX").older_done);

    // Arama düzelince her şey olağan akışına döner.
    server.with(|st| st.search_fails = false);
    sync_mailbox(&mut s, &store, &info(&store, "INBOX"), true)
        .await
        .unwrap();
    let o = fetch_older(&mut s, &store, &info(&store, "INBOX"), &none)
        .await
        .unwrap();
    assert_eq!((o.fetched, o.more), (20, false));
    let _ = s.logout().await;
}

#[tokio::test]
async fn uidplus_yokken_arama_reddedilirse_hicbir_posta_silinmez() {
    let (server, store, mgr) = purge_setup(false).await;
    // "Silinecek" işaretli postaları soran arama reddediliyor: düz EXPUNGE
    // başkasının işaretlediği UID 2'yi de silerdi; hiç EXPUNGE gönderilmez.
    server.with(|st| st.search_fails = true);
    let t4 = local_id(&store, "Trash", 4);
    mgr.purge_messages(&[t4]).unwrap();
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    server.with(|st| {
        assert_eq!(st.count("EXPUNGE"), 0);
        assert_eq!(st.mailbox("Trash").uids(), [1, 2, 3, 4, 5]);
        assert_eq!(st.mailbox("Trash").flagged("\\Deleted"), [2]);
        st.search_fails = false;
    });
    // Silinemeyen posta listeye geri döner.
    mgr.sync_now();
    wait_for("posta yerele geri döner", || {
        local_uids(&store, "Trash") == [1, 2, 3, 4, 5]
    })
    .await;
    mgr.stop(ACC);
}

#[tokio::test]
async fn yonetici_daha_eski_postalari_ayri_baglantiyla_indirir() {
    let server = Server::start().await;
    server.with(|st| {
        for i in 1..=350 {
            st.deliver("INBOX", mail(&format!("m{i}")), &[]);
        }
    });
    let store = store_with_boxes(server.port);
    let rec = Arc::new(Recorder::default());
    let mgr = manager(&store, &rec);
    mgr.start(account(server.port));
    wait_for("ilk eşitleme", || local_uids(&store, "INBOX").len() == 300).await;

    let o = mgr.fetch_older("t:INBOX").await.unwrap();
    assert_eq!((o.fetched, o.more), (50, false));
    assert_eq!(local_uids(&store, "INBOX").len(), 350);
    assert!(
        rec.new_mail.lock().unwrap().is_empty(),
        "eski postalar yeni posta bildirimi üretmez"
    );
    // Başına ulaşılmış kutu için sunucuya yeniden gidilmez.
    let examines = server.with(|st| st.count("EXAMINE"));
    let o = mgr.fetch_older("t:INBOX").await.unwrap();
    assert_eq!((o.fetched, o.more), (0, false));
    assert_eq!(server.with(|st| st.count("EXAMINE")), examines);
    // Bilinmeyen kutu.
    assert_eq!(
        mgr.fetch_older("t:Yok").await.unwrap_err().code,
        ErrorCode::NotFound
    );
    mgr.stop(ACC);
}

#[tokio::test]
async fn cevrimdisiyken_daha_eski_postalar_ag_hatasi_verir() {
    // Dinleyen kimse olmayan kapı.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let store = store_with_boxes(port);
    store
        .set_mailbox_sync_state("t:INBOX", Some(7), Some(11), 10)
        .unwrap();
    let rec = Arc::new(Recorder::default());
    let mgr = manager(&store, &rec);
    let err = mgr.fetch_older("t:INBOX").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Network);
    assert!(!err.message.contains(PASS));
}

/// Çöp'te beş posta (UID 2 başka bir istemcinin "silinecek" diye işaretleyip
/// bıraktığı posta), gelen kutusunda üç posta.
async fn purge_setup(uidplus: bool) -> (Server, Arc<Store>, SyncManager) {
    let server = Server::start().await;
    server.with(|st| {
        st.uidplus = uidplus;
        for i in 1..=3 {
            st.deliver("INBOX", mail(&format!("g{i}")), &[]);
        }
        for i in 1..=5 {
            let flags: &[&str] = if i == 2 { &["\\Deleted"] } else { &[] };
            st.deliver("Trash", mail(&format!("c{i}")), flags);
        }
    });
    let store = store_with_boxes(server.port);
    let rec = Arc::new(Recorder::default());
    let mgr = manager(&store, &rec);
    mgr.start(account(server.port));
    wait_for("ilk eşitleme", || {
        local_uids(&store, "Trash").len() == 5 && local_uids(&store, "INBOX").len() == 3
    })
    .await;
    (server, store, mgr)
}

#[tokio::test]
async fn kalici_silme_yalniz_cop_ve_istenmeyende_ve_yalniz_secilenleri_siler() {
    let (server, store, mgr) = purge_setup(true).await;
    let inbox1 = local_id(&store, "INBOX", 1);
    let (t1, t3) = (local_id(&store, "Trash", 1), local_id(&store, "Trash", 3));

    // Gelen kutusundaki posta kalıcı silinemez; karışık istekte hiçbirine dokunulmaz.
    for ids in [vec![inbox1], vec![t1, inbox1, t3]] {
        let err = mgr.purge_messages(&ids).unwrap_err();
        assert_eq!(err.code, ErrorCode::Invalid);
    }
    assert_eq!(local_uids(&store, "Trash").len(), 5);
    assert_eq!(local_uids(&store, "INBOX").len(), 3);
    assert!(store.actions(ACC).unwrap().is_empty(), "kuyruğa yazılmadı");

    // Çöp'teki iki posta: yerelden hemen, sunucudan kuyrukla.
    let touched = mgr.purge_messages(&[t1, t3]).unwrap();
    assert_eq!(touched, ["t:Trash"]);
    assert_eq!(local_uids(&store, "Trash"), [2, 4, 5]);
    wait_for("sunucudan silinme", || {
        server.with(|st| st.mailbox("Trash").uids()) == [2, 4, 5]
    })
    .await;
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    server.with(|st| {
        // Başkasının işaretlediği posta ne silindi ne işareti kalktı.
        assert_eq!(st.mailbox("Trash").flagged("\\Deleted"), [2]);
        assert_eq!(st.mailbox("INBOX").uids(), [1, 2, 3]);
        assert!(st.count("UID EXPUNGE") >= 1);
        assert_eq!(
            st.count("EXPUNGE"),
            0,
            "UIDPLUS varken düz EXPUNGE kullanılmaz"
        );
    });
    // Yerelde olmayan kimlik sessizce atlanır.
    assert!(mgr.purge_messages(&[9_999_999]).unwrap().is_empty());
    assert_eq!(local_uids(&store, "Trash"), [2, 4, 5]);
    mgr.stop(ACC);
}

#[tokio::test]
async fn uidplus_yokken_baskasinin_isaretledigi_posta_silinmez() {
    let (server, store, mgr) = purge_setup(false).await;
    let (t4, t5) = (local_id(&store, "Trash", 4), local_id(&store, "Trash", 5));
    mgr.purge_messages(&[t4, t5]).unwrap();
    wait_for("sunucudan silinme", || {
        server.with(|st| st.mailbox("Trash").uids()) == [1, 2, 3]
    })
    .await;
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    server.with(|st| {
        // Düz EXPUNGE kullanıldı ama başkasının işaretlediği UID 2 yerinde ve
        // işareti geri kondu.
        assert_eq!(st.count("UID EXPUNGE"), 0);
        assert!(st.count("EXPUNGE") >= 1);
        assert_eq!(st.mailbox("Trash").flagged("\\Deleted"), [2]);
        assert!(st.count("UID STORE 2 -FLAGS.SILENT (\\Deleted)") >= 1);
        assert!(st.count("UID STORE 2 +FLAGS.SILENT (\\Deleted)") >= 1);
    });
    mgr.stop(ACC);
}

#[tokio::test]
async fn sunucu_silmeyi_reddederse_postalar_listeye_geri_doner() {
    let (server, store, mgr) = purge_setup(true).await;
    // Sunucu bayrak yazmayı reddediyor (salt okunur kutu, izin yok).
    server.with(|st| st.read_only = true);
    let (t1, t3) = (local_id(&store, "Trash", 1), local_id(&store, "Trash", 3));
    mgr.purge_messages(&[t1, t3]).unwrap();
    assert_eq!(local_uids(&store, "Trash"), [2, 4, 5]);
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    // İşlemden vazgeçildi: sunucuda duran postalar sessizce kaybolmaz.
    wait_for("postalar yerele geri döner", || {
        local_uids(&store, "Trash") == [1, 2, 3, 4, 5]
    })
    .await;
    assert_eq!(
        server.with(|st| st.mailbox("Trash").uids()),
        [1, 2, 3, 4, 5]
    );

    // Kutu boşaltma da reddedilirse aynı biçimde geri gelir.
    let (n, _) = mgr.empty_mailbox("t:Trash").unwrap();
    assert_eq!(n, 5);
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    wait_for("postalar yerele geri döner", || {
        local_uids(&store, "Trash") == [1, 2, 3, 4, 5]
    })
    .await;
    assert_eq!(local_uids(&store, "INBOX"), [1, 2, 3]);
    mgr.stop(ACC);
}

#[tokio::test]
async fn kutuyu_bosaltma_inmemis_eski_postalari_da_siler_yeni_gelene_dokunmaz() {
    let server = Server::start().await;
    server.with(|st| {
        for i in 1..=400 {
            st.deliver("Trash", mail(&format!("c{i}")), &[]);
        }
        for i in 1..=3 {
            st.deliver("INBOX", mail(&format!("g{i}")), &[]);
        }
        st.deliver("Junk", mail("i1"), &[]);
    });
    let store = store_with_boxes(server.port);
    let rec = Arc::new(Recorder::default());
    let mgr = manager(&store, &rec);
    mgr.start(account(server.port));
    wait_for("ilk eşitleme", || {
        local_uids(&store, "Trash").len() == 300
            && local_uids(&store, "INBOX").len() == 3
            && local_uids(&store, "Junk").len() == 1
    })
    .await;

    // Gelen kutusu boşaltılamaz.
    assert_eq!(
        mgr.empty_mailbox("t:INBOX").unwrap_err().code,
        ErrorCode::Invalid
    );
    assert_eq!(
        mgr.empty_mailbox("t:Yok").unwrap_err().code,
        ErrorCode::NotFound
    );

    // İstekten önce sunucuya düşmüş ama henüz eşitlenmemiş posta (filigranın
    // üstünde): kullanıcı onu hiç görmedi, silinmez.
    server.with(|st| st.deliver("Trash", mail("sonradan"), &[]));
    let (n, touched) = mgr.empty_mailbox("t:Trash").unwrap();
    assert_eq!(n, 300, "yerelde görünen postalar");
    assert_eq!(touched, ["t:Trash"]);
    assert!(
        store.message_ids_in("t:Trash").unwrap().is_empty() || {
            // Ardından gelen eşitleme yeni postayı indirmiş olabilir.
            local_uids(&store, "Trash") == [401]
        }
    );
    wait_for("sunucuda kutu boşalır", || {
        server.with(|st| st.mailbox("Trash").uids()) == [401]
    })
    .await;
    wait_for("yeni posta yerelde", || {
        local_uids(&store, "Trash") == [401]
    })
    .await;
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    server.with(|st| {
        assert_eq!(st.mailbox("INBOX").uids(), [1, 2, 3]);
        assert_eq!(st.mailbox("Junk").uids(), [1]);
    });

    // İstenmeyen de boşaltılabilir.
    let (n, _) = mgr.empty_mailbox("t:Junk").unwrap();
    assert_eq!(n, 1);
    wait_for("istenmeyen boşalır", || {
        server.with(|st| st.mailbox("Junk").uids().is_empty())
    })
    .await;

    // Kuyruğa (rolü bu arada değişmiş bir kutu için) düşmüş boşaltma işlemi
    // çalışırken reddedilir: gelen kutusu yerinde kalır.
    store
        .action_push(
            ACC,
            &Action::Empty {
                path: "INBOX".into(),
                upto: 3,
                uidvalidity: Some(7),
                since: None,
            },
            None,
            1,
        )
        .unwrap();
    mgr.sync_now();
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    assert_eq!(server.with(|st| st.mailbox("INBOX").uids()), [1, 2, 3]);
    assert_eq!(local_uids(&store, "INBOX"), [1, 2, 3]);
    mgr.stop(ACC);
}

#[tokio::test]
async fn tasimasi_bekleyen_posta_kaynaginda_yalniz_kendi_uid_siyle_silinir() {
    let (server, store, mgr) = purge_setup(true).await;
    // Çevrimdışı: motor durur; işlemler kuyrukta bekler.
    mgr.stop(ACC);
    let g2 = local_id(&store, "INBOX", 2);
    let g3 = local_id(&store, "INBOX", 3);
    mgr.move_messages(&[g2, g3], MoveTarget::Trash).unwrap();
    assert!(store.location(g2).unwrap().unwrap().pending);
    // Çöp'te görünen (sunucuda hâlâ gelen kutusunda duran) postalar kalıcı silinir.
    mgr.purge_messages(&[g2, g3]).unwrap();
    assert!(store.location(g2).unwrap().is_none());

    // g3'ün taşıması bu arada başka bir yoldan uygulanmış olsun: sunucuda Çöp'te
    // yeni bir UID'yle duruyor. Gelen kutusunda aynı Message-ID'li başka bir
    // kopya var; ona dokunulmamalı.
    let kopya = server.with(|st| {
        let m = st
            .mailbox("INBOX")
            .msgs
            .iter()
            .find(|m| m.uid == 3)
            .cloned()
            .unwrap();
        st.mailbox_mut("INBOX").msgs.retain(|m| m.uid != 3);
        st.deliver("Trash", m.raw.clone(), &[]);
        st.deliver("INBOX", m.raw, &[])
    });

    mgr.start(account(server.port));
    wait_for("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    server.with(|st| {
        // g2 kaynağında UID'siyle silindi; g3 hedefte (Çöp) Message-ID ile.
        assert_eq!(st.mailbox("INBOX").uids(), [1, kopya]);
        assert_eq!(st.mailbox("Trash").uids(), [1, 2, 3, 4, 5]);
    });
    mgr.stop(ACC);
}
