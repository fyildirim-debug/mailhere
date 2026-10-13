//! Gerçek bir IMAP sunucusuna (Dovecot `oauth2` passdb) karşı XOAUTH2 testi.
//!
//! Yalnız `MH_TEST_XOAUTH2_IMAP=host:port:kullanıcı:introspection_kapısı`
//! tanımlıysa çalışır. Test, sahte OAuth sağlayıcısını (jeton ve
//! introspection uç noktaları) `127.0.0.1:<introspection_kapısı>` üzerinde
//! kendisi başlatır; Dovecot ayarında
//! `introspection_url = http://127.0.0.1:<kapı>/introspect`,
//! `introspection_mode = post` ve `username_attribute = username` olmalı.
//! Sunucuda şifresiz bağlantı kullanılır (yalnız localhost).
//!
//! Test kullanıcısının INBOX'ına bir posta eklenir; başka veri silinmez.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mh_core::{
    Account, AccountKind, AuthKind, ErrorCode, MessageSummary, Security, ServerConfig, SyncState,
    SyncStatus,
};
use mh_oauth::testing::FakeProvider;
use mh_store::Store;
use mh_sync::{
    connect_with, oauth_secret_key, test_login_with, token_rejected, Credentials, EventSink,
    MemorySecrets, SecretStore, SyncManager, TokenProvider,
};

struct Env {
    cfg: ServerConfig,
    introspect_port: u16,
}

fn env_config() -> Option<Env> {
    let v = std::env::var("MH_TEST_XOAUTH2_IMAP").ok()?;
    let p: Vec<&str> = v.split(':').collect();
    assert_eq!(
        p.len(),
        4,
        "MH_TEST_XOAUTH2_IMAP biçimi: host:port:kullanıcı:introspection_kapısı"
    );
    Some(Env {
        cfg: ServerConfig {
            host: p[0].into(),
            port: p[1].parse().expect("kapı sayı olmalı"),
            security: Security::None,
            username: p[2].into(),
        },
        introspect_port: p[3].parse().expect("introspection kapısı sayı olmalı"),
    })
}

#[derive(Default)]
struct Recorder {
    statuses: Mutex<Vec<SyncStatus>>,
}

impl EventSink for Recorder {
    fn sync_status(&self, status: SyncStatus) {
        self.statuses.lock().unwrap().push(status);
    }
    fn changed(&self, _mailbox_ids: Vec<String>) {}
    fn new_mail(&self, _account: &Account, _messages: Vec<MessageSummary>) {}
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dovecot_xoauth2_uctan_uca() {
    let Some(env) = env_config() else {
        eprintln!("MH_TEST_XOAUTH2_IMAP tanımlı değil; XOAUTH2 tümleşik testi atlandı");
        return;
    };
    let user = env.cfg.username.clone();
    let fake = FakeProvider::start_on(env.introspect_port, &user).await;
    fake.lock().username = user.clone();

    // 1. Geçersiz jeton reddedilir (Türkçe yetki hatası).
    let err = test_login_with(&env.cfg, &Credentials::OAuth2("gecersiz".into()))
        .await
        .unwrap_err();
    assert_eq!(err, token_rejected());

    // 2. Sağlayıcının verdiği jetonla giriş; INBOX'a örnek posta.
    let token = fake.issue_access_token();
    test_login_with(&env.cfg, &Credentials::OAuth2(token.clone()))
        .await
        .expect("XOAUTH2 girişi");
    let subject = format!("XOAUTH2 denemesi {}", std::process::id());
    let raw = format!(
        "From: Elif <elif@ornek.test>\r\nTo: {user}@ornek.test\r\nSubject: {subject}\r\n\
         Message-ID: <xoauth2-{}@ornek.test>\r\nDate: Tue, 13 Oct 2026 09:00:00 +0300\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nJetonla giriş.\r\n",
        std::process::id()
    );
    let mut s = connect_with(&env.cfg, &Credentials::OAuth2(token))
        .await
        .unwrap();
    s.append("INBOX", None, None, raw.as_bytes()).await.unwrap();
    s.logout().await.unwrap();

    // 3. Eşitleme motoru: yenileme jetonu anahtar zincirinde, erişim jetonu
    //    jeton sağlayıcısından; tam eşitleme XOAUTH2 ile yapılır.
    let store = Arc::new(Store::open_in_memory().unwrap());
    let account = Account {
        id: "oauth-hesap".into(),
        name: "Gmail".into(),
        email: format!("{user}@ornek.test"),
        display_name: "Furkan".into(),
        color: "#4C6FD8".into(),
        kind: AccountKind::Imap,
        imap: Some(env.cfg.clone()),
        smtp: None,
        jmap_url: None,
        signature: None,
        created_at: 0,
        auth: AuthKind::Google,
    };
    store.insert_account(&account).unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    let refresh = fake.issue_refresh_token();
    secrets
        .set(&oauth_secret_key(&account.id), &refresh)
        .unwrap();
    let google = fake.provider(AuthKind::Google);
    let tokens = Arc::new(TokenProvider::new(Arc::new(move |k| {
        (k == AuthKind::Google).then(|| google.clone())
    })));
    let sink = Arc::new(Recorder::default());
    let mgr = SyncManager::new(
        store.clone(),
        secrets,
        sink.clone(),
        tokio::runtime::Handle::current(),
    )
    .with_tokens(tokens);
    mgr.start(account.clone());

    let has_mail = |store: &Store| {
        store
            .search(&subject, 10)
            .map(|r| r.iter().any(|m| m.subject == subject))
            .unwrap_or(false)
    };
    wait_for("XOAUTH2 ile ilk eşitleme", 30, || {
        sink.statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.state == SyncState::Online)
            && has_mail(&store)
    })
    .await;
    assert!(
        fake.refresh_count() >= 1,
        "erişim jetonu yenilemeyle alınır"
    );

    // 4. Erişim kaldırıldı: yeniden bağlanınca Türkçe "yeniden giriş" hatası.
    fake.revoke(&refresh);
    fake.expire_access_tokens();
    sink.statuses.lock().unwrap().clear();
    mgr.start(account.clone());
    let revoked = mh_oauth::revoked(AuthKind::Google).message;
    wait_for("iptal edilen izin hatası", 30, || {
        sink.statuses
            .lock()
            .unwrap()
            .iter()
            .any(|s| matches!(&s.state, SyncState::Error { message } if *message == revoked))
    })
    .await;
    mgr.stop(&account.id);
    assert_eq!(mh_oauth::revoked(AuthKind::Google).code, ErrorCode::Auth);
}
