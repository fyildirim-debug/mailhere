//! Mailhere ortak tipleri.
//!
//! Bu dosya arayüzle yapılan sözleşmedir: her tip JSON'a camelCase alanlarla
//! dönüşür ve `app/src/lib/api/types.ts` ile birebir aynı tutulur. Bir alan
//! eklenir ya da değişirse iki dosya aynı committe güncellenir.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Hesap kimliği (UUID metni).
pub type AccountId = String;
/// Yerel posta kimliği (SQLite satır kimliği).
pub type MessageId = i64;
/// Unix zaman damgası, milisaniye.
pub type Millis = i64;

// ---------------------------------------------------------------- hesaplar

/// Bağlantı güvenliği.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    /// Doğrudan TLS (IMAP 993, SMTP 465).
    Tls,
    /// Düz bağlantı + STARTTLS (IMAP 143, SMTP 587). Yükseltme zorunludur.
    Starttls,
    /// Şifresiz; yalnız yerel test sunucuları için.
    None,
}

/// Bir sunucunun bağlantı ayarı.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub username: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Imap,
    Jmap,
}

/// Kayıtlı bir hesap. Parola burada yoktur; anahtar zincirindedir.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: AccountId,
    /// Kenar çubuğunda görünen kısa ad ("İş", "Kişisel").
    pub name: String,
    pub email: String,
    /// Gönderirken "Kimden" adı.
    pub display_name: String,
    /// Hesap noktası rengi (#RRGGBB).
    pub color: String,
    pub kind: AccountKind,
    pub imap: Option<ServerConfig>,
    pub smtp: Option<ServerConfig>,
    pub jmap_url: Option<String>,
    pub signature: Option<String>,
    pub created_at: Millis,
}

/// Hesap ekleme isteği. `password` yalnız bir kez kullanılır ve anahtar
/// zincirine yazılır.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NewAccount {
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub color: Option<String>,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiscoverySource {
    /// Alan adının otomatik yapılandırma kaydı.
    Autoconfig,
    /// DNS SRV kayıtları (RFC 6186).
    Srv,
    /// Yaygın sunucu adlarının denenmesi.
    Guess,
    /// Uygulamaya gömülü büyük sağlayıcı bilgisi.
    Builtin,
}

/// E-posta adresinden bulunan sunucu ayarları.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredConfig {
    pub domain: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub source: DiscoverySource,
}

// ------------------------------------------------------------------- kutular

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum MailboxRole {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Trash,
    Junk,
    Other,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    /// "<hesap>:<yol>" biçiminde kararlı kimlik.
    pub id: String,
    pub account_id: AccountId,
    /// Sunucudaki tam yol (IMAP adı).
    pub path: String,
    /// Görünen ad.
    pub name: String,
    pub role: MailboxRole,
    pub unread: u32,
    pub total: u32,
}

// -------------------------------------------------------------------- postalar

/// Akıllı süzgeç kategorisi.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Bir insandan gelen posta.
    People,
    /// Otomatik bildirim (kargo, fatura, CI, hesap uyarısı).
    Notifications,
    /// Bülten ve toplu gönderim.
    Newsletters,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

impl Address {
    pub fn new(name: Option<&str>, email: &str) -> Self {
        Self {
            name: name.map(str::to_owned),
            email: email.to_owned(),
        }
    }

    /// Görünen ad; yoksa adresin @ öncesi.
    pub fn label(&self) -> &str {
        match self.name.as_deref() {
            Some(n) if !n.trim().is_empty() => n,
            _ => self.email.split('@').next().unwrap_or(&self.email),
        }
    }
}

/// Listede bir satır.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: MessageId,
    pub account_id: AccountId,
    pub mailbox_id: String,
    pub thread_id: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub subject: String,
    /// Gövdenin ilk ~200 karakteri, boşlukları sadeleştirilmiş.
    pub snippet: String,
    pub date: Millis,
    pub unread: bool,
    pub starred: bool,
    pub has_attachments: bool,
    pub category: Category,
    pub snoozed_until: Option<Millis>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// Posta içindeki ek sırası (0'dan).
    pub index: u32,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Aynı konuşmadaki önceki bir ileti (katlanmış satır).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEntry {
    pub id: MessageId,
    pub from: Address,
    pub date: Millis,
    pub snippet: String,
}

