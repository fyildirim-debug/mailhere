//! Mailhere yerel deposu: SQLite + FTS5, diskte SQLCipher ile şifreli.
//!
//! Tüm işlemler tek bir bağlantı üzerinden, kısa kilitlerle yapılır; `Store`
//! iş parçacıkları arasında paylaşılabilir (`Arc<Store>`).

mod accounts;
mod actions;
mod codec;
mod contacts;
mod demo;
mod drafts;
mod encryption;
mod followups;
mod jmap;
mod key;
mod mailboxes;
mod messages;
mod outbox;
mod rules;
mod scheduled;
mod schema;
mod screener;
mod search;
mod settings;
mod threads;

use std::path::Path;
use std::sync::Mutex;

use mh_core::{AppError, AppResult, ErrorCode};
use rusqlite::{Connection, Transaction};

pub use actions::{Action, QueuedAction, ServerFlag, ServerRef};
pub use contacts::CONTACT_RECENT;
pub use demo::{seed_demo, DEMO_HOST};
pub use drafts::DraftPut;
pub use encryption::{private_dir, probe, relocate, DbFile};
pub use jmap::{JmapAccountState, JmapLocation, JmapUpsert};
pub use key::{DbKey, KeyVault, DB_KEY_SECRET};
pub use mailboxes::{mailbox_id, MailboxInfo, Namespace};
pub use messages::{
    is_full_address, is_remote_entry, MessageLocation, NewMessage, PendingMove, UpsertOutcome,
};
pub use outbox::OutboxRow;
pub use rules::{
    plan as rule_plan, CompiledRule, ForwardClaim, MoveDest, RuleInput, RulePlan,
    FORWARDS_PER_HOUR, MAX_RULES,
};
pub use screener::normalize_email;
pub use search::{fold_loose, fts_query};
pub use threads::THREAD_LIMIT;

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
    /// Diskteki şifreli veritabanını `key` ile açar (yoksa oluşturur) ve
    /// şemayı günceller. Önceki sürümlerden kalan şifresiz veritabanı önce
    /// şifreli biçime dönüştürülür. Anahtar uymazsa ya da dosya bozuksa hata
    /// döner; dosyaya dokunulmaz.
    pub fn open(path: &Path, key: &DbKey) -> AppResult<Self> {
        encryption::prepare(path, key)?;
        let conn = encryption::open_keyed(path, key).map_err(|e| {
            if encryption::is_undecryptable(&e) {
                encryption::undecryptable_error(path)
            } else {
                storage_err(e)
            }
        })?;
        encryption::finish(path);
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(storage_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(storage_err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage_err)?;
        Self::init(conn)
    }

    /// Bellek içi veritabanı (testler); diske yazılmadığından şifrelenmez.
    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory().map_err(storage_err)?)
    }

    fn init(mut conn: Connection) -> AppResult<Self> {
        // Silinen kayıtların içeriği sayfalarda kalmasın; sıfırlarla ezilir.
        conn.pragma_update(None, "secure_delete", true)
            .map_err(storage_err)?;
        schema::migrate(&mut conn).map_err(storage_err)?;
        search::fts_secure_delete(&conn).map_err(storage_err)?;
        search::register_functions(&conn).map_err(storage_err)?;
        // Çökme sonrası yarıda kalan gönderimleri yeniden sıraya al.
        conn.execute("UPDATE outbox SET claimed = 0 WHERE claimed = 1", [])
            .map_err(storage_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Toplu silmeden (ör. hesap kaldırma) sonra WAL'ı ana dosyaya işleyip
    /// sıfırlar: silinen sayfaların eski sürümleri WAL'da beklemez.
    /// `secure_delete` açık olduğundan boşalan sayfalar zaten sıfırlanır; tüm
    /// dosyayı yeniden yazan VACUUM büyük posta kutularında pahalı (ve
    /// veritabanı kadar boş alan isteyen) olduğundan yapılmaz.
    pub fn purge_freed(&self) -> AppResult<()> {
        let busy: i64 =
            self.with(|c| c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0)))?;
        if busy != 0 {
            tracing::warn!("WAL sıfırlanamadı; başka okuyucu var");
        }
        Ok(())
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
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 1,
            auth: Default::default(),
            notify: true,
        }
    }

    pub fn store_with_account(id: &str) -> Store {
        let s = Store::open_in_memory().unwrap();
        s.insert_account(&account(id)).unwrap();
        s
    }

    /// Düşerken silinen geçici test klasörü.
    pub struct TempDir(std::path::PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "mh-store-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod encryption_tests;

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
        let dir = testutil::TempDir::new("yeniden");
        let path = dir.path().join("t.db");
        let key = DbKey::generate().unwrap();
        {
            let s = Store::open(&path, &key).unwrap();
            s.insert_account(&testutil::account("a")).unwrap();
        }
        let s = Store::open(&path, &key).unwrap();
        assert_eq!(s.accounts().unwrap().len(), 1);
    }
}
