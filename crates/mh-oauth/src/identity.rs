//! Giriş yapan kişinin adresi ve adı.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;

use crate::token::{endpoint, http_client, network};
use crate::{OAuthError, Provider, TokenSet};

/// Hesabın adresi ve (varsa) görünen adı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Deserialize, Default)]
struct Claims {
    #[serde(default)]
    email: Option<String>,
    /// Microsoft: çoğunlukla oturum açma adresi.
    #[serde(default)]
    preferred_username: Option<String>,
    /// Microsoft iş/okul hesaplarında kullanıcı asıl adı.
    #[serde(default)]
    upn: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl Claims {
    fn identity(self) -> Option<Identity> {
        let email = [self.email, self.preferred_username, self.upn]
            .into_iter()
            .flatten()
            .map(|e| e.trim().to_owned())
            .find(|e| looks_like_email(e))?;
        let name = self
            .name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty());
        Some(Identity { email, name })
    }
}

fn looks_like_email(s: &str) -> bool {
    match s.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty() && domain.contains('.') && !s.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

/// `id_token` yükünden adres ve ad.
///
/// İmza doğrulanmaz, yalnız çözülür: jeton tarayıcıdan değil, doğrudan
/// sağlayıcının jeton uç noktasından TLS (sertifika doğrulamalı) ile alındı
/// ve yalnız adresi göstermek için kullanılır; yetki kararı vermez
/// (OpenID Connect Core 1.0, 3.1.3.7 madde 6).
pub fn from_id_token(id_token: &str) -> Option<Identity> {
    let payload = id_token.split('.').nth(1)?;
    // Bazı sağlayıcılar dolgu bırakabilir; kırpıp çöz.
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice::<Claims>(&bytes).ok()?.identity()
}

/// OpenID Connect `userinfo` uç noktasından adres ve ad.
pub async fn userinfo(p: &Provider, access_token: &str) -> Result<Identity, OAuthError> {
    let url_str = p
        .userinfo_url
        .as_deref()
        .ok_or_else(|| OAuthError::Protocol("kimlik bilgisi uç noktası yok".into()))?;
    let url = endpoint(url_str)?;
    let resp = http_client(&url)?
        .get(url)
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(network)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(OAuthError::Protocol(format!(
            "kimlik bilgisi uç noktası HTTP {} döndü",
            status.as_u16()
        )));
    }
    let body = resp.bytes().await.map_err(network)?;
    serde_json::from_slice::<Claims>(&body)
        .ok()
        .and_then(Claims::identity)
        .ok_or_else(|| OAuthError::Protocol("yanıtta e-posta adresi yok".into()))
}

/// Önce `id_token`, olmazsa `userinfo`.
pub async fn identify(p: &Provider, tokens: &TokenSet) -> Result<Identity, OAuthError> {
    if let Some(id) = tokens.id_token.as_deref().and_then(from_id_token) {
        return Ok(id);
    }
    if p.userinfo_url.is_some() {
        return userinfo(p, &tokens.access_token).await;
    }
    Err(OAuthError::Protocol(
        "kimlik jetonunda e-posta adresi yok".into(),
    ))
}

/// Testler için imzasız `id_token` üretir.
#[cfg(any(test, feature = "testing"))]
pub fn unsigned_id_token(claims: &serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let body = URL_SAFE_NO_PAD.encode(claims.to_string());
    format!("{header}.{body}.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn google_kimlik_jetonu() {
        let t = unsigned_id_token(&json!({
            "iss": "https://accounts.google.com",
            "email": "furkan@gmail.com",
            "email_verified": true,
            "name": "Furkan Yıldırım"
        }));
        assert_eq!(
            from_id_token(&t),
            Some(Identity {
                email: "furkan@gmail.com".into(),
                name: Some("Furkan Yıldırım".into())
            })
        );
    }

    #[test]
    fn microsoft_tercih_edilen_ad() {
        let t = unsigned_id_token(&json!({
            "preferred_username": "furkan@outlook.com",
            "name": "  "
        }));
        assert_eq!(
            from_id_token(&t),
            Some(Identity {
                email: "furkan@outlook.com".into(),
                name: None
            })
        );
        // Adres olmayan tercih edilen ad atlanır, upn kullanılır.
        let t = unsigned_id_token(&json!({
            "preferred_username": "furkan",
            "upn": "furkan@sirket.com.tr"
        }));
        assert_eq!(from_id_token(&t).unwrap().email, "furkan@sirket.com.tr");
    }

    #[test]
    fn bozuk_jeton() {
        assert!(from_id_token("").is_none());
        assert!(from_id_token("a.b.c").is_none());
        assert!(from_id_token(&unsigned_id_token(&json!({"name": "x"}))).is_none());
        assert!(from_id_token(&unsigned_id_token(&json!({"email": "a b@c.d"}))).is_none());
    }
}
