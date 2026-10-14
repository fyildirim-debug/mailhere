//! Yeni gönderici eleği: bilinen göndericiler ve elekte bekleyen postalar.
//!
//! Bilinen gönderici, hesap başına: gönderdiğimiz postaların alıcıları
//! (`sent`; Gönderilenler'e yazılan her postada kendiliğinden eklenir, ilk
//! açılışta mevcut Gönderilenler'den bir kez doldurulur) ve elekten kabul
//! edilenler (`accepted`). Elekteki posta gelen kutusunda ve sayaçlarda
//! görünmez; kabul edilince geri döner, engellenince çöpe gider.

use mh_core::{Address, AppResult, MessageId, Millis, ScreenedSender};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::codec::{addr_list, from_json};
use crate::messages::NewMessage;
use crate::Store;

/// Adresin karşılaştırma biçimi.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// `account_id` hesabında bu alıcıları bilinen gönderici yapar.
fn remember(
    tx: &Transaction<'_>,
    account_id: &str,
    recipients: impl IntoIterator<Item = String>,
    source: &str,
    at: Millis,
) -> rusqlite::Result<u32> {
    let mut st = tx.prepare_cached(
        "INSERT OR IGNORE INTO known_senders (account_id, email, source, at) VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut n = 0;
    for email in recipients {
        let email = normalize_email(&email);
        if !crate::is_full_address(&email) {
            continue;
        }
        n += st.execute(params![account_id, email, source, at])? as u32;
    }
    Ok(n)
}

fn emails(list: &[Address]) -> impl Iterator<Item = String> + '_ {
    list.iter().map(|a| a.email.clone())
}

