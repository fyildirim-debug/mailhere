//! Kuralların uygulanması: gelen kutusuna yeni posta düşünce (kart
//! kararından önce) ve kural elle çalıştırılınca (gelen kutusundaki mevcut
//! postalara).
//!
//! Eylemler mevcut yöneticilerle uygulanır: IMAP postaları kalıcı işlem
//! kuyruğuna, JMAP postaları `Email/set`e gider; tanıtım hesapları yalnız
//! yereldir. İletme, giden kutusu üzerinden postayı alan hesaptan yapılır.

use std::cell::Cell;
use std::collections::HashSet;
use std::time::Duration;

use chrono::TimeZone;
use mh_core::{
    Account, Address, AppError, AppResult, ErrorCode, Language, MailboxRole, MessageId,
    MessageSummary, Millis, MoveTarget, OutgoingMessage, Rule,
};
use mh_jmap::JmapManager;
use mh_send::Outbox;
use mh_store::{CompiledRule, ForwardClaim, MoveDest, RuleInput, RulePlan, Store};
use mh_sync::{EventSink, SyncManager};
use tauri::{AppHandle, Manager};

use crate::state::{now_ms, AppState};

/// Gövdeye bakan kural için gövdenin inmesi en çok bu kadar beklenir;
/// inmezse önizlemeyle karar verilir.
const BODY_WAIT: Duration = Duration::from_secs(20);
/// Elle çalıştırmada gelen kutusu bu büyüklükte sayfalarla taranır.
const SCAN_PAGE: u32 = 500;

thread_local! {
    /// Kurallardan geçmiş postalar `new_mail`e geri verilirken açıktır.
    static AFTER_RULES: Cell<bool> = const { Cell::new(false) };
}

/// Kuralların kullandığı yöneticiler.
pub struct Engines<'a> {
    pub store: &'a Store,
    pub sync: &'a SyncManager,
    pub jmap: &'a JmapManager,
    pub outbox: &'a Outbox,
}

impl<'a> Engines<'a> {
    pub fn of(state: &'a AppState) -> Self {
        Self {
            store: &state.store,
            sync: &state.sync,
            jmap: &state.jmap,
            outbox: &state.outbox,
        }
    }

    fn is_jmap(&self, account_id: &str) -> AppResult<bool> {
        Ok(self
            .store
            .account(account_id)?
            .is_some_and(|a| mh_jmap::is_syncable(&a)))
    }

    /// Postaları (IMAP ya da yerel, gerçek JMAP) diye ayırır.
    fn split(&self, ids: &[MessageId]) -> AppResult<(Vec<MessageId>, Vec<MessageId>)> {
        let (mut other, mut jmap) = (Vec::new(), Vec::new());
        let mut kinds: Vec<(String, bool)> = Vec::new();
        for loc in self.store.locations(ids)? {
            let is_jmap = match kinds.iter().find(|(a, _)| *a == loc.account_id) {
                Some((_, k)) => *k,
                None => {
                    let k = self.is_jmap(&loc.account_id)?;
                    kinds.push((loc.account_id.clone(), k));
                    k
                }
            };
            if is_jmap {
                jmap.push(loc.id);
            } else {
                other.push(loc.id);
            }
        }
        Ok((other, jmap))
    }

    fn set_flags(
        &self,
        ids: &[MessageId],
        unread: Option<bool>,
        starred: Option<bool>,
    ) -> AppResult<Vec<String>> {
        let (other, jmap) = self.split(ids)?;
        let mut touched = self.sync.set_flags(&other, unread, starred)?;
        touched.extend(self.jmap.set_flags(&jmap, unread, starred)?);
        Ok(touched)
    }

    fn move_messages(&self, ids: &[MessageId], target: MoveTarget) -> AppResult<Vec<String>> {
        let (other, jmap) = self.split(ids)?;
        let mut touched = self.sync.move_messages(&other, target)?;
        touched.extend(self.jmap.move_messages(&jmap, target)?);
        Ok(touched)
    }

