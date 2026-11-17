//! "Daha eski postalar" (`fetch_older`), kalıcı silme (`purge_messages`) ve
//! kutu boşaltma (`empty_mailbox`): süreç içi sahte JMAP sunucusuna karşı.

mod fake;

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fake::{raw_mail, FakeServer};
use mh_core::{Account, AccountKind, ErrorCode, MessageId, MessageSummary, MoveTarget, SyncStatus};
use mh_jmap::{JmapManager, Timing, OLDER_BATCH};
use mh_store::{Action, Store};
use mh_sync::{EventSink, MemorySecrets, SecretStore};

const ACC: &str = "acc";

#[derive(Default)]
struct Rec {
    new_mail: Mutex<Vec<MessageSummary>>,
}

impl EventSink for Rec {
    fn sync_status(&self, _: SyncStatus) {}
    fn changed(&self, _: Vec<String>) {}
    fn new_mail(&self, _: &Account, messages: Vec<MessageSummary>) {
        self.new_mail.lock().unwrap().extend(messages);
    }
}

fn account(url: &str) -> Account {
    Account {
        id: ACC.into(),
        name: "Sunucu".into(),
        email: "furkan@ornek.com".into(),
        display_name: "Furkan Yıldırım".into(),
        color: "#D08A1E".into(),
        kind: AccountKind::Jmap,
        imap: None,
        smtp: None,
        jmap_url: Some(url.into()),
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: 0,
        auth: Default::default(),
        notify: true,
    }
}

async fn setup() -> (FakeServer, Arc<Store>, Arc<dyn SecretStore>, Account) {
    let server = FakeServer::start("furkan", "gizli").await;
    // Büyük kutular: testler çok sayıda küçük isteğe boğulmasın.
    server.with(|s| s.max_get = 500);
    let store = Arc::new(Store::open_in_memory().unwrap());
    let account = account(&server.session_url());
    store.insert_account(&account).unwrap();
    store.set_jmap_username(ACC, "furkan").unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    secrets.set(ACC, "gizli").unwrap();
    (server, store, secrets, account)
}

fn mail(tag: &str) -> Vec<u8> {
    raw_mail(
        "Ayşe Demir <ayse@eposta.com>",
        "Furkan <furkan@ornek.com>",
        &format!("Posta {tag}"),
        &format!("{tag}@eposta.com"),
        &[],
        "Merhaba.",
    )
}

/// Kutunun yerelde bilinen sunucu kimlikleri.
fn local_ids(store: &Store, mailbox: &str) -> BTreeSet<String> {
    let known = store.jmap_known(ACC).unwrap();
    let here: HashSet<MessageId> = store
        .message_ids_in(&format!("{ACC}:{mailbox}"))
        .unwrap()
        .into_iter()
        .collect();
    known
        .into_iter()
        .filter(|(_, id)| here.contains(id))
        .map(|(j, _)| j)
        .collect()
}

fn local_id(store: &Store, jmap_id: &str) -> MessageId {
    store
        .jmap_message(ACC, jmap_id)
        .unwrap()
        .unwrap_or_else(|| panic!("{jmap_id} yerelde yok"))
}

fn total(store: &Store, mailbox: &str) -> usize {
    store
        .mailbox(&format!("{ACC}:{mailbox}"))
        .unwrap()
        .unwrap()
        .mailbox
        .total as usize
}

async fn older(
    client: &mh_jmap::Client,
    store: &Store,
    mailbox: &str,
    skip: &HashSet<String>,
) -> (u32, bool) {
    let o = mh_jmap::fetch_older(
        client,
        store,
        ACC,
        &format!("{ACC}:{mailbox}"),
        mailbox,
        total(store, mailbox),
        skip,
    )
    .await
    .unwrap();
    (o.fetched, o.more)
}

fn done(store: &Store, mailbox: &str) -> bool {
    store
        .mailbox(&format!("{ACC}:{mailbox}"))
        .unwrap()
        .unwrap()
        .older_done
}

async fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..400 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("beklenen olmadı: {what}");
}

fn quick() -> Timing {
    Timing {
        poll: Duration::from_secs(60),
        periodic: Duration::from_secs(600),
        ping_secs: 5,
        min_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(500),
    }
}

fn manager(store: &Arc<Store>, secrets: &Arc<dyn SecretStore>, rec: &Arc<Rec>) -> JmapManager {
    JmapManager::with_timing(
        store.clone(),
        secrets.clone(),
        rec.clone(),
        tokio::runtime::Handle::current(),
        quick(),
    )
}

