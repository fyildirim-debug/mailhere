//! Google / Microsoft OAuth 2 komutları: `oauth_providers`, `oauth_sign_in`,
//! `account_add_oauth` (`app/src/lib/api/types.ts` → `Commands`).
//!
//! Giriş tarayıcıda tamamlanınca jetonlar arka uçta, `grant_id` ile bellekte
//! 10 dakika bekler; arayüze yalnız kimlik ve adres döner. Hesap eklenince
//! yenileme jetonu anahtar zincirine (`oauth:<hesap>`) yazılır.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mh_core::{
    Account, AccountKind, AppError, AppResult, AuthKind, ErrorCode, NewOAuthAccount, OAuthGrant,
};
use mh_oauth::{OAuthError, Provider, SignedIn, SIGN_IN_TIMEOUT};
use mh_sync::Credentials;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;

use crate::state::{now_ms, AppState};

/// Tamamlanmış girişin hesap eklenmeden bekleyebileceği süre.
const GRANT_TTL: Duration = Duration::from_secs(10 * 60);

type Res<T> = Result<T, AppError>;

struct Pending {
    signed: SignedIn,
    created: Instant,
}

/// Bekleyen girişler (`grant_id` → jetonlar). Yalnız bellekte.
#[derive(Default)]
struct Grants {
    map: HashMap<String, Pending>,
}

impl Grants {
    fn purge(&mut self, now: Instant) {
        self.map
            .retain(|_, p| now.saturating_duration_since(p.created) < GRANT_TTL);
    }

    fn insert(&mut self, id: String, signed: SignedIn, now: Instant) {
        self.purge(now);
        self.map.insert(
            id,
            Pending {
                signed,
                created: now,
            },
        );
    }

    /// Girişi alır (tek kullanımlık); süresi dolmuşsa `None`.
    fn take(&mut self, id: &str, now: Instant) -> Option<(SignedIn, Instant)> {
        self.purge(now);
        self.map.remove(id).map(|p| (p.signed, p.created))
    }

    /// Hesap eklenemezse giriş yeniden denenebilsin (süresi aynı kalır).
    fn put_back(&mut self, id: String, signed: SignedIn, created: Instant) {
        self.map.insert(id, Pending { signed, created });
    }
}

/// OAuth komutlarının durumu (`main.rs` içinde `manage` edilir).
#[derive(Default)]
pub struct OAuthState {
    grants: Mutex<Grants>,
    /// Süren girişin iptal kancası: yeni giriş eskisini iptal eder.
    current: Mutex<Option<(u64, oneshot::Sender<()>)>>,
    next: AtomicU64,
}

impl OAuthState {
    fn grants(&self) -> std::sync::MutexGuard<'_, Grants> {
        self.grants.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Yeni girişi kaydeder; süren giriş varsa kancası bırakılarak iptal olur.
    fn begin(&self) -> (u64, oneshot::Receiver<()>) {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, tx));
        (id, rx)
    }

    fn finish(&self, id: u64) {
        let mut cur = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if cur.as_ref().is_some_and(|(c, _)| *c == id) {
            *cur = None;
        }
    }
}

fn not_oauth() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Bu giriş yolu tarayıcıyla giriş desteklemiyor.",
    )
}

/// Bu derlemede istemci kimliği tanımlı sağlayıcılar.
#[tauri::command]
pub fn oauth_providers() -> Vec<AuthKind> {
    mh_oauth::configured_kinds()
}

/// Sistem tarayıcısında girişi başlatır ve dönüşü bekler (en çok 5 dakika).
/// `email` adres ise sağlayıcının giriş ekranına önceden yazılır.
#[tauri::command]
pub async fn oauth_sign_in(
    app: AppHandle,
    window: WebviewWindow,
    oauth: State<'_, OAuthState>,
    provider: AuthKind,
    email: Option<String>,
) -> Res<OAuthGrant> {
    if provider == AuthKind::Password {
        return Err(not_oauth());
    }
    let config =
        Provider::configured(provider).ok_or_else(|| OAuthError::NotConfigured.to_app(provider))?;
    let (run, cancelled) = oauth.begin();
    let opener = app.clone();
    let result = mh_oauth::sign_in(
        &config,
        email.as_deref(),
        SIGN_IN_TIMEOUT,
        move |url| match opener.try_state::<crate::state::AppState>() {
            Some(state) => state.platform.open_url(&opener, url),
            None => opener
                .opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string()),
        },
        async move {
            let _ = cancelled.await;
        },
    )
    .await;
    oauth.finish(run);
    // Tarayıcıdan Mailhere'ye dön.
    let _ = window.set_focus();
    let signed = result.map_err(|e| {
        tracing::info!(provider = ?provider, error = %e, "OAuth girişi tamamlanmadı");
        e.to_app(provider)
    })?;

    let grant = OAuthGrant {
        grant_id: uuid::Uuid::new_v4().to_string(),
        provider,
        email: signed.identity.email.clone(),
        display_name: signed.identity.name.clone(),
    };
    oauth
        .grants()
        .insert(grant.grant_id.clone(), signed, Instant::now());
    // Kullanılmayan giriş süresi dolunca bellekten silinsin.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(GRANT_TTL + Duration::from_secs(1)).await;
        if let Some(st) = app.try_state::<OAuthState>() {
            st.grants().purge(Instant::now());
        }
    });
    Ok(grant)
}

