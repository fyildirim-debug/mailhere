//! Yanıt hatırlatmaları: gönderilmiş bir postaya "yanıt gelmezse hatırlat"
//! kaydı.
//!
//! Kayıt, Gönderilenler'deki yerel kopyanın satırına bağlıdır (posta
//! silinince düşer). Süre dolunca (`due_at <= şimdi`) kayıt "ateşlenir"
//! (`fired_at`) ve posta gelen kutusunun üstünde "Yanıt bekleniyor"
//! işaretiyle görünür; konuşmaya kendi adreslerimiz dışından bir yanıt
//! gelirse kayıt kendiliğinden kapanır (silinir).

use mh_core::{
    AppError, AppResult, Category, ErrorCode, FollowUp, MailboxRole, MessageId, MessageSummary,
    Millis,
};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, OptionalExtension};

use crate::codec::{addr_list, category_str, from_json, role_from};
use crate::messages::{row_to_summary, SUMMARY_COLS};
use crate::Store;

/// Hatırlatma kurulmak istenen postanın yeri.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    account_id: String,
    thread_id: String,
    message_id_header: Option<String>,
    role: MailboxRole,
}

/// Açık bir hatırlatmanın yanıt denetimi için gereken alanları.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Open {
    message_id: MessageId,
    account_id: String,
    thread_id: String,
    /// Gönderilmiş postanın tarihi; yanıt bundan sonra gelmiş olmalı.
    sent_at: Millis,
}

fn not_found() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Posta bulunamadı; silinmiş ya da taşınmış olabilir.",
    )
}

impl Store {
    /// Hatırlatma kurar (`at` verilmişse; varsa yeniden kurulur ve ateşlenme
    /// sıfırlanır) ya da kaldırır (`None`). Yalnız Gönderilenler'deki
    /// postalara kurulabilir.
    pub fn followup_set(&self, id: MessageId, at: Option<Millis>, now: Millis) -> AppResult<()> {
        let target = self.with(|c| {
            c.query_row(
                "SELECT m.account_id, m.thread_id, m.message_id_header, b.role
                 FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id WHERE m.id = ?1",
                [id],
                |r| {
                    Ok(Target {
                        account_id: r.get(0)?,
                        thread_id: r.get(1)?,
                        message_id_header: r.get(2)?,
                        role: role_from(&r.get::<_, String>(3)?),
                    })
                },
            )
            .optional()
        })?;
        let Some(target) = target else {
            return Err(not_found());
        };
        match at {
            None => self.with(|c| {
                c.execute("DELETE FROM followups WHERE message_id = ?1", [id])?;
                Ok(())
            }),
            Some(at) => {
                if target.role != MailboxRole::Sent {
                    return Err(AppError::new(
                        ErrorCode::Invalid,
                        "Yanıt hatırlatması yalnız gönderilmiş postalara kurulabilir.",
                    ));
                }
                self.with(|c| {
                    c.execute(
                        "INSERT INTO followups (message_id, account_id, message_id_header, thread_id, due_at, created_at, fired_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
                         ON CONFLICT(message_id) DO UPDATE SET due_at = excluded.due_at,
                           created_at = excluded.created_at, fired_at = NULL",
                        params![
                            id,
                            target.account_id,
                            target.message_id_header,
                            target.thread_id,
                            at,
                            now
                        ],
                    )?;
                    Ok(())
                })
            }
        }
    }

