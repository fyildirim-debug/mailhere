//! Giden kutusu: geri alma süresi ve zamanlanmış gönderim.

use mh_core::{AppResult, Millis, OutgoingMessage};
use rusqlite::{params, OptionalExtension};

use crate::codec::{from_json, to_json};
use crate::Store;

/// Gönderilmeyi bekleyen posta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub id: String,
    pub account_id: String,
    pub send_at: Millis,
    pub msg: OutgoingMessage,
    pub attempts: u32,
}

impl Store {
    pub fn outbox_add(
        &self,
        id: &str,
        msg: &OutgoingMessage,
        send_at: Millis,
        now: Millis,
    ) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO outbox (id, account_id, send_at, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, msg.account_id, send_at, to_json(msg), now],
            )?;
            Ok(())
        })
    }

    /// Henüz gönderime alınmamışsa siler; silindiyse `true` (geri alma).
    pub fn outbox_cancel(&self, id: &str) -> AppResult<bool> {
        self.with(|c| Ok(c.execute("DELETE FROM outbox WHERE id = ?1 AND claimed = 0", [id])? > 0))
    }

    /// Gönderimi biten satırı siler.
    pub fn outbox_remove(&self, id: &str) -> AppResult<()> {
        self.with(|c| {
            c.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Zamanı gelmiş postaları gönderim için sahiplenir; geri alma artık
    /// mümkün değildir.
    pub fn outbox_claim_due(&self, now: Millis) -> AppResult<Vec<OutboxRow>> {
        self.with_tx(|tx| {
            let rows = {
                let mut st = tx.prepare_cached(
                    "SELECT id, account_id, send_at, payload, attempts FROM outbox
                     WHERE claimed = 0 AND send_at <= ?1 ORDER BY send_at",
                )?;
                let rows = st.query_map([now], |r| {
                    Ok(OutboxRow {
                        id: r.get(0)?,
                        account_id: r.get(1)?,
                        send_at: r.get(2)?,
                        msg: from_json(3, &r.get::<_, String>(3)?)?,
                        attempts: r.get::<_, i64>(4)? as u32,
                    })
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for r in &rows {
                tx.execute("UPDATE outbox SET claimed = 1 WHERE id = ?1", [&r.id])?;
            }
            Ok(rows)
        })
    }

    /// Başarısız gönderimi yeniden zamanlar.
    pub fn outbox_retry(&self, id: &str, send_at: Millis, error: &str) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "UPDATE outbox SET claimed = 0, attempts = attempts + 1, send_at = ?2, last_error = ?3 WHERE id = ?1",
                params![id, send_at, error],
            )?;
            Ok(())
        })
    }

    /// Sıradaki gönderim anı.
    pub fn outbox_next_due(&self) -> AppResult<Option<Millis>> {
        self.with(|c| {
            c.query_row(
                "SELECT MIN(send_at) FROM outbox WHERE claimed = 0",
                [],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(Option::flatten)
        })
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{Address, OutgoingMessage};

    use crate::testutil::store_with_account;

    fn out() -> OutgoingMessage {
        OutgoingMessage {
            account_id: "a".into(),
            to: vec![Address::new(Some("Ayşe"), "ayse@eposta.com")],
            cc: vec![],
            bcc: vec![],
            subject: "Selam".into(),
            body_text: "Merhaba".into(),
            in_reply_to: None,
            attachments: vec![],
            send_at: None,
            forward_from: None,
            draft_id: None,
        }
    }

    #[test]
    fn geri_alma_ve_sahiplenme() {
        let s = store_with_account("a");
        s.outbox_add("o1", &out(), 100, 0).unwrap();
        s.outbox_add("o2", &out(), 200, 0).unwrap();
        assert_eq!(s.outbox_next_due().unwrap(), Some(100));
        assert!(s.outbox_claim_due(50).unwrap().is_empty());
        assert!(s.outbox_cancel("o1").unwrap());
        assert!(!s.outbox_cancel("o1").unwrap());

        let due = s.outbox_claim_due(250).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].msg, out());
        assert!(
            !s.outbox_cancel("o2").unwrap(),
            "gönderimdeki posta geri alınamaz"
        );
        assert!(s.outbox_claim_due(250).unwrap().is_empty());
        s.outbox_retry("o2", 400, "ağ").unwrap();
        assert_eq!(s.outbox_next_due().unwrap(), Some(400));
        let due = s.outbox_claim_due(400).unwrap();
        assert_eq!(due[0].attempts, 1);
        s.outbox_remove("o2").unwrap();
        assert_eq!(s.outbox_next_due().unwrap(), None);
    }
}
