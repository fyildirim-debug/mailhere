//! Kurallar: kayıt, sıra, kuralla iletilen postaların kaydı ve gelen postaya
//! uygulanacak eylemlerin planı.
//!
//! Eşleşme büyük/küçük harfe ve Türkçe harflere duyarsızdır (`fold_loose`);
//! boşluklar sadeleştirilir. Kurallar `position` sırasıyla denenir; `stop`
//! işaretli kural eşleşirse sonrakiler denenmez.

use std::cell::OnceCell;

use mh_core::{
    Address, AppResult, Category, MessageId, Millis, Rule, RuleAction, RuleCondition, RuleMatch,
    RuleOp,
};
use mh_mime::collapse_ws;
use rusqlite::{params, OptionalExtension, Row};

use crate::codec::{addr_list, category_from, from_json, to_json};
use crate::search::fold_loose;
use crate::Store;

/// En çok kural sayısı.
pub const MAX_RULES: usize = 200;
/// Kural başına bir saatte en çok iletme.
pub const FORWARDS_PER_HOUR: u32 = 30;
/// Kuralla iletme kaydının saklandığı süre: aynı posta bu süre içinde aynı
/// kuralla yeniden iletilmez.
const FORWARD_LOG_TTL: Millis = 90 * 24 * 3_600_000;
const HOUR: Millis = 3_600_000;

/// Kuralın baktığı posta alanları.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleInput {
    pub id: MessageId,
    pub account_id: String,
    pub mailbox_id: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub subject: String,
    pub snippet: String,
    /// Gövde metni; gövde henüz inmediyse (ya da istenmediyse) `None`.
    pub body: Option<String>,
    pub has_attachments: bool,
    pub category: Category,
    pub message_id_header: Option<String>,
}

impl RuleInput {
    /// Kuralla iletme kaydındaki kalıcı anahtar (Message-ID; yoksa yerel kimlik).
    pub fn forward_key(&self) -> String {
        match self.message_id_header.as_deref().filter(|m| !m.is_empty()) {
            Some(mid) => format!("mid:{mid}"),
            None => format!("id:{}", self.id),
        }
    }
}

/// Karşılaştırma biçimi: katlanmış, boşlukları sadeleştirilmiş.
fn norm(s: &str) -> String {
    collapse_ws(&fold_loose(s))
}

/// Bir adresin karşılaştırma biçimi: (ad, adres).
fn norm_addr(a: &Address) -> (String, String) {
    (
        norm(a.name.as_deref().unwrap_or("")),
        norm(&a.email).replace(' ', ""),
    )
}

/// Postanın katlanmış alanları; yalnız gereken alan bir kez hesaplanır.
struct Folded<'a> {
    input: &'a RuleInput,
    from: OnceCell<(String, String)>,
    recipients: OnceCell<Vec<(String, String)>>,
    subject: OnceCell<String>,
    body: OnceCell<String>,
}

impl<'a> Folded<'a> {
    fn new(input: &'a RuleInput) -> Self {
        Self {
            input,
            from: OnceCell::new(),
            recipients: OnceCell::new(),
            subject: OnceCell::new(),
            body: OnceCell::new(),
        }
    }

    fn from(&self) -> &(String, String) {
        self.from.get_or_init(|| norm_addr(&self.input.from))
    }

    fn recipients(&self) -> &[(String, String)] {
        self.recipients.get_or_init(|| {
            self.input
                .to
                .iter()
                .chain(&self.input.cc)
                .map(norm_addr)
                .collect()
        })
    }

    fn subject(&self) -> &str {
        self.subject.get_or_init(|| norm(&self.input.subject))
    }

    fn body(&self) -> &str {
        self.body.get_or_init(|| match &self.input.body {
            Some(b) if !b.trim().is_empty() => norm(b),
            _ => norm(&self.input.snippet),
        })
    }
}

/// Adres koşulu: "ad adres" metninde arar; `is` adresin ya da adın kendisi.
fn addr_hit(op: RuleOp, value: &str, (name, email): &(String, String)) -> bool {
    match op {
        RuleOp::Is => email == value || (!name.is_empty() && name == value),
        _ => {
            let both = if name.is_empty() {
                email.clone()
            } else {
                format!("{name} {email}")
            };
            both.contains(value)
        }
    }
}

