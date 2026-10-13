//! Parola deposu soyutlaması. Gerçek uygulama işletim sisteminin anahtar
//! zincirini kullanır (`src-tauri`); testler ve tanıtım kipi bellek içi
//! depoyu kullanır. Parolalar asla veritabanına ya da günlüğe yazılmaz.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mh_core::{AppError, AppResult, ErrorCode};

pub trait SecretStore: Send + Sync + 'static {
    fn get(&self, account_id: &str) -> AppResult<Option<String>>;
    fn set(&self, account_id: &str, secret: &str) -> AppResult<()>;
    fn delete(&self, account_id: &str) -> AppResult<()>;
}

/// Bellek içi depo.
#[derive(Default)]
pub struct MemorySecrets {
    map: Mutex<HashMap<String, String>>,
}

impl MemorySecrets {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, account_id: &str) -> AppResult<Option<String>> {
        Ok(self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account_id)
            .cloned())
    }

    fn set(&self, account_id: &str, secret: &str) -> AppResult<()> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account_id.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account_id);
        Ok(())
    }
}

/// Parolayı engelleyici olmayan biçimde okur (anahtar zinciri D-Bus ya da
/// Windows API çağrısı yapabilir).
pub async fn password_for(secrets: &Arc<dyn SecretStore>, account_id: &str) -> AppResult<String> {
    let s = secrets.clone();
    let id = account_id.to_owned();
    let got = tokio::task::spawn_blocking(move || s.get(&id))
        .await
        .map_err(|_| AppError::new(ErrorCode::Internal, "Parola okunurken iç hata oluştu."))??;
    got.ok_or_else(|| {
        AppError::new(
            ErrorCode::Keyring,
            "Bu hesabın parolası anahtar zincirinde bulunamadı. Hesabı kaldırıp yeniden ekle.",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bellek_deposu() {
        let s: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
        assert_eq!(
            password_for(&s, "a").await.unwrap_err().code,
            ErrorCode::Keyring
        );
        s.set("a", "gizli").unwrap();
        assert_eq!(password_for(&s, "a").await.unwrap(), "gizli");
        s.delete("a").unwrap();
        assert!(s.get("a").unwrap().is_none());
    }
}