    /// Postaları klasöre taşır (`messages_move_to`). Boş liste bir şey
    /// yapmaz; klasör yoksa ya da posta başka hesaptaysa hata döner.
    /// Etkilenen kutuları döner.
    pub fn move_to_mailbox(&self, ids: &[MessageId], mailbox_id: &str) -> AppResult<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let dest = self.store.mailbox(mailbox_id)?.ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "Klasör bulunamadı; sunucuda silinmiş ya da yeniden adlandırılmış olabilir.",
            )
        })?;
        let locations = self.store.locations(ids)?;
        if locations
            .iter()
            .any(|l| l.account_id != dest.mailbox.account_id)
        {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Posta yalnız kendi hesabının klasörlerine taşınabilir.",
            ));
        }
        if self.is_jmap(&dest.mailbox.account_id)? {
            self.jmap.move_to_mailbox(ids, mailbox_id)
        } else {
            self.sync.move_to_mailbox(ids, mailbox_id)
        }
    }

    /// Klasör var ve bu hesapta mı? Varsa rolünü döner.
    fn mailbox_role(&self, mailbox_id: &str, account_id: &str) -> Option<MailboxRole> {
        self.store
            .mailbox(mailbox_id)
            .ok()
            .flatten()
            .filter(|m| m.mailbox.account_id == account_id)
            .map(|m| m.mailbox.role)
    }

    /// Gövde yerelde yoksa sunucudan indirir (`commands::ensure_body` gibi).
    async fn ensure_body(&self, id: MessageId) -> AppResult<()> {
        let Some(loc) = self.store.location(id)? else {
            return Err(AppError::new(ErrorCode::NotFound, "Posta bulunamadı."));
        };
        if loc.has_raw {
            return Ok(());
        }
        if self.is_jmap(&loc.account_id)? {
            self.jmap.fetch_body(id).await
        } else {
            self.sync.fetch_body(id).await
        }
    }

    /// Planın bayrak ve taşıma eylemlerini `account_id` hesabının
    /// postalarına uygular. (etkilenen kutular, gelen kutusundan çıktı mı)
    fn apply(
        &self,
        ids: &[MessageId],
        account_id: &str,
        plan: &RulePlan,
    ) -> AppResult<(Vec<String>, bool)> {
        let mut touched = Vec::new();
        let unread = plan.mark_read.then_some(false);
        let starred = plan.star.then_some(true);
        if unread.is_some() || starred.is_some() {
            touched.extend(self.set_flags(ids, unread, starred)?);
        }
        // Yerel etiketler: silinmiş etiket sessizce atlanır.
        for tag in &plan.tags {
            if self.store.tag(tag)?.is_some() {
                touched.extend(self.store.set_message_tag(ids, tag, true)?);
            }
        }
        let dest = plan.destination(|id| self.mailbox_role(id, account_id).is_some());
        let removed = match dest {
            None => false,
            Some(MoveDest::Trash) => {
                touched.extend(self.move_messages(ids, MoveTarget::Trash)?);
                true
            }
            Some(MoveDest::Archive) => {
                touched.extend(self.move_messages(ids, MoveTarget::Archive)?);
                true
            }
            Some(MoveDest::Mailbox(id)) => {
                touched.extend(self.move_to_mailbox(ids, &id)?);
                self.mailbox_role(&id, account_id) != Some(MailboxRole::Inbox)
            }
        };
        Ok((touched, removed))
    }
}

/// Hesapların kendi adresleri (küçük harf).
fn own_addresses(store: &Store) -> HashSet<String> {
    store
        .accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.email.trim().to_ascii_lowercase())
        .collect()
}

/// Başlık bloğundaki bir başlığın (katlanmış satırları birleştirilmiş) değeri.
fn header_value(raw: &[u8], name: &str) -> Option<String> {
    let head = &raw[..raw.len().min(256 * 1024)];
    let text = String::from_utf8_lossy(head);
    let mut current: Option<String> = None;
    let mut found = None;
    let mut finish = |h: Option<String>| {
        if let Some((n, v)) = h.as_deref().and_then(|h| h.split_once(':')) {
            if found.is_none() && n.trim().eq_ignore_ascii_case(name) {
                found = Some(v.trim().to_owned());
            }
        }
    };
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(c) = current.as_mut() {
                c.push(' ');
                c.push_str(line.trim());
            }
            continue;
        }
        finish(current.take());
        current = Some(line.to_owned());
    }
    finish(current);
    found
}

/// Otomatik gönderilmiş posta mı (RFC 3834 `Auto-Submitted`, "no" dışında
/// her değer: auto-generated, auto-replied, auto-forwarded …)?
pub fn is_auto_submitted(raw: &[u8]) -> bool {
    header_value(raw, "Auto-Submitted").is_some_and(|v| {
        let v = v
            .split(['(', ';'])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        !v.is_empty() && v != "no"
    })
}

fn format_addr(a: &Address) -> String {
    match a.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!("{n} <{}>", a.email),
        None => a.email.clone(),
    }
}

