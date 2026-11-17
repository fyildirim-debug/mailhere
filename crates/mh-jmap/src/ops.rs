//! Sunucu işlemleri: anahtar sözcükler (okundu, yıldız), taşıma, kalıcı silme
//! ve rol kutuları.

use std::collections::HashSet;

use mh_core::{AppError, ErrorCode, MailboxRole};
use serde_json::{json, Map, Value};

use crate::client::Client;
use crate::error::{JmapError, JmapResult};
use crate::sync::{parse, role_str};
use crate::types::{Email, GetResponse, JMailbox, QueryResponse, SetError, SetResponse};

/// Tek `Email/set` çağrısında güncellenen en fazla posta.
const SET_CHUNK: usize = 200;

pub(crate) fn set_error(e: Option<&SetError>) -> JmapError {
    match e {
        Some(e) => JmapError::Method {
            kind: e.kind.clone(),
            description: e.description.clone(),
        },
        None => JmapError::Protocol("sonuç bildirilmedi".into()),
    }
}

/// `Email/set` güncellemesi; bulunamayan postalar sessizce atlanır (sonraki
/// eşitleme yerelden siler).
async fn update_emails(client: &Client, patches: Vec<(String, Value)>) -> JmapResult<()> {
    for chunk in patches.chunks(SET_CHUNK) {
        let update: Map<String, Value> = chunk.iter().cloned().collect();
        let v = client
            .call(
                "Email/set",
                json!({ "accountId": client.account_id(), "update": update }),
            )
            .await?;
        let r: SetResponse = parse(v)?;
        for (id, err) in r.not_updated.unwrap_or_default() {
            if err.kind == "notFound" {
                tracing::debug!(id, "güncellenecek posta sunucuda yok");
                continue;
            }
            tracing::warn!(id, kind = %err.kind, "posta güncellenemedi");
            return Err(set_error(Some(&err)));
        }
    }
    Ok(())
}

/// Okundu (`$seen`) ve yıldız (`$flagged`) anahtar sözcüklerini yazar.
pub async fn set_keywords(
    client: &Client,
    ids: &[String],
    unread: Option<bool>,
    starred: Option<bool>,
) -> JmapResult<()> {
    if ids.is_empty() || (unread.is_none() && starred.is_none()) {
        return Ok(());
    }
    let mut patch = Map::new();
    if let Some(u) = unread {
        patch.insert(
            "keywords/$seen".into(),
            if u { Value::Null } else { Value::Bool(true) },
        );
    }
    if let Some(s) = starred {
        patch.insert(
            "keywords/$flagged".into(),
            if s { Value::Bool(true) } else { Value::Null },
        );
    }
    let patches = ids
        .iter()
        .map(|id| (id.clone(), Value::Object(patch.clone())))
        .collect();
    update_emails(client, patches).await
}

/// Postaları `dest` kutusuna taşır. Kaynak kutusu bilinen postada yalnız o
/// kutu çıkarılır (IMAP MOVE gibi); bilinmeyende tüm kutular değişir.
pub async fn move_emails(
    client: &Client,
    items: &[(String, Option<String>)],
    dest: &str,
) -> JmapResult<()> {
    let patches = items
        .iter()
        .filter(|(_, src)| src.as_deref() != Some(dest))
        .map(|(id, src)| {
            let patch = match src {
                Some(src) => json!({
                    format!("mailboxIds/{src}"): null,
                    format!("mailboxIds/{dest}"): true,
                }),
                None => json!({ "mailboxIds": { dest: true } }),
            };
            (id.clone(), patch)
        })
        .collect::<Vec<_>>();
    if patches.is_empty() {
        return Ok(());
    }
    update_emails(client, patches).await
}

/// Postaları sunucudan kalıcı olarak siler.
pub async fn destroy_emails(client: &Client, ids: &[String]) -> JmapResult<()> {
    for chunk in ids.chunks(SET_CHUNK) {
        let v = client
            .call(
                "Email/set",
                json!({ "accountId": client.account_id(), "destroy": chunk }),
            )
            .await?;
        let r: SetResponse = parse(v)?;
        for (id, err) in r.not_destroyed.unwrap_or_default() {
            if err.kind != "notFound" {
                tracing::warn!(id, kind = %err.kind, "posta silinemedi");
                return Err(set_error(Some(&err)));
            }
        }
    }
    Ok(())
}

