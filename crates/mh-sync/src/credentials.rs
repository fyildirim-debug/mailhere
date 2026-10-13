//! Kimlik bilgileri: parola ya da OAuth 2 erişim jetonu (XOAUTH2).
//!
//! Hesabın `auth` alanına göre çözülür. Parola hesaplarında parola anahtar
//! zincirindedir (hesap kimliği altında). Google ve Microsoft hesaplarında
//! yenileme jetonu anahtar zincirinde ayrı bir kayıttadır (`oauth:<hesap>`);
//! erişim jetonu yalnız bellekte önbelleğe alınır ve bitişine 60 saniyeden
//! az kalınca yenilenir. IMAP girişi (`conn`) ve SMTP gönderimi (`mh-send`)
//! aynı `Credentials` türünü kullanır.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_imap::{Authenticator, Client};
use mh_core::{Account, AccountId, AppError, AppResult, AuthKind, ErrorCode, ServerConfig};
use mh_oauth::{OAuthError, Provider, TokenSet};

use crate::conn::{connect_with, ImapSession, ImapStream};
use crate::error::{auth_error, SyncError, SyncResult};
use crate::secrets::{password_for, SecretStore};

/// Erişim jetonunun bitişine bundan az kaldıysa yenilenir.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// Bir sunucuya giriş için kimlik bilgisi.
#[derive(Clone, PartialEq, Eq)]
pub enum Credentials {
    Password(String),
    /// OAuth 2 erişim jetonu; SASL XOAUTH2 ile gönderilir.
    OAuth2(String),
}

impl Credentials {
    pub fn is_oauth(&self) -> bool {
        matches!(self, Self::OAuth2(_))
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Password(_) => "Password(***)",
            Self::OAuth2(_) => "OAuth2(***)",
        })
    }
}

/// Yenileme jetonunun anahtar zincirindeki adı; parola kaydından ayrıdır.
pub fn oauth_secret_key(account_id: &str) -> String {
    format!("oauth:{account_id}")
}

/// Sunucu jetonu reddetti (süresi dolmuş, iptal edilmiş ya da IMAP/SMTP
/// erişimi kapalı).
pub fn token_rejected() -> AppError {
    AppError::new(
        ErrorCode::Auth,
        "Sunucu oturum jetonunu kabul etmedi. Hesabı yeniden bağlamak için yeniden giriş yap.",
    )
}

fn missing_grant(kind: AuthKind) -> AppError {
    AppError::new(
        ErrorCode::Auth,
        format!(
            "Bu hesabın {} oturumu anahtar zincirinde bulunamadı. Hesabı yeniden bağlamak için yeniden giriş yap.",
            mh_oauth::provider_name(kind)
        ),
    )
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|_| {
        AppError::new(
            ErrorCode::Internal,
            "Anahtar zinciri okunurken iç hata oluştu.",
        )
    })?
}

/// Sağlayıcı ayarını bulur (gerçek uygulamada ortamdaki istemci kimliği;
/// testlerde sahte sunucu).
pub type ProviderResolver = Arc<dyn Fn(AuthKind) -> Option<Provider> + Send + Sync>;

enum Slot {
    Empty,
    Fresh {
        token: String,
        expires_at: Instant,
    },
    /// Sağlayıcı `invalid_grant` dedi; yeni giriş kaydedilene dek ağa
    /// çıkılmaz (yeniden bağlanma döngüsü sağlayıcıyı yormasın).
    Revoked,
}

/// Hesap başına erişim jetonu önbelleği ve yenileyici.
pub struct TokenProvider {
    resolve: ProviderResolver,
    slots: Mutex<HashMap<AccountId, Arc<tokio::sync::Mutex<Slot>>>>,
}

impl Default for TokenProvider {
    fn default() -> Self {
        Self::new(Arc::new(Provider::configured))
    }
}

impl TokenProvider {
    pub fn new(resolve: ProviderResolver) -> Self {
        Self {
            resolve,
            slots: Mutex::default(),
        }
    }