/// İletilen postanın konusu ve gövdesi: kısa başlık bloğu + metin.
fn forward_text(input: &RuleInput, date: Millis, text: &str, lang: Language) -> (String, String) {
    let subject = input.subject.trim();
    let subject = if mh_store::fold_loose(subject).starts_with("fwd:") {
        subject.to_owned()
    } else {
        format!("Fwd: {subject}")
    };
    let [title, from, when, subj, to, cc] = match lang {
        Language::Tr => ["İletilen posta", "Kimden", "Tarih", "Konu", "Kime", "Bilgi"],
        Language::En => ["Forwarded message", "From", "Date", "Subject", "To", "Cc"],
    };
    let list = |l: &[Address]| l.iter().map(format_addr).collect::<Vec<_>>().join(", ");
    let date = chrono::Local
        .timestamp_millis_opt(date)
        .single()
        .map(|d| d.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_default();
    let mut body = format!(
        "---------- {title} ----------\n{from}: {}\n{when}: {date}\n{subj}: {}\n",
        format_addr(&input.from),
        input.subject.trim(),
    );
    if !input.to.is_empty() {
        body += &format!("{to}: {}\n", list(&input.to));
    }
    if !input.cc.is_empty() {
        body += &format!("{cc}: {}\n", list(&input.cc));
    }
    body.push('\n');
    body.push_str(text);
    (subject, body)
}

impl Engines<'_> {
    /// Postayı kuralla iletir. Döngü koruması: kendi adreslerimizden ya da
    /// otomatik gönderilmiş (`Auto-Submitted`) postalar iletilmez; aynı posta
    /// aynı kuralla bir kez, kural başına saatte en çok 30 posta iletilir.
    /// Sıraya alındıysa `true`.
    async fn forward(
        &self,
        input: &RuleInput,
        rule_id: &str,
        to: &str,
        own: &HashSet<String>,
    ) -> AppResult<bool> {
        if own.contains(&input.from.email.trim().to_ascii_lowercase()) {
            tracing::debug!(rule = rule_id, "kendi adresimizden gelen posta iletilmez");
            return Ok(false);
        }
        match tokio::time::timeout(BODY_WAIT, self.ensure_body(input.id)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(AppError::new(
                    ErrorCode::Network,
                    "İletilecek postanın içeriği indirilemedi.",
                ))
            }
        }
        let raw = self
            .store
            .raw(input.id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Postanın içeriği yerelde yok."))?;
        if is_auto_submitted(&raw) {
            tracing::debug!(rule = rule_id, "otomatik gönderilmiş posta iletilmez");
            return Ok(false);
        }
        let key = input.forward_key();
        let now = now_ms();
        match self.store.rule_forward_claim(rule_id, &key, now)? {
            ForwardClaim::Claimed => {}
            ForwardClaim::AlreadyForwarded => return Ok(false),
            ForwardClaim::LimitReached => {
                tracing::warn!(rule = rule_id, "kuralın saatlik iletme sınırı doldu");
                return Ok(false);
            }
        }
        let lang = self
            .store
            .settings()
            .map(|s| s.language)
            .unwrap_or_default();
        let date = self.store.summary(input.id)?.map_or(now, |s| s.date);
        let snippet = input.snippet.clone();
        // HTML'den metin çıkarmak ağır olabilir (kötü niyetli posta).
        let text = tokio::task::spawn_blocking(move || {
            mh_mime::render(&raw, false)
                .and_then(|r| r.text)
                .unwrap_or(snippet)
        })
        .await
        .map_err(crate::state::internal)?;
        let (subject, body_text) = forward_text(input, date, &text, lang);
        let msg = OutgoingMessage {
            account_id: input.account_id.clone(),
            to: vec![Address::new(None, to)],
            cc: vec![],
            bcc: vec![],
            subject,
            body_text,
            in_reply_to: None,
            attachments: vec![],
            send_at: None,
            forward_from: Some(input.id),
            draft_id: None,
        };
        if let Err(e) = self.outbox.enqueue_auto(&msg, now) {
            let _ = self.store.rule_forward_release(rule_id, &key);
            return Err(e);
        }
        Ok(true)
    }

    /// Yeni postalara kuralları uygular. Kart gösterilecek postaları (kural
    /// bayraklarıyla güncellenmiş) ve etkilenen kutuları döner: susturulan ya
    /// da gelen kutusundan çıkarılan posta için kart yoktur.
    pub async fn apply_new(
        &self,
        rules: &[CompiledRule],
        messages: Vec<MessageSummary>,
    ) -> (Vec<MessageSummary>, Vec<String>) {
        let need_body = rules.iter().any(CompiledRule::needs_body);
        let own = own_addresses(self.store);
        let mut kept = Vec::new();
        let mut touched = Vec::new();
        for mut m in messages {
            if need_body {
                if let Ok(Err(e)) = tokio::time::timeout(BODY_WAIT, self.ensure_body(m.id)).await {
                    tracing::debug!(error = %e.message, "kural için gövde indirilemedi");
                }
            }
            let input = match self.store.rule_input(m.id) {
                Ok(Some(i)) => i,
                // Bu arada silinmiş.
                Ok(None) => continue,
                Err(_) => {
                    kept.push(m);
                    continue;
                }
            };
            // Bu arada gelen kutusundan çıkarıldıysa kurala dokunulmaz.
            if self.mailbox_role(&input.mailbox_id, &input.account_id) != Some(MailboxRole::Inbox) {
                kept.push(m);
                continue;
            }
            let plan = mh_store::rule_plan(rules, &input);
            if plan.is_empty() {
                kept.push(m);
                continue;
            }
            let removed = match self.apply(&[m.id], &input.account_id, &plan) {
                Ok((t, removed)) => {
                    touched.extend(t);
                    removed
                }
                Err(e) => {
                    tracing::warn!(error = %e.message, "kural eylemleri uygulanamadı");
                    false
                }
            };
            for (rule_id, to) in &plan.forwards {
                if let Err(e) = self.forward(&input, rule_id, to, &own).await {
                    tracing::warn!(rule = %rule_id, error = %e.message, "kural postayı iletemedi");
                }
            }
            if plan.mark_read {
                m.unread = false;
            }
            if plan.star {
                m.starred = true;
            }
            if !plan.mute && !removed {
                kept.push(m);
            }
        }
        touched.sort();
        touched.dedup();
        (kept, touched)
    }

    /// Kuralı eşleşen postalara uygular (elle çalıştırma): iletme ve
    /// susturma hariç. Etkilenen kutuları döner.
    pub fn apply_rule(
        &self,
        rule: &Rule,
        matched: &[(MessageId, String)],
    ) -> AppResult<Vec<String>> {
        let mut plan = RulePlan::of_rule(rule);
        plan.mute = false;
        plan.forwards.clear();
        let mut accounts: Vec<&str> = matched.iter().map(|(_, a)| a.as_str()).collect();
        accounts.sort_unstable();
        accounts.dedup();
        let mut touched = Vec::new();
        for account in accounts {
            let ids: Vec<MessageId> = matched
                .iter()
                .filter(|(_, a)| a == account)
                .map(|(id, _)| *id)
                .collect();
            touched.extend(self.apply(&ids, account, &plan)?.0);
        }
        touched.sort();
        touched.dedup();
        Ok(touched)
    }
}

