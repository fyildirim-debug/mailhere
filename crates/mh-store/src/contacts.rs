//! Kişi kartı: bir adresle yazışmanın özeti.
//!
//! Adres karşılaştırması küçük harfe çevrilerek yapılır; değer SQL'e hep
//! parametreyle gider. Çöp, gereksiz ve taslaklar sayılmaz.

use mh_core::{AppResult, ContactInfo, MessageSummary};
use rusqlite::{params, OptionalExtension};

use crate::messages::{row_to_summary, SUMMARY_COLS};
use crate::Store;

/// Kartta listelenen en çok konuşma.
pub const CONTACT_RECENT: usize = 5;

/// Kişi bu postanın göndereni ya da alıcılarından biri mi (SQL koşulu;
/// `?1` küçük harfli adres).
const MATCH: &str = "(lower(json_extract(m.from_addr, '$.email')) = ?1
    OR EXISTS (SELECT 1 FROM json_each(m.to_addrs) t WHERE lower(json_extract(t.value, '$.email')) = ?1)
    OR EXISTS (SELECT 1 FROM json_each(m.cc_addrs) t WHERE lower(json_extract(t.value, '$.email')) = ?1))";

const SKIP_ROLES: &str = "b.role NOT IN ('trash', 'junk', 'drafts')";

impl Store {
    /// Bir kişiyle yazışmanın özeti: toplam posta (aynı postanın kopyaları
    /// bir sayılır), son postanın tarihi, en son görülen görünen ad ve son
    /// `recent` konuşmanın en yeni iletileri (yeniden eskiye).
    pub fn contact(&self, email: &str, recent: usize) -> AppResult<ContactInfo> {
        let email = email.trim().to_ascii_lowercase();
        let recent = recent.clamp(1, 20) as i64;
        self.with(|c| {
            let (total, last_date): (i64, Option<i64>) = c.query_row(
                &format!(
                    "SELECT COUNT(DISTINCT COALESCE(m.message_id_header, 'id:' || m.id)), MAX(m.date)
                     FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                     WHERE {SKIP_ROLES} AND {MATCH}"
                ),
                [&email],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let name: Option<String> = c
                .query_row(
                    "SELECT json_extract(m.from_addr, '$.name') FROM messages m
                     WHERE lower(json_extract(m.from_addr, '$.email')) = ?1
                       AND json_extract(m.from_addr, '$.name') IS NOT NULL
                     ORDER BY m.date DESC, m.id DESC LIMIT 1",
                    [&email],
                    |r| r.get(0),
                )
                .optional()?;
            let mut st = c.prepare_cached(&format!(
                "SELECT {SUMMARY_COLS} FROM messages m WHERE m.id IN (
                    SELECT id FROM (
                        SELECT m.id, ROW_NUMBER() OVER (
                            PARTITION BY m.account_id, m.thread_id ORDER BY m.date DESC, m.id DESC
                        ) AS rn
                        FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                        WHERE {SKIP_ROLES} AND {MATCH}
                    ) WHERE rn = 1)
                 ORDER BY m.date DESC, m.id DESC LIMIT ?2"
            ))?;
            let recent: Vec<MessageSummary> = st
                .query_map(params![email, recent], row_to_summary)?
                .collect::<rusqlite::Result<_>>()?;
            Ok(ContactInfo {
                email,
                name: name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()),
                total: total.max(0) as u32,
                last_date,
                recent,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{Address, MailboxRole};

    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::store_with_account;

    fn setup() -> Store {
        let s = store_with_account("a");
        for (path, role) in [
            ("INBOX", MailboxRole::Inbox),
            ("Sent", MailboxRole::Sent),
            ("Trash", MailboxRole::Trash),
            ("Drafts", MailboxRole::Drafts),
        ] {
            s.upsert_mailbox("a", path, path, role, None, false)
                .unwrap();
        }
        s
    }

    #[test]
    fn kisi_ozeti() {
        let s = setup();
        // Kerem'den iki posta (ikincisi yeni ad ile), bir de ona giden.
        let mut m1 = msg("a", "a:INBOX", "Plan", 100);
        m1.message_id_header = Some("p1@o".into());
        s.upsert_message(&m1).unwrap();
        let mut m2 = msg("a", "a:INBOX", "Bütçe", 300);
        m2.from = Address::new(Some("Kerem A."), "Kerem@ornek.com.tr");
        m2.message_id_header = Some("p2@o".into());
        s.upsert_message(&m2).unwrap();
        let mut sent = msg("a", "a:Sent", "Re: Plan", 200);
        sent.from = Address::new(Some("Ben"), "furkan@ornek.com.tr");
        sent.to = vec![Address::new(None, "kerem@ornek.com.tr")];
        sent.in_reply_to = Some("p1@o".into());
        sent.references = vec!["p1@o".into()];
        s.upsert_message(&sent).unwrap();
        // Kopya (aynı Message-ID, başka kutu) bir sayılır; çöp ve taslak sayılmaz.
        let mut copy = m2.clone();
        copy.mailbox_id = "a:Sent".into();
        s.upsert_message(&copy).unwrap();
        let mut trashed = msg("a", "a:Trash", "Eski", 50);
        trashed.message_id_header = Some("t@o".into());
        s.upsert_message(&trashed).unwrap();
        let mut draft = msg("a", "a:Drafts", "Taslak", 400);
        draft.from = Address::new(None, "furkan@ornek.com.tr");
        draft.to = vec![Address::new(None, "kerem@ornek.com.tr")];
        s.upsert_message(&draft).unwrap();
        // Bilgi (cc) alıcısı olan posta da sayılır.
        let mut cc = msg("a", "a:INBOX", "Duyuru", 350);
        cc.from = Address::new(Some("Ayşe"), "ayse@ornek.com.tr");
        cc.cc = vec![Address::new(None, "KEREM@ornek.com.tr")];
        cc.message_id_header = Some("d@o".into());
        s.upsert_message(&cc).unwrap();

        let c = s.contact(" Kerem@Ornek.com.tr ", 5).unwrap();
        assert_eq!(c.email, "kerem@ornek.com.tr");
        assert_eq!(c.name.as_deref(), Some("Kerem A."), "en son görülen ad");
        assert_eq!(c.total, 4, "p1, yanıt, p2 (kopyasız), duyuru");
        assert_eq!(c.last_date, Some(350));
        // Son konuşmalar: her konuşmanın en yeni iletisi, yeniden eskiye.
        let subjects: Vec<&str> = c.recent.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, vec!["Duyuru", "Bütçe", "Re: Plan"]);
        assert_eq!(s.contact("kerem@ornek.com.tr", 1).unwrap().recent.len(), 1);

        let none = s.contact("yok@ornek.com", 5).unwrap();
        assert_eq!((none.total, none.last_date, none.name), (0, None, None));
        assert!(none.recent.is_empty());
    }
}
