//! Tam metin arama (FTS5) ve arama işleçleri.
//!
//! Sorgu, işleçlere (`from:`, `to:`, `konu:`, `has:ek`, `has:kod`,
//! `is:okunmamış`, `is:şüpheli`, `in:arşiv`, `önce:2026-10-01` …) ve serbest
//! metne ayrılır. Serbest metin
//! FTS dizininde aranır; işleçler SQL koşuluna çevrilir. Kullanıcı girdisi
//! SQL'e hiçbir zaman metin olarak eklenmez, hep parametreyle gider.

use chrono::{Local, NaiveDate, TimeZone};
use mh_core::{AppResult, MailboxRole, MessageSummary, Millis};
use mh_mime::fold_tr;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OptionalExtension};

use crate::codec::role_str;
use crate::messages::{row_to_summary, SUMMARY_COLS};
use crate::Store;

/// Silinen postanın sözcükleri dizinde kalmasın: FTS5 `secure-delete`
/// açıkken silme, dizindeki girdiyi işaretlemek yerine gerçekten kaldırır.
/// İlk açılışta önceki silmelerden kalan girdiler birleştirmeyle (tek
/// seferlik `optimize`) atılır.
pub(crate) fn fts_secure_delete(conn: &Connection) -> rusqlite::Result<()> {
    let on: Option<i64> = conn
        .query_row(
            "SELECT v FROM messages_fts_config WHERE k = 'secure-delete'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if on != Some(1) {
        conn.execute_batch(
            "INSERT INTO messages_fts(messages_fts, rank) VALUES ('secure-delete', 1);
             INSERT INTO messages_fts(messages_fts) VALUES ('optimize');",
        )?;
    }
    Ok(())
}

/// Gevşek karşılaştırma için katlama: küçük harf, ı/İ → i ve aksanlı
/// harflerin aksansız karşılıkları (ç → c, ğ → g, ö → o, ş → s, ü → u,
/// â → a …). Arama dizini de aksanları yok sayar; kurallar ve arama
/// işleçleri aynı biçimde eşleşir ("sirket" "Şirket"i bulur).
pub fn fold_loose(s: &str) -> String {
    fold_tr(s)
        .chars()
        .filter_map(|c| {
            Some(match c {
                // Ayrık yazılmış birleşen aksan işaretleri atılır.
                '\u{0300}'..='\u{036F}' => return None,
                'ç' => 'c',
                'ğ' => 'g',
                'ö' | 'ô' | 'ó' | 'ò' => 'o',
                'ş' => 's',
                'ü' | 'û' | 'ú' | 'ù' => 'u',
                'â' | 'á' | 'à' | 'ä' | 'å' => 'a',
                'î' | 'í' | 'ì' | 'ï' => 'i',
                'ê' | 'é' | 'è' | 'ë' => 'e',
                c => c,
            })
        })
        .collect()
}