/// Kuralın gelen kutusunda eşleştiği postalar: (kimlik, hesap). Ağa çıkmaz;
/// gövdesi inmemiş postada önizlemeye bakılır.
pub fn scan_inbox(store: &Store, rule: &Rule) -> AppResult<Vec<(MessageId, String)>> {
    let compiled = CompiledRule::new(rule.clone());
    let with_body = compiled.needs_body();
    let mut out = Vec::new();
    let mut after = 0;
    loop {
        let page =
            store.inbox_rule_inputs(rule.account_id.as_deref(), after, SCAN_PAGE, with_body)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.id;
        out.extend(
            page.iter()
                .filter(|i| compiled.matches(i))
                .map(|i| (i.id, i.account_id.clone())),
        );
    }
    Ok(out)
}

/// `new_mail`in başında çağrılır. Hesaba uygulanacak açık kural yoksa
/// postaları hemen geri verir. Varsa kuralları eşzamansız uygular (gövde
/// gerekebilir), ardından kalan postaları `new_mail`e yeniden verir ve
/// `None` döner; bildirim kararı kurallardan sonra verilir.
pub fn before_notify(
    app: &AppHandle,
    account: &Account,
    messages: Vec<MessageSummary>,
) -> Option<Vec<MessageSummary>> {
    if AFTER_RULES.get() || messages.is_empty() {
        return Some(messages);
    }
    let Some(state) = app.try_state::<AppState>() else {
        return Some(messages);
    };
    let rules: Vec<CompiledRule> = match state.store.rules() {
        Ok(rules) => rules
            .into_iter()
            .filter(|r| r.enabled)
            .map(CompiledRule::new)
            .filter(|r| r.applies_to(&account.id))
            .collect(),
        Err(_) => return Some(messages),
    };
    if rules.is_empty() {
        return Some(messages);
    }
    let app = app.clone();
    let account = account.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let (kept, touched) = Engines::of(&state).apply_new(&rules, messages).await;
        if !touched.is_empty() {
            crate::sink::changed(&app, touched);
        }
        notify_after_rules(&app, &account, kept);
    });
    None
}

