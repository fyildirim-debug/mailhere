//! İşletim sistemi anahtar zinciri (Linux: Secret Service, Windows: Kimlik
//! Bilgisi Yöneticisi). Hizmet adı "Mailhere", kullanıcı adı hesap kimliği.
//! Veritabanı anahtarı da aynı depoda, `db-key` adıyla durur
//! (`mh_app::state::DbKeyVault`).

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
