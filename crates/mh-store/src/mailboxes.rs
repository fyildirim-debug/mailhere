//! Posta kutuları ve kutu başına eşitleme durumu.

use mh_core::{AppError, AppResult, ErrorCode, Mailbox, MailboxRole};
use rusqlite::{params, OptionalExtension, Row, Transaction};

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
    /// Hedef olarak kullanılır ama eşitlenmez (Gmail "Tüm Postalar").
    pub nosync: bool,
}

/// Yeni rol kutularının açılacağı kişisel IMAP ad alanı.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Namespace {
    /// Kutu adının önüne gelir ("" ya da "INBOX.").
    pub prefix: String,
    pub delimiter: Option<String>,
}

/// `b.hidden` -1 ("henüz karar verilmedi", önceki sürümden) görünür okunur.
const SELECT: &str = "SELECT b.id, b.account_id, b.path, b.name, b.role,
        COALESCE(SUM(m.unread), 0), COUNT(m.id), b.uidvalidity, b.uidnext, b.highest_uid, b.nosync,
        b.parent_id, b.hidden > 0
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
            parent_id: r.get(11)?,
            hidden: r.get(12)?,
        },
        uidvalidity: r.get::<_, Option<i64>>(7)?.map(|v| v as u32),
        uidnext: r.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        highest_uid: r.get::<_, i64>(9)? as u32,
        nosync: r.get(10)?,
    })
}

fn not_found() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Klasör bulunamadı; sunucuda silinmiş ya da yeniden adlandırılmış olabilir.",
    )
}

/// Kutunun etkin rolü (işlem içinde): kullanıcının atadığı rol; yoksa
/// sunucunun rolü, ama hesapta bu rolü kullanıcı başka bir kutuya verdiyse
/// sıradan klasör.
fn effective_role(
    tx: &Transaction<'_>,
    account_id: &str,
    id: &str,
    auto: MailboxRole,
) -> rusqlite::Result<MailboxRole> {
    let own: Option<String> = tx
        .query_row("SELECT role_user FROM mailboxes WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    if let Some(r) = own {
        return Ok(role_from(&r));
    }
    if auto == MailboxRole::Other {
        return Ok(auto);
    }
    let taken: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM mailboxes WHERE account_id = ?1 AND id <> ?2 AND role_user = ?3)",
        params![account_id, id, role_str(auto)],
        |r| r.get(0),
    )?;
    Ok(if taken { MailboxRole::Other } else { auto })
}