/// Okuma alanında gösterilen tam posta.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    /// Message-ID başlığı (yanıt için).
    pub message_id_header: Option<String>,
    pub references: Vec<String>,
    pub body_text: Option<String>,
    /// Temizlenmiş HTML. Uzak içerik izin yoksa engellenmiş haldedir.
    pub body_html: Option<String>,
    pub has_remote_content: bool,
    pub remote_allowed: bool,
    pub attachments: Vec<Attachment>,
    /// Aynı konuşmadaki önceki iletiler, eskiden yeniye.
    pub thread: Vec<ThreadEntry>,
}

/// Listelenecek görünüm.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum View {
    Inbox,
    Snoozed,
    Starred,
    Drafts,
    Sent,
    Archive,
    Trash,
    Mailbox { id: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub view: View,
    /// Yalnız bu hesap; yoksa birleşik görünüm.
    pub account_id: Option<AccountId>,
    pub category: Option<Category>,
    /// Önceki sayfanın `nextCursor` değeri.
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub inbox_unread: u32,
    pub snoozed: u32,
    pub drafts: u32,
    pub per_account_unread: HashMap<AccountId, u32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MoveTarget {
    Inbox,
    Archive,
    Trash,
}

// ------------------------------------------------------------------- gönderim

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMessage {
    pub account_id: AccountId,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub body_text: String,
    /// Yanıtlanan ya da iletilen posta.
    pub in_reply_to: Option<MessageId>,
    /// Eklenecek dosyaların yerel yolları.
    pub attachments: Vec<String>,
    /// Zamanlanmış gönderim; yoksa geri alma süresi sonunda gider.
    pub send_at: Option<Millis>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutboxItem {
    pub id: String,
    pub send_at: Millis,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComposeMode {
    New,
    Reply,
    ReplyAll,
    Forward,
}

/// Yazma penceresini açarken verilen ön bilgi.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComposePrefill {
    pub mode: ComposeMode,
    pub message_id: Option<MessageId>,
}

// ------------------------------------------------------------------ eşitleme

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SyncState {
    /// Bağlı, anlık bildirim bekliyor.
    Online,
    Syncing {
        done: u32,
        total: u32,
    },
    Offline,
    Error {
        message: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub account_id: AccountId,
    #[serde(flatten)]
    pub state: SyncState,
}

// -------------------------------------------------------------------- ayarlar

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Tr,
    En,
}

/// Hangi postalar köşe kartı açar.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NotifyMode {
    /// Yalnız Kişiler kategorisi (varsayılan).
    #[default]
    People,
    /// Her yeni posta.
    All,
    /// Yalnız önemli kişiler listesi.
    Vip,
    /// Kart yok, yalnız tepsi rozeti.
    Off,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum CardPosition {
    #[default]
    BottomRight,
    TopRight,
    BottomLeft,
    TopLeft,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub language: Language,
    pub notify: NotifyMode,
    /// Önemli kişiler (VIP) adresleri.
    pub vip: Vec<String>,
    /// Sessiz saat başlangıcı "HH:MM"; yoksa sessiz saat kapalı.
    pub quiet_start: Option<String>,
    pub quiet_end: Option<String>,
    /// Kartın ekranda kalma süresi.
    pub card_seconds: u32,
    pub card_position: CardPosition,
    /// İşletim sisteminin kendi bildirimini kullan.
    pub native_notifications: bool,
    pub undo_send_seconds: u32,
    pub density: Density,
    /// Uzak görsellere izin verilen gönderici adresleri ya da alan adları.
    pub remote_allow: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            language: Language::Tr,
            notify: NotifyMode::People,
            vip: Vec::new(),
            quiet_start: Some("22:00".into()),
            quiet_end: Some("08:00".into()),
            card_seconds: 8,
            card_position: CardPosition::BottomRight,
            native_notifications: false,
            undo_send_seconds: 10,
            density: Density::Comfortable,
            remote_allow: Vec::new(),
        }
    }
}

