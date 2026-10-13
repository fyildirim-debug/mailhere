//! JMAP nesneleri (yalnız Mailhere'nin kullandığı alanlar).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

pub const CORE: &str = "urn:ietf:params:jmap:core";
pub const MAIL: &str = "urn:ietf:params:jmap:mail";
pub const SUBMISSION: &str = "urn:ietf:params:jmap:submission";

/// JMAP oturum nesnesi (RFC 8620 §2).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub capabilities: HashMap<String, Value>,
    #[serde(default)]
    pub accounts: HashMap<String, SessionAccount>,
    #[serde(default)]
    pub primary_accounts: HashMap<String, String>,
    #[serde(default)]
    pub username: String,
    pub api_url: String,
    pub download_url: String,
    pub upload_url: String,
    #[serde(default)]
    pub event_source_url: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAccount {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_read_only: bool,
    #[serde(default)]
    pub account_capabilities: HashMap<String, Value>,
}

impl Session {
    /// Çekirdek yeteneğindeki sayısal sınır (yoksa `default`).
    pub fn core_limit(&self, name: &str, default: u64) -> u64 {
        self.capabilities
            .get(CORE)
            .and_then(|c| c.get(name))
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .unwrap_or(default)
    }
}

/// Posta kutusu (RFC 8621 §2).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JMailbox {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub sort_order: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EmailAddress {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// Posta (RFC 8621 §4). İstenen `header:Ad` alanları `extra` içinde gelir.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    pub id: String,
    #[serde(default)]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub mailbox_ids: HashMap<String, bool>,
    #[serde(default)]
    pub keywords: HashMap<String, bool>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub received_at: Option<String>,
    #[serde(default)]
    pub message_id: Option<Vec<String>>,
    #[serde(default)]
    pub in_reply_to: Option<Vec<String>>,
    #[serde(default)]
    pub references: Option<Vec<String>>,
    #[serde(default)]
    pub sender: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub from: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub to: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub cc: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub reply_to: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub sent_at: Option<String>,
    #[serde(default)]
    pub has_attachment: bool,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl Email {
    /// Anahtar sözcük (büyük/küçük harf duyarsız) açık mı?
    pub fn keyword(&self, k: &str) -> bool {
        self.keywords
            .iter()
            .any(|(name, on)| *on && name.eq_ignore_ascii_case(k))
    }

    /// Postanın bulunduğu kutular.
    pub fn mailboxes(&self) -> impl Iterator<Item = &str> {
        self.mailbox_ids
            .iter()
            .filter(|(_, on)| **on)
            .map(|(id, _)| id.as_str())
    }

    /// `header:Ad` biçiminde istenmiş başlıklar: (ad, ham değer).
    pub fn headers(&self) -> Vec<(&str, &str)> {
        self.extra
            .iter()
            .filter_map(|(k, v)| {
                let name = k.strip_prefix("header:")?;
                let name = name.split(':').next().unwrap_or(name);
                Some((name, v.as_str()?))
            })
            .collect()
    }
}

/// `*/changes` yanıtı.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub new_state: String,
    #[serde(default)]
    pub has_more_changes: bool,
    #[serde(default)]
    pub created: Vec<String>,
    #[serde(default)]
    pub updated: Vec<String>,
    #[serde(default)]
    pub destroyed: Vec<String>,
    /// Yalnız `Mailbox/changes`: yalnız sayaçlar değiştiyse sayaç adları.
    #[serde(default)]
    pub updated_properties: Option<Vec<String>>,
}

/// `*/get` yanıtı.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResponse<T> {
    pub state: String,
    #[serde(default = "Vec::new")]
    pub list: Vec<T>,
    #[serde(default)]
    pub not_found: Vec<String>,
}

/// `Email/query` yanıtı.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    #[serde(default)]
    pub ids: Vec<String>,
}

/// Kimlik (RFC 8621 §6).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    #[serde(default)]
    pub email: String,
}

/// `*/set` yanıtındaki hata nesnesi.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetError {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// `*/set` yanıtı (yalnız kullanılan alanlar).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetResponse {
    #[serde(default)]
    pub created: Option<HashMap<String, Value>>,
    #[serde(default)]
    pub not_created: Option<HashMap<String, SetError>>,
    #[serde(default)]
    pub not_updated: Option<HashMap<String, SetError>>,
    #[serde(default)]
    pub not_destroyed: Option<HashMap<String, SetError>>,
}

impl SetResponse {
    /// Oluşturulan nesnenin sunucu kimliği.
    pub fn created_id(&self, creation_id: &str) -> Option<String> {
        self.created
            .as_ref()?
            .get(creation_id)?
            .get("id")?
            .as_str()
            .map(str::to_owned)
    }

    pub fn created_field(&self, creation_id: &str, field: &str) -> Option<String> {
        self.created
            .as_ref()?
            .get(creation_id)?
            .get(field)?
            .as_str()
            .map(str::to_owned)
    }

    pub fn create_error(&self, creation_id: &str) -> Option<&SetError> {
        self.not_created.as_ref()?.get(creation_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eposta_alanlari() {
        let e: Email = serde_json::from_value(serde_json::json!({
            "id": "M1",
            "mailboxIds": {"a": true, "b": false},
            "keywords": {"$Seen": true, "$flagged": false},
            "from": [{"name": "Ayşe", "email": "ayse@eposta.com"}],
            "subject": "Merhaba",
            "header:List-Id": " <duyuru.ornek.com>",
            "header:Precedence": null
        }))
        .unwrap();
        assert!(e.keyword("$seen"));
        assert!(!e.keyword("$flagged"));
        assert_eq!(e.mailboxes().collect::<Vec<_>>(), vec!["a"]);
        assert_eq!(e.headers(), vec![("List-Id", " <duyuru.ornek.com>")]);
    }

    #[test]
    fn set_yaniti() {
        let r: SetResponse = serde_json::from_value(serde_json::json!({
            "accountId": "A",
            "created": {"k": {"id": "X1", "blobId": "B1"}},
            "notCreated": {"z": {"type": "invalidProperties", "description": "kötü"}}
        }))
        .unwrap();
        assert_eq!(r.created_id("k").as_deref(), Some("X1"));
        assert_eq!(r.created_field("k", "blobId").as_deref(), Some("B1"));
        assert_eq!(r.create_error("z").unwrap().kind, "invalidProperties");
        assert!(r.created_id("z").is_none());
    }
}
