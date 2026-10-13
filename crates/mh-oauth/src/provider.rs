//! Sağlayıcı ayarları: uç noktalar, kapsamlar, istemci kimlikleri ve
//! Gmail / Outlook posta sunucuları.
//!
//! İstemci kimlikleri koda yazılmaz. Derleme sırasında
//! `MAILHERE_GOOGLE_CLIENT_ID`, `MAILHERE_GOOGLE_CLIENT_SECRET` ve
//! `MAILHERE_MICROSOFT_CLIENT_ID` ortam değişkenlerinden gömülür; çalışma
//! anında aynı adlı ortam değişkenleri bunları ezer. Boş değer yok sayılır.

use std::fmt;

use mh_core::{AuthKind, Security, ServerConfig};

pub const GOOGLE_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
pub const GOOGLE_SCOPES: &str = "https://mail.google.com/ openid email profile";

pub const MICROSOFT_AUTHORIZE_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
pub const MICROSOFT_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
pub const MICROSOFT_SCOPES: &str = "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access openid email profile";

const GOOGLE_CLIENT_ID: &str = "MAILHERE_GOOGLE_CLIENT_ID";
const GOOGLE_CLIENT_SECRET: &str = "MAILHERE_GOOGLE_CLIENT_SECRET";
const MICROSOFT_CLIENT_ID: &str = "MAILHERE_MICROSOFT_CLIENT_ID";

/// Bir OAuth sağlayıcısının bu uygulama için ayarı.
#[derive(Clone, PartialEq, Eq)]
pub struct Provider {
    pub kind: AuthKind,
    pub authorize_url: String,
    pub token_url: String,
    /// `id_token` adres içermezse kimlik buradan okunur (yalnız Google;
    /// Microsoft'un erişim jetonu Outlook içindir, Graph'a gidemez).
    pub userinfo_url: Option<String>,
    /// Boşlukla ayrılmış kapsamlar.
    pub scopes: String,
    pub client_id: String,
    /// Google masaüstü uygulamasının "gizli" değeri. Masaüstü istemcide
    /// saklanamayacağı için Google bunu gizli saymaz; PKCE asıl korumadır.
    pub client_secret: Option<String>,
    /// Yetkilendirme adresine eklenen sağlayıcıya özgü değerler.
    pub extra_params: Vec<(String, String)>,
    /// Jeton isteklerinde `scope` da gönderilir (Microsoft v2.0).
    pub scope_in_token_request: bool,
}

impl fmt::Debug for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Provider")
            .field("kind", &self.kind)
            .field("authorize_url", &self.authorize_url)
            .field("token_url", &self.token_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "***"))
            .finish_non_exhaustive()
    }
}

impl Provider {
    pub fn google(client_id: impl Into<String>, client_secret: Option<String>) -> Self {
        Self {
            kind: AuthKind::Google,
            authorize_url: GOOGLE_AUTHORIZE_URL.into(),
            token_url: GOOGLE_TOKEN_URL.into(),
            userinfo_url: Some(GOOGLE_USERINFO_URL.into()),
            scopes: GOOGLE_SCOPES.into(),
            client_id: client_id.into(),
            client_secret,
            // Yenileme jetonu her girişte verilsin.
            extra_params: vec![
                ("access_type".into(), "offline".into()),
                ("prompt".into(), "consent".into()),
            ],
            scope_in_token_request: false,
        }
    }

    pub fn microsoft(client_id: impl Into<String>) -> Self {
        Self {
            kind: AuthKind::Microsoft,
            authorize_url: MICROSOFT_AUTHORIZE_URL.into(),
            token_url: MICROSOFT_TOKEN_URL.into(),
            userinfo_url: None,
            scopes: MICROSOFT_SCOPES.into(),
            client_id: client_id.into(),
            client_secret: None,
            extra_params: Vec::new(),
            scope_in_token_request: true,
        }
    }

    /// Ortamdan (çalışma anı, yoksa derleme anı) ayarlanmış sağlayıcı.
    /// İstemci kimliği yoksa `None`.
    pub fn configured(kind: AuthKind) -> Option<Self> {
        match kind {
            AuthKind::Google => setting(GOOGLE_CLIENT_ID, option_env!("MAILHERE_GOOGLE_CLIENT_ID"))
                .map(|id| {
                    Self::google(
                        id,
                        setting(
                            GOOGLE_CLIENT_SECRET,
                            option_env!("MAILHERE_GOOGLE_CLIENT_SECRET"),
                        ),
                    )
                }),
            AuthKind::Microsoft => setting(
                MICROSOFT_CLIENT_ID,
                option_env!("MAILHERE_MICROSOFT_CLIENT_ID"),
            )
            .map(Self::microsoft),
            AuthKind::Password => None,
        }
    }
}