// ------------------------------------------------------------------- olaylar

/// Olay adları (arka uç → arayüz).
pub mod events {
    /// `NotifyEvent`: köşe kartında gösterilecek yeni posta.
    pub const NOTIFY: &str = "mh:notify";
    /// `SyncStatus`: hesabın eşitleme durumu.
    pub const SYNC: &str = "mh:sync";
    /// `ChangedEvent`: kutular değişti, liste yenilenmeli.
    pub const CHANGED: &str = "mh:changed";
    /// `OutboxEvent`: giden kutusundaki bir posta gönderildi ya da hata verdi.
    pub const OUTBOX: &str = "mh:outbox";
    /// `Settings`: ayarlar değişti (tüm pencereler tema/dil uygular).
    pub const SETTINGS: &str = "mh:settings";
    /// `OpenEvent`: ana pencere bu postayı açmalı (karttan ya da hızlı okumadan).
    pub const OPEN: &str = "mh:open";
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotifyEvent {
    pub message: MessageSummary,
    pub account_name: String,
    pub account_color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenEvent {
    pub id: MessageId,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedEvent {
    pub mailbox_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEvent {
    pub id: String,
    pub ok: bool,
    pub error: Option<String>,
}

// -------------------------------------------------------------------- hatalar

/// Arayüze dönen hata. `code` makinece okunur, `message` Türkçe ve kullanıcıya
/// gösterilebilir; parola ya da jeton içermez.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    NotFound,
    Auth,
    Network,
    Tls,
    Discovery,
    Keyring,
    Storage,
    Invalid,
    Internal,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        f.write_str(&s)
    }
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_json_bicimi() {
        let v = serde_json::to_value(View::Mailbox {
            id: "a:INBOX".into(),
        })
        .unwrap();
        assert_eq!(v, serde_json::json!({"kind": "mailbox", "id": "a:INBOX"}));
        let v = serde_json::to_value(View::Inbox).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "inbox"}));
    }

    #[test]
    fn detail_ozet_alanlarini_duzlestirir() {
        let s = MessageSummary {
            id: 1,
            account_id: "a".into(),
            mailbox_id: "a:INBOX".into(),
            thread_id: "t".into(),
            from: Address::new(Some("Ayşe"), "ayse@ornek.com"),
            to: vec![],
            subject: "Merhaba".into(),
            snippet: "".into(),
            date: 0,
            unread: true,
            starred: false,
            has_attachments: false,
            category: Category::People,
            snoozed_until: None,
        };
        let d = MessageDetail {
            summary: s,
            cc: vec![],
            reply_to: vec![],
            message_id_header: None,
            references: vec![],
            body_text: Some("x".into()),
            body_html: None,
            has_remote_content: false,
            remote_allowed: false,
            attachments: vec![],
            thread: vec![],
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["subject"], "Merhaba");
        assert_eq!(v["hasRemoteContent"], false);
        assert_eq!(v["category"], "people");
    }

    #[test]
    fn sync_durumu_duz() {
        let s = SyncStatus {
            account_id: "a".into(),
            state: SyncState::Syncing { done: 3, total: 10 },
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"accountId": "a", "state": "syncing", "done": 3, "total": 10})
        );
    }

    #[test]
    fn ayar_varsayilanlari_ve_eksik_alanlar() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.notify, NotifyMode::People);
        assert_eq!(s.undo_send_seconds, 10);
        assert_eq!(
            serde_json::to_value(CardPosition::BottomRight).unwrap(),
            "bottomRight"
        );
    }

    #[test]
    fn adres_etiketi() {
        assert_eq!(Address::new(None, "kerem@ornek.com").label(), "kerem");
        assert_eq!(
            Address::new(Some("Kerem Aydın"), "k@o.com").label(),
            "Kerem Aydın"
        );
    }
}
