//! Konuşma görünümü: bir postanın konuşmasındaki tüm iletiler.

use mh_core::{AppResult, MessageId, MessageSummary};
use rusqlite::{params, OptionalExtension};

use crate::messages::{row_to_summary, SUMMARY_COLS, SUMMARY_LEN};
use crate::Store;

/// Bir konuşmadan en çok bu kadar ileti gösterilir (en yeniler).
pub const THREAD_LIMIT: usize = 100;

impl Store {
    /// `id` postasının konuşmasındaki iletiler (kendisi dahil), eskiden
    /// yeniye; çöp ve gereksizdekiler hariç (posta kendisi oradaysa yine
    /// listede). Aynı postanın başka kutudaki kopyaları (aynı Message-ID)
    /// tek gösterilir; kopyalardan seçili olan, yoksa ilk kaydedilen kalır.
    /// Konuşma `limit`
    /// iletiden uzunsa en yeni `limit` ileti döner. Posta yoksa boş liste.
    pub fn thread_messages(&self, id: MessageId, limit: usize) -> AppResult<Vec<MessageSummary>> {
        let limit = limit.clamp(1, THREAD_LIMIT);
        self.with(|c| {
            let Some((account, thread)) = c
                .query_row(
                    "SELECT account_id, thread_id FROM messages WHERE id = ?1",
                    [id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?
            else {
                return Ok(Vec::new());
            };
            let mut st = c.prepare_cached(&format!(
                "SELECT {SUMMARY_COLS}, m.message_id_header
                 FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE m.account_id = ?1 AND m.thread_id = ?2
                   AND (b.role NOT IN ('trash', 'junk') OR m.id = ?3)
                 ORDER BY m.date DESC, m.id DESC LIMIT ?4"
            ))?;
            let rows = st.query_map(params![account, thread, id, limit as i64], |r| {
                Ok((row_to_summary(r)?, r.get::<_, Option<String>>(SUMMARY_LEN)?))
            })?;
            // Yeniden eskiye gelen satırlar; kopyalar Message-ID ile birleşir.
            let mut out: Vec<(MessageSummary, Option<String>)> = Vec::new();
            for row in rows {
                let (summary, mid) = row?;
                let dup = mid
                    .as_deref()
                    .and_then(|m| out.iter().position(|(_, x)| x.as_deref() == Some(m)));
                match dup {
                    // Kopyalardan seçili olan, yoksa ilk kaydedilen (küçük kimlik) kalır.
                    Some(i)
                        if summary.id == id || (out[i].0.id != id && summary.id < out[i].0.id) =>
                    {
                        out[i] = (summary, mid);
                    }
                    Some(_) => {}
                    None => out.push((summary, mid)),
                }
            }
            out.reverse();
            Ok(out.into_iter().map(|(s, _)| s).collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use mh_core::MailboxRole;

    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::store_with_account;

    fn setup() -> Store {
        let s = store_with_account("a");
        for (path, role) in [
            ("INBOX", MailboxRole::Inbox),
            ("Sent", MailboxRole::Sent),
            ("Archive", MailboxRole::Archive),
            ("Trash", MailboxRole::Trash),
        ] {
            s.upsert_mailbox("a", path, path, role, None, false)
                .unwrap();
        }
        s
    }

    fn chained(
        s: &Store,
        mailbox: &str,
        subject: &str,
        date: i64,
        mid: &str,
        parent: Option<&str>,
    ) -> MessageId {
        let mut m = msg("a", mailbox, subject, date);
        m.message_id_header = Some(mid.into());
        if let Some(p) = parent {
            m.in_reply_to = Some(p.into());
            m.references = vec![p.into()];
        }
        s.upsert_message(&m).unwrap().id
    }

    #[test]
    fn konusma_eskiden_yeniye_ve_kopyasiz() {
        let s = setup();
        let root = chained(&s, "a:Sent", "Plan", 100, "k@o", None);
        let r1 = chained(&s, "a:INBOX", "Re: Plan", 200, "y1@o", Some("k@o"));
        // Aynı yanıtın arşivdeki kopyası ve çöpteki bir ileti.
        let r1_copy = chained(&s, "a:Archive", "Re: Plan", 200, "y1@o", Some("k@o"));
        let trashed = chained(&s, "a:Trash", "Re: Plan", 250, "y2@o", Some("k@o"));
        let r2 = chained(&s, "a:INBOX", "Re: Plan", 300, "y3@o", Some("k@o"));
        let other = chained(&s, "a:INBOX", "Başka", 400, "b@o", None);

        let ids: Vec<MessageId> = s
            .thread_messages(r2, 100)
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec![root, r1, r2], "çöp ve kopya yok, eskiden yeniye");
        // Kopyalardan seçili olan gösterilir.
        let ids: Vec<MessageId> = s
            .thread_messages(r1_copy, 100)
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec![root, r1_copy, r2]);
        // Çöpteki posta seçiliyse kendisi listede kalır.
        let ids: Vec<MessageId> = s
            .thread_messages(trashed, 100)
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec![root, r1, trashed, r2]);
        // Tek iletilik konuşma ve olmayan posta.
        assert_eq!(s.thread_messages(other, 100).unwrap().len(), 1);
        assert!(s.thread_messages(9_999, 100).unwrap().is_empty());
    }

    #[test]
    fn uzun_konusmada_en_yeniler() {
        let s = setup();
        let mut last = 0;
        for i in 0..150 {
            let parent = (i > 0).then(|| "k0@o".to_owned());
            last = chained(
                &s,
                "a:INBOX",
                "Re: Uzun",
                1_000 + i,
                &format!("k{i}@o"),
                parent.as_deref(),
            );
        }
        let list = s.thread_messages(last, 100).unwrap();
        assert_eq!(list.len(), THREAD_LIMIT);
        assert_eq!(list.last().unwrap().id, last);
        assert!(list.windows(2).all(|w| w[0].date <= w[1].date));
        assert_eq!(list[0].date, 1_000 + 50, "en eski 50 ileti düşer");
        assert_eq!(
            s.thread_messages(last, 0).unwrap().len(),
            1,
            "sınır en az 1"
        );
    }
}