    /// Açık hatırlatmalar, hatırlatma anına göre (çöp ve gereksizdeki
    /// postalar hariç).
    pub fn followups_list(&self) -> AppResult<Vec<FollowUp>> {
        self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT f.message_id, f.account_id, f.due_at, f.created_at, m.subject, m.to_addrs
                 FROM followups f JOIN messages m ON m.id = f.message_id
                 JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE b.role NOT IN ('trash', 'junk')
                 ORDER BY f.due_at, f.message_id",
            )?;
            let rows = st.query_map([], |r| {
                Ok(FollowUp {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    at: r.get(2)?,
                    created_at: r.get(3)?,
                    subject: r.get(4)?,
                    to: addr_list(5, &r.get::<_, String>(5)?)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Süresi dolmuş ama henüz ateşlenmemiş hatırlatmaları ateşler ve
    /// postalarının özetlerini döner (bildirim için).
    pub fn followups_fire_due(&self, now: Millis) -> AppResult<Vec<MessageSummary>> {
        self.with_tx(|tx| {
            let ids: Vec<MessageId> = {
                let mut st = tx.prepare_cached(
                    "SELECT f.message_id FROM followups f JOIN messages m ON m.id = f.message_id
                     JOIN mailboxes b ON b.id = m.mailbox_id
                     WHERE f.due_at <= ?1 AND f.fired_at IS NULL AND b.role NOT IN ('trash', 'junk')
                     ORDER BY f.due_at",
                )?;
                let rows = st.query_map([now], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                tx.execute(
                    "UPDATE followups SET fired_at = ?2 WHERE message_id = ?1",
                    params![id, now],
                )?;
                if let Some(s) = tx
                    .query_row(
                        &format!("SELECT {SUMMARY_COLS} FROM messages m WHERE m.id = ?1"),
                        [id],
                        row_to_summary,
                    )
                    .optional()?
                {
                    out.push(s);
                }
            }
            Ok(out)
        })
    }

    /// Süresi dolan (ateşlenmiş) hatırlatmaların postaları, gelen kutusunun
    /// üstüne konmak için; hatırlatma anına göre (en eski önce).
    pub(crate) fn followups_due_summaries(
        &self,
        account_id: Option<&str>,
        category: Option<Category>,
        now: Millis,
    ) -> AppResult<Vec<MessageSummary>> {
        let mut sql = format!(
            "SELECT {SUMMARY_COLS} FROM followups f JOIN messages m ON m.id = f.message_id
             JOIN mailboxes b ON b.id = m.mailbox_id
             WHERE f.due_at <= ? AND b.role NOT IN ('trash', 'junk')"
        );
        let mut p = vec![Value::Integer(now)];
        if let Some(a) = account_id {
            sql += " AND m.account_id = ?";
            p.push(Value::Text(a.to_owned()));
        }
        if let Some(c) = category {
            sql += " AND m.category = ?";
            p.push(Value::Text(category_str(c).into()));
        }
        sql += " ORDER BY f.due_at, f.message_id LIMIT 200";
        self.with(|c| {
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map(params_from_iter(p), row_to_summary)?;
            rows.collect()
        })
    }

    /// Konuşmasına kendi adreslerimiz (`own`, küçük harf) dışından yanıt
    /// gelmiş açık hatırlatmaları kapatır; kapanan postaların kimliklerini döner.
    pub fn followups_close_answered(&self, own: &[String]) -> AppResult<Vec<MessageId>> {
        let open: Vec<Open> = self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT f.message_id, f.account_id, f.thread_id, m.date
                 FROM followups f JOIN messages m ON m.id = f.message_id",
            )?;
            let rows = st.query_map([], |r| {
                Ok(Open {
                    message_id: r.get(0)?,
                    account_id: r.get(1)?,
                    thread_id: r.get(2)?,
                    sent_at: r.get(3)?,
                })
            })?;
            rows.collect()
        })?;
        if open.is_empty() {
            return Ok(Vec::new());
        }
        let is_own = |email: &str| own.contains(&email.trim().to_ascii_lowercase());
        let mut closed = Vec::new();
        for o in open {
            let answered = self.with(|c| {
                let mut st = c.prepare_cached(
                    "SELECT m.from_addr FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                     WHERE m.account_id = ?1 AND m.thread_id = ?2 AND m.id != ?3 AND m.date >= ?4
                       AND b.role != 'drafts' LIMIT 100",
                )?;
                let rows = st.query_map(
                    params![o.account_id, o.thread_id, o.message_id, o.sent_at],
                    |r| from_json::<mh_core::Address>(0, &r.get::<_, String>(0)?),
                )?;
                let mut hit = false;
                for row in rows {
                    if !is_own(&row?.email) {
                        hit = true;
                        break;
                    }
                }
                Ok(hit)
            })?;
            if answered {
                self.with(|c| {
                    c.execute(
                        "DELETE FROM followups WHERE message_id = ?1",
                        [o.message_id],
                    )?;
                    Ok(())
                })?;
                closed.push(o.message_id);
            }
        }
        Ok(closed)
    }

    /// Hatırlatması olan postanın hesabı ve açık hatırlatma anı (yoksa `None`).
    pub fn followup_of(&self, id: MessageId) -> AppResult<Option<Millis>> {
        self.with(|c| {
            c.query_row(
                "SELECT due_at FROM followups WHERE message_id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()
        })
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{Address, ListQuery, View};

    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::store_with_account;
    use crate::NewMessage;

    const ME: &str = "a@ornek.com.tr";

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

    /// Gönderilmiş posta (bizden `to` alıcısına).
    fn sent(subject: &str, mid: &str, date: Millis) -> NewMessage {
        let mut m = msg("a", "a:Sent", subject, date);
        m.from = Address::new(Some("Ben"), ME);
        m.to = vec![Address::new(Some("Kerem Aydın"), "kerem@ornek.com.tr")];
        m.message_id_header = Some(mid.into());
        m.unread = false;
        m
    }

    fn reply(mailbox: &str, from: &str, to_mid: &str, date: Millis) -> NewMessage {
        let mut m = msg("a", mailbox, "Re: Teklif", date);
        m.from = Address::new(None, from);
        m.in_reply_to = Some(to_mid.into());
        m.references = vec![to_mid.into()];
        m.message_id_header = Some(format!("{from}-{date}"));
        m
    }

    fn inbox(s: &Store, now: Millis) -> Vec<MessageSummary> {
        s.list(
            &ListQuery {
                view: View::Inbox,
                account_id: None,
                category: None,
                only: None,
                cursor: None,
                limit: 50,
            },
            now,
        )
        .unwrap()
        .items
    }

    #[test]
    fn kurma_listeleme_ve_kaldirma() {
        let s = setup();
        let id = s.upsert_message(&sent("Teklif", "t1@x", 1_000)).unwrap().id;
        let inbox_id = s
            .upsert_message(&msg("a", "a:INBOX", "Gelen", 1_100))
            .unwrap()
            .id;

        // Gelen kutusundaki postaya kurulamaz; olmayan posta bulunamaz.
        let e = s.followup_set(inbox_id, Some(5_000), 1_200).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert_eq!(
            s.followup_set(99, Some(5_000), 1_200).unwrap_err().code,
            ErrorCode::NotFound
        );

        s.followup_set(id, Some(5_000), 1_200).unwrap();
        let list = s.followups_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].id, list[0].at, list[0].created_at),
            (id, 5_000, 1_200)
        );
        assert_eq!(list[0].subject, "Teklif");
        assert_eq!(list[0].to[0].email, "kerem@ornek.com.tr");
        assert_eq!(s.followup_of(id).unwrap(), Some(5_000));
        assert_eq!(s.summary(id).unwrap().unwrap().follow_up, Some(5_000));
        assert_eq!(s.counts(1_300).unwrap().follow_ups, 1);

        // Süresi gelmeden gelen kutusunda görünmez.
        assert_eq!(
            inbox(&s, 2_000).iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![inbox_id]
        );

        // Yeniden kurma anı değiştirir; kaldırma siler.
        s.followup_set(id, Some(7_000), 1_300).unwrap();
        assert_eq!(s.followup_of(id).unwrap(), Some(7_000));
        s.followup_set(id, None, 1_400).unwrap();
        assert_eq!(s.followup_of(id).unwrap(), None);
        assert!(s.followups_list().unwrap().is_empty());
        assert_eq!(s.summary(id).unwrap().unwrap().follow_up, None);
        // Kaldırma gelen kutusundaki postada da hata vermez.
        s.followup_set(inbox_id, None, 1_400).unwrap();
    }

    #[test]
    fn suresi_dolan_gelen_kutusunun_ustune_duser() {
        let s = setup();
        let id = s.upsert_message(&sent("Teklif", "t1@x", 1_000)).unwrap().id;
        let newer = s
            .upsert_message(&msg("a", "a:INBOX", "Yeni", 9_000))
            .unwrap()
            .id;
        s.followup_set(id, Some(5_000), 1_200).unwrap();

        assert!(s.followups_fire_due(4_999).unwrap().is_empty());
        let fired = s.followups_fire_due(5_000).unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].id, id);
        assert_eq!(fired[0].follow_up, Some(5_000));
        // Bir kez ateşlenir.
        assert!(s.followups_fire_due(6_000).unwrap().is_empty());

