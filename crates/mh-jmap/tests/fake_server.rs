//! Süreç içi sahte JMAP sunucusuna karşı uçtan uca testler: keşif, giriş,
//! ilk ve artımlı eşitleme, sunucu işlemleri, durum akışı, yoklama,
//! gönderim ve taslaklar.

mod fake;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fake::{raw_mail, redirect_to, FakeServer, RawServer};
use mh_core::{
    Account, AccountKind, Category, ErrorCode, ListQuery, MailboxRole, MessageSummary, MoveTarget,
    SyncState, SyncStatus, View,
};
use mh_jmap::{Client, Credentials, JmapManager, Timing};
use mh_store::Store;
use mh_sync::{EventSink, MemorySecrets, SecretStore};

const ACC: &str = "acc";

#[derive(Default)]
struct Rec {
    statuses: Mutex<Vec<SyncStatus>>,
    changed: Mutex<Vec<String>>,
    new_mail: Mutex<Vec<MessageSummary>>,
}

impl EventSink for Rec {
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
        signature: None,
        created_at: 0,
        auth: Default::default(),
        notify: true,
    }
}

async fn setup(password: &str) -> (FakeServer, Arc<Store>, Arc<dyn SecretStore>, Account) {
    let server = FakeServer::start("furkan", "gizli").await;
    let store = Arc::new(Store::open_in_memory().unwrap());
    let account = account(&server.session_url());
    store.insert_account(&account).unwrap();
    store.set_jmap_username(ACC, "furkan").unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    secrets.set(ACC, password).unwrap();
    (server, store, secrets, account)
}

async fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
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
                cursor: None,
                limit: 100,
            },
            0,
        )
        .unwrap()
        .items
}

fn mail(from: &str, subject: &str, msgid: &str, body: &str) -> Vec<u8> {
    raw_mail(from, "Furkan <furkan@ornek.com>", subject, msgid, &[], body)
}

fn quick() -> Timing {
    Timing {
        poll: Duration::from_secs(60),
        periodic: Duration::from_secs(600),
        ping_secs: 5,
        min_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(1),
    }
}

#[tokio::test]
async fn kesif_ve_giris() {
    let server = FakeServer::start("furkan", "gizli").await;
    assert!(
        mh_jmap::probe(&server.session_url()).await,
        "giriş isteyen (401) JMAP adresi kabul edilir"
    );
    assert!(!mh_jmap::probe(&format!("{}/yok", server.base)).await);

    let err = mh_jmap::test_login(&server.session_url(), "furkan", "yanlis")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Auth);
    assert!(!err.message.contains("yanlis"), "parola hata metninde yok");

    let url = mh_jmap::test_login(&server.session_url(), "furkan", "gizli")
        .await
        .unwrap();
    assert!(
        url.as_str().ends_with("/jmap/session"),
        "yönlendirme izlenir, kimlik yeniden gönderilir: {url}"
    );
    let c = Client::connect(&server.session_url(), Credentials::new("furkan", "gizli"))
        .await
        .unwrap();
    assert_eq!(c.account_id(), "A");
    assert!(c.can_submit());
    assert_eq!(c.max_get(), 2);
}

#[tokio::test]
async fn baska_kokene_yonlendirmede_parola_gonderilmez() {
    // Saldırganın sunucusu: gelen her isteği kaydeder.
    let trap = RawServer::start(|_| {
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"
            .into()
    })
    .await;
    // Başka sunucu adı (PoC: 127.0.0.1 → localhost) ve yalnız başka kapı.
    for target in [
        format!("http://localhost:{}/jmap/session", trap.port),
        format!("http://127.0.0.1:{}/jmap/session", trap.port),
    ] {
        let t = target.clone();
        let entry = RawServer::start(move |_| redirect_to(&t)).await;
        let err = mh_jmap::test_login(&entry.url("/.well-known/jmap"), "furkan", "gizli")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Invalid, "{}", err.message);
        assert!(
            err.message.contains(&target),
            "kullanıcıya son adres gösterilir: {}",
            err.message
        );
        assert!(!err.message.contains("gizli"));
        let err = Client::connect(
            &entry.url("/.well-known/jmap"),
            Credentials::new("furkan", "gizli"),
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_app().code, ErrorCode::Invalid);
        // Kullanıcının yazdığı sunucu kimliği alır; yönlendirme izlenmez.
        let seen = entry.requests();
        assert_eq!(seen.len(), 2);
        assert!(seen[0]
            .to_ascii_lowercase()
            .contains("authorization: basic"));
    }
    assert!(
        trap.requests().is_empty(),
        "başka kökene hiç istek gitmez: {:?}",
        trap.requests()
    );
}