fn text_hit(op: RuleOp, value: &str, hay: &str) -> bool {
    match op {
        RuleOp::Is => hay == value,
        RuleOp::Contains | RuleOp::NotContains => hay.contains(value),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    From,
    To,
    Subject,
    Body,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cond {
    Text {
        field: Field,
        op: RuleOp,
        /// Katlanmış değer.
        value: String,
    },
    HasAttachment,
    Category(Category),
}

impl Cond {
    fn of(c: &RuleCondition) -> Self {
        let text = |field, op: &RuleOp, value: &str| Cond::Text {
            field,
            op: *op,
            value: norm(value),
        };
        match c {
            RuleCondition::From { op, value } => text(Field::From, op, value),
            RuleCondition::To { op, value } => text(Field::To, op, value),
            RuleCondition::Subject { op, value } => text(Field::Subject, op, value),
            RuleCondition::Body { op, value } => text(Field::Body, op, value),
            RuleCondition::HasAttachment => Cond::HasAttachment,
            RuleCondition::Category { value } => Cond::Category(*value),
        }
    }

    fn test(&self, f: &Folded<'_>) -> bool {
        let (field, op, value) = match self {
            Cond::HasAttachment => return f.input.has_attachments,
            Cond::Category(c) => return f.input.category == *c,
            Cond::Text { field, op, value } => (field, *op, value.as_str()),
        };
        // Boş değer hiçbir şeyle eşleşmez (kayıtta zaten reddedilir).
        if value.is_empty() {
            return false;
        }
        // "İçermez", "içerir"in tersidir: alıcılardan hiçbiri içermemeli.
        let hit = match field {
            Field::From => addr_hit(op, value, f.from()),
            Field::To => f.recipients().iter().any(|r| addr_hit(op, value, r)),
            Field::Subject => text_hit(op, value, f.subject()),
            Field::Body => text_hit(op, value, f.body()),
        };
        if op == RuleOp::NotContains {
            !hit
        } else {
            hit
        }
    }
}

/// Eşleşmeye hazırlanmış kural (değerler bir kez katlanır).
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub rule: Rule,
    conds: Vec<Cond>,
}

impl CompiledRule {
    pub fn new(rule: Rule) -> Self {
        let conds = rule.conditions.iter().map(Cond::of).collect();
        Self { rule, conds }
    }

    /// Kural bu hesabın postasına bakar mı?
    pub fn applies_to(&self, account_id: &str) -> bool {
        self.rule
            .account_id
            .as_deref()
            .is_none_or(|a| a == account_id)
    }

    /// Koşullardan biri gövdeye bakıyor mu?
    pub fn needs_body(&self) -> bool {
        self.conds.iter().any(|c| {
            matches!(
                c,
                Cond::Text {
                    field: Field::Body,
                    ..
                }
            )
        })
    }

    /// Koşullar postaya uyuyor mu (hesap ve açık/kapalı durumuna bakmaz)?
    pub fn matches(&self, input: &RuleInput) -> bool {
        self.matches_folded(&Folded::new(input))
    }

    fn matches_folded(&self, f: &Folded<'_>) -> bool {
        if self.conds.is_empty() {
            return false;
        }
        match self.rule.match_ {
            RuleMatch::All => self.conds.iter().all(|c| c.test(f)),
            RuleMatch::Any => self.conds.iter().any(|c| c.test(f)),
        }
    }
}

/// Postayı gelen kutusundan çıkaran hedef.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveDest {
    Trash,
    Archive,
    /// Kuralın hesabındaki klasör.
    Mailbox(String),
}

/// Eşleşen kuralların birleşik eylemleri.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RulePlan {
    /// Eşleşen kuralların kimlikleri, sırayla.
    pub matched: Vec<String>,
    pub mark_read: bool,
    pub star: bool,
    pub mute: bool,
    /// Taşıma eylemleri, sırayla.
    pub moves: Vec<MoveDest>,
    /// İletmeler: (kural kimliği, adres).
    pub forwards: Vec<(String, String)>,
    /// Eklenecek yerel etiketlerin kimlikleri (tekrarsız).
    pub tags: Vec<String>,
}