        // Daha yeni bir posta olsa da hatırlatma en üstte, işaretiyle.
        let items = inbox(&s, 6_000);
        assert_eq!(
            items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![id, newer]
        );
        assert_eq!(items[0].follow_up, Some(5_000));
        assert_eq!(items[1].follow_up, None);
        // Sonraki sayfalarda ve hızlı süzgeçte yinelenmez.
        let page2 = s
            .list(
                &ListQuery {
                    view: View::Inbox,
                    account_id: None,
                    category: None,
                    only: None,
                    cursor: Some(format!("{}:{}", 9_000, newer)),
                    limit: 50,
                },
                6_000,
            )
            .unwrap();
        assert!(page2.items.is_empty());
        let unread_only = s
            .list(
                &ListQuery {
                    view: View::Inbox,
                    account_id: None,
                    category: None,
                    only: Some(mh_core::ListOnly::Unread),
                    cursor: None,
                    limit: 50,
                },
                6_000,
            )
            .unwrap();
        assert_eq!(
            unread_only.items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![newer]
        );
        // Başka hesap süzülürse görünmez.
        let other = s
            .list(
                &ListQuery {
                    view: View::Inbox,
                    account_id: Some("b".into()),
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 50,
                },
                6_000,
            )
            .unwrap();
        assert!(other.items.is_empty());