#[tokio::test]
async fn daha_eski_postalar_parti_parti_iner_ve_bastan_esitleme_onlari_silmez() {
    let (server, store, secrets, account) = setup().await;
    // 700 posta; üçer üçer aynı anda alınmış (eşit receivedAt).
    let all: Vec<String> = server.with(|s| {
        // Sunucu sorgu sınırını kısar: sayfalama ve üst üste binme denenir.
        s.query_limit = 320;
        (0..700)
            .map(|i| {
                s.add_email(
                    "mb-in",
                    mail(&format!("g{i}")),
                    &["$seen"],
                    1_791_000_000_000 + (i / 3) * 60_000,
                )
            })
            .collect()
    });
    let client = mh_jmap::connect(&store, &secrets, &account).await.unwrap();
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(out.full && out.new_unread.is_empty());
    assert_eq!(local_ids(&store, "mb-in").len(), 300);

    let none = HashSet::new();
    // Birinci parti: bir sonraki 300; gerisi var.
    assert_eq!(
        older(&client, &store, "mb-in", &none).await,
        (OLDER_BATCH as u32, true)
    );
    assert_eq!(local_ids(&store, "mb-in").len(), 600);
    assert!(!done(&store, "mb-in"));
    // İkinci parti: kalan 100; kutunun başına ulaşıldı, hiçbir posta atlanmadı.
    assert_eq!(older(&client, &store, "mb-in", &none).await, (100, false));
    assert_eq!(
        local_ids(&store, "mb-in"),
        all.iter().cloned().collect::<BTreeSet<_>>()
    );
    assert!(done(&store, "mb-in"));
    // Alım olağan yoldan geçer (konu, okundu bilgisi).
    let oldest = store.summary(local_id(&store, &all[0])).unwrap().unwrap();
    assert_eq!(oldest.subject, "Posta g0");
    assert!(!oldest.unread);
    // Yeniden sorulursa indirilecek bir şey yok.
    assert_eq!(older(&client, &store, "mb-in", &none).await, (0, false));

    // Artımlı eşitleme eski postalara dokunmaz.
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(!out.full && out.new_unread.is_empty());
    assert_eq!(local_ids(&store, "mb-in").len(), 700);

    // Baştan eşitleme (durum kayboldu): en yeni pencerenin dışındaki postalar
    // silinmez; yalnız sunucuda gerçekten silinenler yerelden kalkar.
    server.with(|s| {
        s.destroy_email(&all[5]);
        s.destroy_email(&all[690]);
        s.fail_changes = true;
    });
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(out.full, "fark hesaplanamayınca baştan eşitlenir");
    let local = local_ids(&store, "mb-in");
    assert_eq!(local.len(), 698);
    assert!(!local.contains(&all[5]) && !local.contains(&all[690]));
    assert!(
        !done(&store, "mb-in"),
        "baştan eşitlemeden sonra daha eskisi yeniden sorulabilir"
    );
    assert_eq!(older(&client, &store, "mb-in", &none).await, (0, false));
    assert!(done(&store, "mb-in"));

    // Sunucuda izlenen kutulardan çıkan eski posta da baştan eşitlemede kalkar.
    server.with(|s| {
        s.update_email(&all[7], |e| {
            e.mailbox_ids = BTreeSet::from(["mb-yok".to_owned()]);
        });
        s.fail_changes = true;
    });
    mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(local_ids(&store, "mb-in").len(), 697);
}

