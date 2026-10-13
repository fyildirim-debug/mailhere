//! Kutu ve posta eşitleme.
//!
//! İlk eşitleme her önemli kutunun en yeni 300 postasını `Email/query` ile
//! bulur; sonraki eşitlemeler `Email/changes` ve `Mailbox/changes` ile yalnız
//! farkı alır. Durum dizgeleri depoda (`jmap_accounts`) kalıcıdır. Gövde
//! (ham RFC 5322) gerektiğinde blob olarak indirilir.

use std::collections::{HashMap, HashSet};

use mh_core::{Address, AppError, ErrorCode, MailboxRole, MessageId, Millis};
use mh_store::{NewMessage, Store};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::client::{Call, Client};
use crate::error::{JmapError, JmapResult};
use crate::types::{Changes, Email, EmailAddress, GetResponse, JMailbox, QueryResponse};

/// İlk eşitlemede kutu başına indirilen en yeni posta sayısı.
pub const INITIAL_FETCH: usize = 300;
/// Bu boyuta kadar yeni postaların gövdesi arka planda indirilir.
pub const MAX_INLINE_BODY: u64 = 512 * 1024;
/// Tek eşitlemede arka planda indirilecek en fazla gövde.
const PREFETCH_MAX: usize = 20;
const MAX_CHANGES: u64 = 500;
/// Bundan çok değişiklik birikmişse baştan eşitlemek daha ucuzdur.
const RESYNC_THRESHOLD: usize = 3000;

/// Eşitlenen kutu rolleri (IMAP motoruyla aynı).
pub const SYNC_ROLES: [MailboxRole; 5] = [
    MailboxRole::Inbox,
    MailboxRole::Sent,
    MailboxRole::Drafts,
    MailboxRole::Archive,
    MailboxRole::Trash,
];

/// Posta birden çok eşitlenen kutudaysa yerelde gösterileceği kutu.
const PRIORITY: [MailboxRole; 5] = [
    MailboxRole::Trash,
    MailboxRole::Inbox,
    MailboxRole::Drafts,
    MailboxRole::Sent,
    MailboxRole::Archive,
];

/// Yalnız sayaçları değişen kutular yeniden indirilmez.
const COUNT_PROPS: [&str; 4] = [
    "totalEmails",
    "unreadEmails",
    "totalThreads",
    "unreadThreads",
];

/// Eşitlemenin sonucu.
#[derive(Debug, Default)]
pub struct SyncOutcome {
    /// İçeriği değişen yerel kutular.
    pub changed: Vec<String>,
    /// Gelen kutusuna yeni düşen okunmamış postalar (bildirim için).
    pub new_unread: Vec<MessageId>,
    /// Gövdesi arka planda indirilecek yeni postalar: (yerel kimlik, blob).
    pub prefetch: Vec<(MessageId, String)>,
    /// Baştan eşitleme yapıldı mı?
    pub full: bool,
}

impl SyncOutcome {
    fn finish(mut self) -> Self {
        self.changed.sort();
        self.changed.dedup();
        self
    }
}

/// JMAP kutu rolü → yerel rol.
pub fn role_from(role: Option<&str>) -> MailboxRole {
    match role.map(str::to_ascii_lowercase).as_deref() {
        Some("inbox") => MailboxRole::Inbox,
        Some("sent") => MailboxRole::Sent,
        Some("drafts") => MailboxRole::Drafts,
        Some("archive") => MailboxRole::Archive,
        Some("trash") => MailboxRole::Trash,
        Some("junk") => MailboxRole::Junk,
        _ => MailboxRole::Other,
    }
}

/// Yerel rol → JMAP kutu rolü.
pub fn role_str(role: MailboxRole) -> Option<&'static str> {
    Some(match role {
        MailboxRole::Inbox => "inbox",
        MailboxRole::Sent => "sent",
        MailboxRole::Drafts => "drafts",
        MailboxRole::Archive => "archive",
        MailboxRole::Trash => "trash",
        MailboxRole::Junk => "junk",
        MailboxRole::Other => return None,
    })
}

