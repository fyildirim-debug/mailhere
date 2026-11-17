//! Posta sinyalleri: doğrulama kodu (`otp`) ve istenmeyen puanı
//! (`spam_score`, `spam_reasons`).
//!
//! İkisi de alımda hesaplanır (`mh_mime::parse` → [`NewMessage`]) ve gövde
//! sonradan inince yenilenir. Depo iki kural uygular:
//!
//! - Kendi postamızda (Gönderilenler, Taslaklar) kod ve puan tutulmaz.
//! - Gönderici bu hesapta kabul edilmişse (`known_senders`, kaynak
//!   `accepted`: elekten kabul ya da "Güvenli") puan 0 yazılır.
//!
//! Engellenenler listesi (`Settings::blocked`) sütunlara yazılmaz: liste
//! değişince eski postalar yeniden puanlanmasın diye `Blocked` nedeni okuma
//! anında eklenir ([`Store::detail`]); yeni postayı taşımak uygulama
//! katmanının işidir.

use mh_core::{Address, AppResult, MessageId, Millis, SpamVerdict, SPAM_THRESHOLD};
use rusqlite::{params, Connection, OptionalExtension};

use crate::codec::from_json;
use crate::screener::normalize_email;
use crate::tags::SenderScope;
use crate::Store;

/// Geri doldurmanın `backfills` tablosundaki adı.
const BACKFILL: &str = "signals";
/// Geri doldurma bu kadar geriye bakar.
const BACKFILL_WINDOW: Millis = 30 * 24 * 3600 * 1000;
/// Geri doldurmada bir kilit süresince okunan kimlik sayısı.
const BACKFILL_PAGE: i64 = 200;
/// Tek seferde süresi dolmuş sayılan kod postalarının üst sınırı.
const EXPIRED_LIMIT: i64 = 500;

/// Sütunlara yazılacak değerler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Signals {
    pub otp: Option<String>,
    pub score: u8,
    pub reasons: String,
}

impl Signals {
    const NONE: Signals = Signals {
        otp: None,
        score: 0,
        reasons: String::new(),
    };
}

/// Gönderici bu hesapta kabul edilmiş mi (elekten kabul ya da "Güvenli")?
pub(crate) fn sender_accepted(
    conn: &Connection,
    account_id: &str,
    email: &str,
) -> rusqlite::Result<bool> {
    conn.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM known_senders
                       WHERE account_id = ?1 AND email = ?2 AND source = 'accepted')",
    )?
    .query_row(params![account_id, normalize_email(email)], |r| r.get(0))
}

/// Kutu bizim yazdığımız postaları mı tutuyor (Gönderilenler, Taslaklar)?
fn own_mailbox(conn: &Connection, mailbox_id: &str) -> rusqlite::Result<bool> {
    Ok(conn
        .prepare_cached("SELECT role IN ('sent', 'drafts') FROM mailboxes WHERE id = ?1")?
        .query_row([mailbox_id], |r| r.get(0))
        .optional()?
        .unwrap_or(false))
}

/// Hesaplanan kod ve puandan sütunlara yazılacak değerler (modül başındaki
/// iki kural uygulanır).
pub(crate) fn effective(
    conn: &Connection,
    account_id: &str,
    mailbox_id: &str,
    from: &Address,
    otp: Option<&str>,
    spam: &SpamVerdict,
) -> rusqlite::Result<Signals> {
    if otp.is_none() && spam.score == 0 {
        return Ok(Signals::NONE);
    }
    if own_mailbox(conn, mailbox_id)? {
        return Ok(Signals::NONE);
    }
    let trusted = spam.score > 0 && sender_accepted(conn, account_id, &from.email)?;
    Ok(Signals {
        otp: otp.map(str::to_owned),
        score: if trusted { 0 } else { spam.score.min(100) },
        reasons: if trusted {
            String::new()
        } else {
            spam.reasons_text()
        },
    })
}

/// Var olan satırın kodunu ve puanını yeniler (gövde sonradan indi). Satır
/// yoksa bir şey yapmaz.
pub(crate) fn update(
    conn: &Connection,
    id: MessageId,
    otp: Option<&str>,
    spam: &SpamVerdict,
) -> rusqlite::Result<()> {
    let row: Option<(String, String, String)> = conn
        .prepare_cached("SELECT account_id, mailbox_id, from_addr FROM messages WHERE id = ?1")?
        .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((account_id, mailbox_id, from)) = row else {
        return Ok(());
    };
    let from: Address = from_json(2, &from)?;
    let s = effective(conn, &account_id, &mailbox_id, &from, otp, spam)?;
    conn.prepare_cached(
        "UPDATE messages SET otp = ?2, spam_score = ?3, spam_reasons = ?4 WHERE id = ?1",
    )?
    .execute(params![id, s.otp, i64::from(s.score), s.reasons])?;
    Ok(())
}

