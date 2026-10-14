//! JMAP hesaplarının eşitleme durumu ve postaların sunucudaki kimlikleri.
//!
//! JMAP'te posta kimliği kutudan bağımsızdır; yerelde her JMAP postası tek
//! satırdır ve `jmap_messages` tablosuyla sunucu kimliğine bağlanır. Kutunun
//! yerel yolu (`mailboxes.path`) JMAP kutu kimliğidir.

use std::collections::HashMap;

use mh_core::{AppResult, MailboxRole, MessageId};
use mh_mime::fold_tr;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, OptionalExtension, Transaction};

use crate::codec::role_from;
use crate::messages::{insert_as, NewMessage};
use crate::Store;

/// JMAP hesabının yerelde tutulan eşitleme bilgisi.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JmapAccountState {
    /// Sunucuya giriş adı; yoksa hesabın e-posta adresi kullanılır.
    pub username: Option<String>,
    /// Son eşitlenen `Email` durum dizgesi.
    pub email_state: Option<String>,
    /// Son eşitlenen `Mailbox` durum dizgesi.
    pub mailbox_state: Option<String>,
}

/// Sunucu işlemi için bir postanın JMAP yeri.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JmapLocation {
    pub id: MessageId,
    pub account_id: String,
    pub mailbox_id: String,
    /// Yerel kutunun yolu (JMAP kutu kimliği).
    pub mailbox_path: String,
    pub role: MailboxRole,
    /// Sunucudaki posta kimliği; posta henüz sunucuda yoksa `None`.
    pub jmap_id: Option<String>,
    /// Ham postanın blob kimliği (gövde indirmek için).
    pub blob_id: Option<String>,
    pub has_raw: bool,
}

/// JMAP postasının yerel yazımının sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JmapUpsert {
    pub id: MessageId,
    /// Yeni satır mı (bildirim kararı için)?
    pub inserted: bool,
    /// İçeriği değişen kutular; hiçbir şey değişmediyse boş.
    pub touched: Vec<String>,
}

const CHUNK: usize = 400;