/// Adres listesinin (ya da tek adresin) JSON'undan katlanmış "ad adres"
/// metni; adresler satır sonuyla ayrılır.
pub(crate) fn addresses_text(json: &str) -> String {
    use mh_core::Address;
    let list: Vec<Address> = match serde_json::from_str::<serde_json::Value>(json) {
        Ok(v @ serde_json::Value::Array(_)) => serde_json::from_value(v).unwrap_or_default(),
        Ok(v @ serde_json::Value::Object(_)) => serde_json::from_value(v)
            .map(|a| vec![a])
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    list.iter()
        .map(|a| fold_loose(&format!("{} {}", a.name.as_deref().unwrap_or(""), a.email)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Arama işleçlerinin kullandığı SQL işlevleri: `mh_fold(metin)` ve
/// `mh_addrs(adres JSON'u)`.
pub(crate) fn register_functions(conn: &Connection) -> rusqlite::Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    conn.create_scalar_function("mh_fold", 1, flags, |ctx| {
        Ok(ctx.get::<Option<String>>(0)?.map(|s| fold_loose(&s)))
    })?;
    conn.create_scalar_function("mh_addrs", 1, flags, |ctx| {
        Ok(ctx
            .get::<Option<String>>(0)?
            .map(|s| addresses_text(&s))
            .unwrap_or_default())
    })
}

/// Kullanıcı girdisini güvenli bir FTS5 sorgusuna çevirir: harf/rakam
/// dışındaki her şey ayırıcıdır, her sözcük önek olarak aranır ve tümü
/// birlikte (VE) eşleşmelidir. Boş girdi `None` döner.
pub fn fts_query(input: &str) -> Option<String> {
    let folded = fold_tr(input);
    let terms: Vec<String> = folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(12)
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// Tek aramada dönen en fazla sonuç (arayüzün arama listesi bu kadar ister).
pub const SEARCH_LIMIT_MAX: u32 = 300;

/// `in:` işlecinin hedefi.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    Role(MailboxRole),
    /// Katlanmış klasör adı parçası.
    Name(String),
}

/// Arama işleci; metin değerleri katlanmıştır.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Filter {
    From(String),
    To(String),
    /// Yalnız bilgi (Cc) alıcılarında (`cc:`).
    Cc(String),
    Subject(String),
    HasAttachment,
    /// Doğrulama kodu içeren postalar (`has:kod` / `has:code`).
    HasCode,
    Unread,
    /// Okunmuş postalar (`is:okunmuş` / `is:read`).
    Read,
    Starred,
    /// Bu bayttan büyük postalar (`larger:5mb` / `büyük:`).
    Larger(u64),
    /// Bu bayttan küçük postalar (`smaller:100kb` / `küçük:`).
    Smaller(u64),
    /// Şüpheli postalar (`is:spam` / `is:şüpheli`): istenmeyen puanı eşiğin
    /// üstünde. İstenmeyen kutusu da aranır.
    Spam,
    In(Place),
    /// Bu andan önce (yerel gün başı).
    Before(Millis),
    /// Bu andan sonra ya da bu anda (yerel gün başı).
    After(Millis),
    /// Katlanmış etiket adı parçası (`tag:` / `etiket:`).
    Tag(String),
}

/// İşleçlerine ayrılmış sorgu.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ParsedQuery {
    /// FTS'ye gidecek serbest metin.
    text: String,
    filters: Vec<Filter>,
}

/// Sorguyu boşluklardan böler; tırnak içindeki boşluk bölmez.
fn tokens(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        if c == '"' {
            quoted = !quoted;
            cur.push(c);
        } else if c.is_whitespace() && !quoted {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `in:` değerinden rol (Türkçe ve İngilizce adlar).
fn role_alias(folded: &str) -> Option<MailboxRole> {
    Some(match folded {
        "inbox" | "gelen" => MailboxRole::Inbox,
        "sent" | "gonderilen" | "gonderilenler" => MailboxRole::Sent,
        "archive" | "arsiv" => MailboxRole::Archive,
        "trash" | "cop" => MailboxRole::Trash,
        "drafts" | "draft" | "taslak" | "taslaklar" => MailboxRole::Drafts,
        "junk" | "spam" | "gereksiz" => MailboxRole::Junk,
        _ => return None,
    })
}

/// "YYYY-AA-GG" (ya da "YYYY/AA/GG") gününün yerel saatle başlangıcı.
fn day_start(v: &str) -> Option<Millis> {
    let d = NaiveDate::parse_from_str(v, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(v, "%Y/%m/%d"))
        .ok()?;
    let t = Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    Some(t.timestamp_millis())
}

/// Boyut değeri: sayı ve isteğe bağlı birim (`b`, `kb`, `mb`; 1 kb = 1024
/// bayt). Birimsiz sayı bayttır; ondalık nokta ya da virgülle yazılabilir
/// ("1,5mb"). Tanınmayan değer `None` (parça düz metin sayılır).
fn size_bytes(v: &str) -> Option<u64> {
    let v = v.trim().to_ascii_lowercase().replace(',', ".");
    let (number, unit) = match v.find(|c: char| !(c.is_ascii_digit() || c == '.')) {
        Some(i) => v.split_at(i),
        None => (v.as_str(), ""),
    };
    let scale: f64 = match unit.trim() {
        "" | "b" => 1.0,
        "kb" => 1024.0,
        "mb" => 1024.0 * 1024.0,
        _ => return None,
    };
    if number.is_empty() || number.matches('.').count() > 1 || number.len() > 15 {
        return None;
    }
    let n: f64 = number.parse().ok()?;
    let bytes = n * scale;
    (bytes.is_finite() && bytes >= 0.0).then_some(bytes.round() as u64)
}

/// Tek bir `anahtar:değer` parçasını işlece çevirir; tanınmazsa `None`
/// (parça düz metin sayılır).
fn filter_of(key: &str, raw: &str) -> Option<Filter> {
    let value = raw.trim_matches('"').trim();
    if value.is_empty() {
        return None;
    }
    let folded = fold_loose(value);
    Some(match fold_loose(key).as_str() {
        "from" | "kimden" => Filter::From(folded),
        "to" | "kime" => Filter::To(folded),
        "cc" => Filter::Cc(folded),
        "subject" | "konu" => Filter::Subject(folded),
        "larger" | "buyuk" => Filter::Larger(size_bytes(value)?),
        "smaller" | "kucuk" => Filter::Smaller(size_bytes(value)?),
        "has" => match folded.as_str() {
            "attachment" | "ek" => Filter::HasAttachment,
            "code" | "kod" => Filter::HasCode,
            _ => return None,
        },
        "is" => match folded.as_str() {
            "unread" | "okunmamis" => Filter::Unread,
            "read" | "okunmus" => Filter::Read,
            "starred" | "yildizli" => Filter::Starred,
            "spam" | "supheli" => Filter::Spam,
            _ => return None,
        },
        "in" => Filter::In(match role_alias(&folded) {
            Some(role) => Place::Role(role),
            None => Place::Name(folded),
        }),
        "before" | "once" => Filter::Before(day_start(value)?),
        "after" | "sonra" => Filter::After(day_start(value)?),
        "tag" | "etiket" => Filter::Tag(folded),
        _ => return None,
    })
}

fn parse_query(q: &str) -> ParsedQuery {
    let mut out = ParsedQuery::default();
    let mut text: Vec<String> = Vec::new();
    for t in tokens(q) {
        let filter = t
            .split_once(':')
            .filter(|(k, _)| !k.is_empty() && !k.contains('"'))
            .and_then(|(k, v)| filter_of(k, v));
        match filter {
            Some(f) => out.filters.push(f),
            None => text.push(t),
        }
    }
    out.text = text.join(" ");
    out
}

impl Store {
    /// Konu, gönderen ve gövdede arar; arama işleçlerini uygular. Serbest
    /// metin varsa en alakalı ve yeni önce, yoksa yeniden eskiye. `in:`
    /// verilmedikçe çöp ve istenmeyen kutuları hariçtir; `is:şüpheli`
    /// istenmeyen kutusunu da arar (taşınan şüpheli postalar oradadır).
    ///
    /// `account_ids` verilirse yalnız o hesaplar aranır; süzme SQL'de, sınırdan
    /// (`limit`) önce uygulanır. `None` tüm hesaplar, boş liste sonuç yok.
    pub fn search(
        &self,
        q: &str,
        limit: u32,
        account_ids: Option<&[String]>,
    ) -> AppResult<Vec<MessageSummary>> {
        let parsed = parse_query(q);
        let text = fts_query(&parsed.text);
        if text.is_none() && parsed.filters.is_empty() {
            return Ok(Vec::new());
        }
        if account_ids.is_some_and(<[String]>::is_empty) {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, SEARCH_LIMIT_MAX) as i64;
        self.with(|c| {
            let mut sql = match &text {
                Some(_) => format!(
                    "SELECT {SUMMARY_COLS} FROM messages_fts f
                     JOIN messages m ON m.id = f.rowid
                     JOIN mailboxes b ON b.id = m.mailbox_id
                     WHERE messages_fts MATCH ?"
                ),
                None => format!(
                    "SELECT {SUMMARY_COLS} FROM messages m
                     JOIN mailboxes b ON b.id = m.mailbox_id WHERE 1 = 1"
                ),
            };
            let mut p: Vec<Value> = text.iter().cloned().map(Value::Text).collect();
            let mut placed = false;
            let mut junk_too = false;
            for f in &parsed.filters {
                match f {
                    Filter::From(v) => {
                        sql += " AND instr(mh_addrs(m.from_addr), ?) > 0";
                        p.push(Value::Text(v.clone()));
                    }
                    Filter::To(v) => {
                        sql += " AND (instr(mh_addrs(m.to_addrs), ?) > 0
                                  OR instr(mh_addrs(m.cc_addrs), ?) > 0)";
                        p.push(Value::Text(v.clone()));
                        p.push(Value::Text(v.clone()));
                    }
                    Filter::Cc(v) => {
                        sql += " AND instr(mh_addrs(m.cc_addrs), ?) > 0";
                        p.push(Value::Text(v.clone()));
                    }
                    Filter::Subject(v) => {
                        sql += " AND instr(mh_fold(m.subject), ?) > 0";
                        p.push(Value::Text(v.clone()));
                    }
                    Filter::HasAttachment => sql += " AND m.has_attachments = 1",
                    Filter::HasCode => sql += " AND m.otp IS NOT NULL",
                    Filter::Unread => sql += " AND m.unread = 1",
                    Filter::Read => sql += " AND m.unread = 0",
                    Filter::Larger(bytes) => {
                        sql += " AND m.size > ?";
                        p.push(Value::Integer((*bytes).min(i64::MAX as u64) as i64));
                    }
                    Filter::Smaller(bytes) => {
                        sql += " AND m.size < ?";
                        p.push(Value::Integer((*bytes).min(i64::MAX as u64) as i64));
                    }
                    Filter::Starred => sql += " AND m.starred = 1",
                    Filter::Spam => {
                        junk_too = true;
                        sql += " AND ";
                        sql += &crate::signals::spam_condition();
                    }
                    Filter::In(Place::Role(role)) => {
                        placed = true;
                        sql += " AND b.role = ?";
                        p.push(Value::Text(role_str(*role).into()));
                    }
                    Filter::In(Place::Name(name)) => {
                        placed = true;
                        let ids = mailboxes_named(c, name)?;
                        if ids.is_empty() {
                            return Ok(Vec::new());
                        }
                        sql +=
                            &format!(" AND m.mailbox_id IN ({})", vec!["?"; ids.len()].join(","));
                        p.extend(ids.into_iter().map(Value::Text));
                    }
                    Filter::Before(at) => {
                        sql += " AND m.date < ?";
                        p.push(Value::Integer(*at));
                    }
                    Filter::After(at) => {
                        sql += " AND m.date >= ?";
                        p.push(Value::Integer(*at));
                    }
                    Filter::Tag(name) => {
                        let ids = tags_named(c, name)?;
                        if ids.is_empty() {
                            return Ok(Vec::new());
                        }
                        sql += &format!(
                            " AND EXISTS(SELECT 1 FROM message_tags mt WHERE mt.message_id = m.id
                                         AND mt.tag_id IN ({}))",
                            vec!["?"; ids.len()].join(",")
                        );
                        p.extend(ids.into_iter().map(Value::Text));
                    }
                }
            }
            if let Some(accounts) = account_ids {
                sql += &format!(
                    " AND m.account_id IN ({})",
                    vec!["?"; accounts.len()].join(",")
                );
                p.extend(accounts.iter().cloned().map(Value::Text));
            }
            if !placed {
                sql += if junk_too {
                    " AND b.role != 'trash'"
                } else {
                    " AND b.role NOT IN ('trash', 'junk')"
                };
            }
            sql += match text {
                Some(_) => " ORDER BY bm25(messages_fts, 4.0, 2.0, 1.0), m.date DESC LIMIT ?",
                None => " ORDER BY m.date DESC, m.id DESC LIMIT ?",
            };
            p.push(Value::Integer(limit));
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(p), row_to_summary)?;
            rows.collect()
        })
    }
}