impl RulePlan {
    /// Tek kuralın eylemleri.
    pub fn of_rule(rule: &Rule) -> Self {
        let mut plan = Self::default();
        plan.add(rule);
        plan
    }

    fn add(&mut self, rule: &Rule) {
        self.matched.push(rule.id.clone());
        for a in &rule.actions {
            match a {
                RuleAction::Move { mailbox_id } => {
                    self.moves.push(MoveDest::Mailbox(mailbox_id.clone()))
                }
                RuleAction::Archive => self.moves.push(MoveDest::Archive),
                RuleAction::Trash => self.moves.push(MoveDest::Trash),
                RuleAction::MarkRead => self.mark_read = true,
                RuleAction::Star => self.star = true,
                RuleAction::Mute => self.mute = true,
                RuleAction::Forward { to } => self.forwards.push((rule.id.clone(), to.clone())),
                RuleAction::Tag { tag_id } => {
                    if !self.tags.contains(tag_id) {
                        self.tags.push(tag_id.clone());
                    }
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.matched.is_empty()
    }

    /// Uygulanacak taşıma: çöp her taşımadan önce gelir; yoksa geçerli
    /// (`valid`: klasör var ve postanın hesabında) son taşıma.
    pub fn destination(&self, valid: impl Fn(&str) -> bool) -> Option<MoveDest> {
        if self.moves.contains(&MoveDest::Trash) {
            return Some(MoveDest::Trash);
        }
        self.moves
            .iter()
            .rev()
            .find(|m| match m {
                MoveDest::Mailbox(id) => valid(id),
                _ => true,
            })
            .cloned()
    }
}

/// Postaya sırayla uygulanacak kuralların birleşik planı. Kapalı kurallar ve
/// başka hesaba ait kurallar atlanır.
pub fn plan(rules: &[CompiledRule], input: &RuleInput) -> RulePlan {
    let f = Folded::new(input);
    let mut plan = RulePlan::default();
    for r in rules {
        if !r.rule.enabled || !r.applies_to(&input.account_id) || !r.matches_folded(&f) {
            continue;
        }
        plan.add(&r.rule);
        if r.rule.stop {
            break;
        }
    }
    plan
}

/// Kuralla iletme denemesinin sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardClaim {
    /// İletilebilir; kayıt yazıldı.
    Claimed,
    /// Bu posta bu kuralla zaten iletildi.
    AlreadyForwarded,
    /// Kuralın saatlik iletme sınırı doldu.
    LimitReached,
}

const INPUT_COLS: &str = "m.id, m.account_id, m.mailbox_id, m.from_addr, m.to_addrs, m.cc_addrs,
    m.subject, m.snippet, m.has_attachments, m.category, m.message_id_header";

fn row_to_input(r: &Row<'_>, with_body: bool) -> rusqlite::Result<RuleInput> {
    Ok(RuleInput {
        id: r.get(0)?,
        account_id: r.get(1)?,
        mailbox_id: r.get(2)?,
        from: from_json(3, &r.get::<_, String>(3)?)?,
        to: addr_list(4, &r.get::<_, String>(4)?)?,
        cc: addr_list(5, &r.get::<_, String>(5)?)?,
        subject: r.get(6)?,
        snippet: r.get(7)?,
        has_attachments: r.get(8)?,
        category: category_from(&r.get::<_, String>(9)?),
        message_id_header: r.get(10)?,
        body: if with_body { r.get(11)? } else { None },
    })
}

fn row_to_rule(r: &Row<'_>) -> rusqlite::Result<Option<Rule>> {
    let json: String = r.get(0)?;
    match serde_json::from_str(&json) {
        Ok(rule) => Ok(Some(rule)),
        Err(e) => {
            tracing::warn!(error = %e, "kural okunamadı, atlandı");
            Ok(None)
        }
    }
}

impl Store {
    /// Tüm kurallar, uygulanma sırasıyla.
    pub fn rules(&self) -> AppResult<Vec<Rule>> {
        self.with(|c| {
            let mut st = c.prepare_cached("SELECT json FROM rules ORDER BY position, id")?;
            let rows = st.query_map([], row_to_rule)?;
            Ok(rows
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect())
        })
    }

    pub fn rule(&self, id: &str) -> AppResult<Option<Rule>> {
        self.with(|c| {
            c.query_row("SELECT json FROM rules WHERE id = ?1", [id], row_to_rule)
                .optional()
                .map(Option::flatten)
        })
    }

    pub fn rule_count(&self) -> AppResult<usize> {
        self.with(|c| {
            c.query_row("SELECT COUNT(*) FROM rules", [], |r| r.get::<_, i64>(0))
                .map(|n| n as usize)
        })
    }

    /// Yeni kuralı sona ekler.
    pub fn rule_insert(&self, rule: &Rule) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO rules (id, account_id, position, json)
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(position) + 1, 0) FROM rules), ?3)",
                params![rule.id, rule.account_id, to_json(rule)],
            )?;
            Ok(())
        })
    }

    /// Var olan kuralı yerinde (sırası korunarak) yazar; yoksa `false`.
    pub fn rule_update(&self, rule: &Rule) -> AppResult<bool> {
        self.with(|c| {
            Ok(c.execute(
                "UPDATE rules SET account_id = ?2, json = ?3 WHERE id = ?1",
                params![rule.id, rule.account_id, to_json(rule)],
            )? > 0)
        })
    }

    /// Kuralı (ve iletme kaydını) siler; yoksa `false`.
    pub fn rule_delete(&self, id: &str) -> AppResult<bool> {
        self.with_tx(|tx| {
            tx.execute("DELETE FROM rule_forwards WHERE rule_id = ?1", [id])?;
            Ok(tx.execute("DELETE FROM rules WHERE id = ?1", [id])? > 0)
        })
    }

    /// Kuralları verilen sıraya dizer. Listede olmayan kurallar sırasını
    /// koruyarak sona gelir; bilinmeyen ve yinelenen kimlikler yok sayılır.
    pub fn rules_reorder(&self, ids: &[String]) -> AppResult<()> {
        self.with_tx(|tx| {
            let current: Vec<String> = {
                let mut st = tx.prepare_cached("SELECT id FROM rules ORDER BY position, id")?;
                let rows = st.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut order: Vec<&String> = Vec::with_capacity(current.len());
            for id in ids {
                if current.contains(id) && !order.contains(&id) {
                    order.push(id);
                }
            }
            for id in &current {
                if !order.contains(&id) {
                    order.push(id);
                }
            }
            let mut st = tx.prepare_cached("UPDATE rules SET position = ?2 WHERE id = ?1")?;
            for (i, id) in order.into_iter().enumerate() {
                st.execute(params![id, i as i64])?;
            }
            Ok(())
        })
    }

    /// Kuralların baktığı alanlar (gövde indiyse gövde metniyle).
    pub fn rule_input(&self, id: MessageId) -> AppResult<Option<RuleInput>> {
        self.with(|c| {
            c.query_row(
                &format!(
                    "SELECT {INPUT_COLS}, CASE WHEN m.body_fetched = 1 THEN m.fts_body END
                     FROM messages m WHERE m.id = ?1"
                ),
                [id],
                |r| row_to_input(r, true),
            )
            .optional()
        })
    }

    /// Gelen kutusundaki postalar, kimlik sırasıyla `after`dan sonrası
    /// (sayfalı tarama). `with_body` değilse gövde okunmaz.
    pub fn inbox_rule_inputs(
        &self,
        account_id: Option<&str>,
        after: MessageId,
        limit: u32,
        with_body: bool,
    ) -> AppResult<Vec<RuleInput>> {
        let body = if with_body {
            "CASE WHEN m.body_fetched = 1 THEN m.fts_body END"
        } else {
            "NULL"
        };
        self.with(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {INPUT_COLS}, {body} FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE b.role = 'inbox' AND m.id > ?1 AND (?2 IS NULL OR m.account_id = ?2)
                 ORDER BY m.id LIMIT ?3"
            ))?;
            let rows = st.query_map(params![after, account_id, i64::from(limit)], |r| {
                row_to_input(r, with_body)
            })?;
            rows.collect()
        })
    }

    /// Postayı kuralla iletmeden önce kayda geçirir: aynı posta bu kuralla
    /// iletildiyse ya da kuralın son bir saatteki iletme sayısı sınırdaysa
    /// iletilmez.
    pub fn rule_forward_claim(
        &self,
        rule_id: &str,
        message_key: &str,
        now: Millis,
    ) -> AppResult<ForwardClaim> {
        self.with_tx(|tx| {
            tx.execute(
                "DELETE FROM rule_forwards WHERE at < ?1",
                [now - FORWARD_LOG_TTL],
            )?;
            let done: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM rule_forwards WHERE rule_id = ?1 AND message_key = ?2)",
                params![rule_id, message_key],
                |r| r.get(0),
            )?;
            if done {
                return Ok(ForwardClaim::AlreadyForwarded);
            }
            let recent: i64 = tx.query_row(
                "SELECT COUNT(*) FROM rule_forwards WHERE rule_id = ?1 AND at > ?2",
                params![rule_id, now - HOUR],
                |r| r.get(0),
            )?;
            if recent >= i64::from(FORWARDS_PER_HOUR) {
                return Ok(ForwardClaim::LimitReached);
            }
            tx.execute(
                "INSERT INTO rule_forwards (rule_id, message_key, at) VALUES (?1, ?2, ?3)",
                params![rule_id, message_key, now],
            )?;
            Ok(ForwardClaim::Claimed)
        })
    }

    /// İletme sıraya alınamadıysa kaydı geri alır.
    pub fn rule_forward_release(&self, rule_id: &str, message_key: &str) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM rule_forwards WHERE rule_id = ?1 AND message_key = ?2",
                params![rule_id, message_key],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::{account, store_with_account};
    use mh_core::MailboxRole;

    fn rule(id: &str, conditions: Vec<RuleCondition>, actions: Vec<RuleAction>) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            account_id: None,
            match_: RuleMatch::All,
            conditions,
            actions,
            stop: false,
        }
    }

    fn cond(kind: &str, op: RuleOp, value: &str) -> RuleCondition {
        let value = value.to_owned();
        match kind {
            "from" => RuleCondition::From { op, value },
            "to" => RuleCondition::To { op, value },
            "subject" => RuleCondition::Subject { op, value },
            _ => RuleCondition::Body { op, value },
        }
    }

    fn input() -> RuleInput {
        RuleInput {
            id: 7,
            account_id: "a".into(),
            mailbox_id: "a:INBOX".into(),
            from: Address::new(Some("Şirket Muhasebe"), "Fatura@Sirket.com.tr"),
            to: vec![Address::new(Some("Furkan Yıldırım"), "furkan@ornek.com.tr")],
            cc: vec![Address::new(None, "ayse@ornek.com.tr")],
            subject: "  EKİM   Faturası hazır ".into(),
            snippet: "Tutar 1.250 TL".into(),
            body: None,
            has_attachments: true,
            category: Category::Notifications,
            message_id_header: Some("abc@sirket.com.tr".into()),
        }
    }

    fn hit(c: RuleCondition, i: &RuleInput) -> bool {
        CompiledRule::new(rule("r", vec![c], vec![RuleAction::Star])).matches(i)
    }

    #[test]
    fn metin_kosullari() {
        use RuleOp::*;
        let i = input();
        // Gönderen: ad ve adres birlikte; büyük/küçük harf ve Türkçe harf duyarsız.
        assert!(hit(cond("from", Contains, "şirket"), &i));
        assert!(hit(cond("from", Contains, "SIRKET MUHASEBE"), &i));
        assert!(hit(cond("from", Contains, "muhasebe fatura@"), &i));
        assert!(hit(cond("from", Contains, "@sirket.com.tr"), &i));
        assert!(!hit(cond("from", Contains, "kerem"), &i));
        assert!(hit(cond("from", NotContains, "kerem"), &i));
        assert!(!hit(cond("from", NotContains, "fatura"), &i));
        // "Eşittir": adresin ya da adın kendisi.
        assert!(hit(cond("from", Is, " fatura@sirket.com.tr "), &i));
        assert!(hit(cond("from", Is, "şirket muhasebe"), &i));
        assert!(!hit(cond("from", Is, "sirket.com.tr"), &i));
        // Alıcı: Kime + Bilgi.
        assert!(hit(cond("to", Contains, "yildirim"), &i));
        assert!(hit(cond("to", Is, "AYSE@ornek.com.tr"), &i));
        assert!(hit(cond("to", Contains, "ayse"), &i));
        assert!(!hit(cond("to", NotContains, "ornek.com.tr"), &i));
        assert!(hit(cond("to", NotContains, "kerem"), &i));
        // Konu: boşluklar sadeleşir.
        assert!(hit(cond("subject", Contains, "ekim faturasi"), &i));
        assert!(hit(cond("subject", Is, "Ekim faturası HAZIR"), &i));
        assert!(!hit(cond("subject", Is, "ekim"), &i));
        assert!(hit(cond("subject", NotContains, "reklam"), &i));
        // Gövde inmediyse önizleme.
        assert!(hit(cond("body", Contains, "1.250 tl"), &i));
        let mut with_body = i.clone();
        with_body.body = Some("Sayın müşterimiz,\n\nödeme   son günü 20 Ekim.".into());
        assert!(hit(cond("body", Contains, "odeme son gunu"), &with_body));
        assert!(!hit(cond("body", Contains, "1.250"), &with_body));
        // Boş değer eşleşmez.
        assert!(!hit(cond("subject", Contains, "  "), &i));
        assert!(!hit(cond("subject", NotContains, ""), &i));
        // Ek ve kategori.
        assert!(hit(RuleCondition::HasAttachment, &i));
        assert!(hit(
            RuleCondition::Category {
                value: Category::Notifications
            },
            &i
        ));
        assert!(!hit(
            RuleCondition::Category {
                value: Category::People
            },
            &i
        ));
    }

    #[test]
    fn hepsi_ya_da_biri() {
        use RuleOp::*;
        let i = input();
        let mut r = rule(
            "r",
            vec![
                cond("from", Contains, "sirket"),
                cond("subject", Contains, "kahvaltı"),
            ],
            vec![RuleAction::Star],
        );
        assert!(!CompiledRule::new(r.clone()).matches(&i));
        r.match_ = RuleMatch::Any;
        assert!(CompiledRule::new(r.clone()).matches(&i));
        r.conditions.clear();
        assert!(!CompiledRule::new(r).matches(&i), "koşulsuz kural eşleşmez");
    }

    #[test]
    fn plan_sira_durdurma_ve_oncelik() {
        use RuleOp::*;
        let i = input();
        let mut kapali = rule(
            "kapali",
            vec![cond("from", Contains, "sirket")],
            vec![RuleAction::Trash],
        );
        kapali.enabled = false;
        let mut baska = rule(
            "baska",
            vec![cond("from", Contains, "sirket")],
            vec![RuleAction::Trash],
        );
        baska.account_id = Some("b".into());
        let mut tasi = rule(
            "tasi",
            vec![cond("subject", Contains, "fatura")],
            vec![
                RuleAction::Move {
                    mailbox_id: "a:Faturalar".into(),
                },
                RuleAction::MarkRead,
                RuleAction::Forward {
                    to: "muhasebe@ornek.com".into(),
                },
            ],
        );
        tasi.account_id = Some("a".into());
        let arsiv = rule(
            "arsiv",
            vec![RuleCondition::HasAttachment],
            vec![
                RuleAction::Archive,
                RuleAction::Mute,
                RuleAction::Tag {
                    tag_id: "e1".into(),
                },
                RuleAction::Tag {
                    tag_id: "e1".into(),
                },
            ],
        );
        let mut dur = rule(
            "dur",
            vec![cond("to", Contains, "furkan")],
            vec![RuleAction::Star],
        );
        dur.stop = true;
        let sonra = rule(
            "sonra",
            vec![cond("from", Contains, "sirket")],
            vec![RuleAction::Trash],
        );
        let rules: Vec<CompiledRule> = [kapali, baska, tasi, arsiv, dur, sonra]
            .into_iter()
            .map(CompiledRule::new)
            .collect();
        let p = plan(&rules, &i);
        assert_eq!(
            p.matched,
            vec!["tasi", "arsiv", "dur"],
            "durdurma sonrası denenmez"
        );
        assert!(p.mark_read && p.star && p.mute);
        assert_eq!(
            p.forwards,
            vec![("tasi".to_string(), "muhasebe@ornek.com".to_string())]
        );
        assert_eq!(p.tags, vec!["e1".to_string()], "etiket bir kez");
        // Son geçerli taşıma: arşiv.
        assert_eq!(p.destination(|_| true), Some(MoveDest::Archive));

        // Çöp her taşımadan önce gelir.
        let mut p2 = p.clone();
        p2.moves.insert(0, MoveDest::Trash);
        assert_eq!(p2.destination(|_| true), Some(MoveDest::Trash));
        // Geçersiz klasör atlanır, önceki geçerli taşıma kalır.
        let mut p3 = RulePlan {
            moves: vec![MoveDest::Archive, MoveDest::Mailbox("a:Yok".into())],
            ..RulePlan::default()
        };
        assert_eq!(p3.destination(|id| id != "a:Yok"), Some(MoveDest::Archive));
        p3.moves = vec![MoveDest::Mailbox("a:Var".into())];
        assert_eq!(
            p3.destination(|id| id == "a:Var"),
            Some(MoveDest::Mailbox("a:Var".into()))
        );
        assert_eq!(p3.destination(|_| false), None);
        // Başka hesabın, hiçbir koşula uymayan postası.
        let other = RuleInput {
            account_id: "c".into(),
            from: Address::new(None, "x@y.z"),
            subject: "x".into(),
            to: vec![],
            cc: vec![],
            has_attachments: false,
            ..input()
        };
        assert!(plan(&rules, &other).is_empty());
    }

    #[test]
    fn kayit_sira_ve_hesapla_silinme() {
        let s = store_with_account("a");
        s.insert_account(&account("b")).unwrap();
        let mut r1 = rule(
            "r1",
            vec![RuleCondition::HasAttachment],
            vec![RuleAction::Star],
        );
        r1.account_id = Some("b".into());
        let r2 = rule(
            "r2",
            vec![RuleCondition::HasAttachment],
            vec![RuleAction::Mute],
        );
        let r3 = rule(
            "r3",
            vec![RuleCondition::HasAttachment],
            vec![RuleAction::Archive],
        );
        for r in [&r1, &r2, &r3] {
            s.rule_insert(r).unwrap();
        }
        let ids = |s: &Store| {
            s.rules()
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&s), vec!["r1", "r2", "r3"]);
        assert_eq!(s.rule_count().unwrap(), 3);

        // Güncelleme sırayı korur.
        let mut r2b = r2.clone();
        r2b.name = "Bültenler".into();
        assert!(s.rule_update(&r2b).unwrap());
        assert_eq!(s.rule("r2").unwrap().unwrap().name, "Bültenler");
        assert!(!s.rule_update(&rule("yok", vec![], vec![])).unwrap());
        assert_eq!(ids(&s), vec!["r1", "r2", "r3"]);

        // Sıralama: bilinmeyen ve yinelenen atlanır, eksikler sona.
        s.rules_reorder(&["r3".into(), "yok".into(), "r3".into(), "r1".into()])
            .unwrap();
        assert_eq!(ids(&s), vec!["r3", "r1", "r2"]);
        // Yeni kural sona eklenir.
        s.rule_insert(&rule(
            "r4",
            vec![RuleCondition::HasAttachment],
            vec![RuleAction::Star],
        ))
        .unwrap();
        assert_eq!(ids(&s), vec!["r3", "r1", "r2", "r4"]);

        // Hesabı silinen kural da gider; iletme kaydı kuralla gider.
        assert_eq!(
            s.rule_forward_claim("r1", "mid:x", 0).unwrap(),
            ForwardClaim::Claimed
        );
        s.remove_account(&"b".to_string()).unwrap();
        assert_eq!(ids(&s), vec!["r3", "r2", "r4"]);
        assert!(s.rule_delete("r2").unwrap());
        assert!(!s.rule_delete("r2").unwrap());
        assert_eq!(ids(&s), vec!["r3", "r4"]);
    }

    #[test]
    fn iletme_kaydi_ve_saatlik_sinir() {
        let s = store_with_account("a");
        s.rule_insert(&rule("r", vec![RuleCondition::HasAttachment], vec![]))
            .unwrap();
        let t0 = 10 * HOUR;
        assert_eq!(
            s.rule_forward_claim("r", "mid:1", t0).unwrap(),
            ForwardClaim::Claimed
        );
        assert_eq!(
            s.rule_forward_claim("r", "mid:1", t0 + 5).unwrap(),
            ForwardClaim::AlreadyForwarded
        );
        for n in 2..=FORWARDS_PER_HOUR {
            assert_eq!(
                s.rule_forward_claim("r", &format!("mid:{n}"), t0 + i64::from(n))
                    .unwrap(),
                ForwardClaim::Claimed
            );
        }
        assert_eq!(
            s.rule_forward_claim("r", "mid:son", t0 + 100).unwrap(),
            ForwardClaim::LimitReached
        );
        // Bir saat sonra yeniden açılır; aynı posta yine iletilmez.
        assert_eq!(
            s.rule_forward_claim("r", "mid:son", t0 + HOUR + 100)
                .unwrap(),
            ForwardClaim::Claimed
        );
        assert_eq!(
            s.rule_forward_claim("r", "mid:1", t0 + HOUR + 200).unwrap(),
            ForwardClaim::AlreadyForwarded
        );
        // Geri alınan kayıt yeniden denenebilir.
        s.rule_forward_release("r", "mid:son").unwrap();
        assert_eq!(
            s.rule_forward_claim("r", "mid:son", t0 + HOUR + 300)
                .unwrap(),
            ForwardClaim::Claimed
        );
        // Çok eski kayıt temizlenir.
        assert_eq!(
            s.rule_forward_claim("r", "mid:1", t0 + FORWARD_LOG_TTL + HOUR)
                .unwrap(),
            ForwardClaim::Claimed
        );
    }

    #[test]
    fn gelen_kutusu_taramasi() {
        let s = store_with_account("a");
        s.insert_account(&account("b")).unwrap();
        for a in ["a", "b"] {
            s.upsert_mailbox(a, "INBOX", "INBOX", MailboxRole::Inbox, None, false)
                .unwrap();
            s.upsert_mailbox(a, "Archive", "Archive", MailboxRole::Archive, None, false)
                .unwrap();
        }
        let mut raw = msg("a", "a:INBOX", "Gövdeli", 10);
        raw.raw = Some(b"Subject: x\r\n\r\ngovde".to_vec());
        raw.body_text = Some("Gövde metni".into());
        let a1 = s.upsert_message(&raw).unwrap().id;
        let a2 = s
            .upsert_message(&msg("a", "a:INBOX", "Başlık", 11))
            .unwrap()
            .id;
        s.upsert_message(&msg("a", "a:Archive", "Arşiv", 12))
            .unwrap();
        let b1 = s.upsert_message(&msg("b", "b:INBOX", "B", 13)).unwrap().id;

        let all = s.inbox_rule_inputs(None, 0, 10, true).unwrap();
        assert_eq!(
            all.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![a1, a2, b1]
        );
        assert_eq!(
            all[0].body.as_deref(),
            Some("gövde metni"),
            "katlanmış gövde"
        );
        assert_eq!(all[1].body, None, "gövde inmedi");
        let page = s.inbox_rule_inputs(Some("a"), a1, 10, false).unwrap();
        assert_eq!(page.iter().map(|i| i.id).collect::<Vec<_>>(), vec![a2]);
        assert_eq!(s.inbox_rule_inputs(None, 0, 1, false).unwrap().len(), 1);
        assert!(s.inbox_rule_inputs(None, 0, 10, false).unwrap()[0]
            .body
            .is_none());

        let one = s.rule_input(a1).unwrap().unwrap();
        assert_eq!(one.subject, "Gövdeli");
        assert_eq!(one.mailbox_id, "a:INBOX");
        assert_eq!(one.to, vec![Address::new(None, "furkan@ornek.com.tr")]);
        assert_eq!(one.forward_key(), format!("id:{a1}"));
        assert!(s.rule_input(9_999).unwrap().is_none());
    }
}