        // "Yanıt bekleyenler" görünümü: süresi gelenler önce.
        let later = s.upsert_message(&sent("İkinci", "t2@x", 1_500)).unwrap().id;
        s.followup_set(later, Some(50_000), 1_600).unwrap();
        let view = s
            .list(
                &ListQuery {
                    view: View::Followups,
                    account_id: None,
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 50,
                },
                6_000,
            )
            .unwrap();
        assert_eq!(
            view.items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![id, later]
        );
        assert_eq!(view.next_cursor, None);
        assert_eq!(s.counts(6_000).unwrap().follow_ups, 2);

        // Çöpe giden posta listeden ve sayaçtan düşer; silinince kayıt gider.
        s.move_local_untracked(&[later], "a:Trash").unwrap();
        assert_eq!(s.counts(6_000).unwrap().follow_ups, 1);
        assert_eq!(s.followups_list().unwrap().len(), 1);
        s.delete_messages(&[id]).unwrap();
        assert!(s.followups_list().unwrap().is_empty());
        assert_eq!(s.followup_of(id).unwrap(), None);
    }

    #[test]
    fn baskasindan_yanit_gelince_kapanir() {
        let s = setup();
        let own = vec![ME.to_owned()];
        let id = s.upsert_message(&sent("Teklif", "t1@x", 1_000)).unwrap().id;
        s.followup_set(id, Some(5_000), 1_200).unwrap();
        assert!(s.followups_close_answered(&own).unwrap().is_empty());

        // Kendi ikinci postamız kapatmaz.
        let mut mine = reply("a:Sent", "A@ornek.com.tr", "t1@x", 1_500);
        mine.from = Address::new(Some("Ben"), "A@Ornek.com.tr");
        s.upsert_message(&mine).unwrap();
        assert!(s.followups_close_answered(&own).unwrap().is_empty());
        // Taslak yanıt kapatmaz.
        s.upsert_message(&reply("a:Drafts", "kerem@ornek.com.tr", "t1@x", 1_600))
            .unwrap();
        assert!(s.followups_close_answered(&own).unwrap().is_empty());
        // Konuşmadaki eski bir posta (gönderimden önce) kapatmaz.
        s.upsert_message(&reply("a:INBOX", "kerem@ornek.com.tr", "t1@x", 900))
            .unwrap();
        assert!(s.followups_close_answered(&own).unwrap().is_empty());

        // Karşıdan gelen yanıt kapatır.
        let r = s
            .upsert_message(&reply("a:INBOX", "kerem@ornek.com.tr", "t1@x", 2_000))
            .unwrap()
            .id;
        assert_eq!(
            s.summary(r).unwrap().unwrap().thread_id,
            s.summary(id).unwrap().unwrap().thread_id
        );
        assert_eq!(s.followups_close_answered(&own).unwrap(), vec![id]);
        assert!(s.followups_list().unwrap().is_empty());
        assert_eq!(s.summary(id).unwrap().unwrap().follow_up, None);
    }
}