#[tokio::test]
async fn kesifte_baska_kokene_kimliksiz_yonlendirme_izlenir() {
    let server = FakeServer::start("furkan", "gizli").await;
    let target = server.session_url();
    let entry = RawServer::start(move |_| redirect_to(&target)).await;
    // Kimliksiz yoklama başka kökene geçer (son adres kullanıcıya gösterilir).
    assert!(mh_jmap::probe(&entry.url("/.well-known/jmap")).await);
    // Kullanıcı son adresi yazınca giriş olur.
    mh_jmap::test_login(&server.session_url(), "furkan", "gizli")
        .await
        .unwrap();
}

#[tokio::test]
async fn oturum_adresleri_baska_kokende_olamaz() {
    let trap = RawServer::start(|_| {
        "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}".into()
    })
    .await;
    let cases = [
        ("apiUrl", trap.url("/jmap/api"), "apiUrl"),
        (
            "downloadUrl",
            trap.url("/d/{accountId}/{blobId}/{name}?accept={type}"),
            "downloadUrl",
        ),
        ("uploadUrl", trap.url("/u/{accountId}/"), "uploadUrl"),
        // Aynı sunucu, başka şema/kapı da başka kökendir.
        (
            "apiUrl",
            "http://localhost:1/jmap/api".to_owned(),
            "localhost:1",
        ),
    ];
    for (field, value, needle) in cases {
        let server = FakeServer::start("furkan", "gizli").await;
        server.with(|s| s.session_overrides.push((field.into(), value.clone())));
        let err = Client::connect(&server.session_url(), Credentials::new("furkan", "gizli"))
            .await
            .unwrap_err()
            .to_app();
        assert_eq!(err.code, ErrorCode::Invalid, "{field}: {}", err.message);
        assert!(err.message.contains(needle), "{}", err.message);
        assert!(!err.message.contains("gizli"));
    }

    // Durum akışı isteğe bağlıdır: başka kökendeyse hesap açılır, akış
    // kullanılmaz.
    let server = FakeServer::start("furkan", "gizli").await;
    server.with(|s| {
        s.session_overrides.push((
            "eventSourceUrl".into(),
            trap.url("/es?types={types}&closeafter={closeafter}&ping={ping}"),
        ))
    });
    let c = Client::connect(&server.session_url(), Credentials::new("furkan", "gizli"))
        .await
        .unwrap();
    assert!(c.event_source_url("Email,Mailbox", 5).is_none());
    // Göreli adresler oturumun kökenine çözülür.
    let server = FakeServer::start("furkan", "gizli").await;
    server.with(|s| {
        s.session_overrides
            .push(("apiUrl".into(), "/jmap/api".into()))
    });
    let c = Client::connect(&server.session_url(), Credentials::new("furkan", "gizli"))
        .await
        .unwrap();
    c.call("Mailbox/get", serde_json::json!({ "accountId": "A" }))
        .await
        .unwrap();

    assert!(trap.requests().is_empty(), "{:?}", trap.requests());
}

