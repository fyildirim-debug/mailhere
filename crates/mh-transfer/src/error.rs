//! Aktarım hataları. Mesajlar Türkçe, kullanıcıya gösterilebilir ve hiçbir
//! zaman anahtar, kod ya da paket içeriği taşımaz.

use mh_core::{AppError, ErrorCode};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransferError {
    /// Kod metni çözümlenemedi (biçim, adres, uzunluk).
    #[error("{0}")]
    InvalidCode(String),
    /// Kod bu cihazın anladığından yeni bir sürümde.
    #[error("Bu kod daha yeni bir Mailhere sürümüyle oluşturulmuş (v{0}). Önce bu cihazdaki Mailhere'yi güncelle.")]
    NewerVersion(u32),
    /// Bağlantı kurulamadı ya da koptu.
    #[error("{0}")]
    Network(String),
    /// Karşı taraf protokole uymadı ya da kimlik/şifre doğrulanamadı.
    #[error("{0}")]
    Protocol(String),
    /// Süre doldu.
    #[error("{0}")]
    Timeout(String),
    /// Yerel hata (rastgele kaynak, QR üretimi, soket açma).
    #[error("{0}")]
    Internal(String),
}

impl TransferError {
    /// Arayüze giden hata.
    pub fn to_app(&self) -> AppError {
        let code = match self {
            Self::InvalidCode(_) | Self::NewerVersion(_) => ErrorCode::Invalid,
            Self::Network(_) | Self::Timeout(_) => ErrorCode::Network,
            Self::Protocol(_) => ErrorCode::Invalid,
            Self::Internal(_) => ErrorCode::Internal,
        };
        AppError::new(code, self.to_string())
    }
}

impl From<TransferError> for AppError {
    fn from(e: TransferError) -> Self {
        e.to_app()
    }
}

pub type TransferResult<T> = Result<T, TransferError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hata_kodlari_eslenir() {
        assert_eq!(
            TransferError::InvalidCode("x".into()).to_app().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            TransferError::NewerVersion(3).to_app().code,
            ErrorCode::Invalid
        );
        assert!(TransferError::NewerVersion(3).to_string().contains("v3"));
        assert_eq!(
            TransferError::Network("x".into()).to_app().code,
            ErrorCode::Network
        );
        assert_eq!(
            TransferError::Timeout("x".into()).to_app().code,
            ErrorCode::Network
        );
        assert_eq!(
            TransferError::Internal("x".into()).to_app().code,
            ErrorCode::Internal
        );
        let app: AppError = TransferError::Protocol("p".into()).into();
        assert_eq!((app.code, app.message.as_str()), (ErrorCode::Invalid, "p"));
    }
}