fn in_list(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn map_row(
    tx: &Transaction<'_>,
    id: MessageId,
    account_id: &str,
    jmap_id: &str,
    blob_id: Option<&str>,
) -> rusqlite::Result<()> {
    tx.prepare_cached(
        "INSERT INTO jmap_messages (message_id, account_id, jmap_id, blob_id) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(message_id) DO UPDATE SET jmap_id = excluded.jmap_id,
             blob_id = COALESCE(excluded.blob_id, jmap_messages.blob_id)",
    )?
    .execute(params![id, account_id, jmap_id, blob_id])?;
    Ok(())
}

type Existing = (MessageId, String, bool, bool);

fn find_existing(
    tx: &Transaction<'_>,
    m: &NewMessage,
    jmap_id: &str,
) -> rusqlite::Result<Option<Existing>> {
    let row = |r: &rusqlite::Row<'_>| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?));
    let by_id = tx
        .prepare_cached(
            "SELECT m.id, m.mailbox_id, m.unread, m.starred
             FROM jmap_messages j JOIN messages m ON m.id = j.message_id
             WHERE j.account_id = ?1 AND j.jmap_id = ?2",
        )?
        .query_row(params![m.account_id, jmap_id], row)
        .optional()?;
    if by_id.is_some() {
        return Ok(by_id);
    }
    // Aynı kutuda yerelde kurulmuş, sunucuya bağlanmamış kopya (ör. giden
    // kutusunun Gönderilenler kopyası): Message-ID ile sahiplen.
    match m.message_id_header.as_deref() {
        Some(mid) => tx
            .prepare_cached(
                "SELECT m.id, m.mailbox_id, m.unread, m.starred
                 FROM messages m LEFT JOIN jmap_messages j ON j.message_id = m.id
                 WHERE m.account_id = ?1 AND m.message_id_header = ?2 AND m.mailbox_id = ?3
                   AND j.message_id IS NULL
                 ORDER BY m.id LIMIT 1",
            )?
            .query_row(params![m.account_id, mid, m.mailbox_id], row)
            .optional(),
        None => Ok(None),
    }
}

impl Store {
    /// Hesabın JMAP eşitleme bilgisi (kayıt yoksa boş).
    pub fn jmap_account_state(&self, account_id: &str) -> AppResult<JmapAccountState> {
        self.with(|c| {
            c.query_row(
                "SELECT username, email_state, mailbox_state FROM jmap_accounts WHERE account_id = ?1",
                [account_id],
                |r| {
                    Ok(JmapAccountState {
                        username: r.get(0)?,
                        email_state: r.get(1)?,
                        mailbox_state: r.get(2)?,
                    })
                },
            )
            .optional()
            .map(Option::unwrap_or_default)
        })
    }

    pub fn set_jmap_username(&self, account_id: &str, username: &str) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO jmap_accounts (account_id, username) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET username = excluded.username",
                params![account_id, username],
            )?;
            Ok(())
        })
    }

    /// `Email` durum dizgesini yazar (`None`: baştan eşitlenecek).
    pub fn set_jmap_email_state(&self, account_id: &str, state: Option<&str>) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO jmap_accounts (account_id, email_state) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET email_state = excluded.email_state",
                params![account_id, state],
            )?;
            Ok(())
        })
    }

    /// `Mailbox` durum dizgesini yazar.
    pub fn set_jmap_mailbox_state(&self, account_id: &str, state: Option<&str>) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO jmap_accounts (account_id, mailbox_state) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET mailbox_state = excluded.mailbox_state",
                params![account_id, state],
            )?;
            Ok(())
        })
    }

    /// Hesabın yerelde bilinen JMAP postaları: sunucu kimliği → yerel kimlik.
    pub fn jmap_known(&self, account_id: &str) -> AppResult<HashMap<String, MessageId>> {
        self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT jmap_id, message_id FROM jmap_messages WHERE account_id = ?1",
            )?;
            let rows = st.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    /// Sunucu kimliğiyle yerel posta.
    pub fn jmap_message(&self, account_id: &str, jmap_id: &str) -> AppResult<Option<MessageId>> {
        self.with(|c| {
            c.query_row(
                "SELECT message_id FROM jmap_messages WHERE account_id = ?1 AND jmap_id = ?2",
                params![account_id, jmap_id],
                |r| r.get(0),
            )
            .optional()
        })
    }

    /// Yerel postayı sunucudaki kopyasına bağlar (ör. sunucuya eklenen
    /// taslak). Eşzamanlı bir eşitleme aynı sunucu postasını ayrı satır olarak
    /// yazmışsa o satır silinir; etkilenen kutuları döner.
    pub fn jmap_bind(
        &self,
        id: MessageId,
        account_id: &str,
        jmap_id: &str,
        blob_id: Option<&str>,
    ) -> AppResult<Vec<String>> {
        self.with_tx(|tx| {
            let dupes: Vec<(MessageId, String)> = {
                let mut st = tx.prepare_cached(
                    "SELECT m.id, m.mailbox_id FROM jmap_messages j JOIN messages m ON m.id = j.message_id
                     WHERE j.account_id = ?1 AND j.jmap_id = ?2 AND j.message_id != ?3",
                )?;
                let rows = st.query_map(params![account_id, jmap_id, id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for (dupe, _) in &dupes {
                tx.execute("DELETE FROM messages WHERE id = ?1", [dupe])?;
            }
            map_row(tx, id, account_id, jmap_id, blob_id)?;
            Ok(dupes.into_iter().map(|(_, mb)| mb).collect())
        })
    }

    /// JMAP postasını ekler ya da kutusunu/bayraklarını günceller. Sunucu
    /// kimliği bilinmiyorsa aynı Message-ID'li, henüz bağlanmamış yerel kopya
    /// (ör. giden kutusunun Gönderilenler kopyası) sahiplenilir. `thread_id`
    /// verilirse yerel konuşma kimliğinin yerine geçer.
    pub fn jmap_upsert(
        &self,
        m: &NewMessage,
        jmap_id: &str,
        blob_id: Option<&str>,
        thread_id: Option<&str>,
    ) -> AppResult<JmapUpsert> {
        self.with_tx(|tx| {
            if let Some((id, old_mailbox, unread, starred)) = find_existing(tx, m, jmap_id)? {
                map_row(tx, id, &m.account_id, jmap_id, blob_id)?;
                tx.prepare_cached(
                    "UPDATE messages SET mailbox_id = ?2, unread = ?3, starred = ?4,
                         thread_id = COALESCE(?5, thread_id)
                     WHERE id = ?1",
                )?
                .execute(params![id, m.mailbox_id, m.unread, m.starred, thread_id])?;
                if let Some(raw) = &m.raw {
                    tx.prepare_cached(
                        "UPDATE messages SET raw = ?2, body_fetched = 1, snippet = ?3, has_attachments = ?4,
                             fts_body = ?5
                         WHERE id = ?1 AND raw IS NULL",
                    )?
                    .execute(params![
                        id,
                        raw,
                        m.snippet,
                        m.has_attachments,
                        fold_tr(m.body_text.as_deref().unwrap_or(""))
                    ])?;
                }
                let mut touched = Vec::new();
                if old_mailbox != m.mailbox_id {
                    // Gönderilenler'e taşınan postanın alıcıları bilinen gönderici olur.
                    crate::screener::note_sent_tx(tx, m)?;
                    touched.push(old_mailbox);
                    touched.push(m.mailbox_id.clone());
                } else if unread != m.unread || starred != m.starred {
                    touched.push(m.mailbox_id.clone());
                }
                return Ok(JmapUpsert {
                    id,
                    inserted: false,
                    touched,
                });
            }
            let id = insert_as(tx, m, None)?;
            map_row(tx, id, &m.account_id, jmap_id, blob_id)?;
            // Gönderilenler'e düşen postanın alıcıları bilinen gönderici olur (`screener`).
            crate::screener::note_sent_tx(tx, m)?;
            if let Some(t) = thread_id {
                tx.prepare_cached("UPDATE messages SET thread_id = ?2 WHERE id = ?1")?
                    .execute(params![id, t])?;
            }
            Ok(JmapUpsert {
                id,
                inserted: true,
                touched: vec![m.mailbox_id.clone()],
            })
        })
    }

    /// Bilinen JMAP postasının yalnız kutusunu ve bayraklarını günceller.
    /// Posta yerelde yoksa `None`; varsa içeriği değişen kutular.
    pub fn jmap_update_state(
        &self,
        account_id: &str,
        jmap_id: &str,
        mailbox_id: &str,
        unread: bool,
        starred: bool,
    ) -> AppResult<Option<Vec<String>>> {
        self.with_tx(|tx| {
            let Some((id, old_mailbox, old_unread, old_starred)) = tx
                .prepare_cached(
                    "SELECT m.id, m.mailbox_id, m.unread, m.starred
                     FROM jmap_messages j JOIN messages m ON m.id = j.message_id
                     WHERE j.account_id = ?1 AND j.jmap_id = ?2",
                )?
                .query_row(params![account_id, jmap_id], |r| {
                    Ok((
                        r.get::<_, MessageId>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, bool>(3)?,
                    ))
                })
                .optional()?
            else {
                return Ok(None);
            };
            let mut touched = Vec::new();
            if old_mailbox != mailbox_id {
                touched.push(old_mailbox);
                touched.push(mailbox_id.to_owned());
            } else if old_unread != unread || old_starred != starred {
                touched.push(mailbox_id.to_owned());
            }
            if !touched.is_empty() {
                tx.prepare_cached(
                    "UPDATE messages SET mailbox_id = ?2, unread = ?3, starred = ?4 WHERE id = ?1",
                )?
                .execute(params![id, mailbox_id, unread, starred])?;
            }
            Ok(Some(touched))
        })
    }

    /// Sunucuda silinen (ya da eşitlenen kutulardan çıkan) postaları yerelden
    /// siler; etkilenen kutuları döner.
    pub fn jmap_remove(&self, account_id: &str, jmap_ids: &[String]) -> AppResult<Vec<String>> {
        let mut touched: Vec<String> = Vec::new();
        for chunk in jmap_ids.chunks(CHUNK) {
            let list = in_list(chunk.len());
            let p = || {
                std::iter::once(Value::Text(account_id.into()))
                    .chain(chunk.iter().map(|s| Value::Text(s.clone())))
                    .collect::<Vec<_>>()
            };
            let boxes: Vec<String> = self.with_tx(|tx| {
                let boxes = {
                    let mut st = tx.prepare(&format!(
                        "SELECT DISTINCT m.mailbox_id FROM jmap_messages j JOIN messages m ON m.id = j.message_id
                         WHERE j.account_id = ? AND j.jmap_id IN ({list})"
                    ))?;
                    let rows = st.query_map(params_from_iter(p()), |r| r.get(0))?;
                    rows.collect::<rusqlite::Result<Vec<String>>>()?
                };
                tx.execute(
                    &format!(
                        "DELETE FROM messages WHERE id IN (SELECT message_id FROM jmap_messages
                         WHERE account_id = ? AND jmap_id IN ({list}))"
                    ),
                    params_from_iter(p()),
                )?;
                Ok(boxes)
            })?;
            touched.extend(boxes);
        }
        touched.sort();
        touched.dedup();
        Ok(touched)
    }

    /// Postaların JMAP yerleri (sunucu işlemleri için).
    pub fn jmap_locations(&self, ids: &[MessageId]) -> AppResult<Vec<JmapLocation>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(CHUNK) {
            let rows = self.with(|c| {
                let mut st = c.prepare(&format!(
                    "SELECT m.id, m.account_id, m.mailbox_id, b.path, b.role, j.jmap_id, j.blob_id,
                            m.raw IS NOT NULL
                     FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                     LEFT JOIN jmap_messages j ON j.message_id = m.id
                     WHERE m.id IN ({})",
                    in_list(chunk.len())
                ))?;
                let rows = st.query_map(
                    params_from_iter(chunk.iter().map(|i| Value::Integer(*i))),
                    |r| {
                        Ok(JmapLocation {
                            id: r.get(0)?,
                            account_id: r.get(1)?,
                            mailbox_id: r.get(2)?,
                            mailbox_path: r.get(3)?,
                            role: role_from(&r.get::<_, String>(4)?),
                            jmap_id: r.get(5)?,
                            blob_id: r.get(6)?,
                            has_raw: r.get(7)?,
                        })
                    },
                )?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })?;
            out.extend(rows);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::store_with_account;
    use mh_core::{ListQuery, View};

    fn setup() -> Store {
        let s = store_with_account("a");
        s.upsert_mailbox("a", "mb-in", "Gelen", MailboxRole::Inbox, None, false)
            .unwrap();
        s.upsert_mailbox("a", "mb-tr", "Çöp", MailboxRole::Trash, None, false)
            .unwrap();
        s.upsert_mailbox("a", "mb-se", "Giden", MailboxRole::Sent, None, false)
            .unwrap();
        s
    }

    #[test]
    fn durum_dizgeleri() {
        let s = setup();
        assert_eq!(
            s.jmap_account_state("a").unwrap(),
            JmapAccountState::default()
        );
        s.set_jmap_username("a", "furkan").unwrap();
        s.set_jmap_email_state("a", Some("e1")).unwrap();
        s.set_jmap_mailbox_state("a", Some("m1")).unwrap();
        s.set_jmap_username("a", "furkan2").unwrap();
        let st = s.jmap_account_state("a").unwrap();
        assert_eq!(st.username.as_deref(), Some("furkan2"));
        assert_eq!(st.email_state.as_deref(), Some("e1"));
        assert_eq!(st.mailbox_state.as_deref(), Some("m1"));
        s.set_jmap_email_state("a", None).unwrap();
        assert_eq!(s.jmap_account_state("a").unwrap().email_state, None);
        // Hesap silinince durum da silinir.
        s.remove_account(&"a".to_string()).unwrap();
        assert_eq!(
            s.jmap_account_state("a").unwrap(),
            JmapAccountState::default()
        );
    }

    #[test]
    fn gonderilenlerdeki_alici_bilinen_gonderici_olur() {
        let s = setup();
        let mut m = msg("a", "a:mb-se", "Teklif", 1_000);
        m.to = vec![mh_core::Address::new(Some("Kerem"), "Kerem@Ornek.com")];
        s.jmap_upsert(&m, "S1", None, None).unwrap();
        assert!(s.sender_known("a", "kerem@ornek.com").unwrap());
        // Gelen kutusundaki postanın alıcısı öğrenilmez; Gönderilenler'e
        // taşınınca öğrenilir.
        let mut g = msg("a", "a:mb-in", "Not", 2_000);
        g.to = vec![mh_core::Address::new(None, "ayse@ornek.com")];
        s.jmap_upsert(&g, "S2", None, None).unwrap();
        assert!(!s.sender_known("a", "ayse@ornek.com").unwrap());
        g.mailbox_id = "a:mb-se".into();
        s.jmap_upsert(&g, "S2", None, None).unwrap();
        assert!(s.sender_known("a", "ayse@ornek.com").unwrap());
    }

    #[test]
    fn ekle_guncelle_tasi_sil() {
        let s = setup();
        let mut m = msg("a", "a:mb-in", "Merhaba", 1_000);
        m.message_id_header = Some("x@ornek.com".into());
        let r = s.jmap_upsert(&m, "E1", Some("B1"), Some("j:T1")).unwrap();
        assert!(r.inserted);
        assert_eq!(r.touched, vec!["a:mb-in".to_string()]);
        assert_eq!(s.summary(r.id).unwrap().unwrap().thread_id, "j:T1");
        assert_eq!(s.jmap_message("a", "E1").unwrap(), Some(r.id));

        // Aynı durum: değişiklik yok.
        let again = s.jmap_upsert(&m, "E1", None, Some("j:T1")).unwrap();
        assert_eq!((again.id, again.inserted), (r.id, false));
        assert!(again.touched.is_empty());

        // Bayrak değişti.
        m.unread = false;
        let f = s.jmap_upsert(&m, "E1", None, None).unwrap();
        assert_eq!(f.touched, vec!["a:mb-in".to_string()]);
        assert!(!s.summary(r.id).unwrap().unwrap().unread);

        // Sunucuda çöpe taşındı.
        m.mailbox_id = "a:mb-tr".into();
        let t = s.jmap_upsert(&m, "E1", None, None).unwrap();
        assert_eq!(
            t.touched,
            vec!["a:mb-in".to_string(), "a:mb-tr".to_string()]
        );
        let loc = &s.jmap_locations(&[r.id]).unwrap()[0];
        assert_eq!(loc.mailbox_path, "mb-tr");
        assert_eq!(loc.role, MailboxRole::Trash);
        assert_eq!(loc.jmap_id.as_deref(), Some("E1"));
        assert_eq!(loc.blob_id.as_deref(), Some("B1"), "blob kimliği korunur");
        assert!(!loc.has_raw);

        let known = s.jmap_known("a").unwrap();
        assert_eq!(known.get("E1"), Some(&r.id));

        // Yalnız durum güncellemesi (Email/changes → bilinen posta).
        assert_eq!(
            s.jmap_update_state("a", "E1", "a:mb-tr", false, false)
                .unwrap(),
            Some(vec![])
        );
        assert_eq!(
            s.jmap_update_state("a", "E1", "a:mb-in", false, true)
                .unwrap(),
            Some(vec!["a:mb-tr".to_string(), "a:mb-in".to_string()])
        );
        assert!(s.summary(r.id).unwrap().unwrap().starred);
        s.jmap_update_state("a", "E1", "a:mb-tr", false, false)
            .unwrap();
        assert_eq!(
            s.jmap_update_state("a", "YOK", "a:mb-in", true, false)
                .unwrap(),
            None
        );

        let gone = s.jmap_remove("a", &["E1".into(), "YOK".into()]).unwrap();
        assert_eq!(gone, vec!["a:mb-tr".to_string()]);
        assert!(s.summary(r.id).unwrap().is_none());
        assert!(s.jmap_known("a").unwrap().is_empty(), "eşleme de silinir");
    }

    #[test]
    fn yerel_kopya_message_id_ile_sahiplenilir() {
        let s = setup();
        // Giden kutusunun yerel kopyası (sunucu kimliği yok).
        let mut local = msg("a", "a:mb-se", "Gönderdim", 2_000);
        local.message_id_header = Some("giden@ornek.com".into());
        local.unread = false;
        let lid = s.upsert_message(&local).unwrap().id;
        // Eşitleme aynı postayı sunucudan getirir.
        let mut server = local.clone();
        server.raw = None;
        let r = s
            .jmap_upsert(&server, "E9", Some("B9"), Some("j:T9"))
            .unwrap();
        assert_eq!((r.id, r.inserted), (lid, false), "çift kayıt oluşmaz");
        assert_eq!(s.jmap_message("a", "E9").unwrap(), Some(lid));
        let page = s
            .list(
                &ListQuery {
                    view: View::Sent,
                    account_id: Some("a".into()),
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 10,
                },
                0,
            )
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].thread_id, "j:T9");
    }

    #[test]
    fn govde_sonradan_yazilir() {
        let s = setup();
        let m = msg("a", "a:mb-in", "Gövde", 3_000);
        let id = s.jmap_upsert(&m, "E2", None, None).unwrap().id;
        let mut with_raw = m.clone();
        with_raw.raw = Some(b"Subject: x\r\n\r\ngovde".to_vec());
        s.jmap_upsert(&with_raw, "E2", None, None).unwrap();
        assert!(s.jmap_locations(&[id]).unwrap()[0].has_raw);
        assert!(s.jmap_bind(id, "a", "E2", Some("B2")).unwrap().is_empty());
        assert_eq!(
            s.jmap_locations(&[id]).unwrap()[0].blob_id.as_deref(),
            Some("B2")
        );
    }

    #[test]
    fn baglama_eszamanli_kopyayi_siler() {
        let s = setup();
        // Taslak yerelde kuruldu; eşitleme aynı sunucu postasını ayrı satır yazdı.
        let local = s
            .upsert_message(&msg("a", "a:mb-in", "Taslak", 1_000))
            .unwrap()
            .id;
        let synced = s
            .jmap_upsert(&msg("a", "a:mb-in", "Taslak", 1_000), "D1", None, None)
            .unwrap()
            .id;
        assert_ne!(local, synced);
        let touched = s.jmap_bind(local, "a", "D1", Some("B")).unwrap();
        assert_eq!(touched, vec!["a:mb-in".to_string()]);
        assert!(s.summary(synced).unwrap().is_none());
        assert_eq!(s.jmap_message("a", "D1").unwrap(), Some(local));
        // Yeni sürüm: aynı satır başka sunucu postasına bağlanır.
        s.jmap_bind(local, "a", "D2", None).unwrap();
        assert_eq!(s.jmap_message("a", "D2").unwrap(), Some(local));
        assert_eq!(s.jmap_message("a", "D1").unwrap(), None);
    }
}
