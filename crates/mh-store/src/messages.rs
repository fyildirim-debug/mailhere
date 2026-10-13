//! Postalar: ekleme, listeleme, bayraklar, taşıma, erteleme, konuşmalar.

use mh_core::{
    Address, AppError, AppResult, Category, Counts, ErrorCode, ListQuery, MailboxRole,
    MessageDetail, MessageId, MessageSummary, Millis, Page, ThreadEntry, View,
};
use mh_mime::{fold_tr, normalize_subject, ParsedMessage};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, OptionalExtension, Row, Transaction};

use crate::codec::{
    addr_list, category_from, category_str, from_json, role_from, role_str, sender_text, to_json,
};
use crate::Store;

/// Depoya yazılacak posta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMessage {
    pub account_id: String,
    pub mailbox_id: String,
    pub uid: Option<u32>,
    pub uidvalidity: Option<u32>,
    pub message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: String,
    pub snippet: String,
    /// Arama için düz metin; gövde henüz indirilmediyse `None`.
    pub body_text: Option<String>,
    pub date: Millis,
    pub unread: bool,
    pub starred: bool,
    pub has_attachments: bool,
    pub category: Category,
    pub size: u64,
    /// Tam ham posta; yalnız başlık indirildiyse `None`.
    pub raw: Option<Vec<u8>>,
}

impl NewMessage {
    /// Ayrıştırılmış postadan kayıt hazırlar. Tarih yoksa `fallback_date`.
    pub fn from_parsed(
        p: ParsedMessage,
        account_id: &str,
        mailbox_id: &str,
        fallback_date: Millis,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            mailbox_id: mailbox_id.into(),
            uid: None,
            uidvalidity: None,
            message_id_header: p.message_id,
            in_reply_to: p.in_reply_to,
            references: p.references,
            from: p.from,
            to: p.to,
            cc: p.cc,
            reply_to: p.reply_to,
            subject: p.subject,
            snippet: p.snippet,
            body_text: p.body_text,
            date: p.date.unwrap_or(fallback_date),
            unread: true,
            starred: false,
            has_attachments: p.has_attachments,
            category: p.category,
            size: 0,
            raw: None,
        }
    }

    /// Ham baytlardan kayıt hazırlar. `headers_only` doğruysa ham veri
    /// saklanmaz; gövde açılınca indirilir.
    pub fn from_raw(
        raw: Vec<u8>,
        headers_only: bool,
        account_id: &str,
        mailbox_id: &str,
        fallback_date: Millis,
    ) -> Option<Self> {
        let parsed = mh_mime::parse(&raw, headers_only)?;
        let mut m = Self::from_parsed(parsed, account_id, mailbox_id, fallback_date);
        m.size = raw.len() as u64;
        if !headers_only {
            m.raw = Some(raw);
        }
        Some(m)
    }
}

/// Ekleme sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpsertOutcome {
    pub id: MessageId,
    /// Yeni satır mı (bildirim kararı için)?
    pub inserted: bool,
}

/// Sunucu işlemleri için bir postanın yeri.
///
/// `mailbox_*`, `role`, `uid` ve `uidvalidity` postanın yerelde göründüğü
/// kutuyu anlatır. Taşıma sunucuda henüz onaylanmadıysa (`pending`) posta
/// sunucuda hâlâ eski kutudadır; sunucu işlemleri `server_*` alanlarını kullanır.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLocation {
    pub id: MessageId,
    pub account_id: String,
    pub mailbox_id: String,
    pub mailbox_path: String,
    pub role: MailboxRole,
    pub uid: Option<u32>,
    pub uidvalidity: Option<u32>,
    pub has_raw: bool,
    pub message_id_header: Option<String>,
    pub size: u64,
    /// Sunucu onayı bekleyen bir taşıma var.
    pub pending: bool,
    /// Postanın sunucudaki kutusu.
    pub server_path: String,
    pub server_uid: Option<u32>,
    pub server_uidvalidity: Option<u32>,
}

/// Sunucu onayı bekleyen taşıma: posta sunucuda `src_*` yerinde, yerelde
/// `dest_*` kutusunda görünüyor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMove {
    pub id: MessageId,
    pub src_mailbox_id: String,
    pub src_path: String,
    pub src_uid: Option<u32>,
    pub src_uidvalidity: Option<u32>,
    pub dest_mailbox_id: String,
    pub dest_path: String,
    pub message_id_header: Option<String>,
}

/// "<hesap>:<yol>" kimliğinden yolu çıkarır.
fn path_of(mailbox_id: &str, account_id: &str) -> String {
    mailbox_id
        .strip_prefix(account_id)
        .and_then(|r| r.strip_prefix(':'))
        .unwrap_or(mailbox_id)
        .to_owned()
}

/// `MessageLocation` için sütunlar (`messages m JOIN mailboxes b`).
pub(crate) const LOCATION_COLS: &str = "m.id, m.account_id, m.mailbox_id, b.path, b.role, m.uid,
    m.uidvalidity, m.raw IS NOT NULL, m.message_id_header, m.size, m.src_mailbox_id, m.src_uid,
    m.src_uidvalidity";

pub(crate) fn row_to_location(r: &Row<'_>) -> rusqlite::Result<MessageLocation> {
    let u32_at = |i: usize| -> rusqlite::Result<Option<u32>> {
        Ok(r.get::<_, Option<i64>>(i)?.map(|v| v as u32))
    };
    let account_id: String = r.get(1)?;
    let mailbox_path: String = r.get(3)?;
    let src: Option<String> = r.get(10)?;
    let (uid, uidvalidity) = (u32_at(5)?, u32_at(6)?);
    let (server_path, server_uid, server_uidvalidity) = match &src {
        Some(src) => (path_of(src, &account_id), u32_at(11)?, u32_at(12)?),
        None => (mailbox_path.clone(), uid, uidvalidity),
    };
    Ok(MessageLocation {
        id: r.get(0)?,
        mailbox_id: r.get(2)?,
        role: role_from(&r.get::<_, String>(4)?),
        uid,
        uidvalidity,
        has_raw: r.get(7)?,
        message_id_header: r.get(8)?,
        size: r.get::<_, i64>(9)? as u64,
        pending: src.is_some(),
        server_path,
        server_uid,
        server_uidvalidity,
        account_id,
        mailbox_path,
    })
}

/// `(kutu, uid)` başka bir satırda kayıtlıysa (yinelenen kopya) onu siler ki
/// UID bu satıra yazılabilsin.
fn free_uid(
    tx: &Transaction<'_>,
    id: MessageId,
    mailbox_id: &str,
    uid: Option<u32>,
) -> rusqlite::Result<()> {
    if let Some(uid) = uid {
        tx.execute(
            "DELETE FROM messages WHERE mailbox_id = ?1 AND uid = ?2 AND id != ?3",
            params![mailbox_id, i64::from(uid), id],
        )?;
    }
    Ok(())
}

pub(crate) const SUMMARY_COLS: &str = "m.id, m.account_id, m.mailbox_id, m.thread_id, m.from_addr, m.to_addrs,
    m.subject, m.snippet, m.date, m.unread, m.starred, m.has_attachments, m.category, m.snoozed_until";

pub(crate) fn row_to_summary(r: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    Ok(MessageSummary {
        id: r.get(0)?,
        account_id: r.get(1)?,
        mailbox_id: r.get(2)?,
        thread_id: r.get(3)?,
        from: from_json(4, &r.get::<_, String>(4)?)?,
        to: addr_list(5, &r.get::<_, String>(5)?)?,
        subject: r.get(6)?,
        snippet: r.get(7)?,
        date: r.get(8)?,
        unread: r.get(9)?,
        starred: r.get(10)?,
        has_attachments: r.get(11)?,
        category: category_from(&r.get::<_, String>(12)?),
        snoozed_until: r.get(13)?,
    })
}