/// Kalıcı silmede bir postaya yapılacak iş (saf karar).
#[derive(Debug, PartialEq, Eq)]
enum Purge {
    /// Posta yalnız Çöp / İstenmeyen kutularında: yok edilir.
    Destroy,
    /// Posta başka bir kutuda da duruyor: yalnız bu kutulardan çıkarılır.
    Detach(Vec<String>),
    /// Posta Çöp ya da İstenmeyen'de değil: dokunulmaz.
    Keep,
}

fn purge_plan<'a>(mailboxes: impl Iterator<Item = &'a str>, allowed: &HashSet<String>) -> Purge {
    let (inside, outside): (Vec<&str>, Vec<&str>) = mailboxes.partition(|m| allowed.contains(*m));
    if inside.is_empty() {
        Purge::Keep
    } else if outside.is_empty() {
        Purge::Destroy
    } else {
        Purge::Detach(inside.into_iter().map(str::to_owned).collect())
    }
}

/// Postaları Çöp ve İstenmeyen kutularından (`allowed`: JMAP kutu
/// kimlikleri) **kalıcı** siler (`messages_delete`, `mailbox_empty`).
///
/// JMAP'te posta birden çok kutuda durabilir ve `destroy` hepsinden siler.
/// Bu yüzden her postanın kutuları önce sunucudan okunur: yalnız Çöp /
/// İstenmeyen'de duran posta yok edilir; başka bir kutuda da duran posta
/// yalnız Çöp / İstenmeyen'den çıkarılır (öteki kutudaki kopya kalır); bu
/// kutularda hiç olmayan postaya (taşıması sunucuya ulaşmamış ya da başka
/// bir yerden geri alınmış) dokunulmaz. Sunucuda artık olmayan posta
/// sessizce atlanır. Dokunulmayan (sunucuda duran) postaların kimliklerini
/// döner: çağıran, yerelden silinmiş bu postaları yeniden eşitler.
pub async fn purge_emails(
    client: &Client,
    ids: &[String],
    allowed: &HashSet<String>,
) -> JmapResult<Vec<String>> {
    let mut kept: Vec<String> = Vec::new();
    if allowed.is_empty() {
        return Ok(ids.to_vec());
    }
    for chunk in ids.chunks(SET_CHUNK.min(client.max_get().max(1))) {
        let v = client
            .call(
                "Email/get",
                json!({
                    "accountId": client.account_id(),
                    "ids": chunk,
                    "properties": ["id", "mailboxIds"],
                }),
            )
            .await?;
        let r: GetResponse<Email> = parse(v)?;
        let mut destroy: Vec<String> = Vec::new();
        let mut patches: Vec<(String, Value)> = Vec::new();
        for e in &r.list {
            match purge_plan(e.mailboxes(), allowed) {
                Purge::Destroy => destroy.push(e.id.clone()),
                Purge::Detach(boxes) => {
                    let patch: Map<String, Value> = boxes
                        .into_iter()
                        .map(|b| (format!("mailboxIds/{b}"), Value::Null))
                        .collect();
                    patches.push((e.id.clone(), Value::Object(patch)));
                }
                Purge::Keep => {
                    tracing::info!(id = %e.id, "posta çöp ya da istenmeyende değil; kalıcı silinmedi");
                    kept.push(e.id.clone());
                }
            }
        }
        destroy_emails(client, &destroy).await?;
        update_emails(client, patches).await?;
    }
    Ok(kept)
}

/// Tek taramada alınan en fazla kimlik.
const SWEEP_PAGE: usize = 500;

/// Kutuyu sunucuda boşaltır (`mailbox_empty`): kutudaki tüm postalar
/// ([`purge_emails`] kurallarıyla) kalıcı silinir; `keep` kümesindekiler
/// (son eşitlemeden beri oluşan ya da değişen, kullanıcının bu hâliyle
/// görmediği postalar) hariç. Silinmeyen (korunan ya da sunucunun
/// bırakmadığı) postalarda takılmamak için her tur ilerleme arar.
pub async fn sweep_mailbox(
    client: &Client,
    mailbox: &str,
    allowed: &HashSet<String>,
    keep: &HashSet<String>,
) -> JmapResult<()> {
    if !allowed.contains(mailbox) {
        return Ok(());
    }
    let mut tried: HashSet<String> = HashSet::new();
    let mut position = 0usize;
    loop {
        let v = client
            .call(
                "Email/query",
                json!({
                    "accountId": client.account_id(),
                    "filter": { "inMailbox": mailbox },
                    "sort": [{ "property": "receivedAt", "isAscending": false }],
                    "position": position,
                    "limit": SWEEP_PAGE,
                }),
            )
            .await?;
        let q: QueryResponse = parse(v)?;
        if q.ids.is_empty() {
            return Ok(());
        }
        let fresh: Vec<String> = q
            .ids
            .iter()
            .filter(|id| !keep.contains(*id) && tried.insert((*id).clone()))
            .cloned()
            .collect();
        if fresh.is_empty() {
            // Bu sayfadakiler korunuyor ya da denenmiş: sonrakine geç.
            position += q.ids.len();
            continue;
        }
        // Kutuda oldukları sorguyla görüldü: hepsi yok edilir ya da kutudan çıkar.
        purge_emails(client, &fresh, allowed).await?;
        // Silinenler sırayı öne çekti: aynı konumdan yeniden sorulur.
    }
}

