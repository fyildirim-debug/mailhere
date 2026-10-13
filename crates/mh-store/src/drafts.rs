//! Taslaklar: yerel kaydın yerine yazılması ve silinmesi.
//!
//! Taslak, hesabın Taslaklar kutusundaki sıradan bir postadır. Kaydedilen her
//! sürüm yeni bir Message-ID taşır; yerel satır aynı kimlikle yeniden yazılır,
//! sunucudaki eski kopyanın silinmesi eşitleme katmanının işidir.

use mh_core::{AppError, AppResult, ErrorCode, MailboxRole, MessageId};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::messages::{insert_as, row_to_location, MessageLocation, NewMessage, LOCATION_COLS};
use crate::Store;

/// Taslak kaydının sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftPut {
    /// Taslağın (yeni ya da korunan) kimliği.
    pub id: MessageId,
    /// Yerine yazılan ya da başka hesaba taşındığı için silinen önceki
    /// sürümün yeri; sunucudaki kopyası kaldırılmalıdır.
    pub previous: Option<MessageLocation>,
}

fn location_tx(tx: &Transaction<'_>, id: MessageId) -> rusqlite::Result<Option<MessageLocation>> {
    tx.query_row(
        &format!(
            "SELECT {LOCATION_COLS} FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
             WHERE m.id = ?1"
        ),
        [id],
        row_to_location,
    )
    .optional()
}

impl Store {
    /// Taslağı yazar. `replace` aynı hesabın bir taslağıysa satır aynı
    /// kimlikle yeniden yazılır; başka hesabın taslağıysa silinir ve yeni
    /// kimlik verilir. Taslak olmayan bir postaya dokunulmaz.
    pub fn put_draft(&self, m: &NewMessage, replace: Option<MessageId>) -> AppResult<DraftPut> {
        self.with_tx(|tx| {
            let previous = match replace {
                Some(id) => location_tx(tx, id)?.filter(|l| l.role == MailboxRole::Drafts),
                None => None,
            };
            let keep = previous
                .as_ref()
                .filter(|p| p.account_id == m.account_id)
                .map(|p| p.id);
            if let Some(p) = &previous {
                tx.execute("DELETE FROM messages WHERE id = ?1", params![p.id])?;
            }
            let id = insert_as(tx, m, keep)?;
            Ok(DraftPut { id, previous })
        })
    }

