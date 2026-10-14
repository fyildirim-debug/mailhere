//! Giden kutusundaki planlanmış gönderimler: liste, hemen gönderme ve
//! taslağa çevirme için gönderimden geri alma.

use mh_core::{AppResult, Millis, OutgoingMessage, ScheduledItem};
use rusqlite::{params, OptionalExtension};

use crate::codec::{from_json, to_json};
use crate::{OutboxRow, Store};

impl Store {
    /// Giden kutusundaki tüm gönderimler, gönderim anına göre (en yakın önce).
    /// İçeriği okunamayan satır listede görünmez.
    pub fn outbox_list(&self) -> AppResult<Vec<ScheduledItem>> {
        let rows = self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT id, account_id, send_at, payload, last_error FROM outbox
                 ORDER BY send_at, created_at, id",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, account_id, send_at, payload, error)| {
                let msg: OutgoingMessage = match serde_json::from_str(&payload) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(error = %e, "giden kutusundaki posta okunamadı");
                        return None;
                    }
                };
                let to = msg.to.into_iter().chain(msg.cc).chain(msg.bcc).collect();
                Some(ScheduledItem {
                    id,
                    account_id,
                    send_at,
                    to,
                    subject: msg.subject,
                    error,
                })
            })
            .collect())
    }

    /// Henüz gönderime alınmamış postanın gönderim anını en geç `now`a çeker.
    /// Böyle bir satır yoksa (gönderiliyor ya da hiç yok) `false`.
    pub fn outbox_send_now(&self, id: &str, now: Millis) -> AppResult<bool> {
        self.with(|c| {
            Ok(c.execute(
                "UPDATE outbox SET send_at = MIN(send_at, ?2) WHERE id = ?1 AND claimed = 0",
                params![id, now],
            )? > 0)
        })
    }

    /// Giden kutusundaki postanın içeriği.
    pub fn outbox_message(&self, id: &str) -> AppResult<Option<OutgoingMessage>> {
        self.with(|c| {
            c.query_row("SELECT payload FROM outbox WHERE id = ?1", [id], |r| {
                from_json(0, &r.get::<_, String>(0)?)
            })
            .optional()
        })
    }

    /// Satırın durumu: yoksa `None`, gönderime alınmışsa `Some(true)`.
    pub fn outbox_claimed(&self, id: &str) -> AppResult<Option<bool>> {
        self.with(|c| {
            c.query_row("SELECT claimed FROM outbox WHERE id = ?1", [id], |r| {
                r.get::<_, bool>(0)
            })
            .optional()
        })
    }

    /// Henüz gönderime alınmamış postayı giden kutusundan çıkarıp döner (geri
    /// alma gibi, aynı işlemde). Gönderiliyorsa ya da yoksa `None`.
    pub fn outbox_take(&self, id: &str) -> AppResult<Option<OutboxRow>> {
        self.with_tx(|tx| {
            let row = tx
                .query_row(
                    "SELECT id, account_id, send_at, payload, attempts FROM outbox
                     WHERE id = ?1 AND claimed = 0",
                    [id],
                    |r| {
                        Ok(OutboxRow {
                            id: r.get(0)?,
                            account_id: r.get(1)?,
                            send_at: r.get(2)?,
                            msg: from_json(3, &r.get::<_, String>(3)?)?,
                            attempts: r.get::<_, i64>(4)? as u32,
                        })
                    },
                )
                .optional()?;
            if row.is_some() {
                tx.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
            }
            Ok(row)
        })
    }

    /// [`Store::outbox_take`] ile alınan satırı geri koyar (taslağa
    /// çevrilemediyse gönderim sürer).
    pub fn outbox_restore(&self, row: &OutboxRow, now: Millis) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR IGNORE INTO outbox (id, account_id, send_at, payload, attempts, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    row.id,
                    row.account_id,
                    row.send_at,
                    to_json(&row.msg),
                    i64::from(row.attempts),
                    now
                ],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{Address, OutgoingMessage};

    use crate::testutil::store_with_account;

    fn out(subject: &str) -> OutgoingMessage {
        OutgoingMessage {
            account_id: "a".into(),
            to: vec![Address::new(Some("Ayşe"), "ayse@eposta.com")],
            cc: vec![Address::new(None, "kerem@ornek.com")],
            bcc: vec![Address::new(None, "gizli@ornek.com")],
            subject: subject.into(),
            body_text: "Merhaba".into(),
            in_reply_to: None,
            attachments: vec![],
            send_at: Some(500),
            forward_from: None,
            draft_id: None,
            follow_up_at: None,
        }
    }

    #[test]
    fn planli_gonderimler() {
        let s = store_with_account("a");
        s.outbox_add("o2", &out("İkinci"), 900, 0).unwrap();
        s.outbox_add("o1", &out("Birinci"), 500, 0).unwrap();
        assert_eq!(s.counts(0).unwrap().scheduled, 2);

        let list = s.outbox_list().unwrap();
        assert_eq!(
            list.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["o1", "o2"]
        );
        assert_eq!(list[0].subject, "Birinci");
        assert_eq!(list[0].account_id, "a");
        assert_eq!(list[0].send_at, 500);
        assert_eq!(list[0].to.len(), 3, "tüm alıcılar");
        assert_eq!(list[0].error, None);

        // Hemen gönder: zaman öne çekilir, ileri atılmaz.
        assert!(s.outbox_send_now("o2", 100).unwrap());
        assert_eq!(s.outbox_next_due().unwrap(), Some(100));
        assert!(s.outbox_send_now("o2", 300).unwrap());
        assert_eq!(s.outbox_next_due().unwrap(), Some(100));
        assert!(!s.outbox_send_now("yok", 100).unwrap());
        assert_eq!(s.outbox_claimed("yok").unwrap(), None);

        // Hata, listede görünür.
        let due = s.outbox_claim_due(100).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(s.outbox_claimed("o2").unwrap(), Some(true));
        assert!(!s.outbox_send_now("o2", 50).unwrap(), "gönderimdeki posta");
        assert!(s.outbox_take("o2").unwrap().is_none(), "gönderimdeki posta");
        s.outbox_retry("o2", 1_000, "Sunucuya bağlanılamadı.")
            .unwrap();
        let list = s.outbox_list().unwrap();
        assert_eq!(list[1].error.as_deref(), Some("Sunucuya bağlanılamadı."));

        // Taslağa çevirme için alınır ve geri konabilir.
        assert_eq!(s.outbox_message("o1").unwrap().unwrap().subject, "Birinci");
        assert!(s.outbox_message("yok").unwrap().is_none());
        let row = s.outbox_take("o1").unwrap().unwrap();
        assert_eq!(row.msg.subject, "Birinci");
        assert!(s.outbox_take("o1").unwrap().is_none());
        assert_eq!(s.counts(0).unwrap().scheduled, 1);
        s.outbox_restore(&row, 0).unwrap();
        assert_eq!(s.outbox_list().unwrap()[0].id, "o1");
        assert_eq!(s.counts(0).unwrap().scheduled, 2);
    }
}
