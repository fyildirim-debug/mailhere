//! Kalıcı sunucu işlemleri kuyruğu.
//!
//! Bayrak, taşıma, ekleme ve silme işlemleri önce burada sıraya girer; eşitleme
//! motoru bağlantı kurunca hesabın kuyruğunu baştan sona, sırayla uygular ve
//! her işlemi sunucu onayından sonra siler. Uygulama kapanıp açılsa da,
//! bağlantı kopsa da işlemler kaybolmaz. Hesap silinince kuyruğu da silinir.
//!
//! Birleştirme: kuyruğun başı (o an uygulanıyor olabilir) hiç değiştirilmez;
//! sonrasındaki aynı türden işlemler yeni istekle birleştirilir.

use mh_core::{AppResult, MessageId, Millis};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::codec::to_json;
use crate::Store;

/// Sunucuda değiştirilen IMAP bayrağı.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ServerFlag {
    /// `\Seen` (okundu).
    Seen,
    /// `\Flagged` (yıldızlı).
    Flagged,
}

/// Bir postanın sunucudaki yeri (yerel satırı artık olmayan işlemler için).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerRef {
    pub path: String,
    pub uid: Option<u32>,
    pub uidvalidity: Option<u32>,
    /// UID bilinmiyor ya da geçersizse Message-ID ile aranır.
    pub message_id: Option<String>,
}

/// Sıradaki sunucu işlemi.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// Bayrağı ayarla (`value`: bayrak sunucuda olmalı mı). Postanın
    /// sunucudaki yeri işlem çalışırken çözülür.
    SetFlag {
        ids: Vec<MessageId>,
        flag: ServerFlag,
        value: bool,
    },
    /// Hesabın sunucu onayı bekleyen tüm taşımalarını uygula (hedef,
    /// postanın o anki yerel kutusudur; geri alınan taşıma kendiliğinden düşer).
    Moves,
    /// Ham postayı kutuya ekle (ör. Gönderilenler); ham veri ayrı sütunda.
    Append {
        path: String,
        message_id: Option<String>,
    },
    /// Sunucudan kalıcı sil (yerel satırlar zaten silindi).
    Delete { items: Vec<ServerRef> },
}

/// Kuyruktaki bir işlem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedAction {
    pub id: i64,
    pub account_id: String,
    pub action: Action,
    pub raw: Option<Vec<u8>>,
    /// Bağlantı koptuğu için yarıda kalan deneme sayısı.
    pub attempts: u32,
}

type Row = (i64, String, Option<Vec<u8>>, u32);