impl Store {
    /// Kutuyu ekler ya da adını, rolünü ve üst klasörünü günceller; kimliğini
    /// döner. `role` sunucudan bulunan (otomatik) roldür; kullanıcı bu kutuya
    /// elle rol atadıysa o, bu rolü başka bir kutuya atadıysa "diğer" yazılır.
    /// `hidden` yalnız ilk eklemede (ve önceki sürümden kalan, kararsız
    /// kutularda) kullanılır; sonra kullanıcının seçimine hiç dokunulmaz. Rol
    /// kutularının üst klasörü olmaz (`parent_id` NULL).
    pub fn upsert_mailbox(
        &self,
        account_id: &str,
        path: &str,
        name: &str,
        role: MailboxRole,
        parent_id: Option<&str>,
        hidden: bool,
    ) -> AppResult<String> {
        let id = mailbox_id(account_id, path);
        self.with_tx(|tx| {
            let role = effective_role(tx, account_id, &id, role)?;
            let parent = parent_id.filter(|p| role == MailboxRole::Other && *p != id);
            tx.execute(
                "INSERT INTO mailboxes (id, account_id, path, name, role, parent_id, hidden)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, role = excluded.role,
                   parent_id = excluded.parent_id,
                   hidden = CASE WHEN mailboxes.hidden < 0 THEN excluded.hidden ELSE mailboxes.hidden END",
                params![id, account_id, path, name, role_str(role), parent, hidden],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    /// Kutuyu kenar çubuğunda gizler ya da gösterir. Gelen kutusu gizlenemez.
    /// Güncel kutuyu döner.
    pub fn set_mailbox_hidden(&self, id: &str, hidden: bool) -> AppResult<MailboxInfo> {
        let info = self.mailbox(id)?.ok_or_else(not_found)?;
        if hidden && info.mailbox.role == MailboxRole::Inbox {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Gelen kutusu gizlenemez.",
            ));
        }
        self.with(|c| {
            c.execute(
                "UPDATE mailboxes SET hidden = ?2 WHERE id = ?1",
                params![id, hidden],
            )
        })?;
        self.mailbox(id)?.ok_or_else(not_found)
    }

    /// Kutuya elle rol atar (Gönderilenler, Taslaklar, Arşiv, Çöp, İstenmeyen).
    /// Hesapta bu rolü taşıyan diğer kutular sıradan klasöre döner ve sonraki
    /// eşitlemelerde de öyle kalır. Etkilenen kutuları döner.
    pub fn set_mailbox_role(&self, id: &str, role: MailboxRole) -> AppResult<Vec<String>> {
        if !role.is_special() {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Bir klasöre yalnız Gönderilenler, Taslaklar, Arşiv, Çöp ya da İstenmeyen görevi atanabilir.",
            ));
        }
        let info = self.mailbox(id)?.ok_or_else(not_found)?;
        if info.mailbox.role == MailboxRole::Inbox {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Gelen kutusunun görevi değiştirilemez.",
            ));
        }
        let account = info.mailbox.account_id;
        let r = role_str(role);
        self.with_tx(|tx| {
            let mut touched: Vec<String> = {
                let mut st = tx.prepare(
                    "SELECT id FROM mailboxes WHERE account_id = ?1 AND id <> ?2 AND role = ?3",
                )?;
                let rows = st.query_map(params![account, id, r], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            tx.execute(
                "UPDATE mailboxes SET role = 'other', role_user = 'other'
                 WHERE account_id = ?1 AND id <> ?2 AND role = ?3",
                params![account, id, r],
            )?;
            tx.execute(
                "UPDATE mailboxes SET role = ?2, role_user = ?2, parent_id = NULL WHERE id = ?1",
                params![id, r],
            )?;
            touched.push(id.to_owned());
            touched.sort();
            Ok(touched)
        })
    }

    /// Hesabın elle atanan rollerini kaldırır. Etkin roller bir sonraki
    /// eşitlemede sunucudan yeniden bulunur. Elle rolü olan kutuları döner.
    pub fn reset_mailbox_roles(&self, account_id: &str) -> AppResult<Vec<String>> {
        self.with_tx(|tx| {
            let ids: Vec<String> = {
                let mut st = tx.prepare(
                    "SELECT id FROM mailboxes WHERE account_id = ?1 AND role_user IS NOT NULL ORDER BY id",
                )?;
                let rows = st.query_map([account_id], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            tx.execute(
                "UPDATE mailboxes SET role_user = NULL WHERE account_id = ?1",
                [account_id],
            )?;
            Ok(ids)
        })
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

    /// Kutunun eşitlenip eşitlenmeyeceğini kalıcı yazar (LIST sonucundan).
    pub fn set_mailbox_nosync(&self, id: &str, nosync: bool) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "UPDATE mailboxes SET nosync = ?2 WHERE id = ?1",
                params![id, nosync],
            )?;
            Ok(())
        })
    }

    /// Hesabın kişisel ad alanı; henüz bilinmiyorsa `None`.
    pub fn namespace(&self, account_id: &str) -> AppResult<Option<Namespace>> {
        self.with(|c| {
            c.query_row(
                "SELECT prefix, delimiter FROM imap_namespace WHERE account_id = ?1",
                [account_id],
                |r| {
                    Ok(Namespace {
                        prefix: r.get(0)?,
                        delimiter: r.get(1)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn set_namespace(&self, account_id: &str, ns: &Namespace) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO imap_namespace (account_id, prefix, delimiter) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id) DO UPDATE SET prefix = excluded.prefix, delimiter = excluded.delimiter",
                params![account_id, ns.prefix, ns.delimiter],
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
                tx.execute(
                    "UPDATE mailboxes SET parent_id = NULL WHERE parent_id = ?1",
                    [id],
                )?;
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
        s.upsert_mailbox("a", "Trash", "Trash", MailboxRole::Trash, None, false)
            .unwrap();
        s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox, None, false)
            .unwrap();
        let id = s
            .upsert_mailbox("a", "Sent", "Sent", MailboxRole::Other, None, false)
            .unwrap();
        s.upsert_mailbox("a", "Sent", "Gönderilenler", MailboxRole::Sent, None, false)
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

    #[test]
    fn eslenmeyen_kutu_ve_ad_alani_kalici() {
        let s = store_with_account("a");
        let id = s
            .upsert_mailbox(
                "a",
                "[Gmail]/All Mail",
                "All Mail",
                MailboxRole::Archive,
                None,
                false,
            )
            .unwrap();
        assert!(!s.mailbox(&id).unwrap().unwrap().nosync);
        s.set_mailbox_nosync(&id, true).unwrap();
        // Yeniden LIST (ad/rol güncellemesi) bayrağı silmez.
        s.upsert_mailbox(
            "a",
            "[Gmail]/All Mail",
            "Tüm Postalar",
            MailboxRole::Archive,
            None,
            false,
        )
        .unwrap();
        let info = s
            .mailbox_by_role("a", MailboxRole::Archive)
            .unwrap()
            .unwrap();
        assert!(info.nosync);

        assert_eq!(s.namespace("a").unwrap(), None);
        let ns = Namespace {
            prefix: "INBOX.".into(),
            delimiter: Some(".".into()),
        };
        s.set_namespace("a", &ns).unwrap();
        s.set_namespace("a", &ns).unwrap();
        assert_eq!(s.namespace("a").unwrap(), Some(ns));
        s.remove_account(&"a".to_string()).unwrap();
        assert_eq!(s.namespace("a").unwrap(), None);
    }

    fn info(s: &Store, id: &str) -> Mailbox {
        s.mailbox(id).unwrap().unwrap().mailbox
    }

    #[test]
    fn ust_klasor_ve_ilk_eklemede_gizlilik() {
        let s = store_with_account("a");
        let parent = s
            .upsert_mailbox("a", "Projeler", "Projeler", MailboxRole::Other, None, false)
            .unwrap();
        let child = s
            .upsert_mailbox(
                "a",
                "Projeler/2024",
                "2024",
                MailboxRole::Other,
                Some(&parent),
                true,
            )
            .unwrap();
        let c = info(&s, &child);
        assert_eq!(c.parent_id.as_deref(), Some("a:Projeler"));
        assert!(c.hidden, "ilk eklemede varsayılan gizli");
        assert!(!info(&s, &parent).hidden);

        // Kullanıcı gösterdi: sonraki listeleme varsayılanı yeniden yazmaz.
        s.set_mailbox_hidden(&child, false).unwrap();
        s.upsert_mailbox(
            "a",
            "Projeler/2024",
            "2024",
            MailboxRole::Other,
            Some(&parent),
            true,
        )
        .unwrap();
        assert!(!info(&s, &child).hidden);
        // Kullanıcı gizledi: görünür varsayılan da ezmez.
        s.set_mailbox_hidden(&parent, true).unwrap();
        s.upsert_mailbox("a", "Projeler", "Projeler", MailboxRole::Other, None, false)
            .unwrap();
        assert!(info(&s, &parent).hidden);

        // Üst klasör sunucudan silinince alt klasör köke çıkar.
        s.remove_mailboxes_except("a", &["Projeler/2024".into()])
            .unwrap();
        assert_eq!(info(&s, &child).parent_id, None);

        // Rol kutusunun üst klasörü olmaz.
        let sent = s
            .upsert_mailbox(
                "a",
                "Projeler/Giden",
                "Giden",
                MailboxRole::Sent,
                Some(&child),
                false,
            )
            .unwrap();
        assert_eq!(info(&s, &sent).parent_id, None);
    }

    #[test]
    fn gelen_kutusu_gizlenemez() {
        let s = store_with_account("a");
        let inbox = s
            .upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox, None, false)
            .unwrap();
        let e = s.set_mailbox_hidden(&inbox, true).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert_eq!(e.message, "Gelen kutusu gizlenemez.");
        assert!(!info(&s, &inbox).hidden);
        // Göstermek sorun değil.
        assert!(!s.set_mailbox_hidden(&inbox, false).unwrap().mailbox.hidden);
        let junk = s
            .upsert_mailbox("a", "Spam", "Spam", MailboxRole::Junk, None, false)
            .unwrap();
        assert!(s.set_mailbox_hidden(&junk, true).unwrap().mailbox.hidden);
        assert_eq!(
            s.set_mailbox_hidden("a:yok", true).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    /// Sunucunun gördüğü haliyle bir LIST turu.
    fn list(s: &Store) {
        for (path, role) in [
            ("INBOX", MailboxRole::Inbox),
            ("Sent", MailboxRole::Sent),
            ("Gonderilen-Eski", MailboxRole::Other),
            ("Junk", MailboxRole::Junk),
        ] {
            s.upsert_mailbox("a", path, path, role, None, false)
                .unwrap();
        }
    }

    fn role(s: &Store, path: &str) -> MailboxRole {
        info(s, &mailbox_id("a", path)).role
    }

    #[test]
    fn elle_rol_atama_cakisma_ve_sifirlama() {
        let s = store_with_account("a");
        let other = store_with_account("b");
        list(&s);
        other
            .upsert_mailbox("b", "Sent", "Sent", MailboxRole::Sent, None, false)
            .unwrap();

        // Rol dışı ve gelen kutusu reddedilir.
        for bad in [MailboxRole::Inbox, MailboxRole::Other] {
            let e = s.set_mailbox_role("a:Gonderilen-Eski", bad).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid);
        }
        let e = s
            .set_mailbox_role("a:INBOX", MailboxRole::Sent)
            .unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::Invalid, "Gelen kutusunun görevi değiştirilemez.")
        );
        assert_eq!(
            s.set_mailbox_role("a:yok", MailboxRole::Sent)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        // Atama: önceki sahibi sıradan klasöre döner.
        let touched = s
            .set_mailbox_role("a:Gonderilen-Eski", MailboxRole::Sent)
            .unwrap();
        assert_eq!(touched, vec!["a:Gonderilen-Eski", "a:Sent"]);
        assert_eq!(role(&s, "Gonderilen-Eski"), MailboxRole::Sent);
        assert_eq!(role(&s, "Sent"), MailboxRole::Other);
        assert_eq!(
            s.mailbox_by_role("a", MailboxRole::Sent)
                .unwrap()
                .unwrap()
                .mailbox
                .path,
            "Gonderilen-Eski"
        );
        // Başka hesap etkilenmez.
        assert_eq!(
            other.mailbox("b:Sent").unwrap().unwrap().mailbox.role,
            MailboxRole::Sent
        );

        // Sonraki eşitleme kullanıcı seçimini korur.
        list(&s);
        assert_eq!(role(&s, "Gonderilen-Eski"), MailboxRole::Sent);
        assert_eq!(role(&s, "Sent"), MailboxRole::Other);
        assert_eq!(role(&s, "Junk"), MailboxRole::Junk);

        // Aynı rol başka kutuya: önceki elle atanan da sıradan klasöre döner.
        s.set_mailbox_role("a:Junk", MailboxRole::Sent).unwrap();
        list(&s);
        assert_eq!(role(&s, "Junk"), MailboxRole::Sent);
        assert_eq!(role(&s, "Gonderilen-Eski"), MailboxRole::Other);
        assert_eq!(role(&s, "Sent"), MailboxRole::Other);

        // Sıfırlama: sonraki eşitlemede sunucunun rolleri geri gelir.
        let reset = s.reset_mailbox_roles("a").unwrap();
        assert_eq!(reset, vec!["a:Gonderilen-Eski", "a:Junk", "a:Sent"]);
        list(&s);
        assert_eq!(role(&s, "Sent"), MailboxRole::Sent);
        assert_eq!(role(&s, "Gonderilen-Eski"), MailboxRole::Other);
        assert_eq!(role(&s, "Junk"), MailboxRole::Junk);
        assert!(s.reset_mailbox_roles("a").unwrap().is_empty());
    }

    #[test]
    fn rol_atanan_alt_klasor_koke_cikar() {
        let s = store_with_account("a");
        let p = s
            .upsert_mailbox("a", "Is", "Is", MailboxRole::Other, None, false)
            .unwrap();
        let c = s
            .upsert_mailbox(
                "a",
                "Is/Arsiv",
                "Arsiv",
                MailboxRole::Other,
                Some(&p),
                false,
            )
            .unwrap();
        s.set_mailbox_role(&c, MailboxRole::Archive).unwrap();
        assert_eq!(info(&s, &c).parent_id, None);
        s.upsert_mailbox(
            "a",
            "Is/Arsiv",
            "Arsiv",
            MailboxRole::Other,
            Some(&p),
            false,
        )
        .unwrap();
        let m = info(&s, &c);
        assert_eq!((m.role, m.parent_id), (MailboxRole::Archive, None));
    }
}