/// İstemci kimliği tanımlı sağlayıcılar (arayüzde gösterilecek düğmeler).
pub fn configured_kinds() -> Vec<AuthKind> {
    [AuthKind::Google, AuthKind::Microsoft]
        .into_iter()
        .filter(|k| Provider::configured(*k).is_some())
        .collect()
}

fn setting(name: &str, built_in: Option<&'static str>) -> Option<String> {
    // Çalışma anında ortam değişkeniyle değiştirme yalnız geliştirme
    // derlemesinde: yayın sürümünde kullanıcıya başka bir OAuth uygulamasına
    // izin verdirilemez.
    let runtime = if cfg!(debug_assertions) {
        std::env::var(name).ok()
    } else {
        None
    };
    pick(runtime.as_deref(), built_in)
}

/// Çalışma anı değeri doluysa o, değilse derleme anı değeri; boşluklar
/// kırpılır, boş değer yok sayılır.
fn pick(runtime: Option<&str>, built_in: Option<&str>) -> Option<String> {
    [runtime, built_in]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Sağlayıcının gelen (IMAP) ve giden (SMTP) sunucuları; kullanıcı adı
/// e-posta adresidir.
pub fn mail_servers(kind: AuthKind, email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let server = |host: &str, port, security| ServerConfig {
        host: host.into(),
        port,
        security,
        username: email.trim().into(),
    };
    match kind {
        AuthKind::Google => Some((
            server("imap.gmail.com", 993, Security::Tls),
            server("smtp.gmail.com", 465, Security::Tls),
        )),
        AuthKind::Microsoft => Some((
            server("outlook.office365.com", 993, Security::Tls),
            server("smtp.office365.com", 587, Security::Starttls),
        )),
        AuthKind::Password => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calisma_ani_degeri_ezer_bos_yok_sayilir() {
        assert_eq!(
            pick(Some("calisma"), Some("derleme")).as_deref(),
            Some("calisma")
        );
        assert_eq!(
            pick(Some("  "), Some("derleme")).as_deref(),
            Some("derleme")
        );
        assert_eq!(pick(None, Some(" derleme ")).as_deref(), Some("derleme"));
        assert_eq!(pick(Some(""), Some("")), None);
        assert_eq!(pick(None, None), None);
    }

    #[test]
    fn saglayici_ayarlari() {
        let g = Provider::google("id", Some("gizli".into()));
        assert_eq!(g.kind, AuthKind::Google);
        assert!(g.scopes.contains("https://mail.google.com/"));
        assert!(g
            .extra_params
            .contains(&("access_type".into(), "offline".into())));
        assert!(g
            .extra_params
            .contains(&("prompt".into(), "consent".into())));
        assert!(!format!("{g:?}").contains("gizli"));

        let m = Provider::microsoft("id");
        assert!(m.scopes.contains("IMAP.AccessAsUser.All"));
        assert!(m.scopes.contains("SMTP.Send"));
        assert!(m.scopes.contains("offline_access"));
        assert!(m
            .authorize_url
            .starts_with("https://login.microsoftonline.com/common/"));
        assert!(m.client_secret.is_none());
        assert!(Provider::configured(AuthKind::Password).is_none());
    }

    #[test]
    fn posta_sunuculari() {
        let (imap, smtp) = mail_servers(AuthKind::Google, "furkan@gmail.com").unwrap();
        assert_eq!((imap.host.as_str(), imap.port), ("imap.gmail.com", 993));
        assert_eq!((smtp.host.as_str(), smtp.port), ("smtp.gmail.com", 465));
        assert_eq!(smtp.security, Security::Tls);
        assert_eq!(imap.username, "furkan@gmail.com");

        let (imap, smtp) = mail_servers(AuthKind::Microsoft, "f@outlook.com").unwrap();
        assert_eq!(
            (imap.host.as_str(), imap.port, imap.security),
            ("outlook.office365.com", 993, Security::Tls)
        );
        assert_eq!(
            (smtp.host.as_str(), smtp.port, smtp.security),
            ("smtp.office365.com", 587, Security::Starttls)
        );
        assert!(mail_servers(AuthKind::Password, "a@b.c").is_none());
    }
}
