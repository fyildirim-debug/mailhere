//! Gönderim ve taslaklar.
//!
//! Posta, mh-send'in kurduğu ham RFC 5322 haliyle (Message-ID, References,
//! ekler — iletilen postanın ekleri dahil — aynen) blob olarak yüklenir ve
//! `Email/import` ile Taslaklar kutusuna `$draft` anahtar sözcüğüyle eklenir.
//! Gönderim `EmailSubmission/set` iledir; başarıda `onSuccessUpdateEmail`
//! postayı Gönderilenler'e taşır ve `$draft`ı kaldırır.

use mh_core::{AppError, ErrorCode, MailboxRole, OutgoingMessage};
use serde_json::{json, Value};

use crate::client::{Call, Client};
use crate::error::{JmapError, JmapResult};
use crate::ops::{destroy_emails, ensure_role_mailbox, set_error};
use crate::sync::parse;
use crate::types::{GetResponse, Identity, SetError, SetResponse};

/// Zarfın alıcıları: Kime + Bilgi + Gizli, yinelenmeden. Gizli alıcılar
/// postanın başlığında yer almaz, yalnız zarfta bulunur.
pub fn envelope_recipients(msg: &OutgoingMessage) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in msg.to.iter().chain(&msg.cc).chain(&msg.bcc) {
        let e = a.email.trim();
        if !e.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(e)) {
            out.push(e.to_owned());
        }
    }
    out
}

/// Sunucuya kaydedilen taslak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedDraft {
    /// Sunucudaki posta kimliği.
    pub id: String,
    pub blob_id: Option<String>,
}

/// Ham postayı yükleyip kutuya ekler: (posta kimliği, blob kimliği).
async fn import(
    client: &Client,
    raw: &[u8],
    mailbox: &str,
    keywords: Value,
) -> JmapResult<(String, Option<String>)> {
    let blob = client.upload(raw.to_vec(), "message/rfc822").await?;
    let v = client
        .call(
            "Email/import",
            json!({
                "accountId": client.account_id(),
                "emails": {
                    "k": { "blobId": blob, "mailboxIds": { (mailbox.to_owned()): true }, "keywords": keywords }
                },
            }),
        )
        .await?;
    let r: SetResponse = parse(v)?;
    match r.created_id("k") {
        Some(id) => Ok((id, r.created_field("k", "blobId").or(Some(blob)))),
        None => Err(set_error(r.create_error("k"))),
    }
}

/// Gönderen kimliği: adres eşleşen, yoksa alan adı joker (`*@alan`), yoksa
/// ilk kimlik; hiç yoksa oluşturulur.
async fn identity_for(client: &Client, email: &str, name: &str) -> JmapResult<String> {
    let acc = client.account_id();
    let v = client
        .call("Identity/get", json!({ "accountId": acc, "ids": null }))
        .await?;
    let r: GetResponse<Identity> = parse(v)?;
    let domain = email.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
    let pick = r
        .list
        .iter()
        .find(|i| i.email.eq_ignore_ascii_case(email))
        .or_else(|| {
            r.list
                .iter()
                .find(|i| i.email.eq_ignore_ascii_case(&format!("*@{domain}")))
        })
        .or_else(|| r.list.first());
    if let Some(i) = pick {
        return Ok(i.id.clone());
    }
    let v = client
        .call(
            "Identity/set",
            json!({ "accountId": acc, "create": { "k": { "name": name, "email": email } } }),
        )
        .await?;
    let s: SetResponse = parse(v)?;
    s.created_id("k")
        .ok_or_else(|| set_error(s.create_error("k")))
}

fn submission_error(e: Option<&SetError>) -> AppError {
    let kind = e.map(|e| e.kind.as_str()).unwrap_or("serverFail");
    let message = match kind {
        "forbiddenFrom" | "forbiddenMailFrom" => {
            "Sunucu bu adresten gönderime izin vermedi. Hesabın gönderen kimliklerini denetle."
                .to_owned()
        }
        "forbiddenToSend" => "Bu hesabın posta gönderme izni yok.".to_owned(),
        "noRecipients" => "Postanın alıcısı yok. En az bir alıcı ekle.".to_owned(),
        "invalidRecipients" => "Alıcı adreslerinden biri sunucu tarafından reddedildi.".to_owned(),
        "tooManyRecipients" => "Alıcı sayısı sunucunun sınırını aşıyor.".to_owned(),
        "tooLarge" => "Posta sunucunun izin verdiği boyutu aşıyor.".to_owned(),
        other => format!("Sunucu postayı göndermedi ({other})."),
    };
    AppError::new(ErrorCode::Invalid, message)
}

