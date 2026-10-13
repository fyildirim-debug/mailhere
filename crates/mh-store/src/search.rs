//! Tam metin arama (FTS5) ve arama işleçleri.
//!
//! Sorgu, işleçlere (`from:`, `to:`, `konu:`, `has:ek`, `is:okunmamış`,
//! `in:arşiv`, `önce:2026-10-01` …) ve serbest metne ayrılır. Serbest metin
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
    Subject(String),
    HasAttachment,
    Unread,
    Starred,
    In(Place),
    /// Bu andan önce (yerel gün başı).
    Before(Millis),
    /// Bu andan sonra ya da bu anda (yerel gün başı).
    After(Millis),
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

/// Tek bir `anahtar:değer` parçasını işlece çevirir; tanınmazsa `None`
/// (parça düz metin sayılır).
fn filter_of(key: &str, raw: &str) -> Option<Filter> {
    let value = raw.trim_matches('"').trim();
    if value.is_empty() {
        return None;
    }
    let folded = fold_loose(value);
    Some(match fold_loose(key).as_str() {
        "from" => Filter::From(folded),
        "to" => Filter::To(folded),
        "subject" | "konu" => Filter::Subject(folded),
        "has" => match folded.as_str() {
            "attachment" | "ek" => Filter::HasAttachment,
            _ => return None,
        },
        "is" => match folded.as_str() {
            "unread" | "okunmamis" => Filter::Unread,
            "starred" | "yildizli" => Filter::Starred,
            _ => return None,
        },
        "in" => Filter::In(match role_alias(&folded) {
            Some(role) => Place::Role(role),
            None => Place::Name(folded),
        }),
        "before" | "once" => Filter::Before(day_start(value)?),
        "after" | "sonra" => Filter::After(day_start(value)?),
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
    /// verilmedikçe çöp ve istenmeyen kutuları hariçtir.
    pub fn search(&self, q: &str, limit: u32) -> AppResult<Vec<MessageSummary>> {
        let parsed = parse_query(q);
        let text = fts_query(&parsed.text);
        if text.is_none() && parsed.filters.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 200) as i64;
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
                    Filter::Subject(v) => {
                        sql += " AND instr(mh_fold(m.subject), ?) > 0";
                        p.push(Value::Text(v.clone()));
                    }
                    Filter::HasAttachment => sql += " AND m.has_attachments = 1",
                    Filter::Unread => sql += " AND m.unread = 1",
                    Filter::Starred => sql += " AND m.starred = 1",
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
                }
            }
            if !placed {
                sql += " AND b.role NOT IN ('trash', 'junk')";
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
            s.search(q, 10)
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
            s.search(q, 50)
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