/// Kurallardan geçen postaları bildirim kararına verir.
fn notify_after_rules(app: &AppHandle, account: &Account, kept: Vec<MessageSummary>) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            AFTER_RULES.set(false);
        }
    }
    AFTER_RULES.set(true);
    let _reset = Reset;
    crate::sink::TauriSink::new(app.clone()).new_mail(account, kept);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mh_core::{
        AccountKind, Category, MessageSummary, RuleAction, RuleCondition, RuleMatch, RuleOp,
        Security, ServerConfig, SyncStatus,
    };
    use mh_store::NewMessage;
    use mh_sync::{MemorySecrets, SecretStore};

    use super::*;

    #[derive(Default)]
    struct Rec {
        changed: Mutex<Vec<String>>,
    }

    impl EventSink for Rec {
        fn sync_status(&self, _: SyncStatus) {}
        fn changed(&self, ids: Vec<String>) {
            self.changed.lock().unwrap().extend(ids);
        }
        fn new_mail(&self, _: &Account, _: Vec<MessageSummary>) {}
    }

    struct Env {
        store: Arc<Store>,
        sync: Arc<SyncManager>,
        jmap: Arc<JmapManager>,
        outbox: Arc<Outbox>,
    }

    impl Env {
        fn engines(&self) -> Engines<'_> {
            Engines {
                store: &self.store,
                sync: &self.sync,
                jmap: &self.jmap,
                outbox: &self.outbox,
            }
        }
    }

    /// Ağa çıkmayan (tanıtım sunuculu) hesap.
    fn account(id: &str, email: &str) -> Account {
        let server = |host: &str| ServerConfig {
            host: host.into(),
            port: 993,
            security: Security::Tls,
            username: email.into(),
        };
        Account {
            id: id.into(),
            name: id.into(),
            email: email.into(),
            display_name: "Furkan Yıldırım".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: Some(server("imap.ornek.invalid")),
            smtp: Some(server("smtp.ornek.invalid")),
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
        }
    }

    fn env() -> Env {
        let store = Arc::new(Store::open_in_memory().unwrap());
        for (id, email) in [("a", "furkan@ornek.com.tr"), ("b", "furkan@eposta.com")] {
            store.insert_account(&account(id, email)).unwrap();
            for (path, role) in [
                ("INBOX", MailboxRole::Inbox),
                ("Archive", MailboxRole::Archive),
                ("Trash", MailboxRole::Trash),
                ("Sent", MailboxRole::Sent),
                ("Drafts", MailboxRole::Drafts),
                ("Faturalar", MailboxRole::Other),
            ] {
                store
                    .upsert_mailbox(id, path, path, role, None, false)
                    .unwrap();
            }
        }
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
        let rec = Arc::new(Rec::default());
        let rt = tokio::runtime::Handle::current();
        let sync = Arc::new(SyncManager::new(
            store.clone(),
            secrets.clone(),
            rec.clone(),
            rt.clone(),
        ));
        let jmap = Arc::new(JmapManager::new(
            store.clone(),
            secrets.clone(),
            rec.clone(),
            rt,
        ));
        let outbox = Outbox::new(store.clone(), secrets, sync.clone(), rec);
        outbox.set_jmap(jmap.clone());
        Env {
            store,
            sync,
            jmap,
            outbox,
        }
    }

    fn raw_mail(from: &str, subject: &str, mid: &str, extra: &str, body: &str) -> Vec<u8> {
        format!(
            "From: {from}\r\nTo: Furkan <furkan@ornek.com.tr>\r\nSubject: {subject}\r\n\
             Message-ID: <{mid}>\r\nDate: Tue, 13 Oct 2026 10:00:00 +0300\r\n{extra}\
             Content-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
        )
        .into_bytes()
    }

    /// Gelen kutusuna (gövdesiyle) yeni posta yazar; özetini döner.
    fn deliver(env: &Env, account: &str, raw: Vec<u8>) -> MessageSummary {
        let mailbox = format!("{account}:INBOX");
        let m = NewMessage::from_raw(raw, false, account, &mailbox, 1_000).unwrap();
        let id = env.store.upsert_message(&m).unwrap().id;
        env.store.summary(id).unwrap().unwrap()
    }

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

    fn from_has(v: &str) -> RuleCondition {
        RuleCondition::From {
            op: RuleOp::Contains,
            value: v.into(),
        }
    }

    fn compile(env: &Env, rules: &[Rule]) -> Vec<CompiledRule> {
        for r in rules {
            env.store.rule_insert(r).unwrap();
        }
        env.store
            .rules()
            .unwrap()
            .into_iter()
            .map(CompiledRule::new)
            .collect()
    }

    fn mailbox_of(env: &Env, id: MessageId) -> String {
        env.store.location(id).unwrap().unwrap().mailbox_id
    }

    #[tokio::test]
    async fn yeni_postaya_kurallar_karttan_once_uygulanir() {
        let env = env();
        let mut fatura = rule(
            "fatura",
            vec![from_has("muhasebe")],
            vec![
                RuleAction::Move {
                    mailbox_id: "a:Faturalar".into(),
                },
                RuleAction::MarkRead,
            ],
        );
        fatura.account_id = Some("a".into());
        let bulten = rule(
            "bulten",
            vec![RuleCondition::Body {
                op: RuleOp::Contains,
                value: "abonelikten çık".into(),
            }],
            vec![RuleAction::Mute, RuleAction::Star],
        );
        let cop = rule(
            "cop",
            vec![RuleCondition::Subject {
                op: RuleOp::Is,
                value: "kazandınız".into(),
            }],
            vec![
                RuleAction::Archive,
                RuleAction::Trash,
                RuleAction::Move {
                    mailbox_id: "a:Faturalar".into(),
                },
            ],
        );
        let rules = compile(&env, &[fatura, bulten, cop]);

        let m1 = deliver(
            &env,
            "a",
            raw_mail(
                "Muhasebe <muhasebe@sirket.com>",
                "Ekim",
                "m1@x",
                "",
                "Ekte.",
            ),
        );
        let m2 = deliver(
            &env,
            "a",
            raw_mail(
                "Bülten <haber@site.com>",
                "Haftalık",
                "m2@x",
                "",
                "İstemiyorsan abonelikten ÇIK.",
            ),
        );
        let m3 = deliver(
            &env,
            "a",
            raw_mail("x@y.com", "KAZANDINIZ", "m3@x", "", "!"),
        );
        let m4 = deliver(
            &env,
            "a",
            raw_mail("Ayşe <ayse@eposta.com>", "Selam", "m4@x", "", "Naber"),
        );
        // Hesabı farklı: taşıma kuralı uygulanmaz.
        let m5 = deliver(
            &env,
            "b",
            raw_mail(
                "Muhasebe <muhasebe@sirket.com>",
                "Ekim",
                "m5@x",
                "",
                "Ekte.",
            ),
        );

        let (kept, touched) = env
            .engines()
            .apply_new(
                &rules,
                vec![m1.clone(), m2.clone(), m3.clone(), m4.clone(), m5.clone()],
            )
            .await;
        // Taşınan, susturulan ve çöpe giden için kart yok.
        assert_eq!(
            kept.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![m4.id, m5.id]
        );
        assert!(kept[0].unread);
        assert_eq!(mailbox_of(&env, m1.id), "a:Faturalar");
        assert!(!env.store.summary(m1.id).unwrap().unwrap().unread);
        assert_eq!(mailbox_of(&env, m2.id), "a:INBOX");
        assert!(env.store.summary(m2.id).unwrap().unwrap().starred);
        assert_eq!(mailbox_of(&env, m3.id), "a:Trash", "çöp önceliklidir");
        assert_eq!(mailbox_of(&env, m5.id), "b:INBOX");
        assert!(touched.contains(&"a:Faturalar".to_string()));
        assert!(touched.contains(&"a:Trash".to_string()));
    }

    #[tokio::test]
    async fn durdurma_ve_kapali_kural() {
        let env = env();
        let mut ilk = rule("ilk", vec![from_has("ayse")], vec![RuleAction::Star]);
        ilk.stop = true;
        let sonraki = rule("sonraki", vec![from_has("ayse")], vec![RuleAction::Archive]);
        let mut kapali = rule("kapali", vec![from_has("ayse")], vec![RuleAction::Trash]);
        kapali.enabled = false;
        let rules = compile(&env, &[ilk, sonraki, kapali]);
        let m = deliver(
            &env,
            "a",
            raw_mail("Ayşe <ayse@eposta.com>", "Selam", "s@x", "", "."),
        );
        let (kept, _) = env.engines().apply_new(&rules, vec![m.clone()]).await;
        assert_eq!(kept.len(), 1);
        assert!(kept[0].starred, "kartta yıldız görünür");
        assert_eq!(mailbox_of(&env, m.id), "a:INBOX");
    }

    #[tokio::test]
    async fn iletme_ve_dongu_korumasi() {
        let env = env();
        let ilet = rule(
            "ilet",
            vec![RuleCondition::Subject {
                op: RuleOp::Contains,
                value: "rapor".into(),
            }],
            vec![RuleAction::Forward {
                to: "arsiv@baska.com".into(),
            }],
        );
        let rules = compile(&env, &[ilet]);
        let m = deliver(
            &env,
            "a",
            raw_mail(
                "Kerem <kerem@ornek.com>",
                "Haftalık rapor",
                "r1@x",
                "",
                "Rapor metni",
            ),
        );
        let own = deliver(
            &env,
            "a",
            raw_mail("Ben <FURKAN@eposta.com>", "Kendi rapor", "r2@x", "", "."),
        );
        let auto = deliver(
            &env,
            "a",
            raw_mail(
                "Robot <robot@ornek.com>",
                "Otomatik rapor",
                "r3@x",
                "Auto-Submitted: auto-generated\r\n",
                ".",
            ),
        );
        let (kept, _) = env
            .engines()
            .apply_new(&rules, vec![m.clone(), own, auto])
            .await;
        assert_eq!(kept.len(), 3, "iletme kartı engellemez");
        let list = env.outbox.scheduled().unwrap();
        assert_eq!(list.len(), 1, "yalnız ilk posta iletilir: {list:?}");
        assert!(list[0].id.starts_with("kural-"));
        assert_eq!(list[0].account_id, "a", "postayı alan hesaptan");
        assert_eq!(list[0].subject, "Fwd: Haftalık rapor");
        assert_eq!(list[0].to, vec![Address::new(None, "arsiv@baska.com")]);

        // Aynı posta aynı kuralla yeniden iletilmez.
        env.engines().apply_new(&rules, vec![m]).await;
        assert_eq!(env.outbox.scheduled().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn elle_calistirma() {
        let env = env();
        env.store
            .tag_insert(&mh_core::Tag {
                id: "e1".into(),
                name: "Fatura".into(),
                color: "#4C6FD8".into(),
            })
            .unwrap();
        let mut r = rule(
            "r",
            vec![from_has("muhasebe")],
            vec![
                RuleAction::Star,
                RuleAction::Mute,
                RuleAction::Forward {
                    to: "x@baska.com".into(),
                },
                RuleAction::Move {
                    mailbox_id: "a:Faturalar".into(),
                },
                RuleAction::Tag {
                    tag_id: "e1".into(),
                },
                RuleAction::Tag {
                    tag_id: "silinmis".into(),
                },
            ],
        );
        r.account_id = Some("a".into());
        env.store.rule_insert(&r).unwrap();
        let m1 = deliver(
            &env,
            "a",
            raw_mail("muhasebe@sirket.com", "1", "e1@x", "", "."),
        );
        let m2 = deliver(
            &env,
            "a",
            raw_mail("muhasebe@sirket.com", "2", "e2@x", "", "."),
        );
        let other = deliver(
            &env,
            "b",
            raw_mail("muhasebe@sirket.com", "3", "e3@x", "", "."),
        );
        deliver(&env, "a", raw_mail("ayse@eposta.com", "4", "e4@x", "", "."));
        let matched = scan_inbox(&env.store, &r).unwrap();
        assert_eq!(
            matched,
            vec![(m1.id, "a".to_string()), (m2.id, "a".to_string())]
        );
        let touched = env.engines().apply_rule(&r, &matched).unwrap();
        assert!(touched.contains(&"a:Faturalar".to_string()));
        assert_eq!(mailbox_of(&env, m1.id), "a:Faturalar");
        assert!(env.store.summary(m2.id).unwrap().unwrap().starred);
        assert_eq!(
            env.store.summary(m2.id).unwrap().unwrap().tags,
            vec!["e1".to_string()],
            "etiket eklenir, silinmiş etiket atlanır"
        );
        assert_eq!(mailbox_of(&env, other.id), "b:INBOX");
        assert!(env.outbox.scheduled().unwrap().is_empty(), "iletme yok");
        // İkinci çalıştırma: gelen kutusunda eşleşen kalmadı.
        assert!(scan_inbox(&env.store, &r).unwrap().is_empty());

        // Tüm hesaplar: arşivleme her postayı kendi hesabının arşivine taşır.
        let mut hepsi = rule("hepsi", vec![from_has("ayse")], vec![RuleAction::Archive]);
        hepsi.account_id = None;
        let n = deliver(&env, "b", raw_mail("ayse@eposta.com", "5", "e5@x", "", "."));
        let matched = scan_inbox(&env.store, &hepsi).unwrap();
        assert_eq!(matched.len(), 2);
        env.engines().apply_rule(&hepsi, &matched).unwrap();
        assert_eq!(mailbox_of(&env, n.id), "b:Archive");
    }

    #[tokio::test]
    async fn klasore_tasima_denetlenir() {
        let env = env();
        let a = deliver(&env, "a", raw_mail("x@y.com", "1", "k1@x", "", "."));
        let b = deliver(&env, "b", raw_mail("x@y.com", "2", "k2@x", "", "."));
        let eng = env.engines();
        assert!(eng.move_to_mailbox(&[], "yok:kutu").unwrap().is_empty());
        let e = eng.move_to_mailbox(&[a.id], "a:Yok").unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        assert_eq!(
            e.message,
            "Klasör bulunamadı; sunucuda silinmiş ya da yeniden adlandırılmış olabilir."
        );
        let e = eng
            .move_to_mailbox(&[a.id, b.id], "a:Faturalar")
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert_eq!(
            e.message,
            "Posta yalnız kendi hesabının klasörlerine taşınabilir."
        );
        assert_eq!(mailbox_of(&env, a.id), "a:INBOX", "hata olunca taşınmaz");
        // Erteleme kalkar.
        env.store.snooze(&[a.id], Some(i64::MAX)).unwrap();
        let touched = eng.move_to_mailbox(&[a.id], "a:Faturalar").unwrap();
        assert_eq!(touched, vec!["a:Faturalar".to_string(), "a:INBOX".into()]);
        let s = env.store.summary(a.id).unwrap().unwrap();
        assert_eq!(
            (s.mailbox_id.as_str(), s.snoozed_until),
            ("a:Faturalar", None)
        );
    }

    #[test]
    fn otomatik_gonderim_basligi() {
        let raw = |h: &str| {
            format!("From: a@b.c\r\n{h}Subject: x\r\n\r\nAuto-Submitted: auto-replied\r\n")
                .into_bytes()
        };
        assert!(!is_auto_submitted(&raw("")), "gövdedeki satır sayılmaz");
        assert!(is_auto_submitted(&raw("Auto-Submitted: auto-replied\r\n")));
        assert!(is_auto_submitted(&raw(
            "auto-submitted:\r\n  auto-generated (robot)\r\n"
        )));
        assert!(is_auto_submitted(&raw(
            "AUTO-SUBMITTED: auto-notified; owner-email=x\r\n"
        )));
        assert!(!is_auto_submitted(&raw("Auto-Submitted: no\r\n")));
        assert!(!is_auto_submitted(&raw("Auto-Submitted: No (insan)\r\n")));
        assert!(!is_auto_submitted(&raw(
            "X-Auto-Submitted: auto-replied\r\n"
        )));
        assert!(is_auto_submitted(b"Auto-Submitted: auto-forwarded\n\nx"));
    }

    #[test]
    fn iletme_metni() {
        let input = RuleInput {
            id: 1,
            account_id: "a".into(),
            mailbox_id: "a:INBOX".into(),
            from: Address::new(Some("Ayşe Demir"), "ayse@eposta.com"),
            to: vec![Address::new(None, "furkan@ornek.com.tr")],
            cc: vec![Address::new(Some("Kerem"), "kerem@ornek.com")],
            subject: " Bütçe ".into(),
            snippet: String::new(),
            body: None,
            has_attachments: false,
            category: Category::People,
            message_id_header: None,
        };
        let (subject, body) = forward_text(&input, 0, "Metin", Language::Tr);
        assert_eq!(subject, "Fwd: Bütçe");
        assert!(body.starts_with(
            "---------- İletilen posta ----------\nKimden: Ayşe Demir <ayse@eposta.com>\n"
        ));
        assert!(body.contains(
            "\nKonu: Bütçe\nKime: furkan@ornek.com.tr\nBilgi: Kerem <kerem@ornek.com>\n\nMetin"
        ));
        let mut fwd = input.clone();
        fwd.subject = "FWD: Bütçe".into();
        let (subject, body) = forward_text(&fwd, 0, "Text", Language::En);
        assert_eq!(subject, "FWD: Bütçe", "önek yinelenmez");
        assert!(body.contains("Forwarded message") && body.contains("\nCc: Kerem"));
    }
}