pub(crate) fn parse<T: DeserializeOwned>(v: Value) -> JmapResult<T> {
    serde_json::from_value(v).map_err(|e| JmapError::Protocol(format!("yanıt çözülemedi: {e}")))
}

fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

fn to_address(a: &EmailAddress) -> Option<Address> {
    let email = a
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())?;
    let name = a
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty() && *n != email);
    Some(Address::new(name, email))
}

fn addresses(list: Option<&Vec<EmailAddress>>) -> Vec<Address> {
    list.map(|l| l.iter().filter_map(to_address).collect())
        .unwrap_or_default()
}

fn clean_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_owned()
}

fn parse_date(s: &str) -> Option<Millis> {
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Liste için istenen posta alanları (gövde hariç).
pub fn email_properties() -> Vec<String> {
    let mut p: Vec<String> = [
        "id",
        "blobId",
        "threadId",
        "mailboxIds",
        "keywords",
        "size",
        "receivedAt",
        "messageId",
        "inReplyTo",
        "references",
        "sender",
        "from",
        "to",
        "cc",
        "replyTo",
        "subject",
        "sentAt",
        "hasAttachment",
        "preview",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    p.extend(
        mh_mime::category::HEADERS
            .iter()
            .map(|h| format!("header:{h}")),
    );
    p
}

/// Yalnız durum değişikliği için istenen alanlar.
const LIGHT: [&str; 3] = ["id", "mailboxIds", "keywords"];

/// JMAP postasından yerel kayıt (gövdesiz).
pub fn to_new_message(e: &Email, account_id: &str, mailbox_id: &str, now: Millis) -> NewMessage {
    let from = addresses(e.from.as_ref())
        .into_iter()
        .next()
        .or_else(|| addresses(e.sender.as_ref()).into_iter().next())
        .unwrap_or_else(|| Address::new(None, ""));
    let category = mh_mime::classify(&e.headers(), &from.email);
    let received = e.received_at.as_deref().and_then(parse_date);
    let mut date = e
        .sent_at
        .as_deref()
        .and_then(parse_date)
        .or(received)
        .unwrap_or(now);
    // Gelecekten gelen (bozuk) tarihlerde sunucunun alış zamanı esas alınır.
    if date > now + 86_400_000 {
        date = received.unwrap_or(now);
    }
    let first_id = |v: Option<&Vec<String>>| {
        v.and_then(|l| l.first())
            .map(|s| clean_id(s))
            .filter(|s| !s.is_empty())
    };
    NewMessage {
        account_id: account_id.into(),
        mailbox_id: mailbox_id.into(),
        uid: None,
        uidvalidity: None,
        message_id_header: first_id(e.message_id.as_ref()),
        in_reply_to: first_id(e.in_reply_to.as_ref()),
        references: e
            .references
            .as_ref()
            .map(|l| {
                l.iter()
                    .map(|s| clean_id(s))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        to: addresses(e.to.as_ref()),
        cc: addresses(e.cc.as_ref()),
        reply_to: addresses(e.reply_to.as_ref()),
        subject: e.subject.as_deref().unwrap_or("").trim().to_owned(),
        snippet: e
            .preview
            .as_deref()
            .map(mh_mime::snippet)
            .unwrap_or_default(),
        body_text: None,
        date,
        unread: !e.keyword("$seen"),
        starred: e.keyword("$flagged"),
        has_attachments: e.has_attachment,
        category,
        size: e.size,
        raw: None,
        from,
    }
}

/// Eşitlenen kutular: JMAP kutu kimliği → (yerel kimlik, rol).
struct Boxes(HashMap<String, (String, MailboxRole)>);

impl Boxes {
    fn load(store: &Store, account_id: &str) -> JmapResult<Self> {
        Ok(Self(
            store
                .mailboxes_for(account_id)?
                .into_iter()
                .filter(|m| SYNC_ROLES.contains(&m.mailbox.role))
                .map(|m| (m.mailbox.path, (m.mailbox.id, m.mailbox.role)))
                .collect(),
        ))
    }

    /// Postanın yerelde gösterileceği kutu; eşitlenen bir kutuda değilse `None`.
    fn primary(&self, e: &Email) -> Option<(&str, MailboxRole)> {
        let mut found: Vec<(&str, MailboxRole)> = e
            .mailboxes()
            .filter_map(|id| self.0.get(id))
            .map(|(local, role)| (local.as_str(), *role))
            .collect();
        found.sort_by_key(|(_, r)| PRIORITY.iter().position(|p| p == r));
        found.into_iter().next()
    }
}

/// Kutuları sunucudan alır ve yerelde günceller. (değişen kutular, sunucudaki
/// kutu kimlikleri) döner.
pub async fn sync_mailboxes(
    client: &Client,
    store: &Store,
    account_id: &str,
) -> JmapResult<(Vec<String>, HashSet<String>)> {
    let v = client
        .call(
            "Mailbox/get",
            json!({
                "accountId": client.account_id(),
                "ids": null,
                "properties": ["id", "name", "parentId", "role", "sortOrder"],
            }),
        )
        .await?;
    let r: GetResponse<JMailbox> = parse(v)?;
    let before: HashSet<(String, String, MailboxRole)> = store
        .mailboxes_for(account_id)?
        .into_iter()
        .map(|m| (m.mailbox.id, m.mailbox.name, m.mailbox.role))
        .collect();
    let mut list = r.list;
    // Aynı rol birden çok kutuda görünürse sıradaki ilk kutu alır.
    list.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
    let mut taken: HashSet<MailboxRole> = HashSet::new();
    let mut keep: Vec<String> = Vec::new();
    for mb in &list {
        let mut role = role_from(mb.role.as_deref());
        if role != MailboxRole::Other && !taken.insert(role) {
            role = MailboxRole::Other;
        }
        let name = if mb.name.trim().is_empty() {
            mb.id.as_str()
        } else {
            mb.name.trim()
        };
        store.upsert_mailbox(account_id, &mb.id, name, role)?;
        keep.push(mb.id.clone());
    }
    let server: HashSet<String> = keep.iter().cloned().collect();
    // Yerelde oluşturulup sunucuda karşılığı olmayan rol kutularını koru.
    for info in store.mailboxes_for(account_id)? {
        if !server.contains(&info.mailbox.path)
            && info.mailbox.total > 0
            && info.mailbox.role != MailboxRole::Other
        {
            keep.push(info.mailbox.path);
        }
    }
    let mut changed = store.remove_mailboxes_except(account_id, &keep)?;
    for m in store.mailboxes_for(account_id)? {
        if !before.contains(&(m.mailbox.id.clone(), m.mailbox.name, m.mailbox.role)) {
            changed.push(m.mailbox.id);
        }
    }
    store.set_jmap_mailbox_state(account_id, Some(&r.state))?;
    Ok((changed, server))
}

/// Postaları parça parça getirir; (postalar, bulunamayan kimlikler).
async fn get_emails(
    client: &Client,
    ids: &[String],
    props: &[String],
    progress: &(dyn Fn(u32, u32) + Send + Sync),
) -> JmapResult<(Vec<Email>, Vec<String>)> {
    let mut emails = Vec::with_capacity(ids.len());
    let mut not_found = Vec::new();
    let chunks: Vec<&[String]> = ids.chunks(client.max_get()).collect();
    let total = chunks.len() as u32;
    for (i, chunk) in chunks.into_iter().enumerate() {
        progress(i as u32, total);
        let v = client
            .call(
                "Email/get",
                json!({
                    "accountId": client.account_id(),
                    "ids": chunk,
                    "properties": props,
                }),
            )
            .await?;
        let r: GetResponse<Email> = parse(v)?;
        emails.extend(r.list);
        not_found.extend(r.not_found);
    }
    Ok((emails, not_found))
}

fn light_props() -> Vec<String> {
    LIGHT.iter().map(|s| (*s).to_owned()).collect()
}

/// Bilinen postaların kutu/bayrak durumunu yazar. Yerelde satırı olmayan
/// ama eşitlenen bir kutuya giren postaları (tam alınmak üzere) döner.
fn apply_light(
    store: &Store,
    account_id: &str,
    boxes: &Boxes,
    emails: &[Email],
    out: &mut SyncOutcome,
) -> JmapResult<Vec<String>> {
    let mut refetch = Vec::new();
    let mut remove = Vec::new();
    for e in emails {
        match boxes.primary(e) {
            Some((mailbox, _)) => match store.jmap_update_state(
                account_id,
                &e.id,
                mailbox,
                !e.keyword("$seen"),
                e.keyword("$flagged"),
            )? {
                Some(touched) => out.changed.extend(touched),
                None => refetch.push(e.id.clone()),
            },
            None => remove.push(e.id.clone()),
        }
    }
    if !remove.is_empty() {
        out.changed.extend(store.jmap_remove(account_id, &remove)?);
    }
    Ok(refetch)
}

/// Tam alınmış postaları yazar.
fn apply_full(
    store: &Store,
    account_id: &str,
    boxes: &Boxes,
    emails: &[Email],
    notify: bool,
    out: &mut SyncOutcome,
) -> JmapResult<()> {
    let now = now_ms();
    let mut remove = Vec::new();
    for e in emails {
        let Some((mailbox, role)) = boxes.primary(e) else {
            remove.push(e.id.clone());
            continue;
        };
        let m = to_new_message(e, account_id, mailbox, now);
        let thread = e.thread_id.as_deref().map(|t| format!("j:{t}"));
        let r = store.jmap_upsert(&m, &e.id, e.blob_id.as_deref(), thread.as_deref())?;
        out.changed.extend(r.touched);
        if r.inserted && notify {
            if role == MailboxRole::Inbox && m.unread && !e.keyword("$draft") {
                out.new_unread.push(r.id);
            }
            if e.size <= MAX_INLINE_BODY && out.prefetch.len() < PREFETCH_MAX {
                if let Some(blob) = &e.blob_id {
                    out.prefetch.push((r.id, blob.clone()));
                }
            }
        }
    }
    if !remove.is_empty() {
        out.changed.extend(store.jmap_remove(account_id, &remove)?);
    }
    Ok(())
}

/// Baştan eşitleme: kutular ve her önemli kutunun en yeni postaları. İlk
/// eşitlemede (ve durum kaybolunca) bildirim üretmez.
pub async fn full_sync(
    client: &Client,
    store: &Store,
    account_id: &str,
    progress: &(dyn Fn(u32, u32) + Send + Sync),
) -> JmapResult<SyncOutcome> {
    let mut out = SyncOutcome {
        full: true,
        ..SyncOutcome::default()
    };
    let (changed, server) = sync_mailboxes(client, store, account_id).await?;
    out.changed.extend(changed);
    let boxes = Boxes::load(store, account_id)?;
    let acc = client.account_id();
    // Önce durum: sorgular sırasında gelen değişiklikler sonraki artımlı
    // eşitlemede yeniden bildirilir (yazım tekrarlanabilir).
    let mut calls: Vec<Call> = vec![(
        "Email/get".into(),
        json!({ "accountId": acc, "ids": [], "properties": ["id"] }),
    )];
    let mut paths: Vec<&String> = boxes.0.keys().filter(|p| server.contains(*p)).collect();
    paths.sort();
    for path in paths {
        calls.push((
            "Email/query".into(),
            json!({
                "accountId": acc,
                "filter": { "inMailbox": path },
                "sort": [{ "property": "receivedAt", "isAscending": false }],
                "limit": INITIAL_FETCH,
            }),
        ));
    }
    let mut responses = Vec::with_capacity(calls.len());
    for chunk in calls.chunks(client.max_calls().max(1)) {
        responses.extend(client.calls(chunk).await?);
    }
    let mut responses = responses.into_iter();
    let state = responses
        .next()
        .and_then(|v| v.get("state").and_then(Value::as_str).map(str::to_owned))
        .ok_or_else(|| JmapError::Protocol("Email/get durumu yok".into()))?;
    let mut ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for v in responses {
        let q: QueryResponse = parse(v)?;
        for id in q.ids {
            if seen.insert(id.clone()) {
                ids.push(id);
            }
        }
    }
    let known = store.jmap_known(account_id)?;
    let gone: Vec<String> = known
        .keys()
        .filter(|k| !seen.contains(*k))
        .cloned()
        .collect();
    if !gone.is_empty() {
        out.changed.extend(store.jmap_remove(account_id, &gone)?);
    }
    let (old, new): (Vec<String>, Vec<String>) =
        ids.into_iter().partition(|id| known.contains_key(id));
    let (light, _) = get_emails(client, &old, &light_props(), &|_, _| {}).await?;
    let mut new = new;
    new.extend(apply_light(store, account_id, &boxes, &light, &mut out)?);
    let (full, _) = get_emails(client, &new, &email_properties(), progress).await?;
    apply_full(store, account_id, &boxes, &full, false, &mut out)?;
    store.set_jmap_email_state(account_id, Some(&state))?;
    Ok(out.finish())
}

/// Artımlı eşitleme. Durum yoksa ya da sunucu farkı hesaplayamıyorsa baştan
/// eşitler.
pub async fn incremental_sync(
    client: &Client,
    store: &Store,
    account_id: &str,
    progress: &(dyn Fn(u32, u32) + Send + Sync),
) -> JmapResult<SyncOutcome> {
    let st = store.jmap_account_state(account_id)?;
    let Some(mut since) = st.email_state else {
        return full_sync(client, store, account_id, progress).await;
    };
    let acc = client.account_id().to_owned();
    let mut out = SyncOutcome::default();

    // 1) Kutular
    let refresh_boxes = match st.mailbox_state.as_deref() {
        None => true,
        Some(ms) => match client
            .call(
                "Mailbox/changes",
                json!({ "accountId": acc, "sinceState": ms }),
            )
            .await
        {
            Ok(v) => {
                let c: Changes = parse(v)?;
                let nothing = c.created.is_empty()
                    && c.updated.is_empty()
                    && c.destroyed.is_empty()
                    && !c.has_more_changes;
                let counts_only = c.created.is_empty()
                    && c.destroyed.is_empty()
                    && !c.has_more_changes
                    && c.updated_properties
                        .as_ref()
                        .is_some_and(|p| p.iter().all(|x| COUNT_PROPS.contains(&x.as_str())));
                if nothing || counts_only {
                    store.set_jmap_mailbox_state(account_id, Some(&c.new_state))?;
                    false
                } else {
                    true
                }
            }
            Err(e) if e.cannot_calculate_changes() => true,
            Err(e) => return Err(e),
        },
    };
    if refresh_boxes {
        out.changed
            .extend(sync_mailboxes(client, store, account_id).await?.0);
    }
    let boxes = Boxes::load(store, account_id)?;

    // 2) Postalar
    let mut created: Vec<String> = Vec::new();
    let mut updated: Vec<String> = Vec::new();
    let mut destroyed: Vec<String> = Vec::new();
    loop {
        let res = client
            .call(
                "Email/changes",
                json!({ "accountId": acc, "sinceState": since, "maxChanges": MAX_CHANGES }),
            )
            .await;
        let c: Changes = match res {
            Ok(v) => parse(v)?,
            Err(e) if e.cannot_calculate_changes() => {
                tracing::info!(
                    account = account_id,
                    "sunucu farkı hesaplayamadı, baştan eşitleniyor"
                );
                return resync(client, store, account_id, progress, out).await;
            }
            Err(e) => return Err(e),
        };
        created.extend(c.created);
        updated.extend(c.updated);
        destroyed.extend(c.destroyed);
        since = c.new_state;
        if !c.has_more_changes {
            break;
        }
        if created.len() + updated.len() > RESYNC_THRESHOLD {
            return resync(client, store, account_id, progress, out).await;
        }
    }
    let destroyed_set: HashSet<&String> = destroyed.iter().collect();
    if !destroyed.is_empty() {
        out.changed
            .extend(store.jmap_remove(account_id, &destroyed)?);
    }
    let known = store.jmap_known(account_id)?;
    let created_set: HashSet<&String> = created.iter().collect();
    let mut seen: HashSet<&String> = HashSet::new();
    let mut light_ids = Vec::new();
    let mut full_ids = Vec::new();
    for id in created.iter().chain(updated.iter()) {
        if destroyed_set.contains(id) || !seen.insert(id) {
            continue;
        }
        if !created_set.contains(id) && known.contains_key(id) {
            light_ids.push(id.clone());
        } else {
            full_ids.push(id.clone());
        }
    }
    let (light, mut gone) = get_emails(client, &light_ids, &light_props(), &|_, _| {}).await?;
    full_ids.extend(apply_light(store, account_id, &boxes, &light, &mut out)?);
    let (full, gone_full) = get_emails(client, &full_ids, &email_properties(), progress).await?;
    apply_full(store, account_id, &boxes, &full, true, &mut out)?;
    gone.extend(gone_full);
    if !gone.is_empty() {
        out.changed.extend(store.jmap_remove(account_id, &gone)?);
    }
    store.set_jmap_email_state(account_id, Some(&since))?;
    Ok(out.finish())
}

async fn resync(
    client: &Client,
    store: &Store,
    account_id: &str,
    progress: &(dyn Fn(u32, u32) + Send + Sync),
    before: SyncOutcome,
) -> JmapResult<SyncOutcome> {
    let mut out = full_sync(client, store, account_id, progress).await?;
    out.changed.extend(before.changed);
    Ok(out.finish())
}

/// Postanın ham halini indirip depoya yazar.
pub async fn fetch_body(
    client: &Client,
    store: &Store,
    id: MessageId,
    jmap_id: &str,
    blob_id: Option<&str>,
) -> JmapResult<()> {
    let blob = match blob_id {
        Some(b) => b.to_owned(),
        None => {
            let v = client
                .call(
                    "Email/get",
                    json!({
                        "accountId": client.account_id(),
                        "ids": [jmap_id],
                        "properties": ["id", "blobId"],
                    }),
                )
                .await?;
            let r: GetResponse<Email> = parse(v)?;
            r.list
                .into_iter()
                .next()
                .and_then(|e| e.blob_id)
                .ok_or_else(|| {
                    JmapError::App(AppError::new(
                        ErrorCode::NotFound,
                        "Posta sunucuda bulunamadı; başka bir yerden silinmiş olabilir.",
                    ))
                })?
        }
    };
    let raw = client
        .download(&blob, "posta.eml", "message/rfc822")
        .await?;
    store.store_body(id, &raw)?;
    Ok(())
}

/// Yeni postaların gövdelerini indirir; indirilenlerin kimliklerini döner.
pub async fn prefetch(
    client: &Client,
    store: &Store,
    list: &[(MessageId, String)],
) -> Vec<MessageId> {
    let mut done = Vec::new();
    for (id, blob) in list {
        match client.download(blob, "posta.eml", "message/rfc822").await {
            Ok(raw) => match store.store_body(*id, &raw) {
                Ok(()) => done.push(*id),
                Err(e) => tracing::warn!(error = %e, "gövde yazılamadı"),
            },
            Err(e) => {
                tracing::debug!(error = %e, "gövde önceden indirilemedi");
                if e.is_connection() {
                    break;
                }
            }
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(v: Value) -> Email {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn roller() {
        assert_eq!(role_from(Some("inbox")), MailboxRole::Inbox);
        assert_eq!(role_from(Some("Sent")), MailboxRole::Sent);
        assert_eq!(role_from(Some("all")), MailboxRole::Other);
        assert_eq!(role_from(None), MailboxRole::Other);
        for r in SYNC_ROLES {
            assert_eq!(role_from(role_str(r)), r);
        }
        assert_eq!(role_str(MailboxRole::Other), None);
    }

    #[test]
    fn eposta_yerel_kayda_donusur() {
        let e = email(json!({
            "id": "M1",
            "threadId": "T1",
            "mailboxIds": {"mb1": true},
            "keywords": {"$seen": true, "$flagged": true},
            "size": 1234,
            "receivedAt": "2026-10-12T05:20:00Z",
            "sentAt": "2026-10-12T08:15:00+03:00",
            "messageId": ["kahvalti-1@eposta.com"],
            "inReplyTo": ["onceki@eposta.com"],
            "references": ["kok@eposta.com", "onceki@eposta.com"],
            "from": [{"name": "Ayşe Demir", "email": "ayse.demir@eposta.com"}],
            "to": [{"name": null, "email": "furkan@eposta.com"}],
            "cc": [],
            "replyTo": [{"name": "ayse@baska.com", "email": "ayse@baska.com"}],
            "subject": " Cumartesi kahvaltı? ",
            "hasAttachment": true,
            "preview": "Moda'daki   yeni yere\n gidelim mi?",
            "header:List-Unsubscribe": null,
        }));
        let m = to_new_message(&e, "a", "a:mb1", 1_791_800_000_000);
        assert_eq!(m.mailbox_id, "a:mb1");
        assert_eq!(
            m.from,
            Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com")
        );
        assert_eq!(m.to, vec![Address::new(None, "furkan@eposta.com")]);
        assert_eq!(m.reply_to, vec![Address::new(None, "ayse@baska.com")]);
        assert_eq!(m.subject, "Cumartesi kahvaltı?");
        assert_eq!(m.snippet, "Moda'daki yeni yere gidelim mi?");
        assert_eq!(
            m.message_id_header.as_deref(),
            Some("kahvalti-1@eposta.com")
        );
        assert_eq!(m.in_reply_to.as_deref(), Some("onceki@eposta.com"));
        assert_eq!(m.references.len(), 2);
        assert_eq!(m.date, 1_791_782_100_000);
        assert!(!m.unread);
        assert!(m.starred);
        assert!(m.has_attachments);
        assert_eq!(m.size, 1234);
        assert_eq!(m.category, mh_core::Category::People);
        assert!(m.raw.is_none());
    }

    #[test]
    fn kategori_basliklardan() {
        let e = email(json!({
            "id": "M2",
            "from": [{"email": "bulten@ornek.com"}],
            "header:List-Id": "<duyuru.ornek.com>",
            "receivedAt": "2026-10-12T05:20:00Z"
        }));
        let m = to_new_message(&e, "a", "a:x", i64::MAX / 2);
        assert_eq!(m.category, mh_core::Category::Newsletters);
        assert!(m.unread);
        let e = email(json!({"id": "M3", "sender": [{"email": "noreply@banka.com"}]}));
        let m = to_new_message(&e, "a", "a:x", 5);
        assert_eq!(m.from.email, "noreply@banka.com");
        assert_eq!(m.category, mh_core::Category::Notifications);
        assert_eq!(m.date, 5, "tarih yoksa şimdi");
    }

    #[test]
    fn gelecek_tarih_alis_zamanina_doner() {
        let e = email(json!({
            "id": "M4",
            "receivedAt": "2026-10-12T05:20:00Z",
            "sentAt": "2099-01-01T00:00:00Z"
        }));
        let m = to_new_message(&e, "a", "a:x", 1_791_790_000_000);
        assert_eq!(m.date, 1_791_782_400_000);
    }

    #[test]
    fn oncelikli_kutu() {
        let boxes = Boxes(HashMap::from([
            ("in".to_string(), ("a:in".to_string(), MailboxRole::Inbox)),
            ("tr".to_string(), ("a:tr".to_string(), MailboxRole::Trash)),
            ("ar".to_string(), ("a:ar".to_string(), MailboxRole::Archive)),
        ]));
        let e = |ids: &[&str]| {
            email(json!({
                "id": "x",
                "mailboxIds": ids.iter().map(|i| (i.to_string(), true)).collect::<HashMap<_, _>>()
            }))
        };
        assert_eq!(boxes.primary(&e(&["ar", "in"])).unwrap().0, "a:in");
        assert_eq!(boxes.primary(&e(&["in", "tr"])).unwrap().0, "a:tr");
        assert_eq!(boxes.primary(&e(&["ozel", "ar"])).unwrap().0, "a:ar");
        assert!(boxes.primary(&e(&["ozel"])).is_none());
    }

    #[test]
    fn istenen_alanlar() {
        let p = email_properties();
        assert!(p.contains(&"header:List-Unsubscribe".to_string()));
        assert!(p.contains(&"preview".to_string()));
        assert!(!p.iter().any(|x| x == "bodyValues"));
    }
}