#[tokio::test]
async fn daha_eski_postalar_yeni_postayi_bildirimsiz_yutmaz() {
    let (server, store, secrets, account) = setup().await;
    let ids: Vec<String> = server.with(|s| {
        (0..6)
            .map(|i| {
                s.add_email(
                    "mb-in",
                    mail(&format!("a{i}")),
                    &[],
                    1_791_000_000_000 + i * 60_000,
                )
            })
            .collect()
    });
    let junk: Vec<String> = server.with(|s| {
        (0..3)
            .map(|i| s.add_email("mb-ju", mail(&format!("i{i}")), &[], 1_790_000_000_000 + i))
            .collect()
    });
    let client = mh_jmap::connect(&store, &secrets, &account).await.unwrap();
    mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(local_ids(&store, "mb-in").len(), 6);
    let none = HashSet::new();

    // Henüz eşitlenmemiş iki yeni posta: yereldeki en yeni postadan da yeni.
    let fresh: Vec<String> = server.with(|s| {
        (0..2)
            .map(|i| {
                s.add_email(
                    "mb-in",
                    mail(&format!("yeni{i}")),
                    &[],
                    1_792_000_000_000 + i,
                )
            })
            .collect()
    });
    // Yerelde bir boşluk (yarıda kalmış indirme) ve kalıcı silinmesi bekleyen posta.
    store
        .jmap_remove(ACC, &[ids[1].clone(), ids[2].clone()])
        .unwrap();
    let skip: HashSet<String> = [ids[2].clone()].into_iter().collect();
    assert_eq!(older(&client, &store, "mb-in", &skip).await, (1, false));
    let local = local_ids(&store, "mb-in");
    assert!(local.contains(&ids[1]), "boşluk tamamlanır");
    assert!(!local.contains(&ids[2]), "silinmesi bekleyen posta inmez");
    assert!(
        fresh.iter().all(|f| !local.contains(f)),
        "yeni posta bu yoldan inmez"
    );
    // Yeni postalar olağan eşitlemeyle ve bildirimle gelir.
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(out.new_unread.len(), 2);

    // Yerelde hiç postası kalmamış kutu da doldurulur.
    store.jmap_remove(ACC, &junk).unwrap();
    assert!(local_ids(&store, "mb-ju").is_empty());
    assert_eq!(older(&client, &store, "mb-ju", &none).await, (3, false));
    assert_eq!(local_ids(&store, "mb-ju").len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kalici_silme_yalniz_cop_ve_istenmeyenden_cikarir() {
    let (server, store, secrets, account) = setup().await;
    let (i1, i2, t1, t2, t3, x, j1) = server.with(|s| {
        let i1 = s.add_email("mb-in", mail("g1"), &[], 1_791_000_000_000);
        let i2 = s.add_email("mb-in", mail("g2"), &[], 1_791_000_100_000);
        let t1 = s.add_email("mb-tr", mail("c1"), &[], 1_791_000_200_000);
        let t2 = s.add_email("mb-tr", mail("c2"), &[], 1_791_000_300_000);
        let t3 = s.add_email("mb-tr", mail("c3"), &[], 1_791_000_400_000);
        // Hem Çöp'te hem bir klasörde duran posta.
        let x = s.add_email("mb-tr", mail("ortak"), &[], 1_791_000_500_000);
        s.update_email(&x, |e| {
            e.mailbox_ids.insert("mb-pr".into());
        });
        let j1 = s.add_email("mb-ju", mail("i1"), &[], 1_791_000_600_000);
        (i1, i2, t1, t2, t3, x, j1)
    });
    let rec = Arc::new(Rec::default());
    let mgr = manager(&store, &secrets, &rec);
    mgr.start(account.clone());
    wait_until("ilk eşitleme", || {
        local_ids(&store, "mb-tr").len() == 4 && local_ids(&store, "mb-in").len() == 2
    })
    .await;

    // Gelen kutusundaki posta kalıcı silinemez; karışık istekte hiçbirine dokunulmaz.
    let (li1, lt1, lx) = (
        local_id(&store, &i1),
        local_id(&store, &t1),
        local_id(&store, &x),
    );
    for ids in [vec![li1], vec![lt1, li1]] {
        assert_eq!(
            mgr.purge_messages(&ids).unwrap_err().code,
            ErrorCode::Invalid
        );
    }
    assert_eq!(local_ids(&store, "mb-tr").len(), 4);
    assert!(store.actions(ACC).unwrap().is_empty());
    assert_eq!(server.with(|s| s.emails.len()), 7);

    // Çöp'teki posta yok edilir; başka kutuda da duran posta yalnız Çöp'ten çıkar.
    let touched = mgr.purge_messages(&[lt1, lx]).unwrap();
    assert_eq!(touched, [format!("{ACC}:mb-tr")]);
    assert!(store.jmap_message(ACC, &t1).unwrap().is_none());
    wait_until("sunucuda silinme", || {
        server.with(|s| !s.emails.contains_key(&t1))
    })
    .await;
    wait_until("ortak posta çöpten çıkar", || {
        server.with(|s| s.emails[&x].mailbox_ids == BTreeSet::from(["mb-pr".to_owned()]))
    })
    .await;
    wait_until("ortak posta klasöründe görünür", || {
        local_ids(&store, "mb-pr").contains(&x)
    })
    .await;
    wait_until("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    // Yerelde olmayan kimlik sessizce atlanır.
    assert!(mgr.purge_messages(&[9_999_999]).unwrap().is_empty());

    // Taşı + kalıcı sil art arda: sıra korunur, posta sunucuda da silinir.
    let li2 = local_id(&store, &i2);
    mgr.move_messages(&[li2], MoveTarget::Trash).unwrap();
    mgr.purge_messages(&[li2]).unwrap();
    wait_until("taşınan posta sunucuda silinir", || {
        server.with(|s| !s.emails.contains_key(&i2))
    })
    .await;
    wait_until("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;

    // Uygulama kapalıyken (motor durmuş) verilen silme kaybolmaz. Bu arada
    // başka bir istemci t2'yi gelen kutusuna geri almış: silinmez. İstenmeyen
    // kutusundaki posta silinir.
    mgr.stop(ACC);
    server.with(|s| {
        s.update_email(&t2, |e| {
            e.mailbox_ids = BTreeSet::from(["mb-in".to_owned()]);
        })
    });
    let (lt2, lj1) = (local_id(&store, &t2), local_id(&store, &j1));
    mgr.purge_messages(&[lt2, lj1]).unwrap();
    assert_eq!(
        store.actions(ACC).unwrap().len(),
        1,
        "kalıcı kuyrukta bekler"
    );
    assert!(matches!(
        &store.actions(ACC).unwrap()[0].action,
        Action::Purge { ids } if ids.len() == 2
    ));
    // Yeni yönetici (yeniden açılış): kuyruk oturum açılınca uygulanır.
    let mgr = manager(&store, &secrets, &rec);
    mgr.start(account.clone());
    wait_until("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    wait_until("istenmeyen posta silinir", || {
        server.with(|s| !s.emails.contains_key(&j1))
    })
    .await;
    wait_until("geri alınan posta gelen kutusunda", || {
        local_ids(&store, "mb-in").contains(&t2)
    })
    .await;
    server.with(|s| {
        assert!(s.emails.contains_key(&t2), "çöpte olmayan posta silinmez");
        assert!(s.emails.contains_key(&i1) && s.emails.contains_key(&t3));
    });

    // Taşıması sunucuya hiç ulaşmamış posta (yerelde Çöp'te, sunucuda gelen
    // kutusunda; sunucuda hiçbir şey değişmedi): kalıcı silme onu sunucuda
    // yok etmez ve posta sessizce kaybolmaz, gelen kutusuna geri döner.
    mgr.stop(ACC);
    let li1 = local_id(&store, &i1);
    store
        .move_local_untracked(&[li1], &format!("{ACC}:mb-tr"))
        .unwrap();
    mgr.purge_messages(&[li1]).unwrap();
    assert!(store.jmap_message(ACC, &i1).unwrap().is_none());
    let mgr = manager(&store, &secrets, &rec);
    mgr.start(account.clone());
    wait_until("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    wait_until("sunucuda duran posta listeye geri döner", || {
        local_ids(&store, "mb-in").contains(&i1)
    })
    .await;
    assert!(server.with(|s| s.emails[&i1].mailbox_ids == BTreeSet::from(["mb-in".to_owned()])));
    assert!(
        rec.new_mail.lock().unwrap().is_empty(),
        "geri dönen posta yeni posta sayılmaz"
    );
    mgr.stop(ACC);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kutuyu_bosaltma_inmemis_eski_postalari_da_siler_gorulmeyene_dokunmaz() {
    let (server, store, secrets, account) = setup().await;
    let inbox = server.with(|s| {
        for i in 0..320 {
            s.add_email(
                "mb-tr",
                mail(&format!("c{i}")),
                &[],
                1_791_000_000_000 + i * 1000,
            );
        }
        s.add_email("mb-in", mail("g1"), &[], 1_791_000_000_000)
    });
    let rec = Arc::new(Rec::default());
    let mgr = manager(&store, &secrets, &rec);
    mgr.start(account.clone());
    wait_until("ilk eşitleme", || local_ids(&store, "mb-tr").len() == 300).await;
    wait_until("durum yazılır", || {
        store.jmap_account_state(ACC).unwrap().email_state.is_some()
    })
    .await;

    // Gelen kutusu boşaltılamaz.
    assert_eq!(
        mgr.empty_mailbox(&format!("{ACC}:mb-in")).unwrap_err().code,
        ErrorCode::Invalid
    );
    assert_eq!(
        mgr.empty_mailbox(&format!("{ACC}:yok")).unwrap_err().code,
        ErrorCode::NotFound
    );

    // Motor durmuşken (çevrimdışı) Çöp'e yeni bir posta düşer: kullanıcı onu
    // görmedi; boşaltma ona dokunmaz.
    mgr.stop(ACC);
    let late = server.with(|s| s.add_email("mb-tr", mail("sonradan"), &[], 1_792_000_000_000));
    let (n, touched) = mgr.empty_mailbox(&format!("{ACC}:mb-tr")).unwrap();
    assert_eq!(n, 300, "yerelde görünen postalar");
    assert_eq!(touched, [format!("{ACC}:mb-tr")]);
    assert!(local_ids(&store, "mb-tr").is_empty());
    assert_eq!(
        server.with(|s| s.ids_in("mb-tr").len()),
        321,
        "henüz sunucuya gitmedi"
    );

    let mgr = manager(&store, &secrets, &rec);
    mgr.start(account.clone());
    wait_until("sunucuda kutu boşalır", || {
        server.with(|s| s.ids_in("mb-tr")) == [late.clone()]
    })
    .await;
    wait_until("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    wait_until("görülmeyen posta yerelde", || {
        local_ids(&store, "mb-tr") == BTreeSet::from([late.clone()])
    })
    .await;
    assert!(server.with(|s| s.emails.contains_key(&inbox)));

    // Rolü bu arada değişmiş bir kutu için kuyruğa düşmüş boşaltma reddedilir.
    let state = store.jmap_account_state(ACC).unwrap().email_state;
    let id = store
        .action_push(
            ACC,
            &Action::Empty {
                path: "mb-in".into(),
                upto: 0,
                uidvalidity: None,
                since: state,
            },
            None,
            1,
        )
        .unwrap();
    assert!(id.is_some());
    mgr.stop(ACC);
    let mgr = manager(&store, &secrets, &rec);
    mgr.start(account.clone());
    wait_until("kuyruk boşalır", || {
        store.actions(ACC).unwrap().is_empty()
    })
    .await;
    assert!(server.with(|s| s.emails.contains_key(&inbox)));
    mgr.stop(ACC);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn yonetici_daha_eski_postalari_indirir_cevrimdisiyken_ag_hatasi() {
    let (server, store, secrets, account) = setup().await;
    server.with(|s| {
        for i in 0..330 {
            s.add_email(
                "mb-in",
                mail(&format!("g{i}")),
                &[],
                1_791_000_000_000 + i * 1000,
            );
        }
    });
    let rec = Arc::new(Rec::default());
    let mgr = manager(&store, &secrets, &rec);
    let inbox = format!("{ACC}:mb-in");
    // Bilinmeyen kutu.
    assert_eq!(
        mgr.fetch_older(&inbox).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    mgr.start(account.clone());
    wait_until("ilk eşitleme", || local_ids(&store, "mb-in").len() == 300).await;
    wait_until("durum yazılır", || {
        store.jmap_account_state(ACC).unwrap().email_state.is_some()
    })
    .await;

    let o = mgr.fetch_older(&inbox).await.unwrap();
    assert_eq!((o.fetched, o.more), (30, false));
    assert_eq!(o.changed, std::slice::from_ref(&inbox));
    assert_eq!(local_ids(&store, "mb-in").len(), 330);
    assert!(
        rec.new_mail.lock().unwrap().is_empty(),
        "eski postalar yeni posta bildirimi üretmez"
    );
    // Başına ulaşılmış kutu için sunucuya yeniden gidilmez.
    let calls = server.with(|s| s.calls.len());
    let o = mgr.fetch_older(&inbox).await.unwrap();
    assert_eq!((o.fetched, o.more), (0, false));
    assert_eq!(server.with(|s| s.calls.len()), calls);
    mgr.stop(ACC);

    // Çevrimdışı: sunucuya ulaşılamıyor.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut offline = account.clone();
    offline.jmap_url = Some(format!("http://127.0.0.1:{port}/.well-known/jmap"));
    store.insert_account(&offline).unwrap();
    store.set_mailbox_older_done(&inbox, false).unwrap();
    let mgr = manager(&store, &secrets, &rec);
    let err = mgr.fetch_older(&inbox).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Network);
    assert!(!err.message.contains("gizli"));
}