fn in_list(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn ids_params(ids: &[MessageId]) -> Vec<Value> {
    ids.iter().map(|i| Value::Integer(*i)).collect()
}

/// "tarih:kimlik" imlecini çözer.
fn parse_cursor(c: &str) -> AppResult<(Millis, MessageId)> {
    c.split_once(':')
        .and_then(|(d, i)| Some((d.parse().ok()?, i.parse().ok()?)))
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Invalid,
                "Liste imleci geçersiz; listeyi baştan yükle.",
            )
        })
}

/// Adres tam mı: boş olmayan yerel kısım, `@` ve en az iki parçalı alan adı.
/// (`From: <com>` gibi bozuk başlıklar "com" adresine çözülür; bu bir alan adı
/// sanılıp tüm .com göndericilerine izin verilmesin.)
pub fn is_full_address(email: &str) -> bool {
    match email.trim().rsplit_once('@') {
        Some((local, domain)) => !local.is_empty() && is_domain(domain),
        None => false,
    }
}

/// En az iki parçalı, boş parçası ve boşluğu olmayan alan adı.
fn is_domain(d: &str) -> bool {
    let d = d.strip_suffix('.').unwrap_or(d);
    d.contains('.')
        && !d.contains('@')
        && d.split('.').all(|p| !p.is_empty())
        && !d.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Uzak içerik izin listesine yazılabilecek öğe: tam adres, "ornek.com" ya da
/// "@ornek.com". Tek parçalı alan adları ("com") kabul edilmez.
pub fn is_remote_entry(entry: &str) -> bool {
    let e = entry.trim();
    if e.contains('@') && !e.starts_with('@') {
        return is_full_address(e);
    }
    is_domain(e.strip_prefix('@').unwrap_or(e))
}

/// Gönderen uzak içerik izin listesinde mi? Liste öğesi tam adres ya da alan
/// adıdır ("ornek.com" / "@ornek.com"; alt alan adlarını da kapsar). Bozuk
/// adresli gönderen ve geçersiz liste öğeleri hiçbir zaman eşleşmez.
pub fn remote_allowed(list: &[String], email: &str) -> bool {
    let email = email.trim().to_ascii_lowercase();
    if !is_full_address(&email) {
        return false;
    }
    let domain = email.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
    list.iter().any(|entry| {
        let e = entry.trim().to_ascii_lowercase();
        if !is_remote_entry(&e) {
            return false;
        }
        if e.contains('@') && !e.starts_with('@') {
            return e == email;
        }
        let d = e.trim_start_matches('@');
        domain == d || domain.ends_with(&format!(".{d}"))
    })
}

/// Konuşma kimliği: References/In-Reply-To zincirinde bilinen bir posta varsa
/// onun konuşması; yoksa zincirin kökü; zincir yoksa (yalnız Kişiler için)
/// sadeleştirilmiş konu + hesap.
fn compute_thread_id(tx: &Transaction<'_>, m: &NewMessage) -> rusqlite::Result<String> {
    let mut chain: Vec<&str> = m.references.iter().map(String::as_str).collect();
    if let Some(irt) = m.in_reply_to.as_deref() {
        if !chain.contains(&irt) {
            chain.push(irt);
        }
    }
    let mut st = tx.prepare_cached(
        "SELECT thread_id FROM messages WHERE account_id = ?1 AND message_id_header = ?2 LIMIT 1",
    )?;
    for id in chain.iter().rev() {
        if let Some(t) = st
            .query_row(params![m.account_id, id], |r| r.get::<_, String>(0))
            .optional()?
        {
            return Ok(t);
        }
    }
    if let Some(root) = chain.first() {
        return Ok(format!("m:{root}"));
    }
    if let Some(mid) = m.message_id_header.as_deref() {
        // Yanıtı kendisinden önce gelmiş bir kök posta.
        let own = format!("m:{mid}");
        let exists: bool = tx
            .prepare_cached(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE account_id = ?1 AND thread_id = ?2)",
            )?
            .query_row(params![m.account_id, own], |r| r.get(0))?;
        if exists {
            return Ok(own);
        }
    }
    let subject = normalize_subject(&m.subject);
    if m.category == Category::People && !subject.is_empty() {
        return Ok(format!("s:{}:{subject}", m.account_id));
    }
    Ok(match m.message_id_header.as_deref() {
        Some(mid) => format!("m:{mid}"),
        None => format!("u:{}:{}:{}", m.account_id, m.from.email, m.date),
    })
}

fn insert(tx: &Transaction<'_>, m: &NewMessage) -> rusqlite::Result<MessageId> {
    insert_as(tx, m, None)
}

/// Postayı ekler; `id` verilirse satır bu kimlikle yazılır (taslağın yerine
/// yazılması kimliği korur).
pub(crate) fn insert_as(
    tx: &Transaction<'_>,
    m: &NewMessage,
    id: Option<MessageId>,
) -> rusqlite::Result<MessageId> {
    let thread_id = compute_thread_id(tx, m)?;
    tx.prepare_cached(
        "INSERT INTO messages (account_id, mailbox_id, uid, uidvalidity, message_id_header, in_reply_to, refs,
            thread_id, from_addr, to_addrs, cc_addrs, reply_to, subject, snippet, date, unread, starred,
            has_attachments, category, size, body_fetched, fts_subject, fts_sender, fts_body, raw, id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25, ?26)",
    )?
    .execute(params![
        m.account_id,
        m.mailbox_id,
        m.uid.map(i64::from),
        m.uidvalidity.map(i64::from),
        m.message_id_header,
        m.in_reply_to,
        to_json(&m.references),
        thread_id,
        to_json(&m.from),
        to_json(&m.to),
        to_json(&m.cc),
        to_json(&m.reply_to),
        m.subject,
        m.snippet,
        m.date,
        m.unread,
        m.starred,
        m.has_attachments,
        category_str(m.category),
        m.size as i64,
        m.raw.is_some(),
        fold_tr(&m.subject),
        fold_tr(&sender_text(&m.from)),
        fold_tr(m.body_text.as_deref().unwrap_or("")),
        m.raw,
        id,
    ])?;
    Ok(tx.last_insert_rowid())
}