/// Ham postayı gönderir; sunucudaki posta kimliğini döner. Başarısız olursa
/// Taslaklar'a eklenen kopya silinir (giden kutusu kendi taslağını yazar).
pub async fn submit(
    client: &Client,
    from_email: &str,
    from_name: &str,
    raw: &[u8],
    rcpt_to: &[String],
) -> JmapResult<String> {
    if !client.can_submit() {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Bu JMAP sunucusu posta gönderimini desteklemiyor.",
        )
        .into());
    }
    if rcpt_to.is_empty() {
        return Err(AppError::new(ErrorCode::Invalid, "En az bir alıcı ekle.").into());
    }
    let drafts = ensure_role_mailbox(client, MailboxRole::Drafts).await?;
    let sent = ensure_role_mailbox(client, MailboxRole::Sent).await?;
    let identity = identity_for(client, from_email, from_name).await?;
    let (email_id, _) = import(
        client,
        raw,
        &drafts,
        json!({ "$draft": true, "$seen": true }),
    )
    .await?;
    let mut on_success = serde_json::Map::new();
    on_success.insert(format!("mailboxIds/{drafts}"), Value::Null);
    on_success.insert(format!("mailboxIds/{sent}"), Value::Bool(true));
    on_success.insert("keywords/$draft".into(), Value::Null);
    let rcpt: Vec<Value> = rcpt_to.iter().map(|r| json!({ "email": r })).collect();
    let calls: Vec<Call> = vec![(
        "EmailSubmission/set".into(),
        json!({
            "accountId": client.account_id(),
            "create": {
                "s": {
                    "identityId": identity,
                    "emailId": email_id,
                    "envelope": { "mailFrom": { "email": from_email }, "rcptTo": rcpt },
                }
            },
            "onSuccessUpdateEmail": { "#s": on_success },
        }),
    )];
    let result = async {
        let responses = client.request(&calls).await?;
        // İlk yanıt gönderimin kendisi; ardından örtük Email/set gelir.
        let (name, args, _) = responses
            .iter()
            .find(|(_, _, id)| id == "0")
            .ok_or_else(|| JmapError::Protocol("gönderim yanıtı yok".into()))?;
        if name == "error" {
            return Err(crate::client::method_error(args));
        }
        let r: SetResponse = parse(args.clone())?;
        if r.created.as_ref().is_none_or(|c| !c.contains_key("s")) {
            let err = r.create_error("s").cloned();
            tracing::warn!(kind = ?err.as_ref().map(|e| &e.kind), "gönderim reddedildi");
            return Err(JmapError::App(submission_error(err.as_ref())));
        }
        if let Some((_, args, _)) = responses
            .iter()
            .find(|(n, _, id)| id == "0" && n == "Email/set")
        {
            let u: SetResponse = parse(args.clone()).unwrap_or_default();
            if u.not_updated.as_ref().is_some_and(|n| !n.is_empty()) {
                tracing::warn!("gönderilen posta Gönderilenler'e taşınamadı");
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        if let Err(d) = destroy_emails(client, std::slice::from_ref(&email_id)).await {
            tracing::warn!(error = %d, "gönderilemeyen taslak sunucudan silinemedi");
        }
        return Err(e);
    }
    Ok(email_id)
}

/// Taslağı sunucudaki Taslaklar kutusuna kaydeder; `replace` verilirse önceki
/// taslak sunucudan silinir.
pub async fn save_draft(
    client: &Client,
    raw: &[u8],
    replace: Option<&str>,
) -> JmapResult<SavedDraft> {
    let drafts = ensure_role_mailbox(client, MailboxRole::Drafts).await?;
    let (id, blob_id) = import(
        client,
        raw,
        &drafts,
        json!({ "$draft": true, "$seen": true }),
    )
    .await?;
    if let Some(old) = replace.filter(|o| *o != id) {
        if let Err(e) = destroy_emails(client, &[old.to_owned()]).await {
            tracing::warn!(error = %e, "önceki taslak sunucudan silinemedi");
        }
    }
    Ok(SavedDraft { id, blob_id })
}

/// Taslağı sunucudan siler (zaten yoksa hata değildir).
pub async fn delete_draft(client: &Client, email_id: &str) -> JmapResult<()> {
    destroy_emails(client, &[email_id.to_owned()]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::Address;

    #[test]
    fn zarf_alicilari() {
        let msg = OutgoingMessage {
            account_id: "a".into(),
            to: vec![Address::new(Some("Ayşe"), "ayse@eposta.com")],
            cc: vec![
                Address::new(None, " AYSE@eposta.com "),
                Address::new(None, "kerem@ornek.com"),
            ],
            bcc: vec![Address::new(None, "gizli@ornek.com")],
            subject: String::new(),
            body_text: String::new(),
            in_reply_to: None,
            attachments: vec![],
            send_at: None,
            forward_from: None,
            draft_id: None,
            follow_up_at: None,
        };
        assert_eq!(
            envelope_recipients(&msg),
            vec!["ayse@eposta.com", "kerem@ornek.com", "gizli@ornek.com"]
        );
    }

    #[test]
    fn gonderim_hatalari_turkce() {
        let e = SetError {
            kind: "forbiddenFrom".into(),
            description: None,
        };
        let a = submission_error(Some(&e));
        assert_eq!(a.code, ErrorCode::Invalid);
        assert!(a.message.contains("izin vermedi"));
        assert!(submission_error(None).message.contains("serverFail"));
    }
}