#[tokio::test]
async fn cok_buyuk_yanitlar_okunmaz() {
    // Oturum yanıtı 16 MB'tan büyük olduğunu bildiriyor: gövde beklenmez.
    let big = RawServer::start(|_| {
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 20000000\r\n\r\n{"
            .into()
    })
    .await;
    let err = tokio::time::timeout(
        Duration::from_secs(5),
        mh_jmap::test_login(&big.url("/jmap/session"), "furkan", "gizli"),
    )
    .await
    .expect("gövde beklenmeden döner")
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
    assert!(err.message.contains("16 MB"), "{}", err.message);

    // 64 MB'ı aşan blob indirilmez.
    let server = FakeServer::start("furkan", "gizli").await;
    server.with(|s| {
        s.blobs
            .insert("BUYUK".into(), vec![b'x'; 64 * 1024 * 1024 + 1])
    });
    let c = Client::connect(&server.session_url(), Credentials::new("furkan", "gizli"))
        .await
        .unwrap();
    let err = c
        .download("BUYUK", "posta.eml", "message/rfc822")
        .await
        .unwrap_err();
    assert!(!err.is_connection(), "yeniden denenmez");
    assert!(err.to_app().message.contains("64 MB"), "{err}");
}

#[tokio::test]
async fn ilk_ve_artimli_esitleme() {
    let (server, store, secrets, account) = setup("gizli").await;
    let ids = server.with(|s| {
        [
            s.add_email(
                "mb-in",
                mail(
                    "Ayşe Demir <ayse@eposta.com>",
                    "Cumartesi kahvaltı?",
                    "k1@eposta.com",
                    "Moda'ya gidelim mi?",
                ),
                &[],
                1_791_700_000_000,
            ),
            s.add_email(
                "mb-in",
                raw_mail(
                    "Bülten <bulten@ornek.com>",
                    "furkan@ornek.com",
                    "Ekim bülteni",
                    "b1@ornek.com",
                    &[("List-Id", "<duyuru.ornek.com>")],
                    "Yenilikler",
                ),
                &["$seen"],
                1_791_600_000_000,
            ),
            s.add_email(
                "mb-in",
                mail(
                    "Kerem <kerem@ornek.com>",
                    "Bütçe",
                    "c1@ornek.com",
                    "Rakamlar ekte",
                ),
                &["$seen", "$flagged"],
                1_791_500_000_000,
            ),
            s.add_email(
                "mb-se",
                mail(
                    "Furkan <furkan@ornek.com>",
                    "Re: Cumartesi kahvaltı?",
                    "d1@ornek.com",
                    "Geliyorum",
                ),
                &["$seen"],
                1_791_710_000_000,
            ),
            s.add_email(
                "mb-pr",
                mail(
                    "Can <can@ornek.com>",
                    "Proje",
                    "e1@ornek.com",
                    "Ayrı klasörde",
                ),
                &[],
                1_791_400_000_000,
            ),
            s.add_email(
                "mb-ju",
                mail("Kötü <spam@kotu.com>", "Kazandınız", "f1@kotu.com", "Tıkla"),
                &[],
                1_791_300_000_000,
            ),
        ]
    });
    // Kullanıcı özel klasörü ve Gereksiz'i gizlemiş: bunlar yerelde izlenmez.
    for (path, name, role) in [
        ("mb-pr", "Projeler", MailboxRole::Other),
        ("mb-ju", "Junk Mail", MailboxRole::Junk),
    ] {
        store
            .upsert_mailbox(ACC, path, name, role, None, false)
            .unwrap();
        store
            .set_mailbox_hidden(&format!("{ACC}:{path}"), true)
            .unwrap();
    }
    let client = mh_jmap::connect(&store, &secrets, &account).await.unwrap();
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(out.full, "durum yokken baştan eşitlenir");
    assert!(out.new_unread.is_empty(), "ilk eşitlemede bildirim yok");

    // Kutular: kimlik "<hesap>:<jmap kimliği>", yol = jmap kimliği, ad = görünen ad.
    let boxes = store.mailboxes_for(ACC).unwrap();
    let mb = |id: &str| {
        boxes
            .iter()
            .find(|b| b.mailbox.id == format!("{ACC}:{id}"))
            .unwrap_or_else(|| panic!("{id} yok"))
            .mailbox
            .clone()
    };
    assert_eq!(mb("mb-in").role, MailboxRole::Inbox);
    assert_eq!(mb("mb-in").path, "mb-in");
    assert_eq!(mb("mb-se").role, MailboxRole::Sent);
    assert_eq!(mb("mb-se").name, "Sent Items");
    assert_eq!(mb("mb-tr").role, MailboxRole::Trash);
    assert_eq!(mb("mb-ju").role, MailboxRole::Junk);
    assert_eq!(mb("mb-pr").role, MailboxRole::Other);

    let inbox = list(&store, View::Inbox);
    let subjects: Vec<&str> = inbox.iter().map(|m| m.subject.as_str()).collect();
    assert_eq!(
        subjects,
        vec!["Cumartesi kahvaltı?", "Ekim bülteni", "Bütçe"]
    );
    assert!(inbox[0].unread && !inbox[0].starred);
    assert_eq!(inbox[0].category, Category::People);
    assert_eq!(inbox[0].snippet, "Moda'ya gidelim mi?");
    assert!(inbox[0].thread_id.starts_with("j:"));
    assert_eq!(
        inbox[1].category,
        Category::Newsletters,
        "List-Id başlığı istenir"
    );
    assert!(!inbox[1].unread);
    assert!(inbox[2].starred);
    assert_eq!(list(&store, View::Sent).len(), 1);
    assert_eq!(
        store.jmap_known(ACC).unwrap().len(),
        4,
        "gizli özel klasör ve gizli Gereksiz eşitlenmez"
    );
    assert!(
        server.with(|s| s.calls.iter().filter(|c| *c == "Email/get").count()) >= 3,
        "parçalı Email/get"
    );

    // Yeni posta: bildirim ve gövde önden indirme.
    let g = server.with(|s| {
        s.add_email(
            "mb-in",
            mail(
                "Ayşe Demir <ayse@eposta.com>",
                "Yeni",
                "g1@eposta.com",
                "Taze posta",
            ),
            &[],
            1_791_720_000_000,
        )
    });
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(!out.full);
    assert_eq!(out.new_unread.len(), 1);
    assert_eq!(out.changed, vec![format!("{ACC}:mb-in")]);
    assert_eq!(out.prefetch.len(), 1);
    assert_eq!(
        mh_jmap::prefetch(&client, &store, &out.prefetch)
            .await
            .len(),
        1
    );
    let gid = store.jmap_message(ACC, &g).unwrap().unwrap();
    assert!(store.raw(gid).unwrap().is_some());

    // Başka bir istemci okundu işaretledi.
    server.with(|s| {
        s.update_email(&ids[0], |e| {
            e.keywords.insert("$seen".into());
        })
    });
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(out.new_unread.is_empty());
    let aid = store.jmap_message(ACC, &ids[0]).unwrap().unwrap();
    assert!(!store.summary(aid).unwrap().unwrap().unread);
    assert_eq!(out.changed, vec![format!("{ACC}:mb-in")]);

    // Başka yerde çöpe taşındı, özel klasöre taşındı, silindi.
    server.with(|s| {
        s.update_email(&ids[2], |e| {
            e.mailbox_ids = BTreeSet::from(["mb-tr".to_string()])
        });
        s.update_email(&ids[1], |e| {
            e.mailbox_ids = BTreeSet::from(["mb-pr".to_string()])
        });
        s.destroy_email(&ids[3]);
    });
    mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    let cid = store.jmap_message(ACC, &ids[2]).unwrap().unwrap();
    assert_eq!(
        store.jmap_locations(&[cid]).unwrap()[0].role,
        MailboxRole::Trash
    );
    assert!(store.jmap_message(ACC, &ids[1]).unwrap().is_none());
    assert!(store.jmap_message(ACC, &ids[3]).unwrap().is_none());
    assert!(list(&store, View::Sent).is_empty());

    // Özel klasörden gelen kutusuna taşınan posta yeni sayılır.
    server.with(|s| {
        s.update_email(&ids[4], |e| {
            e.mailbox_ids = BTreeSet::from(["mb-in".to_string()])
        })
    });
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(out.new_unread.len(), 1);

    // Sunucu farkı hesaplayamıyor: baştan eşitlenir, sonuç aynı.
    let before: BTreeSet<String> = store.jmap_known(ACC).unwrap().into_keys().collect();
    server.with(|s| s.fail_changes = true);
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(out.full);
    assert!(out.new_unread.is_empty());
    let after: BTreeSet<String> = store.jmap_known(ACC).unwrap().into_keys().collect();
    assert_eq!(before, after);

    // Gövde: blob kimliğiyle ve kimliksiz (Email/get ile bulunur).
    let loc = store.jmap_locations(&[aid]).unwrap().remove(0);
    mh_jmap::fetch_body(&client, &store, aid, &ids[0], loc.blob_id.as_deref())
        .await
        .unwrap();
    let d = store.detail(aid, &[]).unwrap();
    assert!(d.body_text.unwrap().contains("Moda'ya gidelim mi?"));
    mh_jmap::fetch_body(&client, &store, cid, &ids[2], None)
        .await
        .unwrap();
    assert!(store.raw(cid).unwrap().is_some());
}

#[tokio::test]
async fn klasor_agaci_gizli_roller_ve_gorunen_klasorler() {
    let (server, store, secrets, account) = setup("gizli").await;
    let ids = server.with(|s| {
        for (id, name, role) in [
            ("mb-p2", "2024", None),
            ("mb-st", "Starred", Some("flagged")),
            ("mb-al", "All Mail", Some("all")),
            ("mb-im", "Important", Some("important")),
        ] {
            s.mailboxes
                .push((id.into(), name.into(), role.map(str::to_owned)));
        }
        s.parents.insert("mb-p2".into(), "mb-pr".into());
        // Rol kutusunun üst klasörü yok sayılır.
        s.parents.insert("mb-se".into(), "mb-pr".into());
        [
            s.add_email(
                "mb-pr",
                mail("Can <can@ornek.com>", "Proje", "p1@ornek.com", "Klasörde"),
                &[],
                1_791_400_000_000,
            ),
            s.add_email(
                "mb-ju",
                mail("Kötü <spam@kotu.com>", "Kazandınız", "j1@kotu.com", "Tıkla"),
                &[],
                1_791_300_000_000,
            ),
            s.add_email(
                "mb-st",
                mail("Ayşe <ayse@ornek.com>", "Yıldızlı", "s1@ornek.com", "Kopya"),
                &["$flagged"],
                1_791_200_000_000,
            ),
            s.add_email(
                "mb-p2",
                mail(
                    "Kerem <kerem@ornek.com>",
                    "Mart",
                    "m1@ornek.com",
                    "Alt klasör",
                ),
                &[],
                1_791_100_000_000,
            ),
        ]
    });
    let client = mh_jmap::connect(&store, &secrets, &account).await.unwrap();
    mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    let mb = |id: &str| {
        store
            .mailbox(&format!("{ACC}:{id}"))
            .unwrap()
            .unwrap_or_else(|| panic!("{id} yok"))
            .mailbox
    };
    assert_eq!(mb("mb-p2").parent_id.as_deref(), Some("acc:mb-pr"));
    assert_eq!(mb("mb-pr").parent_id, None);
    assert_eq!(mb("mb-se").parent_id, None, "rol kutusu köktedir");
    for id in ["mb-st", "mb-al", "mb-im"] {
        assert!(mb(id).hidden, "{id} ilk eklemede gizli");
        assert_eq!(mb(id).role, MailboxRole::Other);
    }
    for id in ["mb-in", "mb-pr", "mb-p2", "mb-ju", "mb-se"] {
        assert!(!mb(id).hidden, "{id} görünür");
    }
    // Görünen klasör ve Gereksiz izlenir; gizli kopya kutusu izlenmez.
    let known = store.jmap_known(ACC).unwrap();
    assert!(known.contains_key(&ids[0]) && known.contains_key(&ids[1]));
    assert!(known.contains_key(&ids[3]));
    assert!(
        !known.contains_key(&ids[2]),
        "gizli yıldızlı kutusu izlenmez"
    );
    let loc = |j: &str| {
        let id = store.jmap_message(ACC, j).unwrap().unwrap();
        store.jmap_locations(&[id]).unwrap().remove(0)
    };
    assert_eq!(loc(&ids[0]).mailbox_id, "acc:mb-pr");
    assert_eq!(loc(&ids[1]).role, MailboxRole::Junk);

    // Kullanıcının gizleme seçimi sonraki eşitlemede korunur; baştan
    // eşitlemede gizlenen klasörün postaları yerelden kalkar.
    store.set_mailbox_hidden("acc:mb-pr", true).unwrap();
    store.set_mailbox_hidden("acc:mb-st", false).unwrap();
    store.set_jmap_email_state(ACC, None).unwrap();
    let out = mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert!(out.full);
    assert!(mb("mb-pr").hidden && !mb("mb-st").hidden);
    let known = store.jmap_known(ACC).unwrap();
    assert!(!known.contains_key(&ids[0]), "gizlenen klasör izlenmez");
    assert!(known.contains_key(&ids[2]), "gösterilen kutu izlenir");

    // Yalnız klasör ve kopya kutusundaki posta: klasör önce (rol yok, kimlik sırası).
    server.with(|s| {
        s.update_email(&ids[2], |e| {
            e.mailbox_ids = BTreeSet::from(["mb-st".to_string(), "mb-p2".to_string()])
        })
    });
    mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(loc(&ids[2]).mailbox_id, "acc:mb-p2");

    // Elle rol: "2024" arşiv olur; sonraki eşitlemede de öyle kalır.
    store
        .set_mailbox_role("acc:mb-p2", MailboxRole::Archive)
        .unwrap();
    store.set_jmap_email_state(ACC, None).unwrap();
    store.set_jmap_mailbox_state(ACC, None).unwrap();
    mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(mb("mb-p2").role, MailboxRole::Archive);
    assert_eq!(mb("mb-p2").parent_id, None);
    assert_eq!(loc(&ids[3]).role, MailboxRole::Archive);
    // Sıfırlama: roller sunucudan yeniden bulunur.
    store.reset_mailbox_roles(ACC).unwrap();
    store.set_jmap_email_state(ACC, None).unwrap();
    mh_jmap::incremental_sync(&client, &store, ACC, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(mb("mb-p2").role, MailboxRole::Other);
    assert_eq!(mb("mb-p2").parent_id.as_deref(), Some("acc:mb-pr"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn yonetici_islemler_akis_gonderim_taslak() {
    let (server, store, secrets, account) = setup("gizli").await;
    let a = server.with(|s| {
        s.add_email(
            "mb-in",
            mail(
                "Ayşe Demir <ayse@eposta.com>",
                "Cumartesi kahvaltı?",
                "k1@eposta.com",
                "Moda'ya gidelim mi?",
            ),
            &[],
            1_791_700_000_000,
        )
    });
    let rec = Arc::new(Rec::default());
    let mgr = JmapManager::with_timing(
        store.clone(),
        secrets.clone(),
        rec.clone(),
        tokio::runtime::Handle::current(),
        quick(),
    );
    mgr.start(account.clone());
    assert!(mgr.is_running(ACC));
    wait_until("ilk eşitleme", || {
        store.jmap_message(ACC, &a).unwrap().is_some()
    })
    .await;
    wait_until("çevrimiçi", || {
        rec.statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.state == SyncState::Online)
    })
    .await;
    wait_until("durum akışı", || {
        server.with(|s| s.subscriber_count() > 0)
    })
    .await;
    assert!(rec.new_mail.lock().unwrap().is_empty());

    // Durum akışı: yeni posta hemen gelir ve bildirim kararına gider.
    let b = server.with(|s| {
        s.add_email(
            "mb-in",
            mail(
                "Kerem <kerem@ornek.com>",
                "Anlık",
                "b1@ornek.com",
                "Hemen geldi",
            ),
            &[],
            1_791_720_000_000,
        )
    });
    wait_until("anlık bildirim", || {
        rec.new_mail
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.subject == "Anlık" && m.unread)
    })
    .await;

    // Bayraklar: yerelde hemen, sunucuda Email/set ile.
    let id_a = store.jmap_message(ACC, &a).unwrap().unwrap();
    let touched = mgr.set_flags(&[id_a], Some(false), Some(true)).unwrap();
    assert_eq!(touched, vec![format!("{ACC}:mb-in")]);
    assert!(!store.summary(id_a).unwrap().unwrap().unread);
    wait_until("sunucuda $seen ve $flagged", || {
        server.with(|s| {
            let k = &s.emails[&a].keywords;
            k.contains("$seen") && k.contains("$flagged")
        })
    })
    .await;

    // Arşiv kutusu yok: sunucuda rolüyle oluşturulur, taşıma eşitlemeyle gelir.
    let touched = mgr.move_messages(&[id_a], MoveTarget::Archive).unwrap();
    assert!(touched.is_empty());
    wait_until("sunucuda arşive taşındı", || {
        server.with(|s| {
            s.role_id("archive")
                .is_some_and(|ar| s.emails[&a].mailbox_ids == BTreeSet::from([ar]))
        })
    })
    .await;
    wait_until("yerelde arşivde", || {
        store.jmap_locations(&[id_a]).unwrap()[0].role == MailboxRole::Archive
    })
    .await;

    // Var olan kutuya taşıma yerelde hemen olur.
    let touched = mgr.move_messages(&[id_a], MoveTarget::Trash).unwrap();
    assert!(touched.contains(&format!("{ACC}:mb-tr")));
    assert_eq!(
        store.jmap_locations(&[id_a]).unwrap()[0].role,
        MailboxRole::Trash
    );
    wait_until("sunucuda çöpte", || {
        server.with(|s| s.emails[&a].mailbox_ids == BTreeSet::from(["mb-tr".to_string()]))
    })
    .await;

    // Gövde isteğe bağlı indirilir.
    let id_b = store.jmap_message(ACC, &b).unwrap().unwrap();
    mgr.fetch_body(id_b).await.unwrap();
    assert!(store.raw(id_b).unwrap().is_some());

    // Gönderim: Taslaklar'a aktarılır, EmailSubmission ile gider, Gönderilenler'e taşınır.
    let raw = raw_mail(
        "Furkan Yıldırım <furkan@ornek.com>",
        "Ayşe Demir <ayse@eposta.com>",
        "Gönderim denemesi",
        "g1@ornek.com",
        &[],
        "Merhaba",
    );
    let rcpt = vec!["ayse@eposta.com".to_string(), "gizli@ornek.com".to_string()];
    mh_jmap::send_raw(&store, &secrets, &account, &raw, &rcpt)
        .await
        .unwrap();
    server.with(|s| {
        let sub = s.submissions.last().unwrap().clone();
        assert_eq!(sub.rcpt_to, rcpt, "gizli alıcı zarfta");
        assert_eq!(sub.mail_from, "furkan@ornek.com");
        assert_eq!(sub.identity_id, "I1");
        let e = &s.emails[&sub.email_id];
        assert_eq!(e.mailbox_ids, BTreeSet::from(["mb-se".to_string()]));
        assert!(!e.keywords.contains("$draft"));
        assert!(e.keywords.contains("$seen"));
        assert_eq!(e.raw, raw, "ham posta aynen gider");
    });
    wait_until("gönderilen yerelde", || {
        list(&store, View::Sent)
            .iter()
            .any(|m| m.subject == "Gönderim denemesi" && !m.unread)
    })
    .await;

    // Reddedilen gönderim: Türkçe hata, taslak kopyası sunucudan silinir.
    server.with(|s| s.reject_submission = true);
    let before = server.with(|s| s.emails.len());
    let err = mh_jmap::send_raw(&store, &secrets, &account, &raw, &rcpt)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
    assert!(err.message.contains("izin vermedi"), "{}", err.message);
    assert_eq!(server.with(|s| s.emails.len()), before);
    server.with(|s| s.reject_submission = false);

    // Taslaklar: yerelde hemen, sunucuya kuyrukla; yerine yazmada kimlik korunur.
    let drafts_on_server = |server: &FakeServer| {
        server.with(|s| {
            s.emails
                .values()
                .filter(|e| e.mailbox_ids.contains("mb-dr"))
                .map(|e| {
                    assert!(e.keywords.contains("$draft"));
                    mh_mime::parse(&e.raw, true).unwrap().subject
                })
                .collect::<Vec<_>>()
        })
    };
    let now = 1_791_800_000_000;
    let (d1, touched) = mgr
        .save_draft(
            ACC,
            mail(
                "Furkan <furkan@ornek.com>",
                "Taslak 1",
                "t1@ornek.com",
                "Yarım",
            ),
            None,
            now,
        )
        .unwrap();
    assert_eq!(touched, vec![format!("{ACC}:mb-dr")]);
    assert!(list(&store, View::Drafts)
        .iter()
        .any(|m| m.subject == "Taslak 1"));
    wait_until("taslak sunucuda", || {
        drafts_on_server(&server) == vec!["Taslak 1".to_string()]
            && store.jmap_locations(&[d1]).unwrap()[0].jmap_id.is_some()
    })
    .await;
    let j1 = store.jmap_locations(&[d1]).unwrap()[0]
        .jmap_id
        .clone()
        .unwrap();

    let (d2, _) = mgr
        .save_draft(
            ACC,
            mail(
                "Furkan <furkan@ornek.com>",
                "Taslak 2",
                "t2@ornek.com",
                "Daha az yarım",
            ),
            Some(d1),
            now,
        )
        .unwrap();
    assert_eq!(d2, d1, "yerine yazınca kimlik korunur");
    wait_until("yeni sürüm sunucuda, eskisi silindi", || {
        drafts_on_server(&server) == vec!["Taslak 2".to_string()]
            && store.jmap_locations(&[d1]).unwrap()[0]
                .jmap_id
                .as_ref()
                .is_some_and(|j| *j != j1)
    })
    .await;

    // Kuyruk çalışmadan art arda iki kayıt: sunucuda yalnız son sürüm kalır.
    for (subject, msgid) in [("Taslak 3", "t3@ornek.com"), ("Taslak 4", "t4@ornek.com")] {
        let (id, _) = mgr
            .save_draft(
                ACC,
                mail("Furkan <furkan@ornek.com>", subject, msgid, "Son"),
                Some(d1),
                now,
            )
            .unwrap();
        assert_eq!(id, d1);
    }
    wait_until("yalnız son sürüm", || {
        drafts_on_server(&server) == vec!["Taslak 4".to_string()]
            && list(&store, View::Drafts)
                .into_iter()
                .map(|m| m.subject)
                .collect::<Vec<_>>()
                == vec!["Taslak 4".to_string()]
    })
    .await;

    // Silme: yerelden hemen, sunucudan kuyrukla; taslak olmayan posta silinmez.
    assert_eq!(mgr.delete_draft(id_b).unwrap_err().code, ErrorCode::Invalid);
    assert_eq!(mgr.delete_draft(d1).unwrap(), vec![format!("{ACC}:mb-dr")]);
    assert!(store.summary(d1).unwrap().is_none());
    assert!(
        mgr.delete_draft(d1).unwrap().is_empty(),
        "ikinci silme sessiz"
    );
    wait_until("sunucuda taslak kalmadı", || {
        drafts_on_server(&server).is_empty()
    })
    .await;
    wait_until("eşitleme taslağı geri getirmez", || {
        list(&store, View::Drafts).is_empty()
    })
    .await;

    mgr.stop(ACC);
    assert!(!mgr.is_running(ACC));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn akis_yoksa_yoklanir() {
    let (server, store, secrets, account) = setup("gizli").await;
    server.with(|s| s.no_eventsource = true);
    let rec = Arc::new(Rec::default());
    let mgr = JmapManager::with_timing(
        store.clone(),
        secrets,
        rec.clone(),
        tokio::runtime::Handle::current(),
        Timing {
            poll: Duration::from_millis(300),
            ..quick()
        },
    );
    mgr.start(account);
    wait_until("ilk eşitleme", || {
        rec.statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.state == SyncState::Online)
    })
    .await;
    server.with(|s| {
        s.add_email(
            "mb-in",
            mail(
                "Ayşe <ayse@eposta.com>",
                "Yoklamayla",
                "y1@eposta.com",
                "Geldi",
            ),
            &[],
            1_791_720_000_000,
        )
    });
    wait_until("yoklamayla bildirim", || {
        rec.new_mail
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.subject == "Yoklamayla")
    })
    .await;
    mgr.stop(ACC);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn yanlis_parola_hata_durumu() {
    let (_server, store, secrets, account) = setup("yanlis").await;
    let rec = Arc::new(Rec::default());
    let mgr = JmapManager::with_timing(
        store,
        secrets,
        rec.clone(),
        tokio::runtime::Handle::current(),
        quick(),
    );
    mgr.start(account);
    wait_until("kimlik hatası", || {
        rec.statuses.lock().unwrap().iter().any(|s| {
            matches!(&s.state, SyncState::Error { message } if message.contains("parolayı kabul etmedi"))
        })
    })
    .await;
    mgr.stop(ACC);
}

#[tokio::test]
async fn tanitim_hesabi_aga_cikmaz() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    mh_store::seed_demo(
        &store,
        chrono::DateTime::parse_from_rfc3339("2026-10-13T10:30:00+03:00").unwrap(),
    )
    .unwrap();
    let demo = store
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.kind == AccountKind::Jmap)
        .unwrap();
    assert!(mh_jmap::is_demo(&demo));
    let rec = Arc::new(Rec::default());
    let mgr = JmapManager::new(
        store.clone(),
        Arc::new(MemorySecrets::new()),
        rec.clone(),
        tokio::runtime::Handle::current(),
    );
    mgr.start_all().unwrap();
    assert!(!mgr.is_running(&demo.id));
    assert_eq!(
        rec.statuses.lock().unwrap().clone(),
        vec![SyncStatus {
            account_id: demo.id.clone(),
            state: SyncState::Online
        }]
    );
    // Tanıtım taslağı yalnız yerelde.
    let (id, _) = mgr
        .save_draft(
            &demo.id,
            mail(
                "Furkan <posta@alanadi.com>",
                "Yerel taslak",
                "yt@alanadi.com",
                "x",
            ),
            None,
            0,
        )
        .unwrap();
    assert!(store.jmap_locations(&[id]).unwrap()[0].jmap_id.is_none());
    assert_eq!(
        store.jmap_locations(&[id]).unwrap()[0].role,
        MailboxRole::Drafts
    );
    mgr.delete_draft(id).unwrap();
    assert!(store.summary(id).unwrap().is_none());
}