/// Var olan satıra sunucu bayraklarını ve (eksikse) gövdeyi yazar. Taşıması
/// bekleyen satırın (`pending`) UID'si `src_*` sütunlarındadır; ona dokunulmaz.
fn refresh(
    tx: &Transaction<'_>,
    id: MessageId,
    m: &NewMessage,
    pending: bool,
) -> rusqlite::Result<()> {
    if pending {
        tx.prepare_cached("UPDATE messages SET unread = ?2, starred = ?3 WHERE id = ?1")?
            .execute(params![id, m.unread, m.starred])?;
    } else {
        tx.prepare_cached(
            "UPDATE messages SET uid = ?2, uidvalidity = ?3, unread = ?4, starred = ?5 WHERE id = ?1",
        )?
        .execute(params![
            id,
            m.uid.map(i64::from),
            m.uidvalidity.map(i64::from),
            m.unread,
            m.starred
        ])?;
    }
    if let Some(raw) = &m.raw {
        tx.prepare_cached(
            "UPDATE messages SET raw = ?2, body_fetched = 1, snippet = ?3, has_attachments = ?4, fts_body = ?5
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
    Ok(())
}

impl Store {
    /// Postayı ekler. Sunucuda aynı yeri (kutu + UID) tutan satır varsa
    /// bayrakları günceller; taşıma onaylandıktan sonra UID'siz bekleyen kopya
    /// varsa onu sahiplenir. Taşıması bekleyen satır hedef kutuda
    /// sahiplenilmez: posta sunucuda hâlâ kaynaktadır.
    pub fn upsert_message(&self, m: &NewMessage) -> AppResult<UpsertOutcome> {
        self.with_tx(|tx| {
            if let Some(uid) = m.uid {
                let found: Option<(MessageId, bool)> = tx
                    .prepare_cached(
                        "SELECT id, 0 FROM messages WHERE mailbox_id = ?1 AND uid = ?2
                         UNION ALL
                         SELECT id, 1 FROM messages WHERE src_mailbox_id = ?1 AND src_uid = ?2
                         LIMIT 1",
                    )?
                    .query_row(params![m.mailbox_id, i64::from(uid)], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .optional()?;
                let found = match (found, m.message_id_header.as_deref()) {
                    (Some(f), _) => Some(f),
                    (None, Some(mid)) => tx
                        .prepare_cached(
                            "SELECT id, 0 FROM messages WHERE mailbox_id = ?1 AND uid IS NULL
                             AND src_mailbox_id IS NULL AND message_id_header = ?2
                             ORDER BY id LIMIT 1",
                        )?
                        .query_row(params![m.mailbox_id, mid], |r| Ok((r.get(0)?, r.get(1)?)))
                        .optional()?,
                    (None, None) => None,
                };
                if let Some((id, pending)) = found {
                    refresh(tx, id, m, pending)?;
                    return Ok(UpsertOutcome {
                        id,
                        inserted: false,
                    });
                }
            }
            let id = insert(tx, m)?;
            Ok(UpsertOutcome { id, inserted: true })
        })
    }

    /// Sonradan indirilen gövdeyi yazar ve önizleme/arama alanlarını yeniler.
    pub fn store_body(&self, id: MessageId, raw: &[u8]) -> AppResult<()> {
        self.store_parsed_body(id, raw, mh_mime::parse(raw, false))
    }

    /// [`Store::store_body`]; ayrıştırma önceden (ör. eşzamansız görevi
    /// bloklamamak için ayrı iş parçacığında) yapılmıştır.
    pub fn store_parsed_body(
        &self,
        id: MessageId,
        raw: &[u8],
        parsed: Option<ParsedMessage>,
    ) -> AppResult<()> {
        self.with(|c| {
            match parsed {
                Some(p) => c.execute(
                    "UPDATE messages SET raw = ?2, body_fetched = 1, snippet = ?3, has_attachments = ?4,
                        fts_body = ?5 WHERE id = ?1",
                    params![
                        id,
                        raw,
                        p.snippet,
                        p.has_attachments,
                        fold_tr(p.body_text.as_deref().unwrap_or(""))
                    ],
                )?,
                None => c.execute("UPDATE messages SET raw = ?2, body_fetched = 1 WHERE id = ?1", params![id, raw])?,
            };
            Ok(())
        })
    }

    pub fn raw(&self, id: MessageId) -> AppResult<Option<Vec<u8>>> {
        self.with(|c| {
            c.query_row("SELECT raw FROM messages WHERE id = ?1", [id], |r| {
                r.get::<_, Option<Vec<u8>>>(0)
            })
            .optional()
            .map(Option::flatten)
        })
    }

    /// Yanıt için (Message-ID, References) başlıkları. Taslakta taslağın
    /// kendisi değil, yanıtladığı posta esas alınır (In-Reply-To).
    pub fn reply_refs(&self, id: MessageId) -> AppResult<Option<(Option<String>, Vec<String>)>> {
        self.with(|c| {
            c.query_row(
                "SELECT m.message_id_header, m.refs, m.in_reply_to, b.role
                 FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id WHERE m.id = ?1",
                [id],
                |r| {
                    let draft = role_from(&r.get::<_, String>(3)?) == MailboxRole::Drafts;
                    Ok((
                        r.get::<_, Option<String>>(if draft { 2 } else { 0 })?,
                        from_json::<Vec<String>>(1, &r.get::<_, String>(1)?)?,
                    ))
                },
            )
            .optional()
        })
    }

    pub fn summary(&self, id: MessageId) -> AppResult<Option<MessageSummary>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {SUMMARY_COLS} FROM messages m WHERE m.id = ?1"),
                [id],
                row_to_summary,
            )
            .optional()
        })
    }

    /// Verilen kimliklerin özetleri (yeniden eskiye).
    pub fn summaries(&self, ids: &[MessageId]) -> AppResult<Vec<MessageSummary>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {SUMMARY_COLS} FROM messages m WHERE m.id IN ({}) ORDER BY m.date DESC, m.id DESC",
                in_list(ids.len())
            ))?;
            let rows = st.query_map(params_from_iter(ids_params(ids)), row_to_summary)?;
            rows.collect()
        })
    }

    /// Konusu tam eşleşen en yeni posta (tanıtım bildirimi için).
    pub fn find_by_subject(&self, subject: &str) -> AppResult<Option<MessageSummary>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {SUMMARY_COLS} FROM messages m WHERE m.subject = ?1 ORDER BY m.date DESC LIMIT 1"),
                [subject],
                row_to_summary,
            )
            .optional()
        })
    }

    /// Görünüm listesi, yeniden eskiye, imleçli sayfalama.
    pub fn list(&self, q: &ListQuery, now: Millis) -> AppResult<Page<MessageSummary>> {
        let limit = match q.limit {
            0 => 50,
            n => n.min(500),
        } as i64;
        let mut sql = format!(
            "SELECT {SUMMARY_COLS} FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id WHERE 1 = 1"
        );
        let mut p: Vec<Value> = Vec::new();
        let role = |r: MailboxRole| Value::Text(role_str(r).into());
        match &q.view {
            View::Inbox => {
                sql += " AND b.role = ? AND (m.snoozed_until IS NULL OR m.snoozed_until <= ?)";
                p.push(role(MailboxRole::Inbox));
                p.push(Value::Integer(now));
            }
            View::Snoozed => {
                sql += " AND m.snoozed_until > ? AND b.role NOT IN ('trash', 'junk')";
                p.push(Value::Integer(now));
            }
            View::Starred => sql += " AND m.starred = 1 AND b.role NOT IN ('trash', 'junk')",
            View::Drafts | View::Sent | View::Archive | View::Trash => {
                sql += " AND b.role = ?";
                p.push(role(match q.view {
                    View::Drafts => MailboxRole::Drafts,
                    View::Sent => MailboxRole::Sent,
                    View::Archive => MailboxRole::Archive,
                    _ => MailboxRole::Trash,
                }));
            }
            View::Mailbox { id } => {
                sql += " AND m.mailbox_id = ?";
                p.push(Value::Text(id.clone()));
            }
        }
        if let Some(a) = &q.account_id {
            sql += " AND m.account_id = ?";
            p.push(Value::Text(a.clone()));
        }
        if let Some(c) = q.category {
            sql += " AND m.category = ?";
            p.push(Value::Text(category_str(c).into()));
        }
        if let Some(c) = q.cursor.as_deref().filter(|c| !c.is_empty()) {
            let (d, i) = parse_cursor(c)?;
            sql += " AND (m.date < ? OR (m.date = ? AND m.id < ?))";
            p.extend([Value::Integer(d), Value::Integer(d), Value::Integer(i)]);
        }
        sql += " ORDER BY m.date DESC, m.id DESC LIMIT ?";
        p.push(Value::Integer(limit + 1));

        let mut items = self.with(|c| {
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map(params_from_iter(p), row_to_summary)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let next_cursor = if items.len() as i64 > limit {
            items.truncate(limit as usize);
            items.last().map(|m| format!("{}:{}", m.date, m.id))
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }

    /// Sunucu işlemi için postaların yerleri.
    pub fn locations(&self, ids: &[MessageId]) -> AppResult<Vec<MessageLocation>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {LOCATION_COLS} FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE m.id IN ({})",
                in_list(ids.len())
            ))?;
            let rows = st.query_map(params_from_iter(ids_params(ids)), row_to_location)?;
            rows.collect()
        })
    }

    pub fn location(&self, id: MessageId) -> AppResult<Option<MessageLocation>> {
        Ok(self.locations(&[id])?.into_iter().next())
    }

    fn touched_mailboxes(&self, ids: &[MessageId]) -> AppResult<Vec<String>> {
        let mut out: Vec<String> = self
            .locations(ids)?
            .into_iter()
            .map(|l| l.mailbox_id)
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Okunmadı/yıldız bayraklarını yerelde değiştirir; etkilenen kutuları döner.
    pub fn set_flags(
        &self,
        ids: &[MessageId],
        unread: Option<bool>,
        starred: Option<bool>,
    ) -> AppResult<Vec<String>> {
        if ids.is_empty() || (unread.is_none() && starred.is_none()) {
            return Ok(Vec::new());
        }
        self.with_tx(|tx| {
            let list = in_list(ids.len());
            if let Some(u) = unread {
                let mut p = vec![Value::Integer(u as i64)];
                p.extend(ids_params(ids));
                tx.execute(
                    &format!("UPDATE messages SET unread = ? WHERE id IN ({list})"),
                    params_from_iter(p),
                )?;
            }
            if let Some(s) = starred {
                let mut p = vec![Value::Integer(s as i64)];
                p.extend(ids_params(ids));
                tx.execute(
                    &format!("UPDATE messages SET starred = ? WHERE id IN ({list})"),
                    params_from_iter(p),
                )?;
            }
            Ok(())
        })?;
        self.touched_mailboxes(ids)
    }

    /// Sunucudan gelen bayrakları yazar; değiştiyse `true`.
    pub fn set_server_flags(&self, id: MessageId, unread: bool, starred: bool) -> AppResult<bool> {
        self.with(|c| {
            Ok(c.prepare_cached(
                "UPDATE messages SET unread = ?2, starred = ?3 WHERE id = ?1 AND (unread != ?2 OR starred != ?3)",
            )?
            .execute(params![id, unread, starred])?
                > 0)
        })
    }

    /// Postaları yerelde başka kutuya taşır; sunucudaki yerleri (kutu, UID,
    /// UIDVALIDITY) sunucu onayına kadar `src_*` sütunlarında saklanır.
    ///
    /// - Zaten hedefte olan postalara dokunulmaz.
    /// - Taşıması henüz sunucuya gitmemiş postayı kaynağına geri almak (geri
    ///   al) bekleyen taşımayı iptal eder: eski UID geri yazılır, sunucuda bir
    ///   şey yapılmaz.
    /// - Bekleyen taşıma yeniden taşınırsa sunucudaki yer korunur; sunucu
    ///   doğrudan son hedefe taşır.
    ///
    /// Etkilenen kutuları döner.
    pub fn move_local(&self, ids: &[MessageId], target_mailbox_id: &str) -> AppResult<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut touched = self.touched_mailboxes(ids)?;
        let list = in_list(ids.len());
        let target = || Value::Text(target_mailbox_id.into());
        // Parametreler: [hedef], kimlikler, [hedef, hedef].
        let shaped = |lead: bool, tail: usize| {
            let mut p: Vec<Value> = if lead { vec![target()] } else { Vec::new() };
            p.extend(ids_params(ids));
            p.extend((0..tail).map(|_| target()));
            params_from_iter(p)
        };
        let changed = self.with_tx(|tx| {
            // 1) Kaynağına dönen bekleyen taşıma: iptal.
            let mut n = tx.execute(
                &format!(
                    "UPDATE messages SET mailbox_id = src_mailbox_id, uid = src_uid,
                        uidvalidity = src_uidvalidity, src_mailbox_id = NULL, src_uid = NULL,
                        src_uidvalidity = NULL, snoozed_until = NULL
                     WHERE id IN ({list}) AND src_mailbox_id = ?"
                ),
                shaped(false, 1),
            )?;
            // 2) Bekleyen taşıma başka bir kutuya: yalnız görünen kutu değişir.
            n += tx.execute(
                &format!(
                    "UPDATE messages SET mailbox_id = ?, snoozed_until = NULL
                     WHERE id IN ({list}) AND src_mailbox_id IS NOT NULL AND mailbox_id != ?
                       AND account_id = (SELECT account_id FROM mailboxes WHERE id = ?)"
                ),
                shaped(true, 2),
            )?;
            // 3) Yeni taşıma: sunucudaki yer saklanır.
            n += tx.execute(
                &format!(
                    "UPDATE messages SET src_mailbox_id = mailbox_id, src_uid = uid,
                        src_uidvalidity = uidvalidity, mailbox_id = ?, uid = NULL,
                        uidvalidity = NULL, snoozed_until = NULL
                     WHERE id IN ({list}) AND src_mailbox_id IS NULL AND mailbox_id != ?
                       AND account_id = (SELECT account_id FROM mailboxes WHERE id = ?)"
                ),
                shaped(true, 2),
            )?;
            Ok(n)
        })?;
        if changed > 0 && !touched.iter().any(|t| t == target_mailbox_id) {
            touched.push(target_mailbox_id.into());
        }
        Ok(touched)
    }

    /// Yalnız yerel taşıma (sunucusu olmayan tanıtım hesapları): sunucu yeri
    /// tutulmaz. Zaten hedefte olan postalara dokunulmaz.
    pub fn move_local_untracked(
        &self,
        ids: &[MessageId],
        target_mailbox_id: &str,
    ) -> AppResult<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut touched = self.touched_mailboxes(ids)?;
        let changed = self.with(|c| {
            let target = || Value::Text(target_mailbox_id.into());
            let mut p = vec![target()];
            p.extend(ids_params(ids));
            p.extend([target(), target()]);
            c.execute(
                &format!(
                    "UPDATE messages SET mailbox_id = ?, uid = NULL, uidvalidity = NULL,
                        snoozed_until = NULL, src_mailbox_id = NULL, src_uid = NULL,
                        src_uidvalidity = NULL
                     WHERE id IN ({}) AND mailbox_id != ?
                       AND account_id = (SELECT account_id FROM mailboxes WHERE id = ?)",
                    in_list(ids.len())
                ),
                params_from_iter(p),
            )
        })?;
        if changed > 0 && !touched.iter().any(|t| t == target_mailbox_id) {
            touched.push(target_mailbox_id.into());
        }
        Ok(touched)
    }

    /// Hesabın sunucu onayı bekleyen taşımaları.
    pub fn pending_moves(&self, account_id: &str) -> AppResult<Vec<PendingMove>> {
        self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT m.id, m.src_mailbox_id, m.src_uid, m.src_uidvalidity, m.mailbox_id, b.path,
                        m.message_id_header
                 FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE m.account_id = ?1 AND m.src_mailbox_id IS NOT NULL ORDER BY m.id",
            )?;
            let rows = st.query_map([account_id], |r| {
                let src: String = r.get(1)?;
                Ok(PendingMove {
                    id: r.get(0)?,
                    src_path: path_of(&src, account_id),
                    src_mailbox_id: src,
                    src_uid: r.get::<_, Option<i64>>(2)?.map(|v| v as u32),
                    src_uidvalidity: r.get::<_, Option<i64>>(3)?.map(|v| v as u32),
                    dest_mailbox_id: r.get(4)?,
                    dest_path: r.get(5)?,
                    message_id_header: r.get(6)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Sunucuya gitmesi gereken taşıma var mı?
    pub fn has_pending_moves(&self, account_id: &str) -> AppResult<bool> {
        self.with(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE account_id = ?1 AND src_mailbox_id IS NOT NULL)",
                [account_id],
                |r| r.get(0),
            )
        })
    }

    /// Sunucu postayı `dest_mailbox_id` kutusuna taşıdı (yeni UID biliniyorsa
    /// verilir). Posta yerelde de oradaysa taşıma tamamlanır; bu arada kullanıcı
    /// postayı başka yere taşıdıysa (ya da geri aldıysa) sunucudaki yeni yer
    /// kaynak olarak yazılır ve `true` döner: bir taşıma daha gerekir.
    pub fn confirm_move(
        &self,
        id: MessageId,
        dest_mailbox_id: &str,
        new_uid: Option<u32>,
        new_uidvalidity: Option<u32>,
    ) -> AppResult<bool> {
        self.with_tx(|tx| {
            let shown: Option<String> = tx
                .query_row("SELECT mailbox_id FROM messages WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            let Some(shown) = shown else {
                return Ok(false);
            };
            let (uid, validity) = (new_uid.map(i64::from), new_uidvalidity.map(i64::from));
            if shown == dest_mailbox_id {
                free_uid(tx, id, dest_mailbox_id, new_uid)?;
                tx.execute(
                    "UPDATE messages SET uid = ?2, uidvalidity = ?3, src_mailbox_id = NULL,
                        src_uid = NULL, src_uidvalidity = NULL WHERE id = ?1",
                    params![id, uid, validity],
                )?;
                Ok(false)
            } else {
                tx.execute(
                    "UPDATE messages SET uid = NULL, uidvalidity = NULL, src_mailbox_id = ?2,
                        src_uid = ?3, src_uidvalidity = ?4 WHERE id = ?1",
                    params![id, dest_mailbox_id, uid, validity],
                )?;
                Ok(true)
            }
        })
    }

    /// Sunucu taşımayı reddetti: posta yerelde de kaynağına (eski UID'siyle)
    /// döner. Etkilenen kutuları döner.
    pub fn revert_move(&self, id: MessageId) -> AppResult<Vec<String>> {
        self.with_tx(|tx| {
            let row: Option<(String, Option<String>, Option<i64>)> = tx
                .query_row(
                    "SELECT mailbox_id, src_mailbox_id, src_uid FROM messages WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((shown, Some(src), src_uid)) = row else {
                return Ok(Vec::new());
            };
            free_uid(tx, id, &src, src_uid.map(|u| u as u32))?;
            tx.execute(
                "UPDATE messages SET mailbox_id = src_mailbox_id, uid = src_uid,
                    uidvalidity = src_uidvalidity, src_mailbox_id = NULL, src_uid = NULL,
                    src_uidvalidity = NULL WHERE id = ?1",
                [id],
            )?;
            Ok(vec![shown, src])
        })
    }

    /// Posta sunucuda yok ve hiç UID'si olmadı (yalnız yerel kopya): bekleyen
    /// taşıma düşer, posta yerelde göründüğü yerde kalır.
    pub fn forget_server_copy(&self, id: MessageId) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "UPDATE messages SET uid = NULL, uidvalidity = NULL, src_mailbox_id = NULL,
                    src_uid = NULL, src_uidvalidity = NULL WHERE id = ?1",
                [id],
            )?;
            Ok(())
        })
    }

    /// Ertele ya da ertelemeyi kaldır (`until = None`).
    pub fn snooze(&self, ids: &[MessageId], until: Option<Millis>) -> AppResult<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|c| {
            let mut p = vec![until.map(Value::Integer).unwrap_or(Value::Null)];
            p.extend(ids_params(ids));
            c.execute(
                &format!(
                    "UPDATE messages SET snoozed_until = ? WHERE id IN ({})",
                    in_list(ids.len())
                ),
                params_from_iter(p),
            )
        })?;
        self.touched_mailboxes(ids)
    }

    /// `(from, to]` aralığında süresi dolan ertelemelerin sayısı.
    pub fn snoozes_expired_between(&self, from: Millis, to: Millis) -> AppResult<u32> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM messages WHERE snoozed_until > ?1 AND snoozed_until <= ?2",
                params![from, to],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u32)
        })
    }

    pub fn delete_messages(&self, ids: &[MessageId]) -> AppResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.with(|c| {
            c.execute(
                &format!("DELETE FROM messages WHERE id IN ({})", in_list(ids.len())),
                params_from_iter(ids_params(ids)),
            )?;
            Ok(())
        })
    }

    /// Sunucuda bu kutuda bulunan (UID'si bilinen) postalar: (kimlik, uid,
    /// okunmadı, yıldız), UID'ye göre. Taşıması bekleyen postalar sunucuda
    /// hâlâ bulundukları kaynak kutuda sayılır.
    pub fn local_uids(&self, mailbox_id: &str) -> AppResult<Vec<(MessageId, u32, bool, bool)>> {
        self.with(|c| {
            let mut st = c.prepare_cached(
                "SELECT id, uid, unread, starred FROM messages
                 WHERE mailbox_id = ?1 AND uid IS NOT NULL AND src_mailbox_id IS NULL
                 UNION ALL
                 SELECT id, src_uid, unread, starred FROM messages
                 WHERE src_mailbox_id = ?1 AND src_uid IS NOT NULL
                 ORDER BY 2",
            )?;
            let rows = st.query_map([mailbox_id], |r| {
                Ok((r.get(0)?, r.get::<_, i64>(1)? as u32, r.get(2)?, r.get(3)?))
            })?;
            rows.collect()
        })
    }

    /// UIDVALIDITY değişince kutunun sunucu kopyalarını siler. Bu kutudan
    /// taşınmayı bekleyen postaların eski UID'si unutulur (taşıma sırasında
    /// Message-ID ile aranır).
    pub fn clear_mailbox_uids(&self, mailbox_id: &str) -> AppResult<()> {
        self.with_tx(|tx| {
            tx.execute(
                "DELETE FROM messages WHERE mailbox_id = ?1 AND uid IS NOT NULL AND src_mailbox_id IS NULL",
                [mailbox_id],
            )?;
            tx.execute(
                "UPDATE messages SET src_uid = NULL, src_uidvalidity = NULL WHERE src_mailbox_id = ?1",
                [mailbox_id],
            )?;
            Ok(())
        })
    }

    /// Kenar çubuğu sayaçları.
    pub fn counts(&self, now: Millis) -> AppResult<Counts> {
        self.with(|c| {
            let mut counts = Counts::default();
            let mut st = c.prepare_cached(
                "SELECT m.account_id, COUNT(*) FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE b.role = 'inbox' AND m.unread = 1 AND (m.snoozed_until IS NULL OR m.snoozed_until <= ?1)
                 GROUP BY m.account_id",
            )?;
            for row in st.query_map([now], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32)))? {
                let (acc, n) = row?;
                counts.inbox_unread += n;
                counts.per_account_unread.insert(acc, n);
            }
            counts.snoozed = c.query_row(
                "SELECT COUNT(*) FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE m.snoozed_until > ?1 AND b.role NOT IN ('trash', 'junk')",
                [now],
                |r| r.get::<_, i64>(0),
            )? as u32;
            counts.drafts = c.query_row(
                "SELECT COUNT(*) FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id WHERE b.role = 'drafts'",
                [],
                |r| r.get::<_, i64>(0),
            )? as u32;
            Ok(counts)
        })
    }

    /// Aynı konuşmadaki önceki iletiler, eskiden yeniye (çöp hariç).
    pub fn thread(&self, id: MessageId) -> AppResult<Vec<ThreadEntry>> {
        self.with(|c| {
            let Some((account, thread, date, own_mid)) = c
                .query_row(
                    "SELECT account_id, thread_id, date, message_id_header FROM messages WHERE id = ?1",
                    [id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?)),
                )
                .optional()?
            else {
                return Ok(Vec::new());
            };
            let mut st = c.prepare_cached(
                "SELECT m.id, m.from_addr, m.date, m.snippet, m.message_id_header
                 FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE m.account_id = ?1 AND m.thread_id = ?2 AND m.id != ?3
                   AND (m.date < ?4 OR (m.date = ?4 AND m.id < ?3))
                   AND b.role NOT IN ('trash', 'junk')
                 ORDER BY m.date, m.id LIMIT 100",
            )?;
            let rows = st.query_map(params![account, thread, id, date], |r| {
                Ok((
                    ThreadEntry {
                        id: r.get(0)?,
                        from: from_json(1, &r.get::<_, String>(1)?)?,
                        date: r.get(2)?,
                        snippet: r.get(3)?,
                    },
                    r.get::<_, Option<String>>(4)?,
                ))
            })?;
            // Aynı postanın başka kutudaki kopyalarını tek göster.
            let mut seen: Vec<String> = own_mid.into_iter().collect();
            let mut out = Vec::new();
            for row in rows {
                let (entry, mid) = row?;
                if let Some(mid) = mid {
                    if seen.contains(&mid) {
                        continue;
                    }
                    seen.push(mid);
                }
                out.push(entry);
            }
            Ok(out)
        })
    }

    /// Okuma alanı için tam posta. Gövde indirilmemişse metin alanları boştur.
    pub fn detail(&self, id: MessageId, remote_allow: &[String]) -> AppResult<MessageDetail> {
        let row = self.with(|c| {
            c.query_row(
                &format!(
                    "SELECT {SUMMARY_COLS}, m.cc_addrs, m.reply_to, m.message_id_header, m.refs, m.raw
                     FROM messages m WHERE m.id = ?1"
                ),
                [id],
                |r| {
                    Ok((
                        row_to_summary(r)?,
                        addr_list(14, &r.get::<_, String>(14)?)?,
                        addr_list(15, &r.get::<_, String>(15)?)?,
                        r.get::<_, Option<String>>(16)?,
                        from_json::<Vec<String>>(17, &r.get::<_, String>(17)?)?,
                        r.get::<_, Option<Vec<u8>>>(18)?,
                    ))
                },
            )
            .optional()
        })?;
        let Some((summary, cc, reply_to, message_id_header, references, raw)) = row else {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "Posta bulunamadı; silinmiş ya da taşınmış olabilir.",
            ));
        };
        let allowed = remote_allowed(remote_allow, &summary.from.email);
        let rendered = raw.as_deref().and_then(|r| mh_mime::render(r, allowed));
        let thread = self.thread(id)?;
        Ok(MessageDetail {
            cc,
            bcc: rendered.as_ref().map(|r| r.bcc.clone()).unwrap_or_default(),
            reply_to,
            message_id_header,
            references,
            body_text: rendered.as_ref().and_then(|r| r.text.clone()),
            body_html: rendered.as_ref().and_then(|r| r.html.clone()),
            has_remote_content: rendered.as_ref().is_some_and(|r| r.has_remote_content),
            remote_allowed: allowed,
            attachments: rendered.map(|r| r.attachments).unwrap_or_default(),
            thread,
            summary,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testutil::store_with_account;

    pub fn msg(account: &str, mailbox: &str, subject: &str, date: Millis) -> NewMessage {
        NewMessage {
            account_id: account.into(),
            mailbox_id: mailbox.into(),
            uid: None,
            uidvalidity: None,
            message_id_header: None,
            in_reply_to: None,
            references: vec![],
            from: Address::new(Some("Kerem Aydın"), "kerem@ornek.com.tr"),
            to: vec![Address::new(None, "furkan@ornek.com.tr")],
            cc: vec![],
            reply_to: vec![],
            subject: subject.into(),
            snippet: format!("{subject} önizleme"),
            body_text: Some(format!("{subject} gövde")),
            date,
            unread: true,
            starred: false,
            has_attachments: false,
            category: Category::People,
            size: 100,
            raw: None,
        }
    }

    fn query(view: View) -> ListQuery {
        ListQuery {
            view,
            account_id: None,
            category: None,
            cursor: None,
            limit: 2,
        }
    }

    fn setup() -> Store {
        let s = store_with_account("a");
        s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox)
            .unwrap();
        s.upsert_mailbox("a", "Archive", "Archive", MailboxRole::Archive)
            .unwrap();
        s.upsert_mailbox("a", "Trash", "Trash", MailboxRole::Trash)
            .unwrap();
        s.upsert_mailbox("a", "Drafts", "Drafts", MailboxRole::Drafts)
            .unwrap();
        s
    }

    #[test]
    fn imlecli_sayfalama() {
        let s = setup();
        for i in 0..5 {
            s.upsert_message(&msg("a", "a:INBOX", &format!("Konu {i}"), 1000 + i))
                .unwrap();
        }
        // Aynı tarihli iki posta: kimlik sırası belirler.
        s.upsert_message(&msg("a", "a:INBOX", "Eş tarih", 1004))
            .unwrap();
        let mut q = query(View::Inbox);
        let mut seen = Vec::new();
        loop {
            let page = s.list(&q, 0).unwrap();
            seen.extend(page.items.iter().map(|m| m.subject.clone()));
            match page.next_cursor {
                Some(c) => q.cursor = Some(c),
                None => break,
            }
        }
        assert_eq!(
            seen,
            vec!["Eş tarih", "Konu 4", "Konu 3", "Konu 2", "Konu 1", "Konu 0"]
        );
        q.cursor = Some("bozuk".into());
        assert_eq!(s.list(&q, 0).unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn gorunumler_ve_kategori() {
        let s = setup();
        let a = s
            .upsert_message(&msg("a", "a:INBOX", "Gelen", 10))
            .unwrap()
            .id;
        let mut n = msg("a", "a:INBOX", "Bülten", 11);
        n.category = Category::Newsletters;
        s.upsert_message(&n).unwrap();
        let arch = s
            .upsert_message(&msg("a", "a:Archive", "Arşiv", 12))
            .unwrap()
            .id;
        let trash = s
            .upsert_message(&msg("a", "a:Trash", "Çöp", 13))
            .unwrap()
            .id;
        s.set_flags(&[arch, trash], None, Some(true)).unwrap();

        let titles = |v: View, cat: Option<Category>| {
            let mut q = query(v);
            q.limit = 50;
            q.category = cat;
            s.list(&q, 100)
                .unwrap()
                .items
                .into_iter()
                .map(|m| m.subject)
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(View::Inbox, None), vec!["Bülten", "Gelen"]);
        assert_eq!(titles(View::Inbox, Some(Category::People)), vec!["Gelen"]);
        assert_eq!(titles(View::Starred, None), vec!["Arşiv"]);
        assert_eq!(titles(View::Archive, None), vec!["Arşiv"]);
        assert_eq!(titles(View::Trash, None), vec!["Çöp"]);
        assert_eq!(
            titles(
                View::Mailbox {
                    id: "a:INBOX".into()
                },
                None
            ),
            vec!["Bülten", "Gelen"]
        );

        // Erteleme gelen kutusundan gizler, süre dolunca geri getirir.
        s.snooze(&[a], Some(500)).unwrap();
        assert_eq!(titles(View::Inbox, None), vec!["Bülten"]);
        assert_eq!(titles(View::Snoozed, None), vec!["Gelen"]);
        let c = s.counts(100).unwrap();
        assert_eq!((c.inbox_unread, c.snoozed, c.drafts), (1, 1, 0));
        assert_eq!(c.per_account_unread["a"], 1);
        assert_eq!(s.snoozes_expired_between(100, 600).unwrap(), 1);
        let c = s.counts(600).unwrap();
        assert_eq!((c.inbox_unread, c.snoozed), (2, 0));
    }

    #[test]
    fn bayrak_ve_tasima() {
        let s = setup();
        let mut m = msg("a", "a:INBOX", "Taşınacak", 10);
        m.uid = Some(7);
        m.message_id_header = Some("t@ornek".into());
        let id = s.upsert_message(&m).unwrap().id;
        let touched = s.set_flags(&[id], Some(false), Some(true)).unwrap();
        assert_eq!(touched, vec!["a:INBOX".to_string()]);
        let sm = s.summary(id).unwrap().unwrap();
        assert!(!sm.unread && sm.starred);

        let touched = s.move_local(&[id], "a:Archive").unwrap();
        assert_eq!(
            touched,
            vec!["a:INBOX".to_string(), "a:Archive".to_string()]
        );
        let loc = s.location(id).unwrap().unwrap();
        assert_eq!((loc.role, loc.uid), (MailboxRole::Archive, None));
        assert!(loc.pending);
        assert_eq!(
            (loc.server_path.as_str(), loc.server_uid),
            ("INBOX", Some(7)),
            "sunucu onayına kadar posta kaynakta sayılır"
        );

        // Sunucu taşıdıktan sonra eşitleme arşivde aynı postayı yeni UID ile
        // görünce satırı sahiplenir.
        assert!(!s.confirm_move(id, "a:Archive", None, None).unwrap());
        let mut seen = m.clone();
        seen.mailbox_id = "a:Archive".into();
        seen.uid = Some(42);
        seen.unread = false;
        seen.starred = true;
        let out = s.upsert_message(&seen).unwrap();
        assert_eq!(
            out,
            UpsertOutcome {
                id,
                inserted: false
            }
        );
        assert_eq!(
            s.local_uids("a:Archive").unwrap(),
            vec![(id, 42, false, true)]
        );

        assert!(s.set_server_flags(id, true, true).unwrap());
        assert!(!s.set_server_flags(id, true, true).unwrap());
        s.clear_mailbox_uids("a:Archive").unwrap();
        assert!(s.summary(id).unwrap().is_none());
    }

    #[test]
    fn konusma_zinciri() {
        let s = setup();
        s.upsert_mailbox("a", "Sent", "Sent", MailboxRole::Sent)
            .unwrap();
        let mut root = msg("a", "a:Sent", "Sunucu taşıma planı", 100);
        root.message_id_header = Some("kok@ornek".into());
        let root_id = s.upsert_message(&root).unwrap().id;
        let mut reply = msg("a", "a:INBOX", "Re: Sunucu taşıma planı", 200);
        reply.message_id_header = Some("yanit@ornek".into());
        reply.in_reply_to = Some("kok@ornek".into());
        reply.references = vec!["kok@ornek".into()];
        let reply_id = s.upsert_message(&reply).unwrap().id;
        let a = s.summary(root_id).unwrap().unwrap();
        let b = s.summary(reply_id).unwrap().unwrap();
        assert_eq!(a.thread_id, b.thread_id);
        assert_eq!(a.thread_id, "s:a:sunucu taşima plani");
        let t = s.thread(reply_id).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].id, root_id);
        assert!(s.thread(root_id).unwrap().is_empty());

        // Kökü sonradan gelen zincir.
        let mut late = msg("a", "a:INBOX", "Ynt: Başka", 300);
        late.references = vec!["gec-kok@ornek".into()];
        let late_id = s.upsert_message(&late).unwrap().id;
        let mut late_root = msg("a", "a:Archive", "Başka", 250);
        late_root.message_id_header = Some("gec-kok@ornek".into());
        let late_root_id = s.upsert_message(&late_root).unwrap().id;
        assert_eq!(
            s.summary(late_id).unwrap().unwrap().thread_id,
            s.summary(late_root_id).unwrap().unwrap().thread_id
        );

        // Bildirimler konudan birleşmez.
        let mut n1 = msg("a", "a:INBOX", "Faturanız hazır", 400);
        n1.category = Category::Notifications;
        let mut n2 = n1.clone();
        n2.date = 500;
        let i1 = s.upsert_message(&n1).unwrap().id;
        let i2 = s.upsert_message(&n2).unwrap().id;
        assert_ne!(
            s.summary(i1).unwrap().unwrap().thread_id,
            s.summary(i2).unwrap().unwrap().thread_id
        );
    }

    #[test]
    fn uzak_icerik_izin_listesi() {
        let list = vec![
            "ayse@eposta.com".to_string(),
            "@mimarhaftalik.com".into(),
            "bulten.co".into(),
        ];
        assert!(remote_allowed(&list, "Ayse@Eposta.com"));
        assert!(!remote_allowed(&list, "kerem@eposta.com"));
        assert!(remote_allowed(&list, "bulten@mimarhaftalik.com"));
        assert!(remote_allowed(&list, "x@haber.bulten.co"));
        assert!(!remote_allowed(&list, "x@kotubulten.co"));
        assert!(!remote_allowed(&[], "x@y.z"));

        // `From: <com>` "com" adresine çözülür: alan adı sanılmamalı.
        let bad = vec!["com".to_string(), "@com".into(), "@".into(), "x@".into()];
        assert!(!remote_allowed(&bad, "kim@ornek.com"));
        assert!(!remote_allowed(&bad, "com"));
        assert!(
            !remote_allowed(&list, "bulten.co"),
            "bozuk gönderen eşleşmez"
        );
        assert!(is_full_address("ayse@eposta.com"));
        for a in [
            "com",
            "@eposta.com",
            "ayse@",
            "ayse@com",
            "a@b..c",
            "a b@c.d e",
        ] {
            assert!(!is_full_address(a), "{a}");
        }
        assert!(is_remote_entry("ornek.com") && is_remote_entry("@ornek.com"));
        assert!(!is_remote_entry("com") && !is_remote_entry("x@com"));
    }

    fn inbox_msg(s: &Store, subject: &str, uid: u32) -> MessageId {
        let mut m = msg("a", "a:INBOX", subject, 10);
        m.uid = Some(uid);
        m.uidvalidity = Some(5);
        m.message_id_header = Some(format!("{uid}@ornek"));
        s.upsert_message(&m).unwrap().id
    }

    #[test]
    fn geri_alma_bekleyen_tasimayi_iptal_eder() {
        let s = setup();
        let id = inbox_msg(&s, "Geri alınacak", 7);
        s.move_local(&[id], "a:Archive").unwrap();
        assert_eq!(s.pending_moves("a").unwrap().len(), 1);
        assert_eq!(
            s.local_uids("a:INBOX").unwrap(),
            vec![(id, 7, true, false)],
            "sunucu onayına kadar kaynakta"
        );
        assert!(s.local_uids("a:Archive").unwrap().is_empty());

        // Arşivden çöpe: sunucudaki yer korunur, hedef değişir.
        s.move_local(&[id], "a:Trash").unwrap();
        let p = s.pending_moves("a").unwrap();
        assert_eq!(
            (
                p[0].src_path.as_str(),
                p[0].src_uid,
                p[0].dest_path.as_str()
            ),
            ("INBOX", Some(7), "Trash")
        );

        // Geri al: sunucuya hiç gitmeden eski yerine döner.
        let touched = s.move_local(&[id], "a:INBOX").unwrap();
        assert!(touched.contains(&"a:INBOX".to_string()));
        let loc = s.location(id).unwrap().unwrap();
        assert_eq!(
            (loc.role, loc.uid, loc.uidvalidity, loc.pending),
            (MailboxRole::Inbox, Some(7), Some(5), false)
        );
        assert!(!s.has_pending_moves("a").unwrap());
    }

    #[test]
    fn hedefteki_posta_tasinmaz() {
        let s = setup();
        let a = inbox_msg(&s, "Gelen", 1);
        let mut t = msg("a", "a:Trash", "Çöpte", 10);
        t.uid = Some(3);
        let b = s.upsert_message(&t).unwrap().id;
        // Çöpte Del, karışık seçim: yalnız gelen kutusundaki taşınır.
        s.move_local(&[a, b], "a:Trash").unwrap();
        let lb = s.location(b).unwrap().unwrap();
        assert_eq!((lb.uid, lb.pending), (Some(3), false), "UID korunur");
        assert!(s.location(a).unwrap().unwrap().pending);
        assert_eq!(s.pending_moves("a").unwrap().len(), 1);
        // Hiçbiri taşınmazsa hedef etkilenen sayılmaz.
        assert_eq!(s.move_local(&[b], "a:Trash").unwrap(), vec!["a:Trash"]);
        assert_eq!(
            s.move_local_untracked(&[b], "a:Trash").unwrap(),
            vec!["a:Trash"]
        );
    }

    #[test]
    fn bekleyen_tasima_hedefte_sahiplenilmez() {
        let s = setup();
        let id = inbox_msg(&s, "Taşınıyor", 7);
        s.move_local(&[id], "a:Archive").unwrap();
        // Arşivde aynı Message-ID ile başka bir kopya: ayrı satır olur.
        let mut copy = msg("a", "a:Archive", "Taşınıyor", 10);
        copy.uid = Some(3);
        copy.message_id_header = Some("7@ornek".into());
        let out = s.upsert_message(&copy).unwrap();
        assert!(out.inserted && out.id != id);
        // Kaynağın eşitlemesi bekleyen satırı UID'siyle bulur, yalnız bayrak yazar.
        let mut src = msg("a", "a:INBOX", "Taşınıyor", 10);
        src.uid = Some(7);
        src.unread = false;
        let again = s.upsert_message(&src).unwrap();
        assert_eq!((again.id, again.inserted), (id, false));
        let loc = s.location(id).unwrap().unwrap();
        assert_eq!((loc.mailbox_id.as_str(), loc.pending), ("a:Archive", true));
        assert!(!s.summary(id).unwrap().unwrap().unread);
        // Kaynağın UIDVALIDITY'si değişti: eski UID unutulur, satır kalır.
        s.clear_mailbox_uids("a:INBOX").unwrap();
        let loc = s.location(id).unwrap().unwrap();
        assert_eq!((loc.pending, loc.server_uid), (true, None));
    }

    #[test]
    fn ucustayken_geri_alinan_tasima() {
        let s = setup();
        let id = inbox_msg(&s, "Uçuşta", 7);
        s.move_local(&[id], "a:Archive").unwrap();
        // Sunucu taşırken kullanıcı geri aldı: sunucudaki yeni yer kaynak olur.
        s.move_local(&[id], "a:INBOX").unwrap();
        assert!(s.confirm_move(id, "a:Archive", Some(12), Some(9)).unwrap());
        let p = s.pending_moves("a").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(
            (
                p[0].src_path.as_str(),
                p[0].src_uid,
                p[0].dest_path.as_str()
            ),
            ("Archive", Some(12), "INBOX")
        );
        // İkinci taşıma onaylanınca UID bilinmez; eşitleme Message-ID ile bulur.
        assert!(!s.confirm_move(id, "a:INBOX", None, None).unwrap());
        let loc = s.location(id).unwrap().unwrap();
        assert_eq!(
            (loc.mailbox_id.as_str(), loc.uid, loc.pending),
            ("a:INBOX", None, false)
        );
    }

    #[test]
    fn reddedilen_tasima_geri_doner_ve_kopya_temizlenir() {
        let s = setup();
        let id = inbox_msg(&s, "Reddedilecek", 7);
        s.move_local(&[id], "a:Archive").unwrap();
        let touched = s.revert_move(id).unwrap();
        assert_eq!(touched, vec!["a:Archive".to_string(), "a:INBOX".into()]);
        let loc = s.location(id).unwrap().unwrap();
        assert_eq!(
            (loc.role, loc.uid, loc.pending),
            (MailboxRole::Inbox, Some(7), false)
        );
        assert!(
            s.revert_move(id).unwrap().is_empty(),
            "bekleyen yoksa bir şey yapılmaz"
        );

        // Yeni UID'si arşivde başka (yinelenen) bir satırdaysa o satır silinir.
        s.move_local(&[id], "a:Archive").unwrap();
        let mut dup = msg("a", "a:Archive", "Reddedilecek", 10);
        dup.uid = Some(30);
        let dup_id = s.upsert_message(&dup).unwrap().id;
        assert!(!s.confirm_move(id, "a:Archive", Some(30), Some(1)).unwrap());
        assert!(s.summary(dup_id).unwrap().is_none());
        assert_eq!(
            s.local_uids("a:Archive").unwrap(),
            vec![(id, 30, true, false)]
        );

        // Sunucuda hiç olmayan yerel kopya: bekleyen taşıma düşer, posta kalır.
        s.move_local(&[id], "a:Trash").unwrap();
        s.forget_server_copy(id).unwrap();
        let loc = s.location(id).unwrap().unwrap();
        assert_eq!(
            (loc.role, loc.uid, loc.pending),
            (MailboxRole::Trash, None, false)
        );
    }

    #[test]
    fn ayrinti_ve_govde() {
        let s = setup();
        let raw = b"From: Ayse <ayse@eposta.com>\r\nTo: furkan@eposta.com\r\nSubject: Selam\r\nMessage-ID: <s@e>\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Merhaba</p><img src=\"https://iz.example/a.png\">".to_vec();
        let mut m = NewMessage::from_raw(raw.clone(), true, "a", "a:INBOX", 5).unwrap();
        m.uid = Some(1);
        let id = s.upsert_message(&m).unwrap().id;
        let d = s.detail(id, &[]).unwrap();
        assert!(d.body_html.is_none() && d.body_text.is_none());
        assert!(!s.location(id).unwrap().unwrap().has_raw);

        s.store_body(id, &raw).unwrap();
        let d = s.detail(id, &[]).unwrap();
        assert!(d.has_remote_content && !d.remote_allowed);
        assert!(d.body_html.unwrap().contains("data-mh-src"));
        assert_eq!(d.summary.snippet, "Merhaba");
        assert_eq!(d.message_id_header.as_deref(), Some("s@e"));
        let d = s.detail(id, &["eposta.com".into()]).unwrap();
        assert!(d.remote_allowed);
        assert!(d
            .body_html
            .unwrap()
            .contains("src=\"https://iz.example/a.png\""));
        assert_eq!(s.detail(9999, &[]).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn hesap_silinince_postalar_gider() {
        let s = setup();
        let id = s.upsert_message(&msg("a", "a:INBOX", "x", 1)).unwrap().id;
        s.remove_account(&"a".to_string()).unwrap();
        assert!(s.summary(id).unwrap().is_none());
        assert!(s.mailboxes().unwrap().is_empty());
    }
}
