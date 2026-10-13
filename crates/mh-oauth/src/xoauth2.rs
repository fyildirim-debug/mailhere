//! SASL XOAUTH2 (Google ve Microsoft'un IMAP/SMTP jetonla giriş biçimi).

/// İlk yanıt: `user=<adres>^Aauth=Bearer <jeton>^A^A`. IMAP ve SMTP bunu
/// base64 ile kodlayıp gönderir. Sonuç jeton içerir; günlüğe yazılmaz.
pub fn xoauth2_payload(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    #[test]
    fn ilk_yanit_bicimi() {
        let p = xoauth2_payload(
            "someuser@example.com",
            "ya29.vF9dft4qmTc2Nvb3RlckBhdHRhdmlzdGEuY29tCg",
        );
        assert_eq!(
            p,
            "user=someuser@example.com\u{1}auth=Bearer ya29.vF9dft4qmTc2Nvb3RlckBhdHRhdmlzdGEuY29tCg\u{1}\u{1}"
        );
        // Google'ın belgelediği base64 biçimiyle aynı.
        assert_eq!(
            STANDARD.encode(p),
            "dXNlcj1zb21ldXNlckBleGFtcGxlLmNvbQFhdXRoPUJlYXJlciB5YTI5LnZGOWRmdDRxbVRjMk52YjNSbGNrQmhkSFJoZG1semRHRXVZMjl0Q2cBAQ=="
        );
    }
}