/// Girişten IMAP hesabı oluşturur: Gmail / Outlook sunucuları, XOAUTH2 ile
/// IMAP girişi denenir, yenileme jetonu anahtar zincirine yazılır ve
/// eşitleme başlar. Aynı adres ve sağlayıcıyla kayıtlı hesap varsa yeni
/// hesap açılmaz; o hesabın oturumu yenilenir (yeniden giriş).
#[tauri::command]
pub async fn account_add_oauth(
    app: AppHandle,
    state: State<'_, AppState>,
    oauth: State<'_, OAuthState>,
    input: NewOAuthAccount,
) -> Res<Account> {
    let (signed, created) = oauth
        .grants()
        .take(&input.grant_id, Instant::now())
        .ok_or_else(|| AppError::new(ErrorCode::Auth, "Giriş süresi doldu. Yeniden giriş yap."))?;
    match add(&app, &state, &input, &signed).await {
        Ok(a) => Ok(a),
        Err(e) => {
            // Ağ ya da sunucu hatasında aynı girişle yeniden denenebilsin.
            oauth
                .grants()
                .put_back(input.grant_id.clone(), signed, created);
            Err(e)
        }
    }
}

async fn add(
    app: &AppHandle,
    state: &AppState,
    input: &NewOAuthAccount,
    signed: &SignedIn,
) -> AppResult<Account> {
    let kind = signed.provider;
    let (email, _) = mh_sync::discover::split_email(&signed.identity.email)?;
    let (imap, smtp) = mh_oauth::mail_servers(kind, &email).ok_or_else(not_oauth)?;
    let creds = Credentials::OAuth2(signed.tokens.access_token.clone());
    mh_sync::test_login_with(&imap, &creds).await?;

    let existing = state.store.accounts()?;
    if let Some(account) = existing
        .iter()
        .find(|a| a.auth == kind && a.email.eq_ignore_ascii_case(&email))
    {
        tracing::info!(account = %account.id, "OAuth hesabının oturumu yenilendi");
        state
            .sync
            .tokens()
            .save_grant(&state.secrets, &account.id, &signed.tokens)
            .await?;
        state.sync.start(account.clone());
        crate::sink::changed(app, Vec::new());
        return Ok(account.clone());
    }

    let id = uuid::Uuid::new_v4().to_string();
    state
        .sync
        .tokens()
        .save_grant(&state.secrets, &id, &signed.tokens)
        .await?;
    let palette = crate::commands::PALETTE;
    let account = Account {
        name: match input.name.trim() {
            "" => email.split('@').nth(1).unwrap_or(&email).to_owned(),
            n => n.to_owned(),
        },
        display_name: match input.display_name.trim() {
            "" => signed
                .identity
                .name
                .clone()
                .unwrap_or_else(|| email.split('@').next().unwrap_or("").to_owned()),
            d => d.to_owned(),
        },
        color: input
            .color
            .clone()
            .filter(|c| c.len() == 7 && c.starts_with('#'))
            .unwrap_or_else(|| palette[existing.len() % palette.len()].to_owned()),
        id,
        email,
        kind: AccountKind::Imap,
        imap: Some(imap),
        smtp: Some(smtp),
        jmap_url: None,
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: now_ms(),
        auth: kind,
        notify: true,
    };
    if let Err(e) = state.store.insert_account(&account) {
        state.sync.tokens().forget(&account.id);
        let (secrets, key) = (
            state.secrets.clone(),
            mh_sync::oauth_secret_key(&account.id),
        );
        let _ = tokio::task::spawn_blocking(move || secrets.delete(&key)).await;
        return Err(e);
    }
    state.sync.start(account.clone());
    crate::lock::accounts_changed(app);
    crate::sink::changed(app, Vec::new());
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_oauth::{Identity, TokenSet};

    fn signed(email: &str) -> SignedIn {
        SignedIn {
            provider: AuthKind::Google,
            identity: Identity {
                email: email.into(),
                name: None,
            },
            tokens: TokenSet {
                access_token: "erisim".into(),
                refresh_token: Some("yenileme".into()),
                expires_at: Instant::now() + Duration::from_secs(3600),
                id_token: None,
            },
        }
    }

    #[test]
    fn girisler_tek_kullanimlik_ve_sureli() {
        let t0 = Instant::now();
        let mut g = Grants::default();
        g.insert("a".into(), signed("a@gmail.com"), t0);
        g.insert("b".into(), signed("b@gmail.com"), t0);
        let (s, created) = g.take("a", t0 + Duration::from_secs(60)).unwrap();
        assert_eq!(s.identity.email, "a@gmail.com");
        assert!(g.take("a", t0).is_none(), "ikinci kez alınamaz");

        // Geri konan giriş eski süresini korur.
        g.put_back("a".into(), s, created);
        assert!(g
            .take("a", t0 + GRANT_TTL + Duration::from_secs(1))
            .is_none());
        assert!(
            g.take("b", t0 + GRANT_TTL).is_none(),
            "10 dakika sonra düşer"
        );
        assert!(g.map.is_empty());
    }

    #[tokio::test]
    async fn yeni_giris_eskisini_iptal_eder() {
        let st = OAuthState::default();
        let (first, rx1) = st.begin();
        let (second, mut rx2) = st.begin();
        // İlk girişin kancası bırakıldı: bekleyen gelecek hemen tamamlanır.
        assert!(rx1.await.is_err());
        // Biten eski giriş, sürenin kancasına dokunmaz.
        st.finish(first);
        assert!(matches!(
            rx2.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        st.finish(second);
        assert!(matches!(
            rx2.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }
}
