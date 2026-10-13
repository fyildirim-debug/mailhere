//! Sunucu işlemleri: anahtar sözcükler (okundu, yıldız), taşıma ve rol kutuları.

use mh_core::{AppError, ErrorCode, MailboxRole};
use serde_json::{json, Map, Value};

use crate::client::Client;
use crate::error::{JmapError, JmapResult};
use crate::sync::{parse, role_str};
use crate::types::{GetResponse, JMailbox, SetError, SetResponse};

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
