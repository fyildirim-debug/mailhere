//! Yetkilendirme adresi, kod → jeton değişimi ve yenileme.

use std::fmt;
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::Deserialize;

use crate::{OAuthError, Provider};

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Sağlayıcı `expires_in` vermezse varsayılan ömür.
const DEFAULT_LIFETIME: Duration = Duration::from_secs(3600);

/// Jeton uç noktasının yanıtı. Hiçbir alan günlüğe yazılmaz.
#[derive(Clone)]
pub struct TokenSet {
    pub access_token: String,
    /// Yenileme jetonu; yenilemede sağlayıcı yenisini verebilir (Microsoft
    /// her yenilemede döndürür).
    pub refresh_token: Option<String>,
    /// Erişim jetonunun bitiş anı (yerel saat).
    pub expires_at: Instant,
    /// OpenID Connect kimlik jetonu (yalnız ilk girişte).
    pub id_token: Option<String>,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"***")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "***"))
            .field("expires_at", &self.expires_at)
            .field("id_token", &self.id_token.as_ref().map(|_| "***"))
            .finish()
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// `expires_in` sayı ya da metin gelebilir.
fn lifetime(v: Option<&serde_json::Value>) -> Duration {
    let secs = match v {
        Some(serde_json::Value::Number(n)) => n.as_u64(),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    };
    secs.filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_LIFETIME)
}

/// Yerel test sunucusu mu (http'ye yalnız orada izin verilir)?
pub(crate) fn is_loopback(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
    )
}

/// Uç nokta adresini ayrıştırır; https zorunludur (yalnız yerel test
/// sunucusunda http'ye izin verilir).
pub(crate) fn endpoint(url: &str) -> Result<Url, OAuthError> {
    let parsed =
        Url::parse(url).map_err(|_| OAuthError::Protocol(format!("geçersiz adres: {url}")))?;
    match parsed.scheme() {
        "https" => Ok(parsed),
        "http" if is_loopback(&parsed) => Ok(parsed),
        _ => Err(OAuthError::Protocol(format!(
            "güvensiz uç nokta reddedildi: {url}"
        ))),
    }
}

pub(crate) fn http_client(url: &Url) -> Result<reqwest::Client, OAuthError> {
    let mut b = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")));
    if is_loopback(url) {
        b = b.no_proxy();
    } else {
        b = b.https_only(true);
    }
    b.build().map_err(|e| OAuthError::Network(e.to_string()))
}

pub(crate) fn network(e: reqwest::Error) -> OAuthError {
    // reqwest hatası adres içerebilir; jeton gövdede olduğundan sızmaz.
    tracing::warn!(error = %e, "OAuth isteği başarısız");
    OAuthError::Network(e.to_string())
}

/// Tarayıcıda açılacak yetkilendirme adresi.
pub fn authorize_url(
    p: &Provider,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
    login_hint: Option<&str>,
) -> Result<Url, OAuthError> {
    let mut url = endpoint(&p.authorize_url)?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &p.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &p.scopes)
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(hint) = login_hint.map(str::trim).filter(|h| h.contains('@')) {
            q.append_pair("login_hint", hint);
        }
        for (k, v) in &p.extra_params {
            q.append_pair(k, v);
        }
    }
    Ok(url)
}

