//! Tarayıcıda giriş: yetkilendirme kodu + PKCE + yerel dönüş.

use std::future::Future;
use std::time::Duration;

use mh_core::AuthKind;

use crate::identity::{identify, Identity};
use crate::loopback::Loopback;
use crate::pkce::{random_urlsafe, Pkce};
use crate::token::{authorize_url, exchange_code};
use crate::{OAuthError, Provider, TokenSet};

/// Tarayıcıdaki girişin en uzun süresi.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Tamamlanmış giriş.
#[derive(Debug, Clone)]
pub struct SignedIn {
    pub provider: AuthKind,
    pub identity: Identity,
    /// `refresh_token` her zaman doludur.
    pub tokens: TokenSet,
}

/// Girişi baştan sona yürütür:
///
/// 1. 127.0.0.1'de rastgele kapıda dinleyici açar,
/// 2. PKCE ve `state` üretip yetkilendirme adresini `open` ile tarayıcıda açar,
/// 3. dönüşü `limit` kadar ya da `cancel` tamamlanana dek bekler,
/// 4. kodu jetonlara çevirir ve adresi `id_token` / `userinfo` ile bulur.
///
/// `login_hint` adres ise sağlayıcının giriş ekranına önceden yazılır.
pub async fn sign_in<O, C>(
    provider: &Provider,
    login_hint: Option<&str>,
    limit: Duration,
    open: O,
    cancel: C,
) -> Result<SignedIn, OAuthError>
where
    O: FnOnce(&str) -> Result<(), String>,
    C: Future<Output = ()>,
{
    if provider.client_id.trim().is_empty() {
        return Err(OAuthError::NotConfigured);
    }
    let loopback = Loopback::bind().await?;
    let redirect_uri = loopback.redirect_uri();
    let pkce = Pkce::new()?;
    let state = random_urlsafe(24)?;
    let url = authorize_url(provider, &redirect_uri, &state, &pkce.challenge, login_hint)?;
    tracing::info!(provider = ?provider.kind, port = loopback.port(), "OAuth girişi tarayıcıda başlatılıyor");
    open(url.as_str()).map_err(OAuthError::Browser)?;

    let code = tokio::select! {
        r = tokio::time::timeout(limit, loopback.wait(&state)) => match r {
            Ok(r) => r?,
            Err(_) => return Err(OAuthError::TimedOut),
        },
        _ = cancel => return Err(OAuthError::Cancelled),
    };
    let tokens = exchange_code(provider, &code, &pkce.verifier, &redirect_uri).await?;
    if tokens.refresh_token.is_none() {
        return Err(OAuthError::Protocol(
            "sağlayıcı yenileme jetonu vermedi".into(),
        ));
    }
    let identity = identify(provider, &tokens).await?;
    tracing::info!(provider = ?provider.kind, "OAuth girişi tamamlandı");
    Ok(SignedIn {
        provider: provider.kind,
        identity,
        tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeProvider;
    use crate::token::refresh;
    use tokio::sync::oneshot;

    /// Tarayıcı benzetimi: adresi açar, yönlendirmeyi izler ve yerel
    /// dinleyicinin gösterdiği sayfayı döner.
    fn browser() -> (
        impl FnOnce(&str) -> Result<(), String>,
        oneshot::Receiver<String>,
    ) {
        let (tx, rx) = oneshot::channel();
        let open = move |url: &str| {
            let url = url.to_owned();
            tokio::spawn(async move {
                let client = reqwest::Client::builder().no_proxy().build().unwrap();
                let page = match client.get(url).send().await {
                    Ok(r) => r.text().await.unwrap_or_default(),
                    Err(e) => format!("istek hatası: {e}"),
                };
                let _ = tx.send(page);
            });
            Ok(())
        };
        (open, rx)
    }

    fn never() -> std::future::Pending<()> {
        std::future::pending()
    }

    #[tokio::test]
    async fn google_tam_giris_ve_yenileme() {
        let fake = FakeProvider::start("furkan@gmail.com").await;
        fake.lock().client_secret = Some("masaustu-gizli".into());
        let p = fake.provider(AuthKind::Google);
        let (open, page) = browser();
        let done = sign_in(&p, Some("furkan@gmail.com"), SIGN_IN_TIMEOUT, open, never())
            .await
            .unwrap();
        assert_eq!(done.provider, AuthKind::Google);
        assert_eq!(done.identity.email, "furkan@gmail.com");
        assert_eq!(done.identity.name.as_deref(), Some("Furkan Yıldırım"));
        assert!(done.tokens.expires_at > std::time::Instant::now());
        let page = page.await.unwrap();
        assert!(
            page.contains("Giriş tamamlandı, Mailhere'ye dönebilirsin."),
            "{page}"
        );

        {
            let st = fake.lock();
            let auth = st.last_authorize.as_ref().unwrap();
            assert_eq!(auth["login_hint"], "furkan@gmail.com");
            assert_eq!(auth["access_type"], "offline");
            assert_eq!(auth["prompt"], "consent");
            let tr = &st.token_requests[0];
            assert_eq!(tr["grant_type"], "authorization_code");
            assert_eq!(tr["client_secret"], "masaustu-gizli");
            assert!(tr["code_verifier"].len() >= 43);
            assert!(!tr.contains_key("scope"));
        }

        // Yenileme: yeni erişim jetonu, yenileme jetonu değişmez.
        let rt = done.tokens.refresh_token.clone().unwrap();
        let t = refresh(&p, &rt).await.unwrap();
        assert_ne!(t.access_token, done.tokens.access_token);
        assert!(t.refresh_token.is_none());
        assert!(t.expires_at > std::time::Instant::now());

        // Kullanıcı erişimi kaldırdı.
        fake.revoke(&rt);
        assert!(matches!(
            refresh(&p, &rt).await,
            Err(OAuthError::InvalidGrant)
        ));
    }

    #[tokio::test]
    async fn microsoft_kapsam_ve_donen_yenileme_jetonu() {
        let fake = FakeProvider::start("furkan@outlook.com").await;
        fake.lock().rotate_refresh = true;
        let p = fake.provider(AuthKind::Microsoft);
        assert!(p.userinfo_url.is_none());
        let (open, _page) = browser();
        let done = sign_in(&p, None, SIGN_IN_TIMEOUT, open, never())
            .await
            .unwrap();
        assert_eq!(done.identity.email, "furkan@outlook.com");
        let rt = done.tokens.refresh_token.clone().unwrap();
        let t = refresh(&p, &rt).await.unwrap();
        let rotated = t
            .refresh_token
            .expect("Microsoft yeni yenileme jetonu verir");
        assert_ne!(rotated, rt);
        let st = fake.lock();
        assert!(st
            .last_authorize
            .as_ref()
            .unwrap()
            .get("login_hint")
            .is_none());
        assert!(st
            .token_requests
            .iter()
            .all(|f| f["scope"].contains("IMAP.AccessAsUser.All")));
        assert!(st
            .token_requests
            .iter()
            .all(|f| !f.contains_key("client_secret")));
    }

    #[tokio::test]
    async fn id_token_yoksa_userinfo() {
        let fake = FakeProvider::start("elif@gmail.com").await;
        fake.lock().id_token = false;
        let (open, _page) = browser();
        let google = fake.provider(AuthKind::Google);
        let done = sign_in(&google, None, SIGN_IN_TIMEOUT, open, never())
            .await
            .unwrap();
        assert_eq!(done.identity.email, "elif@gmail.com");

        // Microsoft'ta userinfo yok; id_token adres içermezse giriş başarısız.
        let (open, _page) = browser();
        let ms = fake.provider(AuthKind::Microsoft);
        let err = sign_in(&ms, None, SIGN_IN_TIMEOUT, open, never())
            .await
            .unwrap_err();
        assert!(matches!(err, OAuthError::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn kullanici_reddeder() {
        let fake = FakeProvider::start("a@b.co").await;
        fake.lock().deny = true;
        let (open, page) = browser();
        let p = fake.provider(AuthKind::Google);
        let err = sign_in(&p, None, SIGN_IN_TIMEOUT, open, never())
            .await
            .unwrap_err();
        assert!(matches!(err, OAuthError::Denied), "{err:?}");
        assert!(page.await.unwrap().contains("Giriş onaylanmadı"));
        assert!(fake.lock().token_requests.is_empty());
    }

    #[tokio::test]
    async fn yenileme_jetonu_verilmezse_hata() {
        let fake = FakeProvider::start("a@b.co").await;
        fake.lock().issue_refresh = false;
        let (open, _page) = browser();
        let p = fake.provider(AuthKind::Google);
        let err = sign_in(&p, None, SIGN_IN_TIMEOUT, open, never())
            .await
            .unwrap_err();
        assert!(matches!(err, OAuthError::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn yanlis_istemci_kimligi() {
        let fake = FakeProvider::start("a@b.co").await;
        let mut p = fake.provider(AuthKind::Google);
        p.client_id = "baska".into();
        // Yetkilendirme ekranı bilinmeyen istemciyi reddeder; dönüş gelmez.
        let (open, _page) = browser();
        let err = sign_in(&p, None, Duration::from_millis(500), open, never())
            .await
            .unwrap_err();
        assert!(matches!(err, OAuthError::TimedOut), "{err:?}");
        // Jeton uç noktası da reddeder.
        match refresh(&p, "x").await.unwrap_err() {
            OAuthError::Provider { code, .. } => assert_eq!(code, "invalid_client"),
            e => panic!("{e:?}"),
        }
        p.client_id = " ".into();
        let (open, _page) = browser();
        assert!(matches!(
            sign_in(&p, None, SIGN_IN_TIMEOUT, open, never()).await,
            Err(OAuthError::NotConfigured)
        ));
    }

    #[tokio::test]
    async fn pkce_dogrulayicisi_denetlenir() {
        let fake = FakeProvider::start("a@b.co").await;
        let p = fake.provider(AuthKind::Google);
        let pkce = Pkce::new().unwrap();
        let redirect = "http://127.0.0.1:9/";
        let url = authorize_url(&p, redirect, "s", &pkce.challenge, None).unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let code_from = |resp: reqwest::Response| {
            let loc = resp.headers()["location"].to_str().unwrap().to_owned();
            reqwest::Url::parse(&loc)
                .unwrap()
                .query_pairs()
                .find(|(k, _)| k == "code")
                .unwrap()
                .1
                .into_owned()
        };
        let code = code_from(client.get(url.clone()).send().await.unwrap());

        // Yanlış doğrulayıcı reddedilir ve kod yanar (tek kullanımlık).
        let wrong = Pkce::new().unwrap();
        assert!(matches!(
            exchange_code(&p, &code, &wrong.verifier, redirect).await,
            Err(OAuthError::InvalidGrant)
        ));
        assert!(matches!(
            exchange_code(&p, &code, &pkce.verifier, redirect).await,
            Err(OAuthError::InvalidGrant)
        ));

        // Yeni kod, doğru doğrulayıcı ama farklı dönüş adresi: reddedilir.
        let code = code_from(client.get(url.clone()).send().await.unwrap());
        assert!(matches!(
            exchange_code(&p, &code, &pkce.verifier, "http://127.0.0.1:10/").await,
            Err(OAuthError::InvalidGrant)
        ));

        // Doğru olanı geçer.
        let code = code_from(client.get(url).send().await.unwrap());
        let t = exchange_code(&p, &code, &pkce.verifier, redirect)
            .await
            .unwrap();
        assert!(t.refresh_token.is_some());
    }

    #[tokio::test]
    async fn zaman_asimi_iptal_ve_tarayici_hatasi() {
        let fake = FakeProvider::start("a@b.co").await;
        let p = fake.provider(AuthKind::Google);
        // Tarayıcı hiç dönmez.
        let err = sign_in(
            &p,
            None,
            Duration::from_millis(200),
            |_: &str| Ok(()),
            never(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, OAuthError::TimedOut), "{err:?}");

        let (tx, rx) = oneshot::channel::<()>();
        let cancel = async move {
            let _ = rx.await;
        };
        let run = tokio::spawn({
            let p = p.clone();
            async move { sign_in(&p, None, SIGN_IN_TIMEOUT, |_: &str| Ok(()), cancel).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(tx);
        assert!(matches!(run.await.unwrap(), Err(OAuthError::Cancelled)));

        let err = sign_in(
            &p,
            None,
            SIGN_IN_TIMEOUT,
            |_: &str| Err("yok".into()),
            never(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, OAuthError::Browser(_)), "{err:?}");
    }

    #[tokio::test]
    async fn guvensiz_uc_nokta_reddedilir() {
        let mut p = Provider::google("id", None);
        p.token_url = "http://oauth2.googleapis.com/token".into();
        assert!(matches!(
            refresh(&p, "x").await,
            Err(OAuthError::Protocol(_))
        ));
    }
}
