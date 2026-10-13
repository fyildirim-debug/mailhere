//! Mailhere OAuth 2: Google ve Microsoft ile tarayıcıda giriş.
//!
//! - Yetkilendirme kodu akışı, PKCE (S256) ve rastgele `state`,
//! - dönüş `http://127.0.0.1:<rastgele kapı>/` adresinde tek seferlik
//!   dinleyiciyle karşılanır (5 dakika süre sınırı ve iptal),
//! - kod → jeton değişimi ve yenileme (`invalid_grant` ayrı hata),
//! - adres ve ad `id_token` yükünden ya da `userinfo` uç noktasından,
//! - IMAP/SMTP için XOAUTH2 ilk yanıtı.
//!
//! Jetonlar burada saklanmaz: yenileme jetonu anahtar zincirine, erişim
//! jetonu bellekteki önbelleğe gider (`mh-sync`). Hiçbir jeton günlüğe
//! yazılmaz.

mod error;
mod flow;
mod identity;
mod loopback;
mod pkce;
mod provider;
mod token;
mod xoauth2;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use error::{provider_name, revoked, OAuthError};
pub use flow::{sign_in, SignedIn, SIGN_IN_TIMEOUT};
pub use identity::{from_id_token, identify, userinfo, Identity};
pub use loopback::Loopback;
pub use pkce::{challenge_for, random_urlsafe, Pkce};
pub use provider::{configured_kinds, mail_servers, Provider};
pub use token::{authorize_url, exchange_code, refresh, TokenSet};
pub use xoauth2::xoauth2_payload;