/// Gönderilenler'e yazılan postanın alıcılarını bilinen gönderici yapar
/// (`upsert_message` içinden, aynı işlemde). Başka kutulara dokunmaz.
pub(crate) fn note_sent_tx(tx: &Transaction<'_>, m: &NewMessage) -> rusqlite::Result<()> {
    let sent: bool = tx
        .query_row(
            "SELECT role = 'sent' FROM mailboxes WHERE id = ?1",
            [&m.mailbox_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false);
    if !sent {
        return Ok(());
    }
    remember(
        tx,
        &m.account_id,
        emails(&m.to).chain(emails(&m.cc)),
        "sent",
        m.date,
    )?;
    Ok(())
}

impl Store {
    /// Bilinen göndericileri mevcut Gönderilenler'den bir kez doldurur;
    /// eklenen adres sayısını döner (daha önce yapılmışsa 0).
    pub fn known_senders_backfill(&self) -> AppResult<u32> {
        self.with_tx(|tx| {
            let done: bool = tx
                .query_row(
                    "SELECT backfilled FROM screener_state WHERE id = 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(false);
            if done {
                return Ok(0);
            }
            let rows: Vec<(String, String, String, Millis)> = {
                let mut st = tx.prepare(
                    "SELECT m.account_id, m.to_addrs, m.cc_addrs, m.date
                     FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id WHERE b.role = 'sent'",
                )?;
                let rows =
                    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut n = 0;
            for (account, to, cc, date) in rows {
                let to = addr_list(1, &to)?;
                let cc = addr_list(2, &cc)?;
                n += remember(tx, &account, emails(&to).chain(emails(&cc)), "sent", date)?;
            }
            tx.execute(
                "INSERT INTO screener_state (id, backfilled) VALUES (1, 1)
                 ON CONFLICT(id) DO UPDATE SET backfilled = 1",
                [],
            )?;
            Ok(n)
        })
    }

    /// Gönderici bu hesapta biliniyor mu?
    pub fn sender_known(&self, account_id: &str, email: &str) -> AppResult<bool> {
        let email = normalize_email(email);
        self.with(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM known_senders WHERE account_id = ?1 AND email = ?2)",
                params![account_id, email],
                |r| r.get(0),
            )
        })
    }

    /// Postayı eleğe alır (gelen kutusundan gizlenir).
    pub fn screen_message(
        &self,
        id: MessageId,
        account_id: &str,
        email: &str,
        now: Millis,
    ) -> AppResult<()> {
        let email = normalize_email(email);
        self.with(|c| {
            c.execute(
                "INSERT OR IGNORE INTO screened (message_id, account_id, email, at) VALUES (?1, ?2, ?3, ?4)",
                params![id, account_id, email, now],
            )?;
            Ok(())
        })
    }

    /// Elekteki göndericiler: en son postası en üstte.
    pub fn screened_senders(&self) -> AppResult<Vec<ScreenedSender>> {
        self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT s.account_id, s.email, COUNT(*), MAX(m.date),
                        (SELECT m2.from_addr FROM screened s2 JOIN messages m2 ON m2.id = s2.message_id
                          WHERE s2.account_id = s.account_id AND s2.email = s.email
                          ORDER BY m2.date DESC, m2.id DESC LIMIT 1),
                        (SELECT m2.subject FROM screened s2 JOIN messages m2 ON m2.id = s2.message_id
                          WHERE s2.account_id = s.account_id AND s2.email = s.email
                          ORDER BY m2.date DESC, m2.id DESC LIMIT 1)
                 FROM screened s JOIN messages m ON m.id = s.message_id
                 GROUP BY s.account_id, s.email ORDER BY MAX(m.date) DESC",
            )?;
            let rows = st.query_map([], |r| {
                let from: Address = from_json(4, &r.get::<_, String>(4)?)?;
                Ok(ScreenedSender {
                    account_id: r.get(0)?,
                    email: r.get(1)?,
                    count: r.get::<_, i64>(2)? as u32,
                    latest: r.get(3)?,
                    name: from.name.filter(|n| !n.trim().is_empty()),
                    subject: r.get(5)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Göndericinin elekteki postaları.
    pub fn screened_messages_of(&self, account_id: &str, email: &str) -> AppResult<Vec<MessageId>> {
        let email = normalize_email(email);
        self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT message_id FROM screened WHERE account_id = ?1 AND email = ?2 ORDER BY message_id",
            )?;
            let rows = st.query_map(params![account_id, email], |r| r.get(0))?;
            rows.collect()
        })
    }

    /// Elekten karar: kabul edilen gönderici bilinen olur ve postaları gelen
    /// kutusuna döner; engellenende yalnız elek kaydı silinir (çöpe taşıma ve
    /// kural çağıranın işidir). Etkilenen postaların kimliklerini döner.
    pub fn screener_decide(
        &self,
        account_id: &str,
        email: &str,
        allow: bool,
        now: Millis,
    ) -> AppResult<Vec<MessageId>> {
        let ids = self.screened_messages_of(account_id, email)?;
        let email = normalize_email(email);
        self.with_tx(|tx| {
            if allow {
                remember(
                    tx,
                    account_id,
                    std::iter::once(email.clone()),
                    "accepted",
                    now,
                )?;
            }
            tx.execute(
                "DELETE FROM screened WHERE account_id = ?1 AND email = ?2",
                params![account_id, email],
            )?;
            Ok(())
        })?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{ListQuery, MailboxRole, View};

    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::store_with_account;

    fn setup() -> Store {
        let s = store_with_account("a");
        for (path, role) in [
            ("INBOX", MailboxRole::Inbox),
            ("Sent", MailboxRole::Sent),
            ("Trash", MailboxRole::Trash),
        ] {
            s.upsert_mailbox("a", path, path, role, None, false)
                .unwrap();
        }
        s
    }

    fn inbox_ids(s: &Store) -> Vec<MessageId> {
        s.list(
            &ListQuery {
                view: View::Inbox,
                account_id: None,
                category: None,
                only: None,
                cursor: None,
                limit: 50,
            },
            10_000,
        )
        .unwrap()
        .items
        .iter()
        .map(|m| m.id)
        .collect()
    }

    #[test]
    fn gonderilen_postanin_alicilari_bilinir() {
        let s = setup();
        let mut m = msg("a", "a:Sent", "Teklif", 1_000);
        m.to = vec![Address::new(Some("Kerem"), " Kerem@Ornek.com.tr ")];
        m.cc = vec![
            Address::new(None, "ayse@eposta.com"),
            Address::new(None, "bozuk"),
        ];
        s.upsert_message(&m).unwrap();
        assert!(s.sender_known("a", "kerem@ornek.com.tr").unwrap());
        assert!(s.sender_known("a", "AYSE@eposta.com").unwrap());
        assert!(!s.sender_known("a", "bozuk").unwrap());
        assert!(
            !s.sender_known("b", "kerem@ornek.com.tr").unwrap(),
            "hesap başına"
        );
        // Gelen kutusundaki posta kimseyi bilinen yapmaz.
        s.upsert_message(&msg("a", "a:INBOX", "Gelen", 1_100))
            .unwrap();
        assert!(!s.sender_known("a", "furkan@ornek.com.tr").unwrap());
    }

    #[test]
    fn mevcut_gonderilenlerden_bir_kez_doldurulur() {
        let s = setup();
        // Tablo boşken Gönderilenler'e yazılmış gibi: kaydı elle sil.
        let mut m = msg("a", "a:Sent", "Eski", 500);
        m.to = vec![Address::new(None, "eski@ornek.com")];
        s.upsert_message(&m).unwrap();
        s.with(|c| c.execute("DELETE FROM known_senders", []).map(|_| ()))
            .unwrap();
        assert!(!s.sender_known("a", "eski@ornek.com").unwrap());
        assert_eq!(s.known_senders_backfill().unwrap(), 1);
        assert!(s.sender_known("a", "eski@ornek.com").unwrap());
        // İkinci açılışta yeniden yapılmaz.
        s.with(|c| c.execute("DELETE FROM known_senders", []).map(|_| ()))
            .unwrap();
        assert_eq!(s.known_senders_backfill().unwrap(), 0);
        assert!(!s.sender_known("a", "eski@ornek.com").unwrap());
    }

    #[test]
    fn elek_gelen_kutusunu_ve_sayaclari_gizler_karar_geri_getirir() {
        let s = setup();
        let known = s
            .upsert_message(&msg("a", "a:INBOX", "Bilinen", 1_000))
            .unwrap()
            .id;
        let mut n1 = msg("a", "a:INBOX", "İlk posta", 2_000);
        n1.from = Address::new(Some("Yeni Kişi"), "Yeni@Firma.com");
        let new1 = s.upsert_message(&n1).unwrap().id;
        let mut n2 = msg("a", "a:INBOX", "İkinci posta", 3_000);
        n2.from = Address::new(None, "yeni@firma.com");
        let new2 = s.upsert_message(&n2).unwrap().id;
        let mut o = msg("a", "a:INBOX", "Öteki", 2_500);
        o.from = Address::new(None, "oteki@baska.com");
        let other = s.upsert_message(&o).unwrap().id;

        s.screen_message(new1, "a", "Yeni@Firma.com", 2_001)
            .unwrap();
        s.screen_message(new2, "a", "yeni@firma.com", 3_001)
            .unwrap();
        s.screen_message(other, "a", "oteki@baska.com", 2_501)
            .unwrap();
        assert_eq!(inbox_ids(&s), vec![known]);
        let c = s.counts(10_000).unwrap();
        assert_eq!((c.inbox_unread, c.screener), (1, 3));

        let senders = s.screened_senders().unwrap();
        assert_eq!(senders.len(), 2);
        assert_eq!(senders[0].email, "yeni@firma.com");
        assert_eq!(senders[0].count, 2);
        assert_eq!(senders[0].latest, 3_000);
        assert_eq!(senders[0].subject, "İkinci posta");
        // En son postada ad yok; ad olan posta eskide kaldı.
        assert_eq!(senders[0].name, None);
        assert_eq!(senders[1].email, "oteki@baska.com");
        assert_eq!(senders[1].count, 1);

        // Elek görünümü.
        let view = s
            .list(
                &ListQuery {
                    view: View::Screener,
                    account_id: None,
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 50,
                },
                10_000,
            )
            .unwrap();
        assert_eq!(
            view.items.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![new2, other, new1]
        );

        // Kabul: gönderici bilinir, postalar gelen kutusuna döner.
        let back = s
            .screener_decide("a", "YENI@firma.com", true, 4_000)
            .unwrap();
        assert_eq!(back, vec![new1, new2]);
        assert!(s.sender_known("a", "yeni@firma.com").unwrap());
        assert_eq!(inbox_ids(&s), vec![new2, new1, known]);
        assert_eq!(s.counts(10_000).unwrap().screener, 1);

        // Engelle: kayıt silinir, gönderici bilinen olmaz; taşımayı çağıran yapar.
        let blocked = s
            .screener_decide("a", "oteki@baska.com", false, 4_000)
            .unwrap();
        assert_eq!(blocked, vec![other]);
        assert!(!s.sender_known("a", "oteki@baska.com").unwrap());
        assert_eq!(s.counts(10_000).unwrap().screener, 0);
        assert!(s.screened_senders().unwrap().is_empty());
        // Olmayan gönderici için karar sessizce boş döner.
        assert!(s
            .screener_decide("a", "yok@yok.com", true, 4_000)
            .unwrap()
            .is_empty());

        // Silinen posta elekten düşer.
        s.screen_message(other, "a", "oteki@baska.com", 5_000)
            .unwrap();
        s.delete_messages(&[other]).unwrap();
        assert_eq!(s.counts(10_000).unwrap().screener, 0);
    }
}