/// Sütunlardaki puandan okuma alanı kararı: eşiğin altındaysa `None`;
/// gönderen engelliyse (`blocked`) `Blocked` nedeni eklenir ve puan 100 olur.
pub(crate) fn verdict_of(score: i64, reasons: &str, blocked: bool) -> Option<SpamVerdict> {
    let v = SpamVerdict {
        score: score.clamp(0, 100) as u8,
        reasons: SpamVerdict::reasons_from_text(reasons),
    };
    if blocked {
        Some(v.with_blocked())
    } else {
        v.is_spam().then_some(v)
    }
}

/// `messages.spam_score >= eşik` koşulu (arama işleci ve özet sütunu).
pub(crate) fn spam_condition() -> String {
    format!("m.spam_score >= {SPAM_THRESHOLD}")
}

impl Store {
    /// Postanın sütunlardaki istenmeyen kararı (eşiğin altındaysa `None`).
    /// Engellenenler listesi burada bilinmez.
    pub fn spam_verdict(&self, id: MessageId) -> AppResult<Option<SpamVerdict>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT spam_score, spam_reasons FROM messages WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
            .and_then(|(score, reasons)| verdict_of(score, &reasons, false)))
        })
    }

    /// Engellenen göndericiler (`Settings::blocked`); ayarın tamamı
    /// çözülmeden okunur. Kayıt yoksa ya da okunamazsa boş liste.
    pub fn blocked_senders(&self) -> AppResult<Vec<String>> {
        let json: Option<String> = self.with(|c| {
            Ok(c.query_row(
                "SELECT json_extract(json, '$.blocked') FROM settings WHERE id = 1",
                [],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
        })?;
        Ok(json
            .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
            .map(mh_core::normalize_blocked)
            .unwrap_or_default())
    }

    /// Gönderici bu hesapta kabul edilmiş mi (elekten kabul ya da "Güvenli")?
    pub fn sender_accepted(&self, account_id: &str, email: &str) -> AppResult<bool> {
        self.with(|c| sender_accepted(c, account_id, email))
    }

    /// Adres hesaplarımızdan birinin adresi mi? Kendi adresimiz engellenmez.
    pub fn is_own_address(&self, email: &str) -> AppResult<bool> {
        let email = normalize_email(email);
        self.with(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE lower(trim(email)) = ?1)",
                [email],
                |r| r.get(0),
            )
        })
    }

    /// Bu hesabın gelen kutusunda, `email` adresinden gelen postalar
    /// (yeniden eskiye; elekte bekleyenler ve ertelenenler dahil). Adres
    /// büyük/küçük harf farkı gözetmeden tam eşleşir.
    pub fn inbox_messages_from(&self, account_id: &str, email: &str) -> AppResult<Vec<MessageId>> {
        self.messages_by_sender(Some(account_id), email, &SenderScope::Inbox)
    }

    /// "Güvenli / İstenmeyen değil": göndericiyi bu hesapta kabul edilmiş
    /// sayar (`known_senders`, kaynak `accepted`; `sent` kaydı da yükseltilir)
    /// ve o göndericinin bu hesaptaki postalarında puanı sıfırlar. İçeriği
    /// değişen kutuları döner. Bozuk adreste bir şey yapmaz.
    pub fn trust_sender(
        &self,
        account_id: &str,
        email: &str,
        now: Millis,
    ) -> AppResult<Vec<String>> {
        let email = normalize_email(email);
        if !crate::is_full_address(&email) {
            return Ok(Vec::new());
        }
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO known_senders (account_id, email, source, at) VALUES (?1, ?2, 'accepted', ?3)
                 ON CONFLICT(account_id, email) DO UPDATE SET source = 'accepted', at = excluded.at",
                params![account_id, email, now],
            )?;
            // Puanlı postalar azdır (`messages_spam` kısmi dizini).
            let rows: Vec<(MessageId, String)> = {
                let mut st = tx.prepare_cached(
                    "SELECT id, mailbox_id FROM messages
                     WHERE account_id = ?1 AND spam_score > 0
                       AND lower(json_extract(from_addr, '$.email')) = ?2",
                )?;
                let rows = st.query_map(params![account_id, email], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut st = tx.prepare_cached(
                "UPDATE messages SET spam_score = 0, spam_reasons = '' WHERE id = ?1",
            )?;
            let mut touched = Vec::with_capacity(rows.len());
            for (id, mailbox) in rows {
                st.execute([id])?;
                touched.push(mailbox);
            }
            touched.sort();
            touched.dedup();
            Ok(touched)
        })
    }

    /// Göndericinin bu hesaptaki kabulünü geri alır (engellenince): `accepted`
    /// kaydı silinir, `sent` kaydına dokunulmaz. Kayıt silindiyse `true`.
    pub fn untrust_sender(&self, account_id: &str, email: &str) -> AppResult<bool> {
        let email = normalize_email(email);
        self.with(|c| {
            Ok(c.execute(
                "DELETE FROM known_senders WHERE account_id = ?1 AND email = ?2 AND source = 'accepted'",
                params![account_id, email],
            )? > 0)
        })
    }

    /// Kod süresi dolan postalar: kod içeren, gelen kutusunda duran ve
    /// `before` anından eski olanlar (eskiden yeniye, tur başına en çok
    /// [`EXPIRED_LIMIT`]). Yıldızlı ve sabitlenmiş postalar kullanıcının
    /// sakladığı postalardır; listeye girmez.
    pub fn expired_otp_messages(&self, before: Millis) -> AppResult<Vec<MessageId>> {
        self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT m.id FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE m.otp IS NOT NULL AND b.role = 'inbox' AND m.date < ?1
                   AND m.starred = 0 AND m.pinned = 0
                 ORDER BY m.date, m.id LIMIT ?2",
            )?;
            let rows = st.query_map(params![before, EXPIRED_LIMIT], |r| r.get(0))?;
            rows.collect()
        })
    }

    /// Son 30 günün gövdesi yerelde olan postalarında kodu ve puanı bir kez
    /// hesaplar (önceki sürümlerden kalan postalar için); güncellenen posta
    /// sayısını döner. Daha önce yapılmışsa 0. Yarıda kesilirse (uygulama
    /// kapandı) bir sonraki açılışta baştan yapılır; sonuç aynıdır.
    ///
    /// Bağlantı kilidi sayfa başına kısa süre tutulur, ayrıştırma kilitsiz
    /// yapılır: eşitleme ve arayüz beklemez.
    pub fn backfill_signals(&self, now: Millis) -> AppResult<usize> {
        let done: bool = self.with(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM backfills WHERE name = ?1)",
                [BACKFILL],
                |r| r.get(0),
            )
        })?;
        if done {
            return Ok(0);
        }
        let since = now - BACKFILL_WINDOW;
        let mut after: MessageId = 0;
        let mut updated = 0;
        loop {
            let ids: Vec<MessageId> = self.with(|c| {
                let mut st = c.prepare_cached(
                    "SELECT m.id FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                     WHERE m.id > ?1 AND m.date >= ?2 AND m.body_fetched = 1
                       AND b.role NOT IN ('sent', 'drafts')
                     ORDER BY m.id LIMIT ?3",
                )?;
                let rows = st.query_map(params![after, since, BACKFILL_PAGE], |r| r.get(0))?;
                rows.collect()
            })?;
            let Some(last) = ids.last().copied() else {
                break;
            };
            after = last;
            for id in ids {
                let Some(raw) = self.raw(id)? else {
                    continue;
                };
                let Some(p) = mh_mime::parse(&raw, false) else {
                    continue;
                };
                if p.otp.is_none() && p.spam.score == 0 {
                    continue;
                }
                self.with(|c| update(c, id, p.otp.as_deref(), &p.spam))?;
                updated += 1;
            }
        }
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO backfills (name, done_at) VALUES (?1, ?2)",
                params![BACKFILL, now],
            )?;
            Ok(())
        })?;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{ListQuery, MailboxRole, Settings, SpamReason, View};

    use super::*;
    use crate::messages::tests::msg;
    use crate::messages::NewMessage;
    use crate::testutil::store_with_account;

    const DAY: Millis = 24 * 3600 * 1000;
    const HOUR: Millis = 3600 * 1000;

    fn setup() -> Store {
        let s = store_with_account("a");
        for (path, role) in [
            ("INBOX", MailboxRole::Inbox),
            ("Sent", MailboxRole::Sent),
            ("Drafts", MailboxRole::Drafts),
            ("Junk", MailboxRole::Junk),
            ("Trash", MailboxRole::Trash),
            ("Archive", MailboxRole::Archive),
        ] {
            s.upsert_mailbox("a", path, path, role, None, false)
                .unwrap();
        }
        s
    }

    fn verdict(score: u8, reasons: &[SpamReason]) -> SpamVerdict {
        SpamVerdict {
            score,
            reasons: reasons.to_vec(),
        }
    }

    /// Şüpheli posta: kimlik doğrulama başarısız, `from` göndericisinden.
    fn suspicious(mailbox: &str, subject: &str, date: Millis, from: &str) -> NewMessage {
        let mut m = msg("a", mailbox, subject, date);
        m.from = Address::new(Some("Banka"), from);
        m.spam = verdict(65, &[SpamReason::AuthFailed, SpamReason::ReplyToMismatch]);
        m
    }

    fn columns(s: &Store, id: MessageId) -> (Option<String>, i64, String) {
        s.with(|c| {
            c.query_row(
                "SELECT otp, spam_score, spam_reasons FROM messages WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .unwrap()
    }

    /// Çift uzantılı ek ve sunucu işareti taşıyan, gövdesinde kod olan posta.
    const RAW: &str = "X-Spam-Flag: YES\r\nFrom: Banka <bildirim@banka.example>\r\nSubject: Hesap bildirimi\r\nMessage-ID: <b1@banka.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nDogrulama kodunuz: 604218\r\n--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"dekont.pdf.exe\"\r\n\r\nMZ\r\n--b--\r\n";

    #[test]
    fn alimda_kod_ve_puan_yazilir() {
        let s = setup();
        let mut m =
            NewMessage::from_raw(RAW.as_bytes().to_vec(), false, "a", "a:INBOX", 5).unwrap();
        m.uid = Some(1);
        let id = s.upsert_message(&m).unwrap().id;
        assert_eq!(
            columns(&s, id),
            (
                Some("604218".into()),
                100,
                "serverFlagged,riskyAttachment".into()
            )
        );
        let row = s.summary(id).unwrap().unwrap();
        assert_eq!(row.otp.as_deref(), Some("604218"));
        assert!(row.spam);
        // Liste sorgusu da aynı alanları döner.
        let page = s
            .list(
                &ListQuery {
                    view: View::Inbox,
                    account_id: None,
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 10,
                },
                100,
            )
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].otp.as_deref(), Some("604218"));
        assert!(page.items[0].spam);
        // Okuma alanı: nedenler ağırlık sırasıyla.
        let d = s.detail(id, &[]).unwrap();
        assert_eq!(
            d.spam_verdict,
            Some(verdict(
                100,
                &[SpamReason::ServerFlagged, SpamReason::RiskyAttachment]
            ))
        );
        assert!(d.summary.spam);
        assert_eq!(s.spam_verdict(id).unwrap(), d.spam_verdict);
        assert_eq!(s.spam_verdict(9_999).unwrap(), None);

        // Eşiğin altındaki puan saklanır ama "şüpheli" sayılmaz.
        let mut low = msg("a", "a:INBOX", "Form", 6);
        low.spam = verdict(
            35,
            &[SpamReason::RiskyAttachment, SpamReason::ReplyToMismatch],
        );
        let low = s.upsert_message(&low).unwrap().id;
        assert_eq!(columns(&s, low).1, 35);
        assert!(!s.summary(low).unwrap().unwrap().spam);
        assert_eq!(s.detail(low, &[]).unwrap().spam_verdict, None);
        assert_eq!(s.spam_verdict(low).unwrap(), None);
    }

    #[test]
    fn kendi_postamizda_kod_ve_puan_tutulmaz() {
        let s = setup();
        for (i, mailbox) in ["a:Sent", "a:Drafts"].into_iter().enumerate() {
            let mut m =
                NewMessage::from_raw(RAW.as_bytes().to_vec(), false, "a", mailbox, 5).unwrap();
            m.uid = Some(i as u32 + 1);
            let id = s.upsert_message(&m).unwrap().id;
            assert_eq!(columns(&s, id), (None, 0, String::new()), "{mailbox}");
            let d = s.detail(id, &[]).unwrap();
            assert_eq!((d.summary.otp.clone(), d.summary.spam), (None, false));
            assert_eq!(d.spam_verdict, None);
        }
    }

    #[test]
    fn govde_sonradan_inince_kod_ve_puan_yenilenir() {
        let s = setup();
        // Yalnız başlık: konuda kod yok, ek adı bilinmiyor.
        let mut m = NewMessage::from_raw(RAW.as_bytes().to_vec(), true, "a", "a:INBOX", 5).unwrap();
        m.uid = Some(1);
        let id = s.upsert_message(&m).unwrap().id;
        assert_eq!(columns(&s, id), (None, 60, "serverFlagged".into()));
        // Gövde `fetch_body` ile indi.
        s.store_body(id, RAW.as_bytes()).unwrap();
        assert_eq!(
            columns(&s, id),
            (
                Some("604218".into()),
                100,
                "serverFlagged,riskyAttachment".into()
            )
        );

        // Gövde eşitlemeyle (aynı UID, bu kez tam posta) gelirse de yenilenir.
        let mut head =
            NewMessage::from_raw(RAW.as_bytes().to_vec(), true, "a", "a:INBOX", 5).unwrap();
        head.uid = Some(2);
        head.message_id_header = Some("b2@banka.example".into());
        let id2 = s.upsert_message(&head).unwrap().id;
        assert_eq!(columns(&s, id2).0, None);
        let mut full =
            NewMessage::from_raw(RAW.as_bytes().to_vec(), false, "a", "a:INBOX", 5).unwrap();
        full.uid = Some(2);
        let again = s.upsert_message(&full).unwrap();
        assert_eq!((again.id, again.inserted), (id2, false));
        assert_eq!(columns(&s, id2).0.as_deref(), Some("604218"));
        assert_eq!(columns(&s, id2).1, 100);

        // Ayrıştırılamayan gövde sinyallere dokunmaz.
        s.store_parsed_body(id, b"", None).unwrap();
        assert_eq!(columns(&s, id).0.as_deref(), Some("604218"));

        // Gövdede kod yoksa konu/önizlemeden bulunan kod silinir.
        let mut m = msg("a", "a:INBOX", "Selam", 9);
        m.otp = Some("111222".into());
        let id3 = s.upsert_message(&m).unwrap().id;
        assert_eq!(columns(&s, id3).0.as_deref(), Some("111222"));
        s.store_body(
            id3,
            b"From: a@b.com\r\nSubject: Selam\r\n\r\nYarin gorusuruz.\r\n",
        )
        .unwrap();
        assert_eq!(columns(&s, id3), (None, 0, String::new()));
    }

    #[test]
    fn kabul_edilen_gondericide_puan_sifir() {
        let s = setup();
        // Elekten kabul edilmiş gönderici.
        s.screen_message(
            s.upsert_message(&msg("a", "a:INBOX", "İlk", 1)).unwrap().id,
            "a",
            "Bildirim@Banka.Example",
            1,
        )
        .unwrap();
        s.screener_decide("a", "bildirim@banka.example", true, 2)
            .unwrap();
        assert!(s.sender_accepted("a", " BILDIRIM@banka.example ").unwrap());
        let id = s
            .upsert_message(&suspicious("a:INBOX", "Kabul", 3, "bildirim@banka.example"))
            .unwrap()
            .id;
        assert_eq!(columns(&s, id), (None, 0, String::new()));
        assert!(!s.summary(id).unwrap().unwrap().spam);
        // Gövde sonradan inince de puan 0 kalır; kod yazılır.
        s.store_body(id, RAW.as_bytes()).unwrap();
        assert_eq!(columns(&s, id), (Some("604218".into()), 0, String::new()));

        // Yalnız yazıştığımız (kaynak `sent`) gönderici muaf değildir.
        let mut sent = msg("a", "a:Sent", "Teklif", 4);
        sent.to = vec![Address::new(None, "kerem@ornek.com")];
        s.upsert_message(&sent).unwrap();
        assert!(s.sender_known("a", "kerem@ornek.com").unwrap());
        assert!(!s.sender_accepted("a", "kerem@ornek.com").unwrap());
        let id = s
            .upsert_message(&suspicious("a:INBOX", "Yazışılan", 5, "kerem@ornek.com"))
            .unwrap()
            .id;
        assert_eq!(columns(&s, id).1, 65);
        // Başka hesaptaki kabul bu hesabı etkilemez.
        assert!(!s.sender_accepted("b", "bildirim@banka.example").unwrap());
    }

    #[test]
    fn guvenli_gonderici_puani_sifirlar() {
        let s = setup();
        let a = s
            .upsert_message(&suspicious("a:INBOX", "Bir", 1, "Bildirim@Banka.example"))
            .unwrap()
            .id;
        let b = s
            .upsert_message(&suspicious("a:Junk", "İki", 2, "bildirim@banka.example"))
            .unwrap()
            .id;
        let other = s
            .upsert_message(&suspicious("a:INBOX", "Başkası", 3, "x@kotu.example"))
            .unwrap()
            .id;
        // Kod sıfırlamadan etkilenmez.
        s.with(|c| {
            c.execute("UPDATE messages SET otp = '123456' WHERE id = ?1", [a])?;
            Ok(())
        })
        .unwrap();

        let touched = s.trust_sender("a", " bildirim@BANKA.example ", 10).unwrap();
        assert_eq!(touched, ["a:INBOX", "a:Junk"]);
        assert_eq!(columns(&s, a), (Some("123456".into()), 0, String::new()));
        assert_eq!(columns(&s, b).1, 0);
        assert_eq!(columns(&s, other).1, 65, "başka gönderici etkilenmez");
        assert!(s.sender_accepted("a", "bildirim@banka.example").unwrap());
        // İkinci kez: değişen kutu yok.
        assert!(s
            .trust_sender("a", "bildirim@banka.example", 11)
            .unwrap()
            .is_empty());
        // Sonraki postası puansız yazılır.
        let next = s
            .upsert_message(&suspicious("a:INBOX", "Üç", 4, "bildirim@banka.example"))
            .unwrap()
            .id;
        assert_eq!(columns(&s, next).1, 0);

        // `sent` kaydı `accepted`e yükselir.
        let mut sent = msg("a", "a:Sent", "Teklif", 5);
        sent.to = vec![Address::new(None, "kerem@ornek.com")];
        s.upsert_message(&sent).unwrap();
        assert!(!s.sender_accepted("a", "kerem@ornek.com").unwrap());
        s.trust_sender("a", "kerem@ornek.com", 12).unwrap();
        assert!(s.sender_accepted("a", "kerem@ornek.com").unwrap());

        // Bozuk adres: bir şey yapılmaz.
        assert!(s.trust_sender("a", "com", 13).unwrap().is_empty());
        assert!(!s.sender_accepted("a", "com").unwrap());

        // Kabul geri alınır (engelleme): yalnız `accepted` kaydı silinir.
        assert!(s.untrust_sender("a", "Bildirim@banka.example").unwrap());
        assert!(!s.untrust_sender("a", "bildirim@banka.example").unwrap());
        assert!(!s.sender_accepted("a", "bildirim@banka.example").unwrap());
        assert!(!s.sender_known("a", "bildirim@banka.example").unwrap());
        let again = s
            .upsert_message(&suspicious("a:INBOX", "Dört", 6, "bildirim@banka.example"))
            .unwrap()
            .id;
        assert_eq!(columns(&s, again).1, 65);
    }

    #[test]
    fn gondericinin_gelen_kutusu_postalari() {
        let s = setup();
        let from = |mailbox: &str, subject: &str, date, email: &str| {
            let mut m = msg("a", mailbox, subject, date);
            m.from = Address::new(Some("Reklam"), email);
            s.upsert_message(&m).unwrap().id
        };
        let a = from("a:INBOX", "Bir", 1, "reklam@firma.com");
        let b = from("a:INBOX", "İki", 2, "Reklam@Firma.COM");
        from("a:Archive", "Arşivde", 3, "reklam@firma.com");
        from("a:Junk", "İstenmeyende", 4, "reklam@firma.com");
        from("a:INBOX", "Başkası", 5, "baska@firma.com");
        // Adı adresi içeren başka gönderici eşleşmez.
        let mut tricky = msg("a", "a:INBOX", "Ad", 6);
        tricky.from = Address::new(Some("reklam@firma.com"), "x@kotu.example");
        s.upsert_message(&tricky).unwrap();
        assert_eq!(
            s.inbox_messages_from("a", " REKLAM@firma.com ").unwrap(),
            vec![b, a]
        );
        assert!(s
            .inbox_messages_from("b", "reklam@firma.com")
            .unwrap()
            .is_empty());
        assert!(s.inbox_messages_from("a", "").unwrap().is_empty());
    }

    #[test]
    fn engellenen_gonderici_okuma_alaninda() {
        let s = setup();
        let clean = {
            let mut m = msg("a", "a:Junk", "Kampanya", 1);
            m.from = Address::new(Some("Reklam"), "reklam@firma.com");
            s.upsert_message(&m).unwrap().id
        };
        let scored = s
            .upsert_message(&suspicious("a:INBOX", "Askıda", 2, "x@posta.kotu.example"))
            .unwrap()
            .id;
        assert_eq!(s.detail(clean, &[]).unwrap().spam_verdict, None);
        assert!(s.blocked_senders().unwrap().is_empty(), "ayar kaydı yok");

        s.set_settings(&Settings {
            blocked: vec!["reklam@firma.com".into(), "@kotu.example".into()],
            ..Settings::default()
        })
        .unwrap();
        assert_eq!(
            s.blocked_senders().unwrap(),
            ["reklam@firma.com", "@kotu.example"]
        );
        // Engel sütuna yazılmaz; okuma anında eklenir.
        let d = s.detail(clean, &[]).unwrap();
        assert_eq!(d.spam_verdict, Some(verdict(100, &[SpamReason::Blocked])));
        assert!(d.summary.spam);
        assert_eq!(columns(&s, clean).1, 0);
        assert!(!s.summary(clean).unwrap().unwrap().spam);
        // Alan adı engeli alt alan adını kapsar; öteki nedenler korunur.
        assert_eq!(
            s.detail(scored, &[]).unwrap().spam_verdict,
            Some(verdict(
                100,
                &[
                    SpamReason::Blocked,
                    SpamReason::AuthFailed,
                    SpamReason::ReplyToMismatch
                ]
            ))
        );
        // "Güvenli" denen gönderici alan adı engelinden muaftır.
        s.trust_sender("a", "x@posta.kotu.example", 5).unwrap();
        assert_eq!(s.detail(scored, &[]).unwrap().spam_verdict, None);
        // Engel kalkınca uyarı da kalkar (yeniden puanlama gerekmez).
        s.set_settings(&Settings::default()).unwrap();
        assert_eq!(s.detail(clean, &[]).unwrap().spam_verdict, None);
        // Kendi postamızda engel uyarısı yok.
        s.set_settings(&Settings {
            blocked: vec!["@ornek.com.tr".into()],
            ..Settings::default()
        })
        .unwrap();
        let mut own = msg("a", "a:Sent", "Giden", 6);
        own.from = Address::new(None, "a@ornek.com.tr");
        let own = s.upsert_message(&own).unwrap().id;
        assert_eq!(s.detail(own, &[]).unwrap().spam_verdict, None);
        // Kendi adresimizden gelen kutusuna düşen posta da (alan adı engelli
        // olsa bile) engelli sayılmaz; aynı alan adındaki başkası sayılır.
        assert!(s.is_own_address(" A@Ornek.com.tr ").unwrap());
        assert!(!s.is_own_address("baska@ornek.com.tr").unwrap());
        let mut me = msg("a", "a:INBOX", "Kendime not", 7);
        me.from = Address::new(None, "a@ornek.com.tr");
        let me = s.upsert_message(&me).unwrap().id;
        assert_eq!(s.detail(me, &[]).unwrap().spam_verdict, None);
        let mut colleague = msg("a", "a:INBOX", "Selam", 8);
        colleague.from = Address::new(None, "baska@ornek.com.tr");
        let colleague = s.upsert_message(&colleague).unwrap().id;
        assert_eq!(
            s.detail(colleague, &[]).unwrap().spam_verdict,
            Some(verdict(100, &[SpamReason::Blocked]))
        );
    }

    #[test]
    fn kod_suresi_dolan_postalar() {
        let s = setup();
        let now = 100 * DAY;
        let code = |mailbox: &str, subject: &str, age: Millis| {
            let mut m = msg("a", mailbox, subject, now - age);
            m.otp = Some("482913".into());
            s.upsert_message(&m).unwrap().id
        };
        let old = code("a:INBOX", "Eski kod", 3 * HOUR);
        let older = code("a:INBOX", "Daha eski kod", 30 * HOUR);
        let fresh = code("a:INBOX", "Taze kod", 10 * 60 * 1000);
        let archived = code("a:Archive", "Arşivdeki kod", 3 * HOUR);
        let trashed = code("a:Trash", "Çöpteki kod", 3 * HOUR);
        let starred = code("a:INBOX", "Yıldızlı kod", 3 * HOUR);
        let pinned = code("a:INBOX", "Sabit kod", 3 * HOUR);
        s.set_flags(&[starred], None, Some(true)).unwrap();
        s.set_pinned(&[pinned], true).unwrap();
        // Kodsuz eski posta.
        let plain = s
            .upsert_message(&msg("a", "a:INBOX", "Eski posta", now - 3 * HOUR))
            .unwrap()
            .id;

        // 1 saat: eskiden yeniye.
        let expired = s.expired_otp_messages(now - HOUR).unwrap();
        assert_eq!(expired, vec![older, old]);
        for id in [fresh, archived, trashed, starred, pinned, plain] {
            assert!(!expired.contains(&id));
        }
        // 1 gün: yalnız en eskisi.
        assert_eq!(s.expired_otp_messages(now - DAY).unwrap(), vec![older]);
        // Taşınan posta bir daha listelenmez.
        s.move_local(&[older, old], "a:Trash").unwrap();
        assert!(s.expired_otp_messages(now - HOUR).unwrap().is_empty());
    }

    #[test]
    fn geri_doldurma_bir_kez_ve_son_otuz_gun() {
        let s = setup();
        let now = 100 * DAY;
        // Eski sürümden kalan satırlar: gövde var, sütunlar boş.
        let insert = |mailbox: &str, uid: u32, age: Millis, with_body: bool| {
            let mut m =
                NewMessage::from_raw(RAW.as_bytes().to_vec(), !with_body, "a", mailbox, 5).unwrap();
            m.uid = Some(uid);
            m.date = now - age;
            m.message_id_header = Some(format!("g{uid}@banka.example"));
            let id = s.upsert_message(&m).unwrap().id;
            s.with(|c| {
                c.execute(
                    "UPDATE messages SET otp = NULL, spam_score = 0, spam_reasons = '' WHERE id = ?1",
                    [id],
                )?;
                Ok(())
            })
            .unwrap();
            id
        };
        let recent = insert("a:INBOX", 1, DAY, true);
        let edge = insert("a:Archive", 2, 29 * DAY, true);
        let old = insert("a:INBOX", 3, 31 * DAY, true);
        let headers = insert("a:INBOX", 4, DAY, false);
        let own = insert("a:Sent", 5, DAY, true);
        // Kabul edilmiş göndericinin postası: kod yazılır, puan 0 kalır.
        let trusted = {
            let raw = RAW.replace("bildirim@banka.example", "guvenli@banka.example");
            let mut m = NewMessage::from_raw(raw.into_bytes(), false, "a", "a:INBOX", 5).unwrap();
            m.uid = Some(6);
            m.date = now - DAY;
            m.message_id_header = Some("g6@banka.example".into());
            let id = s.upsert_message(&m).unwrap().id;
            s.trust_sender("a", "guvenli@banka.example", 1).unwrap();
            s.with(|c| {
                c.execute("UPDATE messages SET otp = NULL WHERE id = ?1", [id])?;
                Ok(())
            })
            .unwrap();
            id
        };
        // Kodsuz, puansız posta sayılmaz.
        let mut plain = msg("a", "a:INBOX", "Selam", now - DAY);
        plain.raw = Some(b"From: a@b.com\r\nSubject: Selam\r\n\r\nMerhaba\r\n".to_vec());
        let plain = s.upsert_message(&plain).unwrap().id;

        assert_eq!(s.backfill_signals(now).unwrap(), 3);
        let full = (
            Some("604218".to_owned()),
            100,
            "serverFlagged,riskyAttachment".to_owned(),
        );
        assert_eq!(columns(&s, recent), full);
        assert_eq!(columns(&s, edge), full);
        assert_eq!(columns(&s, old), (None, 0, String::new()), "30 günden eski");
        assert_eq!(
            columns(&s, headers),
            (None, 0, String::new()),
            "gövdesi yok"
        );
        assert_eq!(columns(&s, own), (None, 0, String::new()), "kendi postamız");
        assert_eq!(
            columns(&s, trusted),
            (Some("604218".into()), 0, String::new())
        );
        assert_eq!(columns(&s, plain), (None, 0, String::new()));

        // İkinci açılışta yapılmaz: sonradan sıfırlanan sütun öyle kalır.
        s.with(|c| {
            c.execute("UPDATE messages SET otp = NULL WHERE id = ?1", [recent])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(s.backfill_signals(now + DAY).unwrap(), 0);
        assert_eq!(columns(&s, recent).0, None);
    }

    #[test]
    fn geri_doldurma_sayfalari_asar() {
        // Sayfa boyundan fazla posta: hepsi işlenir, bos depoda da işaret konur.
        let empty = setup();
        assert_eq!(empty.backfill_signals(1_000).unwrap(), 0);
        assert_eq!(empty.backfill_signals(2_000).unwrap(), 0);

        let s = setup();
        let now = 100 * DAY;
        let n = BACKFILL_PAGE as u32 + 7;
        for uid in 1..=n {
            let mut m = msg("a", "a:INBOX", "Dogrulama kodunuz", now - HOUR);
            m.uid = Some(uid);
            m.message_id_header = Some(format!("s{uid}@ornek.com"));
            m.raw = Some(
                format!(
                    "From: a@b.com\r\nSubject: Giris\r\n\r\nDogrulama kodunuz: {}\r\n",
                    100_000 + uid
                )
                .into_bytes(),
            );
            s.upsert_message(&m).unwrap();
        }
        assert_eq!(s.backfill_signals(now).unwrap(), n as usize);
        let with_code: i64 = s
            .with(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM messages WHERE otp IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(with_code, i64::from(n));
    }
}