    fn slot(&self, account_id: &str) -> Arc<tokio::sync::Mutex<Slot>> {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(account_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(Slot::Empty)))
            .clone()
    }

    /// Geçerli bir erişim jetonu: önbellekte yeterince taze varsa o,
    /// yoksa yenileme jetonuyla yenisi. Aynı hesap için eşzamanlı istekler
    /// tek yenilemeyi bekler.
    pub async fn access_token(
        &self,
        secrets: &Arc<dyn SecretStore>,
        account: &Account,
    ) -> AppResult<String> {
        let kind = account.auth;
        if kind == AuthKind::Password {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Bu hesap parolayla giriş yapıyor.",
            ));
        }
        let slot = self.slot(&account.id);
        let mut slot = slot.lock().await;
        match &*slot {
            Slot::Fresh { token, expires_at } if *expires_at > Instant::now() + REFRESH_MARGIN => {
                return Ok(token.clone())
            }
            Slot::Revoked => return Err(mh_oauth::revoked(kind)),
            _ => {}
        }
        let provider =
            (self.resolve)(kind).ok_or_else(|| OAuthError::NotConfigured.to_app(kind))?;
        let key = oauth_secret_key(&account.id);
        let refresh_token = {
            let (s, k) = (secrets.clone(), key.clone());
            blocking(move || s.get(&k)).await?
        }
        .ok_or_else(|| missing_grant(kind))?;

        match mh_oauth::refresh(&provider, &refresh_token).await {
            Ok(t) => {
                // Microsoft her yenilemede yeni yenileme jetonu verir; sakla.
                if let Some(new) = t.refresh_token.filter(|n| *n != refresh_token) {
                    let s = secrets.clone();
                    if let Err(e) = blocking(move || s.set(&key, &new)).await {
                        tracing::warn!(account = %account.id, error = %e.message, "yeni yenileme jetonu saklanamadı");
                    }
                }
                *slot = Slot::Fresh {
                    token: t.access_token.clone(),
                    expires_at: t.expires_at,
                };
                Ok(t.access_token)
            }
            Err(OAuthError::InvalidGrant) => {
                tracing::warn!(account = %account.id, "OAuth izni geçersiz; yeniden giriş gerekiyor");
                *slot = Slot::Revoked;
                Err(mh_oauth::revoked(kind))
            }
            Err(e) => {
                *slot = Slot::Empty;
                Err(e.to_app(kind))
            }
        }
    }

    /// Yeni girişi kaydeder: yenileme jetonu anahtar zincirine, erişim
    /// jetonu önbelleğe. Önceki "iptal edildi" durumu temizlenir.
    pub async fn save_grant(
        &self,
        secrets: &Arc<dyn SecretStore>,
        account_id: &str,
        tokens: &TokenSet,
    ) -> AppResult<()> {
        let refresh = tokens.refresh_token.clone().ok_or_else(|| {
            AppError::new(
                ErrorCode::Auth,
                "Sağlayıcı kalıcı erişim izni vermedi. Yeniden giriş yap.",
            )
        })?;
        let (s, key) = (secrets.clone(), oauth_secret_key(account_id));
        blocking(move || s.set(&key, &refresh)).await?;
        *self.slot(account_id).lock().await = Slot::Fresh {
            token: tokens.access_token.clone(),
            expires_at: tokens.expires_at,
        };
        Ok(())
    }

    /// Sunucu bu jetonu reddetti: önbellekteki aynı jetonu düşür ki sonraki
    /// bağlantı yenilesin.
    pub async fn invalidate(&self, account_id: &str, rejected: &str) {
        let slot = self.slot(account_id);
        let mut slot = slot.lock().await;
        if matches!(&*slot, Slot::Fresh { token, .. } if token == rejected) {
            *slot = Slot::Empty;
        }
    }

    /// Hesap kaldırıldı: önbelleği sil (anahtar zinciri kaydını çağıran siler).
    pub fn forget(&self, account_id: &str) {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account_id);
    }
}

/// Hesabın kimlik bilgisini çözer.
pub async fn credentials_for(
    secrets: &Arc<dyn SecretStore>,
    tokens: &TokenProvider,
    account: &Account,
) -> AppResult<Credentials> {
    match account.auth {
        AuthKind::Password => Ok(Credentials::Password(
            password_for(secrets, &account.id).await?,
        )),
        AuthKind::Google | AuthKind::Microsoft => Ok(Credentials::OAuth2(
            tokens.access_token(secrets, account).await?,
        )),
    }
}

/// Hesabın kimlik bilgisiyle IMAP'e bağlanır; sunucu jetonu reddederse
/// önbellekten düşürür.
pub(crate) async fn connect_account(
    cfg: &ServerConfig,
    secrets: &Arc<dyn SecretStore>,
    tokens: &TokenProvider,
    account: &Account,
) -> SyncResult<ImapSession> {
    let creds = credentials_for(secrets, tokens, account).await?;
    let res = connect_with(cfg, &creds).await;
    if let (Err(SyncError::App(e)), Credentials::OAuth2(t)) = (&res, &creds) {
        if e.code == ErrorCode::Auth {
            tokens.invalidate(&account.id, t).await;
        }
    }
    res
}

