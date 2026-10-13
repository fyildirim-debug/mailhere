//! İşletim sistemi anahtar zinciri (Linux: Secret Service, Windows: Kimlik
//! Bilgisi Yöneticisi). Hizmet adı "Mailhere", kullanıcı adı hesap kimliği.

use mh_core::{AppError, AppResult, ErrorCode};
use mh_sync::SecretStore;

const SERVICE: &str = "Mailhere";

pub struct KeyringSecrets;

fn entry(account_id: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, account_id).map_err(keyring_err)
}

fn keyring_err(e: keyring::Error) -> AppError {
    // Hata metni parola içermez; yalnız tür günlüğe yazılır.
    tracing::warn!(error = %e, "anahtar zinciri hatası");
    AppError::new(
        ErrorCode::Keyring,
        "İşletim sisteminin anahtar zincirine erişilemedi. Anahtar zinciri hizmetinin açık olduğunu denetle.",
    )
}

/// Veritabanı şifreleme anahtarının kasası: parolalarla aynı depo (anahtar
/// zinciri ya da bellek kipinde süreç belleği), kullanıcı adı `db-key`.
pub struct DbKeyVault<'a> {
    secrets: &'a dyn SecretStore,
    persistent: bool,
}

impl<'a> DbKeyVault<'a> {
    pub fn new(secrets: &'a dyn SecretStore, persistent: bool) -> Self {
        Self {
            secrets,
            persistent,
        }
    }
}

impl mh_store::KeyVault for DbKeyVault<'_> {
    fn load(&self) -> AppResult<Option<String>> {
        self.secrets.get(mh_store::DB_KEY_SECRET)
    }

    fn save(&self, secret: &str) -> AppResult<()> {
        self.secrets.set(mh_store::DB_KEY_SECRET, secret)
    }

    fn persistent(&self) -> bool {
        self.persistent
    }
}

impl SecretStore for KeyringSecrets {
    fn get(&self, account_id: &str) -> AppResult<Option<String>> {
        match entry(account_id)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        }
    }

    fn set(&self, account_id: &str, secret: &str) -> AppResult<()> {
        entry(account_id)?.set_password(secret).map_err(keyring_err)
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
        match entry(account_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(e)),
        }
    }
}