async fn post_token(p: &Provider, mut form: Vec<(&str, &str)>) -> Result<TokenSet, OAuthError> {
    let url = endpoint(&p.token_url)?;
    form.push(("client_id", &p.client_id));
    if let Some(secret) = p.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    if p.scope_in_token_request {
        form.push(("scope", &p.scopes));
    }
    let started = Instant::now();
    let resp = http_client(&url)?
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&form)
        .send()
        .await
        .map_err(network)?;
    let status = resp.status();
    let body = resp.bytes().await.map_err(network)?;
    if status.is_success() {
        let t: TokenResponse = serde_json::from_slice(&body)
            .map_err(|_| OAuthError::Protocol("jeton yanıtı okunamadı".into()))?;
        if t.access_token.is_empty() {
            return Err(OAuthError::Protocol("erişim jetonu boş".into()));
        }
        if let Some(tt) = t.token_type.as_deref() {
            if !tt.eq_ignore_ascii_case("bearer") {
                return Err(OAuthError::Protocol(format!(
                    "beklenmeyen jeton türü: {tt}"
                )));
            }
        }
        return Ok(TokenSet {
            access_token: t.access_token,
            refresh_token: t.refresh_token.filter(|r| !r.is_empty()),
            // İstek süresi kadar erken bitmiş say (saat kayması payı).
            expires_at: started + lifetime(t.expires_in.as_ref()),
            id_token: t.id_token.filter(|i| !i.is_empty()),
        });
    }
    match serde_json::from_slice::<ErrorResponse>(&body) {
        Ok(e) if e.error == "invalid_grant" => {
            tracing::info!(provider = ?p.kind, "OAuth izni geçersiz (invalid_grant)");
            Err(OAuthError::InvalidGrant)
        }
        Ok(e) => {
            tracing::warn!(provider = ?p.kind, status = %status, error = %e.error, "OAuth jeton isteği reddedildi");
            Err(OAuthError::Provider {
                code: e.error,
                description: e.error_description,
            })
        }
        Err(_) => Err(OAuthError::Protocol(format!(
            "jeton uç noktası HTTP {} döndü",
            status.as_u16()
        ))),
    }
}

/// Yetki kodunu jetonlara çevirir (PKCE doğrulayıcısıyla).
pub async fn exchange_code(
    p: &Provider,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenSet, OAuthError> {
    post_token(
        p,
        vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", code_verifier),
            ("redirect_uri", redirect_uri),
        ],
    )
    .await
}

/// Yenileme jetonuyla yeni erişim jetonu alır. Geçersiz yenileme jetonu
/// `OAuthError::InvalidGrant` döner.
pub async fn refresh(p: &Provider, refresh_token: &str) -> Result<TokenSet, OAuthError> {
    post_token(
        p,
        vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omur_sayi_ya_da_metin() {
        assert_eq!(
            lifetime(Some(&serde_json::json!(3599))),
            Duration::from_secs(3599)
        );
        assert_eq!(
            lifetime(Some(&serde_json::json!("120"))),
            Duration::from_secs(120)
        );
        assert_eq!(lifetime(None), DEFAULT_LIFETIME);
        assert_eq!(lifetime(Some(&serde_json::json!(0))), DEFAULT_LIFETIME);
    }

    #[test]
    fn https_zorunlu_yerelde_http_serbest() {
        assert!(endpoint("https://oauth2.googleapis.com/token").is_ok());
        assert!(endpoint("http://127.0.0.1:8080/token").is_ok());
        assert!(endpoint("http://localhost:8080/token").is_ok());
        assert!(matches!(
            endpoint("http://oauth2.googleapis.com/token"),
            Err(OAuthError::Protocol(_))
        ));
        assert!(endpoint("ftp://x/").is_err());
        assert!(endpoint("bozuk").is_err());
    }

    #[test]
    fn yetkilendirme_adresi() {
        let p = Provider::google("istemci.apps.googleusercontent.com", None);
        let url = authorize_url(
            &p,
            "http://127.0.0.1:5555/",
            "durum",
            "meydan",
            Some("furkan@gmail.com"),
        )
        .unwrap();
        assert!(url
            .as_str()
            .starts_with(crate::provider::GOOGLE_AUTHORIZE_URL));
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "istemci.apps.googleusercontent.com");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555/");
        assert_eq!(q["scope"], "https://mail.google.com/ openid email profile");
        assert_eq!(q["state"], "durum");
        assert_eq!(q["code_challenge"], "meydan");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["login_hint"], "furkan@gmail.com");
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");

        // Adres olmayan ipucu gönderilmez.
        let m = Provider::microsoft("ms");
        let url = authorize_url(&m, "http://127.0.0.1:1/", "s", "c", Some(" ")).unwrap();
        assert!(!url.as_str().contains("login_hint"));
        assert!(!url.as_str().contains("access_type"));
    }

    #[test]
    fn hata_ayiklama_ciktisi_jeton_icermez() {
        let t = TokenSet {
            access_token: "ya29.gizli".into(),
            refresh_token: Some("1//gizli".into()),
            expires_at: Instant::now(),
            id_token: None,
        };
        let s = format!("{t:?}");
        assert!(!s.contains("gizli"), "{s}");
    }
}
