//! Eşitleme hataları ve kullanıcıya gösterilen Türkçe karşılıkları.

use mh_core::{AppError, ErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{0}")]
    App(AppError),
    #[error("imap: {0}")]
    Imap(#[from] async_imap::error::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zaman aşımı")]
    Timeout,
}

impl From<AppError> for SyncError {
    fn from(e: AppError) -> Self {
        Self::App(e)
    }
}

impl From<tokio::time::error::Elapsed> for SyncError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Self::Timeout
    }
}

fn is_tls_io(e: &std::io::Error) -> bool {
    e.get_ref()
        .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some())
        || e.kind() == std::io::ErrorKind::InvalidData
}

impl SyncError {
    /// Bağlantı koptu mu (yeniden bağlanmak gerekir)?
    pub fn is_connection(&self) -> bool {
        use async_imap::error::Error as E;
        match self {
            Self::Io(_) | Self::Timeout => true,
            Self::Imap(E::Io(_) | E::ConnectionLost | E::Parse(_)) => true,
            Self::App(e) => matches!(e.code, ErrorCode::Network | ErrorCode::Tls),
            _ => false,
        }
    }

    /// Arayüze gösterilecek hata.
    pub fn to_app(&self) -> AppError {
        use async_imap::error::Error as E;
        match self {
            Self::App(e) => e.clone(),
            Self::Timeout => AppError::new(
                ErrorCode::Network,
                "Sunucu zamanında yanıt vermedi. Bağlantını denetleyip yeniden dene.",
            ),
            Self::Io(e) | Self::Imap(E::Io(e)) if is_tls_io(e) => tls_error(),
            Self::Io(_) | Self::Imap(E::Io(_) | E::ConnectionLost) => AppError::new(
                ErrorCode::Network,
                "Sunucuyla bağlantı koptu. İnternet bağlantını denetle; yeniden denenecek.",
            ),
            Self::Imap(E::No(m) | E::Bad(m)) => AppError::new(
                ErrorCode::Internal,
                format!("Sunucu isteği reddetti: {}", m.trim()),
            ),
            Self::Imap(e) => AppError::new(
                ErrorCode::Internal,
                format!("Sunucu yanıtı anlaşılamadı: {e}"),
            ),
        }
    }
}

pub fn tls_error() -> AppError {
    AppError::new(
        ErrorCode::Tls,
        "Güvenli bağlantı kurulamadı: sunucu sertifikası doğrulanamadı. Sunucu adını ve güvenlik ayarını denetle.",
    )
}

pub fn auth_error() -> AppError {
    AppError::new(
        ErrorCode::Auth,
        "Kullanıcı adı ya da parola kabul edilmedi. Sağlayıcın uygulama parolası istiyorsa onu kullan.",
    )
}

pub fn network_error(host: &str, port: u16) -> AppError {
    AppError::new(
        ErrorCode::Network,
        format!("{host}:{port} adresine bağlanılamadı. Sunucu adını, kapıyı ve internet bağlantını denetle."),
    )
}

pub type SyncResult<T> = Result<T, SyncError>;