fn default_name(role: MailboxRole) -> &'static str {
    match role {
        MailboxRole::Inbox => "Inbox",
        MailboxRole::Sent => "Sent",
        MailboxRole::Drafts => "Drafts",
        MailboxRole::Archive => "Archive",
        MailboxRole::Trash => "Trash",
        MailboxRole::Junk => "Junk",
        MailboxRole::Other => "Mailhere",
    }
}

/// Roldeki kutunun sunucu kimliği; yoksa oluşturulur (ör. ilk arşivleme).
pub async fn ensure_role_mailbox(client: &Client, role: MailboxRole) -> JmapResult<String> {
    let want = role_str(role)
        .ok_or_else(|| JmapError::App(AppError::new(ErrorCode::Internal, "Kutu rolü geçersiz.")))?;
    let acc = client.account_id();
    let v = client
        .call(
            "Mailbox/get",
            json!({ "accountId": acc, "ids": null, "properties": ["id", "name", "parentId", "role"] }),
        )
        .await?;
    let r: GetResponse<JMailbox> = parse(v)?;
    if let Some(m) = r.list.iter().find(|m| {
        m.role
            .as_deref()
            .is_some_and(|x| x.eq_ignore_ascii_case(want))
    }) {
        return Ok(m.id.clone());
    }
    let name = default_name(role);
    // Aynı adlı rolsüz kutu varsa ona rol verilir.
    if let Some(m) = r
        .list
        .iter()
        .find(|m| m.parent_id.is_none() && m.role.is_none() && m.name.eq_ignore_ascii_case(name))
    {
        let v = client
            .call(
                "Mailbox/set",
                json!({ "accountId": acc, "update": { (m.id.clone()): { "role": want } } }),
            )
            .await?;
        let s: SetResponse = parse(v)?;
        if s.not_updated.as_ref().is_none_or(|n| n.is_empty()) {
            return Ok(m.id.clone());
        }
        tracing::warn!(role = want, "var olan kutuya rol verilemedi");
    }
    let v = client
        .call(
            "Mailbox/set",
            json!({
                "accountId": acc,
                "create": { "k": { "name": name, "role": want, "parentId": null } },
            }),
        )
        .await?;
    let s: SetResponse = parse(v)?;
    s.created_id("k")
        .ok_or_else(|| set_error(s.create_error("k")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalici_silme_karari() {
        let allowed: HashSet<String> = ["cop".to_string(), "istenmeyen".to_string()]
            .into_iter()
            .collect();
        // Yalnız Çöp / İstenmeyen'de: yok edilir.
        assert_eq!(purge_plan(["cop"].into_iter(), &allowed), Purge::Destroy);
        assert_eq!(
            purge_plan(["cop", "istenmeyen"].into_iter(), &allowed),
            Purge::Destroy
        );
        // Başka bir kutuda da duruyor: yalnız Çöp'ten çıkarılır, kopya kalır.
        assert_eq!(
            purge_plan(["gelen", "cop"].into_iter(), &allowed),
            Purge::Detach(vec!["cop".into()])
        );
        // Çöp'te değil (taşıması ulaşmamış ya da geri alınmış): dokunulmaz.
        assert_eq!(purge_plan(["gelen"].into_iter(), &allowed), Purge::Keep);
        assert_eq!(purge_plan([].into_iter(), &allowed), Purge::Keep);
        // İzinli kutu yoksa hiçbir posta silinmez.
        assert_eq!(
            purge_plan(["cop"].into_iter(), &HashSet::new()),
            Purge::Keep
        );
    }
}