/// Adı (katlanmış) `name` ile başlayan etiketler ("fat" → "Fatura"); tam ad
/// varsa yalnız o.
fn tags_named(c: &Connection, name: &str) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare_cached("SELECT id, name FROM tags")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut exact = Vec::new();
    let mut prefix = Vec::new();
    for row in rows {
        let (id, n) = row?;
        let folded = fold_loose(&n);
        if folded == name {
            exact.push(id);
        } else if folded.starts_with(name) {
            prefix.push(id);
        }
    }
    Ok(if exact.is_empty() { prefix } else { exact })
}

/// Adı (katlanmış) `name` parçasını içeren kutular.
fn mailboxes_named(c: &Connection, name: &str) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare_cached("SELECT id, name FROM mailboxes")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, n) = row?;
        if fold_loose(&n).contains(name) {
            out.push(id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::store_with_account;
    use mh_core::{Address, MailboxRole};

    #[test]
    fn sorgu_temizlenir() {
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("\"* - ()"), None);
        assert_eq!(fts_query("x OR y"), Some("\"x\"* \"or\"* \"y\"*".into()));
        assert_eq!(
            fts_query("bütçe NEAR(x)"),
            Some("\"bütçe\"* \"near\"* \"x\"*".into())
        );
        assert_eq!(fts_query("İSTANBUL"), Some("\"istanbul\"*".into()));
        assert_eq!(fts_query("a\"b"), Some("\"a\"* \"b\"*".into()));
    }

    #[test]
    fn gevsek_katlama() {
        assert_eq!(fold_loose("ŞİRKET Çağrı Öğün Üşü"), "sirket cagri ogun usu");
        assert_eq!(fold_loose("Iğdır"), "igdir");
        assert_eq!(fold_loose("Kâğıt"), "kagit");
        assert_eq!(fold_loose("e\u{0301}"), "e", "ayrık aksan atılır");
        assert_eq!(
            addresses_text(
                r#"[{"name":"Ayşe Demir","email":"Ayse@Eposta.com"},{"name":null,"email":"k@o.com"}]"#
            ),
            "ayse demir ayse@eposta.com\n k@o.com"
        );
        assert_eq!(
            addresses_text(r#"{"name":"Kerem","email":"k@o.com"}"#),
            "kerem k@o.com"
        );
        assert_eq!(addresses_text("bozuk"), "");
    }

    #[test]
    fn isleclere_ayrilir() {
        let p = parse_query(r#"from:"Ayşe Demir" bütçe konu:Taslak has:ek is:okunmamış in:arşiv"#);
        assert_eq!(p.text, "bütçe");
        assert_eq!(
            p.filters,
            vec![
                Filter::From("ayse demir".into()),
                Filter::Subject("taslak".into()),
                Filter::HasAttachment,
                Filter::Unread,
                Filter::In(Place::Role(MailboxRole::Archive)),
            ]
        );
        let p = parse_query("IS:Yıldızlı in:\"Proje X\" to:kerem subject:a");
        assert_eq!(
            p.filters,
            vec![
                Filter::Starred,
                Filter::In(Place::Name("proje x".into())),
                Filter::To("kerem".into()),
                Filter::Subject("a".into()),
            ]
        );
        // Bilinmeyen işleç, değersiz işleç ve bozuk tarih düz metindir.
        let p = parse_query("saat:10:30 from: has:resim is:okundu önce:dün http://ornek.com");
        assert!(p.filters.is_empty(), "{:?}", p.filters);
        assert_eq!(
            p.text,
            "saat:10:30 from: has:resim is:okundu önce:dün http://ornek.com"
        );
        let p = parse_query("önce:2026-10-01 sonra:2026/09/01 before:2026-10-02 after:2026-09-02");
        assert_eq!(p.filters.len(), 4);
        assert!(p.text.is_empty());
        assert_eq!(
            p.filters[0],
            Filter::Before(day_start("2026-10-01").unwrap())
        );
        // Kapanmayan tırnak metnin sonuna kadar sürer.
        assert_eq!(
            parse_query("from:\"Ayşe Dem").filters,
            vec![Filter::From("ayse dem".into())]
        );
        assert_eq!(
            parse_query("tag:Fatura etiket:\"Proje X\"").filters,
            vec![Filter::Tag("fatura".into()), Filter::Tag("proje x".into())]
        );
    }

    #[test]
    fn kod_ve_supheli_isleci() {
        assert_eq!(
            parse_query("has:kod has:CODE is:spam is:Şüpheli IS:supheli").filters,
            vec![
                Filter::HasCode,
                Filter::HasCode,
                Filter::Spam,
                Filter::Spam,
                Filter::Spam
            ]
        );
        let s = store_with_account("a");
        for (path, role) in [
            ("INBOX", MailboxRole::Inbox),
            ("Junk", MailboxRole::Junk),
            ("Trash", MailboxRole::Trash),
        ] {
            s.upsert_mailbox("a", path, path, role, None, false)
                .unwrap();
        }
        let add = |mailbox: &str, subject: &str, date, otp: Option<&str>, score: u8| {
            let mut m = msg("a", mailbox, subject, date);
            m.otp = otp.map(str::to_owned);
            m.spam = mh_core::SpamVerdict {
                score,
                reasons: if score > 0 {
                    vec![mh_core::SpamReason::AuthFailed]
                } else {
                    Vec::new()
                },
            };
            s.upsert_message(&m).unwrap().id
        };
        let code = add("a:INBOX", "Giriş kodu", 10, Some("482913"), 0);
        let plain = add("a:INBOX", "Selam", 11, None, 0);
        let low = add(
            "a:INBOX",
            "Az kuşkulu",
            12,
            None,
            mh_core::SPAM_THRESHOLD - 1,
        );
        let spam = add("a:INBOX", "Hesabınız askıda", 13, None, 50);
        let junk = add("a:Junk", "Ödül kazandınız", 14, Some("111222"), 90);
        let trash = add("a:Trash", "Silinmiş kod", 15, Some("333444"), 100);
        let ids = |q: &str| {
            s.search(q, 50, None)
                .unwrap()
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        // Kod: çöp ve istenmeyen hariç (in: verilmedikçe).
        assert_eq!(ids("has:kod"), vec![code]);
        assert_eq!(ids("has:code in:spam"), vec![junk]);
        assert_eq!(ids("has:kod in:çöp"), vec![trash]);
        // Şüpheli: eşik ve üstü; istenmeyen kutusu dahil, çöp hariç.
        assert_eq!(ids("is:spam"), vec![junk, spam]);
        assert_eq!(ids("is:şüpheli in:gelen"), vec![spam]);
        assert_eq!(ids("is:spam has:kod"), vec![junk]);
        assert_eq!(ids("is:spam askıda"), vec![spam], "serbest metinle");
        assert!(!ids("selam").contains(&low) && ids("selam") == vec![plain]);
        // Özet alanları sütunlardan gelir.
        let row = |id| s.summary(id).unwrap().unwrap();
        assert_eq!(row(code).otp.as_deref(), Some("482913"));
        assert!(!row(code).spam && !row(low).spam && !row(plain).spam);
        assert!(row(spam).spam && row(junk).spam);
        assert_eq!(row(plain).otp, None);
    }

    #[test]
    fn boyut_degeri() {
        assert_eq!(size_bytes("0"), Some(0));
        assert_eq!(size_bytes("1500"), Some(1500));
        assert_eq!(size_bytes("1500b"), Some(1500));
        assert_eq!(size_bytes("100kb"), Some(100 * 1024));
        assert_eq!(size_bytes("5MB"), Some(5 * 1024 * 1024));
        assert_eq!(size_bytes("5 mb"), Some(5 * 1024 * 1024));
        assert_eq!(size_bytes("1.5mb"), Some(1_572_864));
        assert_eq!(size_bytes("1,5mb"), Some(1_572_864), "virgüllü ondalık");
        assert_eq!(size_bytes(".5kb"), Some(512));
        // Tanınmayan birim, sayı olmayan ya da anlamsız değer işleç sayılmaz.
        for bad in [
            "", "mb", "5gb", "5k", "beş", "-5mb", "1.2.3mb", "5mbx", "1e9", "..",
        ] {
            assert_eq!(size_bytes(bad), None, "{bad}");
        }
        // Aşırı büyük sayı taşmaz.
        assert_eq!(size_bytes("9999999999999999"), None);
        assert!(size_bytes("999999999999999mb").is_some());
    }

    #[test]
    fn yeni_isleclere_ayrilir() {
        let p = parse_query(
            "larger:5mb smaller:100KB büyük:1,5mb küçük:2000 cc:ayşe is:okunmuş IS:read kimden:Kerem kime:\"Ayşe D\"",
        );
        assert!(p.text.is_empty(), "{}", p.text);
        assert_eq!(
            p.filters,
            vec![
                Filter::Larger(5 * 1024 * 1024),
                Filter::Smaller(100 * 1024),
                Filter::Larger(1_572_864),
                Filter::Smaller(2000),
                Filter::Cc("ayse".into()),
                Filter::Read,
                Filter::Read,
                Filter::From("kerem".into()),
                Filter::To("ayse d".into()),
            ]
        );
        // Aksansız yazım da geçer; bozuk boyut düz metindir.
        assert_eq!(
            parse_query("buyuk:1kb kucuk:1kb is:okunmus").filters,
            vec![Filter::Larger(1024), Filter::Smaller(1024), Filter::Read]
        );
        let p = parse_query("larger:çok smaller: büyük:5gb");
        assert!(p.filters.is_empty(), "{:?}", p.filters);
        assert_eq!(p.text, "larger:çok smaller: büyük:5gb");
    }

    #[test]
    fn boyut_bilgi_okunmus_ve_es_anlamli_islecler() {
        let s = store_with_account("a");
        s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox, None, false)
            .unwrap();
        let add = |subject: &str, date, size: u64, f: &dyn Fn(&mut crate::NewMessage)| {
            let mut m = msg("a", "a:INBOX", subject, date);
            m.size = size;
            f(&mut m);
            s.upsert_message(&m).unwrap().id
        };
        let kucuk = add("Kısa not", 10, 900, &|m| {
            m.from = Address::new(Some("Kerem Öz"), "kerem@ornek.com.tr");
            m.to = vec![Address::new(Some("Furkan"), "furkan@ornek.com.tr")];
        });
        let orta = add("Sunum taslağı", 11, 200 * 1024, &|m| {
            m.from = Address::new(Some("Ayşe Demir"), "ayse@eposta.com");
            m.to = vec![Address::new(Some("Furkan"), "furkan@ornek.com.tr")];
            m.cc = vec![Address::new(Some("Kerem Öz"), "kerem@ornek.com.tr")];
        });
        let buyuk = add("Video kaydı", 12, 6 * 1024 * 1024, &|m| {
            m.from = Address::new(Some("Ayşe Demir"), "ayse@eposta.com");
            m.to = vec![Address::new(Some("Kerem Öz"), "kerem@ornek.com.tr")];
        });
        // Boyutu bilinmeyen (0) eski satır "küçük" sayılır, "büyük" sayılmaz.
        let bilinmeyen = add("Boyutsuz", 13, 0, &|_| {});
        s.set_flags(&[orta, buyuk], Some(false), None).unwrap();
        let ids = |q: &str| {
            s.search(q, 50, None)
                .unwrap()
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        // Boyut: kesin büyük / kesin küçük.
        assert_eq!(ids("larger:5mb"), vec![buyuk]);
        assert_eq!(ids("büyük:100kb"), vec![buyuk, orta]);
        assert_eq!(
            ids("larger:204800"),
            vec![buyuk],
            "eşit boyut büyük sayılmaz"
        );
        assert_eq!(ids("larger:204799"), vec![buyuk, orta]);
        assert_eq!(ids("smaller:100kb"), vec![bilinmeyen, kucuk]);
        assert_eq!(
            ids("küçük:900"),
            vec![bilinmeyen],
            "eşit boyut küçük sayılmaz"
        );
        assert_eq!(ids("larger:1kb smaller:1mb"), vec![orta]);
        assert_eq!(ids("larger:1kb sunum"), vec![orta], "serbest metinle");
        // Bilgi (Cc): yalnız bilgi alıcıları; `to:` ikisini de arar.
        assert_eq!(ids("cc:kerem"), vec![orta]);
        assert_eq!(ids("cc:\"kerem öz\""), vec![orta]);
        assert!(ids("cc:furkan").is_empty());
        assert_eq!(ids("to:kerem"), vec![buyuk, orta]);
        // Okunmuş / okunmamış.
        assert_eq!(ids("is:okunmuş"), vec![buyuk, orta]);
        assert_eq!(ids("is:read"), ids("is:okunmus"));
        assert_eq!(ids("is:unread"), vec![bilinmeyen, kucuk]);
        assert!(ids("is:read is:unread").is_empty());
        // kimden: / kime: from: / to: ile aynıdır.
        assert_eq!(ids("kimden:ayşe"), ids("from:ayse"));
        assert_eq!(ids("kimden:ayşe"), vec![buyuk, orta]);
        assert_eq!(ids("kime:kerem"), ids("to:kerem"));
        assert_eq!(ids("kimden:kerem kime:furkan"), vec![bilinmeyen, kucuk]);
    }

    #[test]
    fn hesap_suzgeci_sinirdan_once() {
        let s = store_with_account("a");
        s.insert_account(&crate::testutil::account("b")).unwrap();
        for acc in ["a", "b"] {
            s.upsert_mailbox(acc, "INBOX", "INBOX", MailboxRole::Inbox, None, false)
                .unwrap();
        }
        // `a` hesabında daha yeni 5 posta, `b` hesabında daha eski 2 posta.
        for i in 0..5 {
            s.upsert_message(&msg("a", "a:INBOX", &format!("Rapor a{i}"), 100 + i))
                .unwrap();
        }
        let b1 = s
            .upsert_message(&msg("b", "b:INBOX", "Rapor b1", 10))
            .unwrap()
            .id;
        let b2 = s
            .upsert_message(&msg("b", "b:INBOX", "Rapor b2", 11))
            .unwrap()
            .id;
        let ids = |q: &str, limit: u32, accounts: Option<&[String]>| {
            s.search(q, limit, accounts)
                .unwrap()
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        let only_b = ["b".to_string()];
        // Süzgeç yokken sınır yeni postalarla dolar: `b` hiç görünmez.
        let top = ids("is:unread", 3, None);
        assert_eq!(top.len(), 3);
        assert!(!top.contains(&b1) && !top.contains(&b2));
        // Süzme SQL'de, sınırdan önce: `b` postaları sınıra takılmaz.
        assert_eq!(ids("is:unread", 3, Some(&only_b)), vec![b2, b1]);
        assert_eq!(ids("rapor", 3, Some(&only_b)).len(), 2, "serbest metinle");
        // Yok (`None`) tüm hesaplar; boş liste sonuç yok; bilinmeyen hesap boş.
        assert_eq!(ids("rapor", 50, None).len(), 7);
        assert!(ids("rapor", 50, Some(&[])).is_empty());
        assert!(ids("rapor", 50, Some(&["yok".to_string()])).is_empty());
        let both = ["a".to_string(), "b".to_string()];
        assert_eq!(ids("rapor", 50, Some(&both)).len(), 7);
        // Hesap kimliği sorguya parametreyle girer.
        assert!(ids("rapor", 50, Some(&["' OR 1=1 --".to_string()])).is_empty());
        // Sınır üst sınırı aşamaz.
        assert_eq!(ids("rapor", 100_000, None).len(), 7);
    }

    #[test]
    fn etiket_isleci() {
        let s = store_with_account("a");
        s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox, None, false)
            .unwrap();
        let tag = |id: &str, name: &str| mh_core::Tag {
            id: id.into(),
            name: name.into(),
            color: "#4C6FD8".into(),
        };
        s.tag_insert(&tag("e1", "Fatura")).unwrap();
        s.tag_insert(&tag("e2", "Faturalar 2025")).unwrap();
        s.tag_insert(&tag("e3", "Proje Işık")).unwrap();
        let m1 = s
            .upsert_message(&msg("a", "a:INBOX", "Ekim faturası", 10))
            .unwrap()
            .id;
        let m2 = s
            .upsert_message(&msg("a", "a:INBOX", "Eski fatura", 11))
            .unwrap()
            .id;
        let m3 = s
            .upsert_message(&msg("a", "a:INBOX", "Proje notu", 12))
            .unwrap()
            .id;
        s.upsert_message(&msg("a", "a:INBOX", "Etiketsiz fatura", 13))
            .unwrap();
        s.set_message_tag(&[m1], "e1", true).unwrap();
        s.set_message_tag(&[m2], "e2", true).unwrap();
        s.set_message_tag(&[m3], "e3", true).unwrap();
        let ids = |q: &str| {
            s.search(q, 50, None)
                .unwrap()
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        // Tam ad varsa yalnız o; yoksa önek eşleşmesi.
        assert_eq!(ids("tag:fatura"), vec![m1]);
        assert_eq!(ids("tag:FATURA"), vec![m1]);
        assert_eq!(ids("tag:fat"), vec![m2, m1]);
        assert_eq!(ids("etiket:\"faturalar 2025\""), vec![m2]);
        assert_eq!(ids("tag:\"proje isik\""), vec![m3], "aksansız yazım");
        // Serbest metinle birlikte.
        assert_eq!(ids("fatura tag:fat"), vec![m2, m1]);
        assert_eq!(ids("ekim tag:fatura"), vec![m1]);
        assert!(ids("tag:yok").is_empty());
        assert!(ids("tag:\"' OR 1=1\"").is_empty());
    }

    #[test]
    fn turkce_dostu_arama() {
        let s = store_with_account("a");
        s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox, None, false)
            .unwrap();
        s.upsert_mailbox("a", "Trash", "Trash", MailboxRole::Trash, None, false)
            .unwrap();
        let mut m1 = msg("a", "a:INBOX", "Q4 bütçe taslağı", 10);
        m1.body_text = Some("Pazarlama kaleminde düşüş var.".into());
        s.upsert_message(&m1).unwrap();
        let mut m2 = msg("a", "a:INBOX", "Cumartesi kahvaltı?", 20);
        m2.from = Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com");
        m2.body_text = Some("Moda'daki yeni yere gidelim mi? IŞIK güzel.".into());
        s.upsert_message(&m2).unwrap();
        let mut gone = msg("a", "a:Trash", "Bütçe eski", 30);
        gone.body_text = Some("silindi".into());
        s.upsert_message(&gone).unwrap();

        let subj = |q: &str| {
            s.search(q, 10, None)
                .unwrap()
                .into_iter()
                .map(|m| m.subject)
                .collect::<Vec<_>>()
        };
        assert_eq!(subj("bütçe"), vec!["Q4 bütçe taslağı"]);
        assert_eq!(subj("butce"), vec!["Q4 bütçe taslağı"], "aksansız yazım");
        assert_eq!(subj("BÜTÇE"), vec!["Q4 bütçe taslağı"]);
        assert_eq!(subj("kahvalti"), vec!["Cumartesi kahvaltı?"], "ı → i");
        assert_eq!(subj("ışık"), vec!["Cumartesi kahvaltı?"]);
        assert_eq!(subj("ayse"), vec!["Cumartesi kahvaltı?"], "gönderen adı");
        assert_eq!(subj("pazar"), vec!["Q4 bütçe taslağı"], "önek");
        assert_eq!(subj("moda gidelim"), vec!["Cumartesi kahvaltı?"]);
        assert!(subj("moda bütçe").is_empty());
        assert!(subj("\")(*").is_empty());
        assert!(subj("zzz").is_empty());
    }

    /// Gün başları yerel saatle hesaplanır; postalar günlerin ortasındadır.
    fn noon(day: &str) -> Millis {
        day_start(day).unwrap() + 12 * 3_600_000
    }

    #[test]
    fn arama_islecleri() {
        let s = store_with_account("a");
        for (path, role) in [
            ("INBOX", MailboxRole::Inbox),
            ("Archive", MailboxRole::Archive),
            ("Trash", MailboxRole::Trash),
            ("Projeler/Kuzey Işığı", MailboxRole::Other),
        ] {
            s.upsert_mailbox(
                "a",
                path,
                path.rsplit('/').next().unwrap(),
                role,
                None,
                false,
            )
            .unwrap();
        }
        let mut fatura = msg("a", "a:INBOX", "Ekim faturası", noon("2026-10-05"));
        fatura.from = Address::new(Some("Şirket Muhasebe"), "fatura@sirket.com.tr");
        fatura.to = vec![Address::new(Some("Furkan"), "furkan@ornek.com.tr")];
        fatura.cc = vec![Address::new(Some("Ayşe Demir"), "ayse@ornek.com.tr")];
        fatura.has_attachments = true;
        fatura.body_text = Some("Tutar ekte.".into());
        let fatura = s.upsert_message(&fatura).unwrap().id;

        let mut kahvalti = msg("a", "a:INBOX", "Cumartesi kahvaltı?", noon("2026-09-20"));
        kahvalti.from = Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com");
        kahvalti.body_text = Some("Moda'da buluşalım; fatura bende.".into());
        let kahvalti = s.upsert_message(&kahvalti).unwrap().id;

        let mut eski = msg("a", "a:Archive", "Eylül faturası", noon("2026-09-03"));
        eski.from = Address::new(Some("Şirket Muhasebe"), "fatura@sirket.com.tr");
        let eski = s.upsert_message(&eski).unwrap().id;

        let mut proje = msg(
            "a",
            "a:Projeler/Kuzey Işığı",
            "Proje notu",
            noon("2026-10-01"),
        );
        proje.body_text = Some("fatura taslağı".into());
        let proje = s.upsert_message(&proje).unwrap().id;

        let mut cop = msg("a", "a:Trash", "Silinen fatura", noon("2026-10-06"));
        cop.from = Address::new(Some("Şirket Muhasebe"), "fatura@sirket.com.tr");
        let cop = s.upsert_message(&cop).unwrap().id;

        s.set_flags(&[fatura, eski, proje, cop], Some(false), None)
            .unwrap();
        s.set_flags(&[eski], None, Some(true)).unwrap();

        let ids = |q: &str| {
            s.search(q, 50, None)
                .unwrap()
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        // Yalnız süzgeç: tarihe göre (yeniden eskiye), çöp hariç.
        assert_eq!(ids("from:sirket"), vec![fatura, eski]);
        assert_eq!(ids("from:\"şirket muhasebe\""), vec![fatura, eski]);
        assert_eq!(ids("from:AYŞE"), vec![kahvalti]);
        assert_eq!(ids("from:fatura@sirket.com.tr"), vec![fatura, eski]);
        // Alıcı: Kime + Bilgi.
        assert_eq!(ids("to:ayse"), vec![fatura]);
        assert_eq!(ids("to:furkan"), vec![fatura, proje, kahvalti, eski]);
        assert!(ids("to:email").is_empty(), "JSON anahtarı eşleşmez");
        assert_eq!(ids("konu:fatura"), vec![fatura, eski]);
        assert_eq!(ids("subject:\"ekim fat\""), vec![fatura]);
        assert_eq!(ids("has:ek"), vec![fatura]);
        assert_eq!(ids("has:attachment"), ids("has:ek"));
        assert_eq!(ids("is:okunmamış"), vec![kahvalti]);
        assert_eq!(ids("is:okunmamis"), ids("is:unread"));
        assert_eq!(ids("is:yıldızlı"), vec![eski]);
        assert_eq!(ids("is:yildizli"), ids("is:starred"));
        // Klasör: rol adı ya da klasör adı; `in:` çöpü de arar.
        assert_eq!(ids("in:arşiv"), vec![eski]);
        assert_eq!(ids("in:archive fatura"), vec![eski]);
        assert_eq!(ids("in:gelen"), vec![fatura, kahvalti]);
        assert_eq!(ids("in:çöp"), vec![cop]);
        assert_eq!(ids("in:trash from:sirket"), vec![cop]);
        assert_eq!(ids("in:\"kuzey isigi\""), vec![proje]);
        assert!(ids("in:olmayan").is_empty());
        // Tarih: önce/sonra gün başına göre.
        assert_eq!(ids("önce:2026-10-01"), vec![kahvalti, eski]);
        assert_eq!(ids("before:2026-10-01"), ids("önce:2026-10-01"));
        assert_eq!(ids("sonra:2026-10-01"), vec![fatura, proje]);
        assert_eq!(
            ids("after:2026-09-10 before:2026-10-02"),
            vec![proje, kahvalti]
        );
        // Serbest metin + işleç birlikte.
        assert_eq!(ids("fatura from:ayse"), vec![kahvalti]);
        assert_eq!(ids("fatura has:ek"), vec![fatura]);
        assert_eq!(ids("fatura önce:2026-10-02 sonra:2026-09-30"), vec![proje]);
        // Bilinmeyen işleç düz metin: "proje:notu" → "proje" "notu".
        assert_eq!(ids("proje:notu"), vec![proje]);
        assert!(ids("renk:mavi").is_empty());
        // SQL'e metin olarak girmez.
        assert!(ids("from:\"x' OR 1=1 --\"").is_empty());
        assert!(ids("in:\"%\"").is_empty());
        assert!(ids("konu:%").is_empty());
        assert!(ids("").is_empty());
    }
}