/// IMAP `AUTHENTICATE XOAUTH2`. İlk sorguya XOAUTH2 ilk yanıtı verilir;
/// sunucu reddederse ikinci bir sorguyla hata ayrıntısını (JSON) gönderir,
/// buna boş yanıt verilir ve sunucu NO ile bitirir.
struct Xoauth2 {
    payload: Option<String>,
}

impl Authenticator for Xoauth2 {
    type Response = Vec<u8>;

    fn process(&mut self, challenge: &[u8]) -> Vec<u8> {
        match self.payload.take() {
            Some(p) => p.into_bytes(),
            None => {
                // Ayrıntı jeton içermez (durum ve kapsam).
                tracing::info!(detail = %String::from_utf8_lossy(challenge), "XOAUTH2 reddedildi");
                Vec::new()
            }
        }
    }
}

/// Karşılama ve (gerekirse) STARTTLS sonrası oturum açma.
pub(crate) async fn imap_login(
    client: Client<ImapStream>,
    host: &str,
    user: &str,
    creds: &Credentials,
) -> SyncResult<ImapSession> {
    let res = match creds {
        Credentials::Password(p) => client.login(user, p).await,
        Credentials::OAuth2(t) => {
            let auth = Xoauth2 {
                payload: Some(mh_oauth::xoauth2_payload(user, t)),
            };
            client.authenticate("XOAUTH2", auth).await
        }
    };
    match res {
        Ok(session) => Ok(session),
        Err((async_imap::error::Error::No(m) | async_imap::error::Error::Bad(m), _)) => {
            tracing::info!(host, reply = %m.trim(), "giriş reddedildi");
            Err(if creds.is_oauth() {
                token_rejected()
            } else {
                auth_error()
            }
            .into())
        }
        Err((e, _)) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemorySecrets;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use mh_core::{AccountKind, Security};
    use mh_oauth::testing::{FakeProvider, FakeState};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    fn account(auth: AuthKind) -> Account {
        Account {
            id: "hesap-1".into(),
            name: "Gmail".into(),
            email: "furkan@gmail.com".into(),
            display_name: "Furkan".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: None,
            smtp: None,
            jmap_url: None,
            signature: None,
            created_at: 0,
            auth,
            notify: true,
        }
    }

    fn resolver(fake: &FakeProvider) -> ProviderResolver {
        let google = fake.provider(AuthKind::Google);
        let microsoft = fake.provider(AuthKind::Microsoft);
        Arc::new(move |k| match k {
            AuthKind::Google => Some(google.clone()),
            AuthKind::Microsoft => Some(microsoft.clone()),
            AuthKind::Password => None,
        })
    }

    /// Sahte sağlayıcı + yenileme jetonu anahtar zincirinde.
    async fn setup() -> (FakeProvider, Arc<dyn SecretStore>, TokenProvider) {
        let fake = FakeProvider::start("furkan@gmail.com").await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
        secrets
            .set(&oauth_secret_key("hesap-1"), &fake.issue_refresh_token())
            .unwrap();
        let tokens = TokenProvider::new(resolver(&fake));
        (fake, secrets, tokens)
    }

    #[tokio::test]
    async fn parola_hesabi() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
        let tokens = TokenProvider::new(Arc::new(|_| None));
        let a = account(AuthKind::Password);
        assert_eq!(
            credentials_for(&secrets, &tokens, &a)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Keyring
        );
        secrets.set("hesap-1", "parola").unwrap();
        // OAuth kaydı parola kaydını ezmez.
        secrets.set(&oauth_secret_key("hesap-1"), "jeton").unwrap();
        assert_eq!(
            credentials_for(&secrets, &tokens, &a).await.unwrap(),
            Credentials::Password("parola".into())
        );
        assert!(tokens.access_token(&secrets, &a).await.is_err());
    }

    #[test]
    fn hata_ayiklama_ciktisi_gizli() {
        let s = format!(
            "{:?} {:?}",
            Credentials::Password("parola".into()),
            Credentials::OAuth2("ya29.gizli".into())
        );
        assert!(!s.contains("parola") && !s.contains("gizli"), "{s}");
        assert_eq!(oauth_secret_key("x"), "oauth:x");
    }

    #[tokio::test]
    async fn onbellek_ve_yenileme_esigi() {
        let (fake, secrets, tokens) = setup().await;
        let a = account(AuthKind::Google);
        let t1 = tokens.access_token(&secrets, &a).await.unwrap();
        let t2 = tokens.access_token(&secrets, &a).await.unwrap();
        assert_eq!(t1, t2);
        assert_eq!(fake.refresh_count(), 1);
        assert_eq!(
            credentials_for(&secrets, &tokens, &a).await.unwrap(),
            Credentials::OAuth2(t1.clone())
        );

        // 60 saniyeden kısa ömürlü jeton her seferinde yenilenir.
        fake.lock().expires_in = 30;
        tokens.forget(&a.id);
        let t3 = tokens.access_token(&secrets, &a).await.unwrap();
        let t4 = tokens.access_token(&secrets, &a).await.unwrap();
        assert_ne!(t3, t4);
        assert_eq!(fake.refresh_count(), 3);
    }

    #[tokio::test]
    async fn eszamanli_istekler_tek_yenileme() {
        let (fake, secrets, tokens) = setup().await;
        let tokens = Arc::new(tokens);
        let a = account(AuthKind::Google);
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let (t, s, a) = (tokens.clone(), secrets.clone(), a.clone());
            tasks.push(tokio::spawn(async move {
                t.access_token(&s, &a).await.unwrap()
            }));
        }
        let mut got = Vec::new();
        for t in tasks {
            got.push(t.await.unwrap());
        }
        got.dedup();
        assert_eq!(got.len(), 1);
        assert_eq!(fake.refresh_count(), 1);
    }

    #[tokio::test]
    async fn donen_yenileme_jetonu_saklanir() {
        let (fake, secrets, tokens) = setup().await;
        fake.lock().rotate_refresh = true;
        let a = account(AuthKind::Microsoft);
        let key = oauth_secret_key(&a.id);
        let before = secrets.get(&key).unwrap().unwrap();
        tokens.access_token(&secrets, &a).await.unwrap();
        let after = secrets.get(&key).unwrap().unwrap();
        assert_ne!(before, after);
        // Eski jeton artık geçmez; saklanan yenisiyle yenileme sürer.
        tokens.forget(&a.id);
        tokens.access_token(&secrets, &a).await.unwrap();
        assert_eq!(fake.refresh_count(), 2);
    }

    #[tokio::test]
    async fn invalid_grant_yeniden_giris_ister() {
        let (fake, secrets, tokens) = setup().await;
        let a = account(AuthKind::Google);
        let rt = secrets.get(&oauth_secret_key(&a.id)).unwrap().unwrap();
        fake.revoke(&rt);
        let e = tokens.access_token(&secrets, &a).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Auth);
        assert!(e.message.contains("Google"), "{}", e.message);
        assert!(e.message.contains("yeniden giriş"), "{}", e.message);
        assert!(!e.message.contains(&rt));
        // İkinci denemede ağa çıkılmaz.
        assert!(tokens.access_token(&secrets, &a).await.is_err());
        assert_eq!(fake.refresh_count(), 1);

        // Yeni giriş kaydedilince düzelir (önbellek tohumlanır, ağa çıkılmaz).
        let fresh = TokenSet {
            access_token: fake.issue_access_token(),
            refresh_token: Some(fake.issue_refresh_token()),
            expires_at: Instant::now() + Duration::from_secs(3600),
            id_token: None,
        };
        tokens.save_grant(&secrets, &a.id, &fresh).await.unwrap();
        assert_eq!(
            tokens.access_token(&secrets, &a).await.unwrap(),
            fresh.access_token
        );
        assert_eq!(fake.refresh_count(), 1);
        assert_eq!(
            secrets.get(&oauth_secret_key(&a.id)).unwrap(),
            fresh.refresh_token
        );
        let mut no_refresh = fresh.clone();
        no_refresh.refresh_token = None;
        assert!(tokens.save_grant(&secrets, "b", &no_refresh).await.is_err());
    }

    #[tokio::test]
    async fn yapilandirma_ve_kayit_eksik() {
        let (fake, secrets, _) = setup().await;
        let none = TokenProvider::new(Arc::new(|_| None));
        let e = none
            .access_token(&secrets, &account(AuthKind::Microsoft))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("Microsoft"), "{}", e.message);

        let tokens = TokenProvider::new(resolver(&fake));
        let mut b = account(AuthKind::Google);
        b.id = "kayitsiz".into();
        let e = tokens.access_token(&secrets, &b).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Auth);
        assert!(e.message.contains("anahtar zincirinde"), "{}", e.message);
    }

    /// AUTHENTICATE XOAUTH2 bilen sahte IMAP sunucusu: jetonu sahte
    /// sağlayıcının verdiği erişim jetonlarıyla karşılaştırır.
    async fn fake_imap(listener: TcpListener, state: Arc<Mutex<FakeState>>, user: &'static str) {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                return;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut lines = BufReader::new(r).lines();
                w.write_all(b"* OK [CAPABILITY IMAP4rev1 AUTH=XOAUTH2] hazir\r\n")
                    .await
                    .unwrap();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut parts = line.splitn(2, ' ');
                    let tag = parts.next().unwrap_or("*").to_owned();
                    let cmd = parts.next().unwrap_or("").to_ascii_uppercase();
                    if cmd == "AUTHENTICATE XOAUTH2" {
                        w.write_all(b"+ \r\n").await.unwrap();
                        let resp = lines.next_line().await.unwrap().unwrap_or_default();
                        let decoded =
                            String::from_utf8(STANDARD.decode(resp.trim()).unwrap()).unwrap();
                        let ok = decoded
                            .strip_prefix(&format!("user={user}\x01auth=Bearer "))
                            .and_then(|r| r.strip_suffix("\x01\x01"))
                            .is_some_and(|t| state.lock().unwrap().access_tokens.contains(t));
                        if ok {
                            w.write_all(format!("{tag} OK giris tamam\r\n").as_bytes())
                                .await
                                .unwrap();
                        } else {
                            // Google gibi: önce JSON ayrıntı, boş yanıt, sonra NO.
                            let detail = STANDARD.encode(r#"{"status":"401","schemes":"Bearer"}"#);
                            w.write_all(format!("+ {detail}\r\n").as_bytes())
                                .await
                                .unwrap();
                            let empty = lines.next_line().await.unwrap().unwrap_or_default();
                            assert_eq!(empty, "", "hata ayrıntısına boş yanıt beklenir");
                            w.write_all(
                                format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n")
                                    .as_bytes(),
                            )
                            .await
                            .unwrap();
                        }
                    } else if cmd == "LOGOUT" {
                        let _ = w
                            .write_all(format!("* BYE\r\n{tag} OK\r\n").as_bytes())
                            .await;
                        return;
                    } else {
                        w.write_all(format!("{tag} OK\r\n").as_bytes())
                            .await
                            .unwrap();
                    }
                }
            });
        }
    }

    #[tokio::test]
    async fn imap_xoauth2_girisi_ve_reddi() {
        let (fake, secrets, tokens) = setup().await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(fake_imap(listener, fake.state.clone(), "furkan@gmail.com"));
        let cfg = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::None,
            username: "furkan@gmail.com".into(),
        };
        let a = account(AuthKind::Google);

        // Geçerli jetonla giriş.
        let mut s = connect_account(&cfg, &secrets, &tokens, &a).await.unwrap();
        let _ = s.logout().await;
        assert_eq!(fake.refresh_count(), 1);

        // Yanlış jeton: Türkçe yetki hatası, jeton metinde yok.
        let bad = Credentials::OAuth2("sahte-jeton".into());
        let e = crate::test_login_with(&cfg, &bad).await.unwrap_err();
        assert_eq!(e, token_rejected());
        assert!(!e.message.contains("sahte-jeton"));

        // Sunucu önbellekteki jetonu reddederse düşürülür; sonraki bağlantı yeniler.
        fake.expire_access_tokens();
        let e = connect_account(&cfg, &secrets, &tokens, &a)
            .await
            .unwrap_err()
            .to_app();
        assert_eq!(e.code, ErrorCode::Auth);
        assert_eq!(fake.refresh_count(), 1);
        let mut s = connect_account(&cfg, &secrets, &tokens, &a).await.unwrap();
        let _ = s.logout().await;
        assert_eq!(fake.refresh_count(), 2);

        // Erişim kaldırıldı: yeniden giriş isteyen hata.
        let rt = secrets.get(&oauth_secret_key(&a.id)).unwrap().unwrap();
        fake.revoke(&rt);
        fake.expire_access_tokens();
        assert!(connect_account(&cfg, &secrets, &tokens, &a).await.is_err());
        let e = connect_account(&cfg, &secrets, &tokens, &a)
            .await
            .unwrap_err()
            .to_app();
        assert_eq!(e, mh_oauth::revoked(AuthKind::Google));
    }
}