    /// Taslağı yerelden siler ve yerini döner (sunucudan kaldırmak için).
    /// Yoksa `None`; taslak değilse hata.
    pub fn take_draft(&self, id: MessageId) -> AppResult<Option<MessageLocation>> {
        let loc = self.with_tx(|tx| {
            let Some(loc) = location_tx(tx, id)? else {
                return Ok(None);
            };
            if loc.role == MailboxRole::Drafts {
                tx.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
            }
            Ok(Some(loc))
        })?;
        match loc {
            Some(l) if l.role != MailboxRole::Drafts => Err(AppError::new(
                ErrorCode::Invalid,
                "Bu posta bir taslak değil; taslak olarak silinemez.",
            )),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::{account, store_with_account};

    fn setup() -> Store {
        let s = store_with_account("a");
        s.insert_account(&account("b")).unwrap();
        for acc in ["a", "b"] {
            s.upsert_mailbox(acc, "INBOX", "INBOX", MailboxRole::Inbox)
                .unwrap();
            s.upsert_mailbox(acc, "Drafts", "Drafts", MailboxRole::Drafts)
                .unwrap();
        }
        s
    }

    fn draft(account: &str, subject: &str, mid: &str) -> NewMessage {
        let mut m = msg(account, &format!("{account}:Drafts"), subject, 10);
        m.message_id_header = Some(mid.into());
        m.unread = false;
        m
    }

    #[test]
    fn taslak_yerine_yazilir_kimlik_korunur() {
        let s = setup();
        let first = s.put_draft(&draft("a", "İlk", "t1@o"), None).unwrap();
        assert_eq!(first.previous, None);

        // Eşitleme ilk sürüme UID verdi.
        let mut seen = draft("a", "İlk", "t1@o");
        seen.uid = Some(5);
        seen.uidvalidity = Some(9);
        assert_eq!(s.upsert_message(&seen).unwrap().id, first.id);

        let second = s
            .put_draft(&draft("a", "İkinci", "t2@o"), Some(first.id))
            .unwrap();
        assert_eq!(second.id, first.id, "aynı kimlik");
        let prev = second.previous.unwrap();
        assert_eq!(
            (
                prev.uid,
                prev.uidvalidity,
                prev.message_id_header.as_deref()
            ),
            (Some(5), Some(9), Some("t1@o"))
        );
        assert_eq!(prev.mailbox_path, "Drafts");
        let now = s.location(first.id).unwrap().unwrap();
        assert_eq!(now.uid, None, "yeni sürüm henüz sunucuda değil");
        assert_eq!(now.message_id_header.as_deref(), Some("t2@o"));
        assert_eq!(s.summary(first.id).unwrap().unwrap().subject, "İkinci");
        assert_eq!(s.counts(0).unwrap().drafts, 1);
        // Arama dizini yeni sürümü bilir.
        assert_eq!(s.search("ikinci", 10).unwrap().len(), 1);
        assert!(s.search("ilk", 10).unwrap().is_empty());
    }

    #[test]
    fn baska_hesaba_tasinan_ve_taslak_olmayan() {
        let s = setup();
        let a = s.put_draft(&draft("a", "Hesap A", "a1@o"), None).unwrap();
        let b = s
            .put_draft(&draft("b", "Hesap B", "b1@o"), Some(a.id))
            .unwrap();
        assert_eq!(b.previous.as_ref().unwrap().account_id, "a");
        let now = s.location(b.id).unwrap().unwrap();
        assert_eq!(
            (now.account_id.as_str(), now.mailbox_id.as_str()),
            ("b", "b:Drafts")
        );
        assert_eq!(s.counts(0).unwrap().drafts, 1, "eski hesaptaki silinir");

        // Gelen kutusundaki bir postanın yerine taslak yazılmaz.
        let inbox = s.upsert_message(&msg("a", "a:INBOX", "Gelen", 1)).unwrap();
        let c = s
            .put_draft(&draft("a", "Yeni", "c1@o"), Some(inbox.id))
            .unwrap();
        assert_ne!(c.id, inbox.id);
        assert_eq!(c.previous, None);
        assert!(s.summary(inbox.id).unwrap().is_some());
        assert_eq!(s.take_draft(inbox.id).unwrap_err().code, ErrorCode::Invalid);
        assert!(s.summary(inbox.id).unwrap().is_some());

        let taken = s.take_draft(c.id).unwrap().unwrap();
        assert_eq!(taken.message_id_header.as_deref(), Some("c1@o"));
        assert!(s.summary(c.id).unwrap().is_none());
        assert_eq!(s.take_draft(c.id).unwrap(), None);
    }

    #[test]
    fn taslagin_yanit_basliklari() {
        let s = setup();
        let mut d = draft("a", "Re: Plan", "t@o");
        d.in_reply_to = Some("kok@o".into());
        d.references = vec!["ilk@o".into(), "kok@o".into()];
        let id = s.put_draft(&d, None).unwrap().id;
        assert_eq!(
            s.reply_refs(id).unwrap(),
            Some((Some("kok@o".into()), vec!["ilk@o".into(), "kok@o".into()]))
        );
        let mut m = msg("a", "a:INBOX", "Plan", 1);
        m.message_id_header = Some("kok@o".into());
        let inbox = s.upsert_message(&m).unwrap().id;
        assert_eq!(
            s.reply_refs(inbox).unwrap(),
            Some((Some("kok@o".into()), vec![]))
        );
    }
}
