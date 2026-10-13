//! Mailhere yerel deposu: SQLite + FTS5.
//!
//! Tüm işlemler tek bir bağlantı üzerinden, kısa kilitlerle yapılır; `Store`
//! iş parçacıkları arasında paylaşılabilir (`Arc<Store>`).

mod accounts;
mod codec;
mod demo;
mod mailboxes;
mod messages;
mod outbox;
mod schema;
mod search;
mod settings;

use std::path::Path;
use std::sync::Mutex;

use mh_core::{AppError, AppResult, ErrorCode};
use rusqlite::{Connection, Transaction};

pub use demo::{seed_demo, DEMO_HOST};
pub use mailboxes::{mailbox_id, MailboxInfo};
pub use messages::{MessageLocation, NewMessage, UpsertOutcome};
pub use outbox::OutboxRow;
pub use search::fts_query;

/// Yerel depo.
pub struct Store {
    conn: Mutex<Connection>,
}

pub(crate) fn storage_err(e: rusqlite::Error) -> AppError {
    tracing::error!(error = %e, "veritabanı hatası");
    AppError::new(
        ErrorCode::Storage,
        "Yerel veritabanına erişilemedi. Uygulamayı yeniden başlatmayı dene.",
    )
}

impl Store {
    /// Diskteki veritabanını açar (yoksa oluşturur) ve şemayı günceller.
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path).map_err(storage_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(storage_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(storage_err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage_err)?;
        Self::init(conn)
    }

    /// Bellek içi veritabanı (testler).
    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory().map_err(storage_err)?)
    }

    fn init(mut conn: Connection) -> AppResult<Self> {
        schema::migrate(&mut conn).map_err(storage_err)?;
        // Çökme sonrası yarıda kalan gönderimleri yeniden sıraya al.
        conn.execute("UPDATE outbox SET claimed = 0 WHERE claimed = 1", [])
            .map_err(storage_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn with<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> AppResult<T> {
        f(&self.lock()).map_err(storage_err)
    }

    pub(crate) fn with_tx<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> AppResult<T> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(storage_err)?;
        let out = f(&tx).map_err(storage_err)?;
        tx.commit().map_err(storage_err)?;
        Ok(out)
    }

    /// Şema sürümü (tanılama için).
    pub fn schema_version(&self) -> AppResult<i64> {
        self.with(|c| c.pragma_query_value(None, "user_version", |r| r.get(0)))
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use mh_core::{Account, AccountKind, Security, ServerConfig};

    use crate::Store;

    pub fn account(id: &str) -> Account {
        Account {
            id: id.into(),
            name: "İş".into(),
            email: format!("{id}@ornek.com.tr"),
            display_name: "Furkan Yıldırım".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: Some(ServerConfig {
                host: "imap.ornek.com.tr".into(),
                port: 993,
                security: Security::Tls,
                username: format!("{id}@ornek.com.tr"),
            }),
            smtp: None,
            jmap_url: None,
            signature: None,
            created_at: 1,
            auth: Default::default(),
        }
    }

    pub fn store_with_account(id: &str) -> Store {
        let s = Store::open_in_memory().unwrap();
        s.insert_account(&account(id)).unwrap();
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sema_son_surumde() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), schema::latest_version() as i64);
    }

    #[test]
    fn disk_veritabani_yeniden_acilir() {
        let dir = std::env::temp_dir().join(format!("mh-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        {
            let s = Store::open(&path).unwrap();
            s.insert_account(&testutil::account("a")).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.accounts().unwrap().len(), 1);
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
}
