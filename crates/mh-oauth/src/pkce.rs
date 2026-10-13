//! PKCE (RFC 7636, S256) ve güvenli rastgele değerler.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};

use crate::OAuthError;

/// Bir giriş için PKCE çifti. `verifier` yalnız bellekte kalır ve jeton
/// isteğinde gönderilir; tarayıcıya yalnız `challenge` gider.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Result<Self, OAuthError> {
        // 32 bayt → 43 karakter (RFC 7636: 43–128 karakter).
        let verifier = random_urlsafe(32)?;
        let challenge = challenge_for(&verifier);
        Ok(Self {
            verifier,
            challenge,
        })
    }
}

/// `BASE64URL(SHA256(verifier))`, dolgusuz.
pub fn challenge_for(verifier: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.as_ref())
}

/// İşletim sisteminin güvenli rastgele kaynağından `bytes` bayt, URL'de
/// güvenli base64 olarak.
pub fn random_urlsafe(bytes: usize) -> Result<String, OAuthError> {
    let mut buf = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| OAuthError::Protocol("güvenli rastgele değer üretilemedi".into()))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_ornegi() {
        // RFC 7636, Ek B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn dogrulayici_bicimi() {
        let p = Pkce::new().unwrap();
        assert_eq!(p.verifier.len(), 43);
        assert!(p
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(challenge_for(&p.verifier), p.challenge);
        assert_ne!(Pkce::new().unwrap().verifier, p.verifier);
    }
}
