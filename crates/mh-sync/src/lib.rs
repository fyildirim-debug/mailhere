//! Mailhere eşitleme: hesap ayarı keşfi, IMAP bağlantısı, kutu eşitleme,
//! IDLE ile anlık bildirim ve sunucu işlemleri.

mod conn;
pub mod discover;
mod engine;
mod error;
mod mailbox;
pub mod mutf7;
mod roles;
mod secrets;
pub mod tls;

use mh_core::{Account, MessageSummary, OutboxEvent, SyncStatus};

pub use conn::{connect, is_local, test_login, tls_config, ImapSession, ImapStream};
pub use discover::discover;
pub use engine::{assign_roles, fetch_body, is_syncable, Listed, SyncManager};
pub use error::{SyncError, SyncResult};
pub use mailbox::{sync_mailbox, uid_set, MailboxOutcome};
pub use secrets::{password_for, MemorySecrets, SecretStore};

/// Arka uçtan arayüze giden olaylar. Kabuk (`src-tauri`) bunları pencerelere
/// iletir; testler kaydeder.
pub trait EventSink: Send + Sync + 'static {
    /// Hesabın eşitleme durumu değişti.
    fn sync_status(&self, status: SyncStatus);
    /// Bu kutuların içeriği değişti.
    fn changed(&self, mailbox_ids: Vec<String>);
    /// Gelen kutusuna yeni, okunmamış postalar geldi (bildirim kararı için).
    fn new_mail(&self, account: &Account, messages: Vec<MessageSummary>);
    /// Giden kutusundaki bir postanın sonucu.
    fn outbox(&self, _event: OutboxEvent) {}
}
