//! OAuth hataları ve kullanıcıya gösterilen Türkçe karşılıkları.
//!
//! Hata metinleri jeton, yetki kodu ya da PKCE doğrulayıcısı içermez.

use mh_core::{AppError, AuthKind, ErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    /// Bu derlemede (ya da ortam değişkeninde) istemci kimliği yok.
    #[error("istemci kimliği tanımlı değil")]
    NotConfigured,
    /// Sistem tarayıcısı açılamadı.
    #[error("tarayıcı açılamadı: {0}")]
    Browser(String),
    /// Kullanıcı izin ekranında reddetti (`access_denied`).
    #[error("kullanıcı izin vermedi")]
    Denied,
    /// Yeni bir giriş başladı ya da uygulama vazgeçti.
    #[error("giriş iptal edildi")]
    Cancelled,
    /// Tarayıcıdan dönüş süresinde gelmedi.
    #[error("giriş zaman aşımına uğradı")]
    TimedOut,
    /// Yetki kodu ya da yenileme jetonu geçersiz, süresi dolmuş ya da iptal
    /// edilmiş (`invalid_grant`). Yeniden giriş gerekir.
    #[error("invalid_grant")]
    InvalidGrant,
    /// Sağlayıcının döndürdüğü diğer OAuth hataları.
    #[error("sağlayıcı hatası: {code}")]
    Provider {
        code: String,
        description: Option<String>,
    },
    /// Ağ hatası (DNS, bağlantı, zaman aşımı).
    #[error("ağ hatası: {0}")]
    Network(String),
    /// Beklenmeyen ya da eksik yanıt.
    #[error("beklenmeyen yanıt: {0}")]
    Protocol(String),
    /// Yerel dönüş dinleyicisi açılamadı.
    #[error("yerel dinleyici: {0}")]
    Io(#[from] std::io::Error),
}

/// Sağlayıcının kullanıcıya gösterilen adı.
pub fn provider_name(kind: AuthKind) -> &'static str {
    match kind {
        AuthKind::Google => "Google",
        AuthKind::Microsoft => "Microsoft",
        AuthKind::Password => "Parola",
    }
}

impl OAuthError {
    /// Arayüze gösterilecek hata.
    pub fn to_app(&self, kind: AuthKind) -> AppError {
        let name = provider_name(kind);
        match self {
            Self::NotConfigured => AppError::new(
                ErrorCode::Invalid,
                format!("Bu Mailhere derlemesinde {name} girişi yapılandırılmamış."),
            ),
            Self::Browser(_) => AppError::new(
                ErrorCode::Internal,
                "Tarayıcı açılamadı. Varsayılan tarayıcının ayarlı olduğunu denetleyip yeniden dene.",
            ),
            Self::Denied => AppError::new(
                ErrorCode::Auth,
                format!("{name} girişi onaylanmadı. Postalarına erişim izni vererek yeniden dene."),
            ),
            Self::Cancelled => AppError::new(ErrorCode::Invalid, "Giriş iptal edildi."),
            Self::TimedOut => AppError::new(
                ErrorCode::Auth,
                "Tarayıcıdaki giriş 5 dakika içinde tamamlanmadı. Yeniden dene.",
            ),
            Self::InvalidGrant => revoked(kind),
            Self::Provider { code, .. } => AppError::new(
                ErrorCode::Auth,
                format!("{name} girişi tamamlanamadı ({code}). Yeniden dene."),
            ),
            Self::Network(_) => AppError::new(
                ErrorCode::Network,
                format!("{name} giriş sunucusuna ulaşılamadı. İnternet bağlantını denetleyip yeniden dene."),
            ),
            Self::Protocol(_) => AppError::new(
                ErrorCode::Auth,
                format!("{name} yanıtı anlaşılamadı. Yeniden giriş yapmayı dene."),
            ),
            Self::Io(_) => AppError::new(
                ErrorCode::Internal,
                "Giriş dönüşü için yerel bağlantı açılamadı. Yeniden dene.",
            ),
        }
    }
}

/// Yenileme jetonu artık geçmiyor: kullanıcı yeniden giriş yapmalı.
pub fn revoked(kind: AuthKind) -> AppError {
    AppError::new(
        ErrorCode::Auth,
        format!(
            "{} oturumunun süresi doldu ya da erişim izni kaldırıldı. Hesabı yeniden bağlamak için yeniden giriş yap.",
            provider_name(kind)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkce_ve_kodlu() {
        let e = OAuthError::InvalidGrant.to_app(AuthKind::Google);
        assert_eq!(e.code, ErrorCode::Auth);
        assert!(e.message.contains("Google"), "{}", e.message);
        assert!(e.message.contains("yeniden giriş"), "{}", e.message);
        let e = OAuthError::Network("x".into()).to_app(AuthKind::Microsoft);
        assert_eq!(e.code, ErrorCode::Network);
        assert!(e.message.contains("Microsoft"));
        let e = OAuthError::NotConfigured.to_app(AuthKind::Microsoft);
        assert_eq!(e.code, ErrorCode::Invalid);
    }
}
