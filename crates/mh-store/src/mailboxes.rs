//! Posta kutuları ve kutu başına eşitleme durumu.

use mh_core::{AppResult, Mailbox, MailboxRole};
use rusqlite::{params, OptionalExtension, Row};

use crate::codec::{role_from, role_str};
use crate::Store;

/// Kararlı kutu kimliği: "<hesap>:<yol>".
pub fn mailbox_id(account_id: &str, path: &str) -> String {
    format!("{account_id}:{path}")
}

/// Kutu + IMAP eşitleme durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxInfo {
    pub mailbox: Mailbox,
    pub uidvalidity: Option<u32>,
    pub uidnext: Option<u32>,
    /// Yerelde görülen en büyük UID.
    pub highest_uid: u32,
}

const SELECT: &str = "SELECT b.id, b.account_id, b.path, b.name, b.role,
        COALESCE(SUM(m.unread), 0), COUNT(m.id), b.uidvalidity, b.uidnext, b.highest_uid
    FROM mailboxes b LEFT JOIN messages m ON m.mailbox_id = b.id";

fn role_order(r: MailboxRole) -> u8 {
    match r {
        MailboxRole::Inbox => 0,
        MailboxRole::Drafts => 1,
        MailboxRole::Sent => 2,
        MailboxRole::Archive => 3,
        MailboxRole::Junk => 4,
        MailboxRole::Trash => 5,
        MailboxRole::Other => 6,
    }
}

fn row_to_info(r: &Row<'_>) -> rusqlite::Result<MailboxInfo> {
    Ok(MailboxInfo {
        mailbox: Mailbox {
            id: r.get(0)?,
            account_id: r.get(1)?,
            path: r.get(2)?,
            name: r.get(3)?,
            role: role_from(&r.get::<_, String>(4)?),
            unread: r.get::<_, i64>(5)? as u32,
            total: r.get::<_, i64>(6)? as u32,
        },
        uidvalidity: r.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        uidnext: r.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        highest_uid: r.get::<_, i64>(9)? as u32,
    })
}

impl Store {
    /// Kutuyu ekler ya da adını/rolünü günceller; kimliğini döner.
    pub fn upsert_mailbox(
        &self,
        account_id: &str,
        path: &str,
        name: &str,
        role: MailboxRole,
    ) -> AppResult<String> {
        let id = mailbox_id(account_id, path);
        self.with(|c| {
            c.execute(
                "INSERT INTO mailboxes (id, account_id, path, name, role) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, role = excluded.role",
                params![id, account_id, path, name, role_str(role)],
            )
        })?;
        Ok(id)
    }

    /// Tüm hesapların kutuları, sayaçlarıyla. Hesap eklenme sırası, sonra rol.
    pub fn mailboxes(&self) -> AppResult<Vec<Mailbox>> {
        let mut list = self.with(|c| {
            let mut st = c.prepare_cached(&format!(
                "{SELECT} JOIN accounts a ON a.id = b.account_id
                 GROUP BY b.id ORDER BY a.created_at, a.rowid, b.path"
            ))?;
            let rows = st.query_map([], row_to_info)?;
            rows.map(|r| r.map(|i| i.mailbox))
                .collect::<rusqlite::Result<Vec<_>>>()
        })?;
        // Hesap sırasını koruyarak rol sırasına diz (sort kararlıdır).
        let accounts: Vec<String> = {
            let mut seen = Vec::new();
            for m in &list {
                if !seen.contains(&m.account_id) {
                    seen.push(m.account_id.clone());
                }
            }
            seen
        };
        list.sort_by_key(|m| {
            (
                accounts
                    .iter()
                    .position(|a| *a == m.account_id)
                    .unwrap_or(usize::MAX),
                role_order(m.role),
            )
        });
        Ok(list)
    }

    pub fn mailboxes_for(&self, account_id: &str) -> AppResult<Vec<MailboxInfo>> {
        self.with(|c| {
            let mut st = c.prepare_cached(&format!(
                "{SELECT} WHERE b.account_id = ?1 GROUP BY b.id ORDER BY b.path"
            ))?;
            let rows = st.query_map([account_id], row_to_info)?;
            rows.collect()
        })
    }

    pub fn mailbox(&self, id: &str) -> AppResult<Option<MailboxInfo>> {
        self.with(|c| {
            c.query_row(
                &format!("{SELECT} WHERE b.id = ?1 GROUP BY b.id"),
                [id],
                row_to_info,
            )
            .optional()
        })
    }

    /// Hesabın verilen roldeki ilk kutusu.
    pub fn mailbox_by_role(
        &self,
        account_id: &str,
        role: MailboxRole,
    ) -> AppResult<Option<MailboxInfo>> {
        self.with(|c| {
            c.query_row(
                &format!("{SELECT} WHERE b.account_id = ?1 AND b.role = ?2 GROUP BY b.id ORDER BY b.path LIMIT 1"),
                params![account_id, role_str(role)],
                row_to_info,
            )
            .optional()
        })
    }

    pub fn set_mailbox_sync_state(
        &self,
        id: &str,
        uidvalidity: Option<u32>,
        uidnext: Option<u32>,
        highest_uid: u32,
    ) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "UPDATE mailboxes SET uidvalidity = ?2, uidnext = ?3, highest_uid = ?4 WHERE id = ?1",
                params![id, uidvalidity.map(i64::from), uidnext.map(i64::from), i64::from(highest_uid)],
            )?;
            Ok(())
        })
    }

    /// Sunucuda artık olmayan kutuları (ve postalarını) siler.
    pub fn remove_mailboxes_except(
        &self,
        account_id: &str,
        keep_paths: &[String],
    ) -> AppResult<Vec<String>> {
        let existing = self.mailboxes_for(account_id)?;
        let gone: Vec<String> = existing
            .into_iter()
            .filter(|m| !keep_paths.contains(&m.mailbox.path))
            .map(|m| m.mailbox.id)
            .collect();
        self.with_tx(|tx| {
            for id in &gone {
                tx.execute("DELETE FROM mailboxes WHERE id = ?1", [id])?;
            }
            Ok(())
        })?;
        Ok(gone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::store_with_account;

    #[test]
    fn kutu_ekle_ve_sirala() {
        let s = store_with_account("a");
        s.upsert_mailbox("a", "Trash", "Trash", MailboxRole::Trash)
            .unwrap();
        s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox)
            .unwrap();
        let id = s
            .upsert_mailbox("a", "Sent", "Sent", MailboxRole::Other)
            .unwrap();
        s.upsert_mailbox("a", "Sent", "Gönderilenler", MailboxRole::Sent)
            .unwrap();
        assert_eq!(id, "a:Sent");
        let list = s.mailboxes().unwrap();
        let roles: Vec<_> = list.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![MailboxRole::Inbox, MailboxRole::Sent, MailboxRole::Trash]
        );
        assert_eq!(list[1].name, "Gönderilenler");
        assert_eq!(list[0].total, 0);

        s.set_mailbox_sync_state("a:INBOX", Some(7), Some(100), 99)
            .unwrap();
        let inbox = s.mailbox_by_role("a", MailboxRole::Inbox).unwrap().unwrap();
        assert_eq!(
            (inbox.uidvalidity, inbox.uidnext, inbox.highest_uid),
            (Some(7), Some(100), 99)
        );

        let gone = s
            .remove_mailboxes_except("a", &["INBOX".into(), "Sent".into()])
            .unwrap();
        assert_eq!(gone, vec!["a:Trash".to_string()]);
        assert_eq!(s.mailboxes_for("a").unwrap().len(), 2);
    }
}