impl Store {
    /// İşlemi hesabın kuyruğuna ekler, gerekirse öncekilerle birleştirir.
    /// Eklenen satırın kimliğini döner; işlem gereksiz kaldıysa `None`.
    pub fn action_push(
        &self,
        account_id: &str,
        action: &Action,
        raw: Option<&[u8]>,
        now: Millis,
    ) -> AppResult<Option<i64>> {
        self.with_tx(|tx| {
            let head: Option<i64> = tx
                .query_row(
                    "SELECT MIN(id) FROM actions WHERE account_id = ?1",
                    [account_id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            // Baştan sonraki (henüz başlamamış) işlemler.
            let later: Vec<(i64, Action)> = {
                let mut st = tx.prepare_cached(
                    "SELECT id, payload FROM actions WHERE account_id = ?1 AND id > ?2 ORDER BY id",
                )?;
                let rows = st.query_map(params![account_id, head.unwrap_or(i64::MAX)], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?;
                rows.filter_map(|r| match r {
                    Ok((id, p)) => serde_json::from_str(&p).ok().map(|a| Ok((id, a))),
                    Err(e) => Some(Err(e)),
                })
                .collect::<rusqlite::Result<_>>()?
            };
            match action {
                Action::SetFlag { ids, flag, .. } => {
                    if ids.is_empty() {
                        return Ok(None);
                    }
                    // Aynı posta ve bayrak için yalnız son istek kalır.
                    for (row_id, old) in &later {
                        let Action::SetFlag {
                            ids: old_ids,
                            flag: old_flag,
                            value,
                        } = old
                        else {
                            continue;
                        };
                        if old_flag != flag {
                            continue;
                        }
                        let rest: Vec<MessageId> = old_ids
                            .iter()
                            .copied()
                            .filter(|i| !ids.contains(i))
                            .collect();
                        if rest.len() == old_ids.len() {
                            continue;
                        }
                        if rest.is_empty() {
                            tx.execute("DELETE FROM actions WHERE id = ?1", [row_id])?;
                        } else {
                            let updated = Action::SetFlag {
                                ids: rest,
                                flag: *old_flag,
                                value: *value,
                            };
                            tx.execute(
                                "UPDATE actions SET payload = ?2 WHERE id = ?1",
                                params![row_id, to_json(&updated)],
                            )?;
                        }
                    }
                }
                // Taşımalar satırların durumundan okunur; sırada bekleyen bir
                // tane yeterlidir.
                Action::Moves => {
                    if later.iter().any(|(_, a)| matches!(a, Action::Moves)) {
                        return Ok(None);
                    }
                }
                Action::Delete { items } if items.is_empty() => return Ok(None),
                Action::Append { .. } | Action::Delete { .. } => {}
            }
            tx.execute(
                "INSERT INTO actions (account_id, payload, raw, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![account_id, to_json(action), raw, now],
            )?;
            Ok(Some(tx.last_insert_rowid()))
        })
    }

    /// Hesabın sıradaki işlemi. Okunamayan (bozuk) satırlar atılır.
    pub fn action_head(&self, account_id: &str) -> AppResult<Option<QueuedAction>> {
        loop {
            let row: Option<Row> = self.with(|c| {
                c.query_row(
                    "SELECT id, payload, raw, attempts FROM actions WHERE account_id = ?1
                     ORDER BY id LIMIT 1",
                    [account_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? as u32)),
                )
                .optional()
            })?;
            let Some((id, payload, raw, attempts)) = row else {
                return Ok(None);
            };
            match serde_json::from_str::<Action>(&payload) {
                Ok(action) => {
                    return Ok(Some(QueuedAction {
                        id,
                        account_id: account_id.to_owned(),
                        action,
                        raw,
                        attempts,
                    }))
                }
                Err(e) => {
                    tracing::warn!(id, error = %e, "okunamayan sunucu işlemi atlandı");
                    self.action_done(id)?;
                }
            }
        }
    }

    /// Hesabın tüm bekleyen işlemleri, sırayla.
    pub fn actions(&self, account_id: &str) -> AppResult<Vec<QueuedAction>> {
        let rows: Vec<Row> = self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT id, payload, raw, attempts FROM actions WHERE account_id = ?1 ORDER BY id",
            )?;
            let rows = st.query_map([account_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? as u32))
            })?;
            rows.collect()
        })?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, payload, raw, attempts)| {
                Some(QueuedAction {
                    id,
                    account_id: account_id.to_owned(),
                    action: serde_json::from_str(&payload).ok()?,
                    raw,
                    attempts,
                })
            })
            .collect())
    }

    /// Tamamlanan (ya da vazgeçilen) işlemi kuyruktan siler.
    pub fn action_done(&self, id: i64) -> AppResult<()> {
        self.with(|c| {
            c.execute("DELETE FROM actions WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Yarıda kalan denemeyi sayar; yeni deneme sayısını döner.
    pub fn action_attempt(&self, id: i64) -> AppResult<u32> {
        self.with(|c| {
            c.query_row(
                "UPDATE actions SET attempts = attempts + 1 WHERE id = ?1 RETURNING attempts",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|n| n.unwrap_or(0) as u32)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{account, store_with_account};

    fn flag(ids: &[MessageId], flag: ServerFlag, value: bool) -> Action {
        Action::SetFlag {
            ids: ids.to_vec(),
            flag,
            value,
        }
    }

    fn kinds(s: &Store, account: &str) -> Vec<Action> {
        s.actions(account)
            .unwrap()
            .into_iter()
            .map(|a| a.action)
            .collect()
    }

    #[test]
    fn sira_ve_tamamlama() {
        let s = store_with_account("a");
        s.insert_account(&account("b")).unwrap();
        let append = Action::Append {
            path: "Sent".into(),
            message_id: Some("m@x".into()),
        };
        let first = s
            .action_push("a", &append, Some(b"ham posta"), 1)
            .unwrap()
            .unwrap();
        s.action_push("b", &Action::Moves, None, 2).unwrap();
        s.action_push("a", &flag(&[1], ServerFlag::Seen, true), None, 3)
            .unwrap();

        let head = s.action_head("a").unwrap().unwrap();
        assert_eq!(head.id, first);
        assert_eq!(head.action, append);
        assert_eq!(head.raw.as_deref(), Some(&b"ham posta"[..]));
        assert_eq!(head.attempts, 0);
        assert_eq!(s.action_attempt(first).unwrap(), 1);
        assert_eq!(s.action_head("a").unwrap().unwrap().attempts, 1);
        s.action_done(first).unwrap();
        assert_eq!(
            s.action_head("a").unwrap().unwrap().action,
            flag(&[1], ServerFlag::Seen, true)
        );
        assert_eq!(kinds(&s, "b"), vec![Action::Moves]);
    }

    #[test]
    fn bayraklar_birlesir_bas_korunur() {
        let s = store_with_account("a");
        // Baş: uygulanıyor olabilir, dokunulmaz.
        s.action_push("a", &flag(&[1, 2], ServerFlag::Flagged, true), None, 1)
            .unwrap();
        s.action_push("a", &flag(&[1, 2, 3], ServerFlag::Flagged, true), None, 2)
            .unwrap();
        s.action_push("a", &flag(&[1], ServerFlag::Seen, true), None, 3)
            .unwrap();
        // Yıldız koy sonra kaldır: yalnız son istek kalır; diğer bayrak etkilenmez.
        s.action_push("a", &flag(&[1, 3], ServerFlag::Flagged, false), None, 4)
            .unwrap();
        assert_eq!(
            kinds(&s, "a"),
            vec![
                flag(&[1, 2], ServerFlag::Flagged, true),
                flag(&[2], ServerFlag::Flagged, true),
                flag(&[1], ServerFlag::Seen, true),
                flag(&[1, 3], ServerFlag::Flagged, false),
            ]
        );
        s.action_push("a", &flag(&[2], ServerFlag::Flagged, false), None, 5)
            .unwrap();
        assert_eq!(
            kinds(&s, "a"),
            vec![
                flag(&[1, 2], ServerFlag::Flagged, true),
                flag(&[1], ServerFlag::Seen, true),
                flag(&[1, 3], ServerFlag::Flagged, false),
                flag(&[2], ServerFlag::Flagged, false),
            ]
        );
        assert_eq!(
            s.action_push("a", &flag(&[], ServerFlag::Seen, true), None, 6)
                .unwrap(),
            None
        );
    }

    #[test]
    fn tasimalar_tek_bekler() {
        let s = store_with_account("a");
        assert!(s
            .action_push("a", &Action::Moves, None, 1)
            .unwrap()
            .is_some());
        // Baş uygulanıyor olabilir: arkasına bir tane daha girer.
        assert!(s
            .action_push("a", &Action::Moves, None, 2)
            .unwrap()
            .is_some());
        assert!(s
            .action_push("a", &Action::Moves, None, 3)
            .unwrap()
            .is_none());
        assert_eq!(kinds(&s, "a"), vec![Action::Moves, Action::Moves]);
    }

    #[test]
    fn hesap_silinince_kuyruk_gider_ve_bozuk_satir_atlanir() {
        let s = store_with_account("a");
        s.action_push("a", &Action::Moves, None, 1).unwrap();
        s.with(|c| {
            c.execute(
                "INSERT INTO actions (account_id, payload, created_at) VALUES ('a', '{\"kind\":\"gelecek\"}', 0)",
                [],
            )
        })
        .unwrap();
        let head = s.action_head("a").unwrap().unwrap();
        s.action_done(head.id).unwrap();
        assert!(s.action_head("a").unwrap().is_none(), "bozuk satır atıldı");

        s.action_push("a", &Action::Moves, None, 2).unwrap();
        s.remove_account(&"a".to_string()).unwrap();
        assert!(s.actions("a").unwrap().is_empty());
    }

    #[test]
    fn kuyruk_yeniden_acilista_kalir() {
        let dir = std::env::temp_dir().join(format!(
            "mh-actions-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        {
            let s = Store::open(&path).unwrap();
            s.insert_account(&account("a")).unwrap();
            s.action_push("a", &flag(&[7], ServerFlag::Seen, true), None, 1)
                .unwrap();
            s.action_push(
                "a",
                &Action::Delete {
                    items: vec![ServerRef {
                        path: "INBOX".into(),
                        uid: Some(4),
                        uidvalidity: Some(9),
                        message_id: None,
                    }],
                },
                None,
                2,
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        let list = kinds(&s, "a");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], flag(&[7], ServerFlag::Seen, true));
        assert!(matches!(&list[1], Action::Delete { items } if items[0].uid == Some(4)));
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }
}
