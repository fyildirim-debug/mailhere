//! JMAP hataları ve kullanıcıya gösterilen Türkçe karşılıkları.
//!
//! Hata metinleri parola ya da jeton içermez: kimlik bilgisi yalnız
//! `Authorization` başlığında gider, adreste ya da gövdede yer almaz.

use mh_core::{AppError, ErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum JmapError {
    #[error("{0}")]
    App(AppError),
    /// Sunucu kimlik bilgisini reddetti (HTTP 401/403).
    #[error("kimlik doğrulama reddedildi")]
    Auth,
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    /// Beklenmeyen HTTP durum kodu.
    #[error("http durumu {0}")]
    Status(u16),
    /// Yanıt JMAP biçiminde değil.
    #[error("protokol: {0}")]
    Protocol(String),
    /// Yöntem düzeyinde hata (`["error", {"type": ...}]`).
    #[error("yöntem hatası: {kind}")]
    Method {
        kind: String,
        description: Option<String>,
    },
    #[error("zaman aşımı")]
    Timeout,
}

pub type JmapResult<T> = Result<T, JmapError>;

impl From<AppError> for JmapError {
    fn from(e: AppError) -> Self {
        Self::App(e)
    }
}

impl From<tokio::time::error::Elapsed> for JmapError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Self::Timeout
    }
}

/// Hata zincirinde TLS (sertifika) hatası var mı?
fn is_tls(e: &(dyn std::error::Error + 'static)) -> bool {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = cur {
        if err.downcast_ref::<rustls::Error>().is_some() {
            return true;
        }
        // `io::Error::source` iç hatayı atlar; iç içe io hatalarına elle in
        // (hyper: Other → InvalidData → rustls).
        let mut io = err.downcast_ref::<std::io::Error>();
        while let Some(inner) = io.and_then(|e| e.get_ref()) {
            if inner.downcast_ref::<rustls::Error>().is_some() {
                return true;
            }
            io = inner.downcast_ref::<std::io::Error>();
        }
        cur = err.source();
    }
    false
}

fn host_of(e: &reqwest::Error) -> String {
    e.url()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| "sunucu".into())
}

pub fn auth_error() -> AppError {
    AppError::new(
        ErrorCode::Auth,
        "JMAP sunucusu kullanıcı adını ya da parolayı kabul etmedi. Bilgileri denetleyip yeniden dene.",
    )
}

pub fn protocol_error() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Sunucunun yanıtı JMAP biçiminde değil. Oturum adresini denetle.",
    )
}

impl JmapError {
    /// Bağlantı ya da geçici sunucu sorunu mu (yeniden denenir)?
    pub fn is_connection(&self) -> bool {
        match self {
            Self::Http(e) => !e.is_status() && !e.is_decode() && !e.is_builder(),
            Self::Timeout => true,
            Self::Status(s) => *s >= 500 || *s == 429 || *s == 408,
            Self::App(e) => matches!(e.code, ErrorCode::Network),
            _ => false,
        }
    }

    /// `Email/changes` gibi yöntemler durumu artık hesaplayamıyor; baştan eşitlenmeli.
    pub fn cannot_calculate_changes(&self) -> bool {
        matches!(self, Self::Method { kind, .. } if kind == "cannotCalculateChanges")
    }

    /// Arayüze gösterilecek hata.
    pub fn to_app(&self) -> AppError {
        match self {
            Self::App(e) => e.clone(),
            Self::Auth => auth_error(),
            Self::Http(e) if is_tls(e) => crate::client::untrusted_for(e.url()).unwrap_or_else(|| {
                AppError::new(
                    ErrorCode::Tls,
                    "Güvenli bağlantı kurulamadı: sunucu sertifikası doğrulanamadı. Sunucu adresini denetle.",
                )
            }),
            Self::Http(e) if e.is_timeout() => timeout_error(),
            Self::Http(e) if e.is_decode() => protocol_error(),
            Self::Http(e) => AppError::new(
                ErrorCode::Network,
                format!(
                    "JMAP sunucusuna ({}) ulaşılamadı. Adresi ve internet bağlantını denetle; yeniden denenecek.",
                    host_of(e)
                ),
            ),
            Self::Timeout => timeout_error(),
            Self::Status(404) => AppError::new(
                ErrorCode::Invalid,
                "JMAP adresi sunucuda bulunamadı (HTTP 404). Oturum adresini denetle.",
            ),
            Self::Status(s) if *s >= 500 || *s == 429 => AppError::new(
                ErrorCode::Network,
                format!("JMAP sunucusu şu an yanıt veremiyor (HTTP {s}). Birazdan yeniden denenecek."),
            ),
            Self::Status(s) => AppError::new(
                ErrorCode::Internal,
                format!("JMAP sunucusu isteği reddetti (HTTP {s})."),
            ),
            Self::Protocol(_) => protocol_error(),
            Self::Method { kind, description } => AppError::new(
                ErrorCode::Internal,
                match description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
                    Some(d) => format!("Sunucu isteği reddetti ({kind}): {d}"),
                    None => format!("Sunucu isteği reddetti ({kind})."),
                },
            ),
        }
    }
}

fn timeout_error() -> AppError {
    AppError::new(
        ErrorCode::Network,
        "Sunucu zamanında yanıt vermedi. Bağlantını denetleyip yeniden dene.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkce_karsiliklar() {
        assert_eq!(JmapError::Auth.to_app().code, ErrorCode::Auth);
        assert_eq!(JmapError::Timeout.to_app().code, ErrorCode::Network);
        assert!(JmapError::Timeout.is_connection());
        assert_eq!(JmapError::Status(503).to_app().code, ErrorCode::Network);
        assert!(JmapError::Status(503).is_connection());
        assert!(!JmapError::Status(400).is_connection());
        assert_eq!(JmapError::Status(404).to_app().code, ErrorCode::Invalid);
        assert_eq!(
            JmapError::Protocol("x".into()).to_app().code,
            ErrorCode::Invalid
        );
        let m = JmapError::Method {
            kind: "cannotCalculateChanges".into(),
            description: None,
        };
        assert!(m.cannot_calculate_changes());
        assert!(m.to_app().message.contains("cannotCalculateChanges"));
        assert!(!JmapError::Auth.is_connection());
    }
}
