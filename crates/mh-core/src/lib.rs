//! Mailhere ortak tipleri.
//!
//! Bu dosya arayüzle yapılan sözleşmedir: her tip JSON'a camelCase alanlarla
//! dönüşür ve `app/src/lib/api/types.ts` ile birebir aynı tutulur. Bir alan
//! eklenir ya da değişirse iki dosya aynı committe güncellenir.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod net;

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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    #[default]
    Imap,
    Jmap,
}

/// Hesabın kimlik doğrulama yolu.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    /// Parola ya da uygulama parolası (anahtar zincirinde).
    #[default]
    Password,
    /// Google OAuth 2 (XOAUTH2); yenileme jetonu anahtar zincirinde.
    Google,
    /// Microsoft OAuth 2 (XOAUTH2); yenileme jetonu anahtar zincirinde.
    Microsoft,
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
    /// Hesabın imzaları (en çok 20).
    #[serde(default)]
    pub signatures: Vec<Signature>,
    /// Yeni postada kullanılan imzanın kimliği; yoksa imza eklenmez.
    #[serde(default)]
    pub signature_new: Option<String>,
    /// Yanıt ve iletmede kullanılan imzanın kimliği; yoksa imza eklenmez.
    #[serde(default)]
    pub signature_reply: Option<String>,
    pub created_at: Millis,
    #[serde(default)]
    pub auth: AuthKind,
    /// Bu hesabın yeni postaları için kart ve bildirim gösterilir.
    #[serde(default = "yes")]
    pub notify: bool,
}

fn yes() -> bool {
    true
}

/// Hesaba ait bir imza.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    /// Kararlı kimlik (boş gelirse kaydederken üretilir).
    pub id: String,
    /// Listede görünen ad ("İş", "Kısa").
    pub name: String,
    /// Düz metin gövde.
    pub body: String,
}

/// Hesap ekleme isteği. `password` yalnız bir kez kullanılır ve anahtar
/// zincirine yazılır; `Debug` çıktısında gizlenir.
#[derive(Serialize, Deserialize, Clone)]
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

impl std::fmt::Debug for NewAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewAccount")
            .field("name", &self.name)
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .field("color", &self.color)
            .field("imap", &self.imap)
            .field("smtp", &self.smtp)
            .field("password", &"***")
            .finish()
    }
}

/// JMAP hesabı ekleme isteği (ör. Stalwart). `password` anahtar zincirine
/// yazılır; `Debug` çıktısında gizlenir.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NewJmapAccount {
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub color: Option<String>,
    /// JMAP oturum adresi (ör. https://posta.alanadi.com/.well-known/jmap).
    pub session_url: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for NewJmapAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewJmapAccount")
            .field("name", &self.name)
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .field("color", &self.color)
            .field("session_url", &self.session_url)
            .field("username", &self.username)
            .field("password", &"***")
            .finish()
    }
}

/// Tarayıcıda tamamlanan OAuth girişi. Jetonlar arka uçta bekler; arayüze
/// yalnız kimlik ve adres döner.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OAuthGrant {
    pub grant_id: String,
    pub provider: AuthKind,
    pub email: String,
    pub display_name: Option<String>,
}

/// OAuth girişinden hesap oluşturma isteği.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NewOAuthAccount {
    pub grant_id: String,
    pub name: String,
    pub display_name: String,
    pub color: Option<String>,
}

/// Hesabın düzenlenebilir alanları (tamamı yeniden yazılır).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdate {
    pub name: String,
    pub display_name: String,
    pub color: String,
    /// İmzaların tamamı (yeniden yazılır).
    #[serde(default)]
    pub signatures: Vec<Signature>,
    #[serde(default)]
    pub signature_new: Option<String>,
    #[serde(default)]
    pub signature_reply: Option<String>,
    #[serde(default = "yes")]
    pub notify: bool,
}

/// Parolalı IMAP hesabının sunucu ayarları. Sunucu adı, kapı ya da kullanıcı
/// adı değişiyorsa `password` zorunludur: kayıtlı parola yeni bir sunucuya
/// kendiliğinden gönderilmez. `Debug` çıktısında parola gizlenir.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerUpdate {
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub password: Option<String>,
}

impl std::fmt::Debug for ServerUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerUpdate")
            .field("imap", &self.imap)
            .field("smtp", &self.smtp)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .finish()
    }
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
    /// Doğrulanmamış kaynaktan (DNS SRV, tahmin) gelen IMAP ya da SMTP
    /// sunucusu e-posta alan adının dışında: arayüz parolayı göndermeden önce
    /// uyarmalı. (HTTPS ile alınan otomatik yapılandırma ve gömülü tablo
    /// güvenilir sayılır; onlarda hep `false`.)
    #[serde(default)]
    pub other_domain: bool,
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
    /// Üst klasörün kimliği; kökteyse (ya da üstü seçilemeyen bir düğümse) `None`.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Kenar çubuğunda gizli. Gizli özel klasör eşitlenmez; rol klasörü
    /// eşitlenmeye devam eder.
    #[serde(default)]
    pub hidden: bool,
}

impl MailboxRole {
    /// Elle bir klasöre atanabilen roller (gelen kutusu ve "diğer" dışında).
    pub fn is_special(self) -> bool {
        !matches!(self, MailboxRole::Inbox | MailboxRole::Other)
    }
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
    /// Gönderilmiş postada açık yanıt hatırlatmasının anı; yoksa `None`.
    /// Süresi geçmişse (`follow_up <= şimdi`) posta "Yanıt bekleniyor"
    /// işaretiyle gelen kutusunun üstünde görünür.
    #[serde(default)]
    pub follow_up: Option<Millis>,
    /// Kullanıcı listede en üste sabitledi. Yalnız yerelde tutulur; sunucuya
    /// yazılmaz.
    #[serde(default)]
    pub pinned: bool,
    /// Postanın etiketlerinin kimlikleri (`Tag::id`). Yalnız yereldir.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Kullanıcının yerel, renkli etiketi. Sunucuya yazılmaz; hesaplar arası
/// ortaktır.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    /// Yeni etikette boş; arka uç kimlik verir.
    pub id: String,
    /// En çok `TAG_NAME_CHARS` karakter; etiketler arasında tek.
    pub name: String,
    /// `TAG_COLORS` paletinden bir renk (#RRGGBB).
    pub color: String,
}

/// Etiket renk paleti (hesap renkleri gibi kullanıcı verisi; jeton değil).
/// Arayüzdeki `TAG_COLORS` ile birebir aynıdır.
pub const TAG_COLORS: [&str; 8] = [
    "#4C6FD8", "#2E9A61", "#D08A1E", "#9A5BD6", "#D0566E", "#2A9BA8", "#B3882A", "#6B7280",
];
/// Etiket adının üst sınırı (karakter).
pub const TAG_NAME_CHARS: usize = 40;
/// En çok etiket sayısı.
pub const TAGS_MAX: usize = 50;

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
    /// Gizli alıcılar; yalnız başlığında Bcc saklanan postalarda (taslaklar) dolu.
    #[serde(default)]
    pub bcc: Vec<Address>,
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
    /// Abonelikten çıkma yolları (`List-Unsubscribe`); ham posta ya da
    /// başlık yoksa `None`.
    #[serde(default)]
    pub unsubscribe: Option<Unsubscribe>,
    /// Postadaki takvim daveti (`text/calendar`, METHOD:REQUEST ya da
    /// CANCEL); yoksa `None`.
    #[serde(default)]
    pub invite: Option<Invite>,
    /// HTML gövdeden atılan izleyici (izleme pikseli) sayısı; uzak içeriğe
    /// izin verilmiş olsa da bunlar hiç yüklenmez.
    #[serde(default)]
    pub trackers: u32,
    /// İzleme parametreleri (`utm_*`, `fbclid` …) sökülen bağlantı sayısı.
    #[serde(default)]
    pub cleaned_links: u32,
    /// Gönderenin kimliğinden kuşku duyduran işaret; yoksa `None`.
    #[serde(default)]
    pub sender_warning: Option<SenderWarning>,
    /// Konu ya da metinde bulunan doğrulama kodu (boşluksuz); yoksa `None`.
    #[serde(default)]
    pub otp: Option<String>,
}

/// Gönderen güveni uyarısı (okuma alanında gönderen satırının altında).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SenderWarning {
    /// Görünen adın içinde başka bir alan adına ait e-posta adresi var
    /// ("Banka destek@banka.com" <x@baska.example>).
    NameHasOtherAddress,
    /// Gönderen alan adı punycode (`xn--`) ya da bir etikette Latin ile
    /// Kiril/Yunan harfleri karışık.
    LookalikeDomain,
    /// Alıcı sunucunun `Authentication-Results` başlığında DMARC başarısız
    /// ya da DKIM ve SPF birlikte başarısız.
    AuthFailed,
}

/// Takvim davetinin amacı (iTIP METHOD).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InviteMethod {
    /// Yeni davet ya da güncelleme; yanıtlanabilir.
    Request,
    /// Etkinlik iptal edildi; yalnız bilgi.
    Cancel,
}

/// Postadaki takvim davetinin okuma alanında gösterilen özeti.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    /// Etkinliğin kalıcı kimliği (UID); yanıtta aynen döner.
    pub uid: String,
    pub summary: String,
    /// Başlangıç (UTC milisaniye; tüm gün etkinlikte günün UTC 00:00'ı).
    pub start: Millis,
    /// Bitiş; tüm gün etkinlikte dışlayıcı (ertesi günün 00:00'ı). Yoksa `None`.
    pub end: Option<Millis>,
    pub all_day: bool,
    pub location: Option<String>,
    /// Düzenleyen; yanıt bu adrese gider.
    pub organizer: Option<Address>,
    /// Katılımcı sayısı.
    pub attendees: u32,
    pub method: InviteMethod,
    /// Davetin sürüm sayacı; yanıtta aynen döner.
    pub sequence: u32,
}

/// Davete verilen yanıt (iTIP PARTSTAT).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InviteResponse {
    Accepted,
    Tentative,
    Declined,
}

/// Bir kişiyle yazışmanın özeti (kişi kartı).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContactInfo {
    pub email: String,
    /// En son görülen görünen ad.
    pub name: Option<String>,
    /// Bu kişiden gelen ve ona giden toplam posta.
    pub total: u32,
    /// En son postanın tarihi.
    pub last_date: Option<Millis>,
    /// Son konuşmalar (her konuşmanın en yeni iletisi, yeniden eskiye).
    pub recent: Vec<MessageSummary>,
}

/// Postanın `List-Unsubscribe` (RFC 2369) ve `List-Unsubscribe-Post`
/// (RFC 8058) başlıklarından çıkan abonelikten çıkma yolları.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Unsubscribe {
    /// Tarayıcıda açılabilen adres (https tercih edilir, yoksa http).
    pub url: Option<String>,
    /// `mailto:` adresi (ör. `mailto:cik@ornek.com?subject=unsubscribe`).
    pub mailto: Option<String>,
    /// Tek tıkla çıkma (RFC 8058): `url` https ve gönderen
    /// `List-Unsubscribe-Post: List-Unsubscribe=One-Click` bildirmiş.
    #[serde(default)]
    pub one_click: bool,
}

/// `unsubscribe` komutunun sonucu.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UnsubscribeResult {
    /// İstek gönderildi (tek tık isteği ya da çıkma postası).
    Done,
    /// Arka uç gönderemedi; arayüz `url` adresini tarayıcıda açar.
    Browser,
}

/// `remote_images` komutunun döndürdüğü görsel: postadaki adres ve arayüzün
/// `img.src`'ye yazacağı `data:` adresi.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImage {
    pub url: String,
    pub data: String,
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
    Mailbox {
        id: String,
    },
    /// Yanıt hatırlatması kurulmuş gönderilmiş postalar (`followups`).
    Followups,
    /// Elekte bekleyen, ilk kez yazan göndericilerin postaları (`screener`).
    Screener,
    /// Bu etiketi taşıyan postalar (çöp ve istenmeyen hariç).
    Tag {
        id: String,
    },
}

/// Listede hızlı süzgeç.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ListOnly {
    /// Yalnız okunmamışlar.
    Unread,
    /// Yalnız yıldızlılar.
    Starred,
    /// Yalnız ekliler.
    Attachments,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub view: View,
    /// Yalnız bu hesap; yoksa birleşik görünüm.
    pub account_id: Option<AccountId>,
    pub category: Option<Category>,
    /// Hızlı süzgeç; yoksa tümü.
    #[serde(default)]
    pub only: Option<ListOnly>,
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
    /// Giden kutusunda bekleyen (planlanmış) gönderimler.
    pub scheduled: u32,
    pub per_account_unread: HashMap<AccountId, u32>,
    /// Açık yanıt hatırlatmaları (süresi gelen ve gelmeyen).
    #[serde(default)]
    pub follow_ups: u32,
    /// Elekte bekleyen postalar.
    #[serde(default)]
    pub screener: u32,
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
    /// İletilen posta: ekleri bu postadan kopyalanır.
    #[serde(default)]
    pub forward_from: Option<MessageId>,
    /// Bu gönderimle silinecek (ya da kaydedilirken yerine yazılacak) taslak.
    #[serde(default)]
    pub draft_id: Option<MessageId>,
    /// Yanıt gelmezse bu anda hatırlat: gönderim tamamlanınca Gönderilenler'deki
    /// kopyaya yanıt hatırlatması kurulur.
    #[serde(default)]
    pub follow_up_at: Option<Millis>,
}

/// Gönderilmiş bir postaya kurulu yanıt hatırlatması.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FollowUp {
    /// Gönderilmiş postanın yerel kimliği.
    pub id: MessageId,
    pub account_id: AccountId,
    /// Hatırlatma anı.
    pub at: Millis,
    /// Hatırlatmanın kurulduğu an.
    pub created_at: Millis,
    pub subject: String,
    /// Alıcılar (Kime).
    pub to: Vec<Address>,
}

/// Elekte bekleyen bir gönderici (hesap başına).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenedSender {
    pub account_id: AccountId,
    /// Küçük harfli adres.
    pub email: String,
    /// En son postadaki görünen ad.
    pub name: Option<String>,
    /// Elekteki posta sayısı.
    pub count: u32,
    /// En son postanın tarihi.
    pub latest: Millis,
    /// En son postanın konusu.
    pub subject: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutboxItem {
    pub id: String,
    pub send_at: Millis,
}

/// Giden kutusundaki planlanmış gönderim.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledItem {
    pub id: String,
    pub account_id: AccountId,
    pub send_at: Millis,
    /// Alıcılar (Kime, Bilgi, Gizli sırasıyla).
    pub to: Vec<Address>,
    pub subject: String,
    /// Son denemenin hatası (gönderilemediyse).
    pub error: Option<String>,
}

// ------------------------------------------------------------------- kurallar

/// Metin koşulunun karşılaştırması. Büyük/küçük harf ve Türkçe harfler
/// (ı/i, ş/s …) ayırt edilmez.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuleOp {
    Contains,
    NotContains,
    /// Tam eşitlik (gönderen ve alıcıda adresin ya da adın kendisi).
    Is,
}

/// Kural koşulu.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleCondition {
    /// Gönderenin adı ve adresi.
    From {
        op: RuleOp,
        value: String,
    },
    /// Alıcıların (Kime + Bilgi) adı ve adresi.
    To {
        op: RuleOp,
        value: String,
    },
    Subject {
        op: RuleOp,
        value: String,
    },
    /// Gövde metni (inmediyse önizleme).
    Body {
        op: RuleOp,
        value: String,
    },
    HasAttachment,
    Category {
        value: Category,
    },
}

/// Kural eylemi.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleAction {
    /// Kuralın hesabındaki bir klasöre taşır.
    Move {
        #[serde(rename = "mailboxId")]
        mailbox_id: String,
    },
    Archive,
    Trash,
    MarkRead,
    Star,
    /// Kart ve bildirim gösterilmez.
    Mute,
    /// Postayı bu adrese iletir (postayı alan hesaptan).
    Forward {
        to: String,
    },
    /// Postaya yerel etiket ekler.
    Tag {
        #[serde(rename = "tagId")]
        tag_id: String,
    },
}

/// Koşulların birleşimi.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleMatch {
    /// Tüm koşullar.
    #[default]
    All,
    /// Herhangi bir koşul.
    Any,
}

/// Gelen kutusuna yeni düşen postaya sırayla uygulanan kural.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Yeni kuralda boş; arka uç kimlik verir.
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// `None`: tüm hesaplar (klasöre taşıma için hesap zorunlu).
    pub account_id: Option<AccountId>,
    #[serde(rename = "match")]
    pub match_: RuleMatch,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    /// Eşleşirse sonraki kurallar denenmez.
    #[serde(default)]
    pub stop: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComposeMode {
    New,
    Reply,
    ReplyAll,
    Forward,
    /// Kayıtlı bir taslağı açar (`message_id` taslağın kimliği).
    Draft,
}

/// Yazma penceresini açarken verilen ön bilgi.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComposePrefill {
    pub mode: ComposeMode,
    pub message_id: Option<MessageId>,
    /// Yeni postada önceden doldurulan alıcılar (ör. kişi kartından).
    #[serde(default)]
    pub to: Vec<Address>,
    /// Yeni postanın gönderileceği hesap (ör. kişi kartının açıldığı
    /// postanın hesabı); yoksa ilk hesap.
    #[serde(default)]
    pub account_id: Option<AccountId>,
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
    /// Sunucunun sertifikası güvenilir değil; kullanıcı onayı bekleniyor.
    Untrusted {
        host: String,
        port: u16,
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

/// Okuma ekranında posta arşivlenince ya da silinince ne açılır (mobil).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AfterRemove {
    /// Listede bir alttaki (daha eski) posta açılır.
    Next,
    /// Listede bir üstteki (daha yeni) posta açılır.
    Previous,
    /// Listeye dönülür (varsayılan; bilinmeyen değer de buraya düşer).
    #[default]
    #[serde(other)]
    List,
}

/// Liste satırını yana kaydırınca yapılan iş (mobil, yön başına).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SwipeAction {
    Archive,
    Trash,
    /// Hesabın İstenmeyen (`junk`) kutusuna taşır; kutu yoksa satır yerinde kalır.
    Spam,
    /// Klasör seçici açılır; seçilen klasöre taşır.
    Move,
    Snooze,
    /// Okundu / okunmadı arasında çevirir.
    ToggleRead,
    /// Sabitler ya da sabitlemeyi kaldırır.
    Pin,
    /// Bu yönde kaydırma kapalı. Bilinmeyen değer (yeni sürümden gelen
    /// ayar) de buraya düşer: tüm ayarlar varsayılana dönmez, istenmeyen
    /// bir eylem de çalışmaz.
    #[serde(other)]
    None,
}

/// Bildirimdeki eylem düğmeleri (masaüstü köşe kartı ve Android bildirimi).
/// Kullanıcı en çok [`NOTIFY_ACTIONS_MAX`] tanesini seçer; sıra korunur.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotifyAction {
    /// Satır içi yanıt (kartta yanıt kutusu, Android'de RemoteInput).
    Reply,
    MarkRead,
    Archive,
    Trash,
    /// Hesabın İstenmeyen (`junk`) kutusuna taşır.
    Spam,
    /// Bilinmeyen değer (yeni sürümden gelen ayar); listeden atılır.
    #[serde(other)]
    Unknown,
}

/// Bildirimde en çok bu kadar eylem düğmesi (Android en çok üç gösterir).
pub const NOTIFY_ACTIONS_MAX: usize = 3;

/// Varsayılan bildirim düğmeleri: Yanıtla, Sil, İstenmeyen.
pub fn default_notify_actions() -> Vec<NotifyAction> {
    vec![NotifyAction::Reply, NotifyAction::Trash, NotifyAction::Spam]
}

/// Bildirim düğmelerini sadeleştirir: bilinmeyenler ve yinelenenler atılır
/// (ilk gelen kalır, sıra korunur), en çok [`NOTIFY_ACTIONS_MAX`] öğe. Boş
/// liste geçerlidir (düğmesiz bildirim).
pub fn normalize_notify_actions(list: Vec<NotifyAction>) -> Vec<NotifyAction> {
    let mut out: Vec<NotifyAction> = Vec::with_capacity(NOTIFY_ACTIONS_MAX);
    for a in list {
        if a != NotifyAction::Unknown && !out.contains(&a) && out.len() < NOTIFY_ACTIONS_MAX {
            out.push(a);
        }
    }
    out
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
    /// Güncellemeleri arka planda denetle.
    pub auto_update: bool,
    /// Kullanıcının onayladığı sertifikalar (sabitlenmiş SHA-256 parmak izi).
    pub trusted_certs: Vec<TrustedCert>,
    /// Hızlı yanıt düğmeleri; `None` ise dile göre varsayılanlar.
    pub quick_replies: Option<Vec<String>>,
    /// Koyu temada kendi zemini olmayan postalar koyu zeminde gösterilir.
    pub dark_mail: bool,
    /// Yazma penceresinde eklenebilen hazır metinler (en çok 50).
    pub templates: Vec<Template>,
    /// Okuma alanında konuşmanın tüm iletileri alt alta gösterilir.
    pub conversation_view: bool,
    /// Yeni gönderici eleği: daha önce yazışılmamış göndericinin postası
    /// gelen kutusu yerine elekte bekler. Varsayılan kapalı.
    pub screener: bool,
    /// Kenar çubuğundan tek tıkla çalıştırılan aramalar (en çok
    /// `SAVED_SEARCHES_MAX`).
    pub saved_searches: Vec<SavedSearch>,
    /// Mobil okuma: arşivleyip silince açılacak yer.
    pub after_remove: AfterRemove,
    /// Mobil liste: sola kaydırma (varsayılan arşivle).
    pub swipe_left: SwipeAction,
    /// Mobil liste: sağa kaydırma (varsayılan yarına ertele).
    pub swipe_right: SwipeAction,
    /// Bildirim düğmeleri (köşe kartı ve Android bildirimi), en çok üç.
    pub notify_actions: Vec<NotifyAction>,
}

/// Kayıtlı arama: ad ve arama sorgusu (işleçler dahil).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedSearch {
    /// Yeni kayıtta boş; arka uç kimlik verir.
    pub id: String,
    /// En çok `SAVED_SEARCH_NAME_CHARS` karakter.
    pub name: String,
    /// En çok `SAVED_SEARCH_QUERY_CHARS` karakter.
    pub query: String,
}

/// Kayıtlı arama sınırları (arayüzle aynı).
pub const SAVED_SEARCHES_MAX: usize = 30;
pub const SAVED_SEARCH_NAME_CHARS: usize = 60;
pub const SAVED_SEARCH_QUERY_CHARS: usize = 300;

/// Hazır posta şablonu.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Template {
    pub id: String,
    pub name: String,
    /// Boşsa konu değiştirilmez.
    pub subject: String,
    pub body: String,
}

/// Sunucu tarafı otomatik yanıt (JMAP `VacationResponse`, RFC 8621 §8).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Vacation {
    pub enabled: bool,
    /// Bu andan önce yanıt verilmez; yoksa hemen başlar.
    pub from_date: Option<Millis>,
    /// Bu andan sonra yanıt verilmez; yoksa süresiz.
    pub to_date: Option<Millis>,
    pub subject: String,
    /// Düz metin yanıt.
    pub body: String,
}

/// Hızlı yanıt sınırları: en çok bu kadar düğme, her biri en çok bu kadar karakter.
pub const QUICK_REPLIES_MAX: usize = 8;
pub const QUICK_REPLY_CHARS: usize = 120;

/// Kullanıcının elle güvendiği sunucu sertifikası.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedCert {
    pub host: String,
    pub port: u16,
    /// Sertifikanın DER kodlu halinin SHA-256 özeti, büyük harf onaltılık, `:` ayraçlı.
    pub sha256: String,
}

/// Sunucu sertifikasının incelemesi (onay ekranı için).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CertInfo {
    pub host: String,
    pub port: u16,
    pub sha256: String,
    pub subject: String,
    pub issuer: String,
    pub not_before: Option<Millis>,
    pub not_after: Option<Millis>,
    /// Sertifikanın kapsadığı adlar (SAN).
    pub names: Vec<String>,
    /// Neden güvenilmedi (Türkçe, kısa): "kendinden imzalı", "süresi dolmuş", "ad uyuşmuyor"…
    pub problem: String,
}

/// Yayımlanmış yeni sürüm.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

// --------------------------------------------------------- parola kilidi

/// Parola kilidinin durumu (`lock_status`; `mh:lock` olayı aynı yapıyı taşır).
///
/// - `enabled`: veritabanı anahtarı parolayla sarılı (kilit açık).
/// - `locked`: çekirdek (veritabanı, eşitleyiciler) kapalı; parola girilene
///   kadar `lock_*` dışındaki komutlar `locked` hatası döner.
/// - `ui_locked`: çekirdek çalışıyor, yalnız arayüz kilitli (pencere yeniden
///   açıldı / uygulama arka plandan döndü); arka planda bildirim sürer.
/// - `relock_on_open`: "pencereyi her açışta parola sor" tercihi.
/// - `retry_after_ms`: yanlış parolalardan sonra bekleme; 0 ise denenebilir.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LockStatus {
    pub enabled: bool,
    pub locked: bool,
    pub ui_locked: bool,
    pub relock_on_open: bool,
    pub retry_after_ms: u64,
}

/// Parolanın en az uzunluğu (karakter); arayüz de aynı sınırı uygular.
pub const LOCK_PASSWORD_MIN: usize = 8;
/// Parolanın en çok uzunluğu (bayt).
pub const LOCK_PASSWORD_MAX: usize = 256;

// ------------------------------------------------------- aktarım (QR ile)

/// QR kodun modül matrisi. `rows` tam `size` satırdır; her satır `size`
/// karakterlik bir `'1'`/`'0'` dizgisidir (`'1'` koyu modül). Sessiz bölge
/// (kenar boşluğu) dahil değildir; arayüz kenar boşluğunu kendisi bırakır
/// ve her `'1'` için bir `rect` çizer. Biçim bilerek yalın tutuldu: 57×57'lik
/// bir kod JSON'da ~3,5 KB'dir ve arayüzde çözme adımı gerektirmez.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct QrMatrix {
    pub size: u32,
    pub rows: Vec<String>,
}

/// Bilgisayarın açtığı aktarım teklifi (`transfer_begin` dönüşü). `text`
/// kodun tamamıdır ve şifreleme anahtarını içerir: yalnız QR ve "kodu
/// kopyala" için kullanılır, günlüğe yazılmaz; `Debug` çıktısında gizlenir.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferOffer {
    /// `mailhere://aktar?v=1&h=…&p=…&t=…&k=…` (QR'ın içeriği).
    pub text: String,
    pub qr: QrMatrix,
    /// Kodda duyurulan yerel adresler (bilgi amaçlı; en çok 6).
    pub hosts: Vec<String>,
    pub port: u16,
    /// Teklifin düştüğü an (geri sayım için).
    pub expires_at: Millis,
}

impl std::fmt::Debug for TransferOffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransferOffer")
            .field("text", &"<gizli>")
            .field("qr", &format_args!("QrMatrix({})", self.qr.size))
            .field("hosts", &self.hosts)
            .field("port", &self.port)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Telefonun aldığı paketin özeti (`transfer_receive` dönüşü); uygulanmadan
/// önce kullanıcı seçer. Gizli değerler burada yoktur, yalnız var/yok bilgisi.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferPreview {
    /// Bellekte bekleyen paketin kimliği (`transfer_apply` ile verilir; 5 dk).
    pub preview_id: String,
    /// Paketi hazırlayan Mailhere sürümü ("0.4.0").
    pub from_version: String,
    /// Paketi hazırlayan platform ("windows", "linux", "android").
    pub from_platform: String,
    /// Paketin hazırlandığı an.
    pub created_at: Millis,
    pub accounts: Vec<TransferPreviewAccount>,
    /// Paketteki kural sayısı.
    pub rules: u32,
    /// Paketteki etiket sayısı.
    pub tags: u32,
    /// Pakette ayarlar var mı.
    pub settings: bool,
}

/// Önizlemedeki bir hesap.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferPreviewAccount {
    pub id: AccountId,
    pub name: String,
    pub email: String,
    pub kind: AccountKind,
    pub auth: AuthKind,
    /// Parola ya da yenileme jetonu pakette var.
    pub has_secret: bool,
    /// Aynı kimlikli hesap bu cihazda zaten var (uygulanırsa güncellenir).
    pub exists: bool,
}

/// Kullanıcının uygulama seçimi (`transfer_apply` girdisi).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferChoice {
    /// Uygulanacak hesapların kimlikleri (önizlemedeki `id`).
    pub accounts: Vec<AccountId>,
    pub rules: bool,
    pub tags: bool,
    pub settings: bool,
}

/// Uygulama sonucu (`transfer_apply` dönüşü).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferReport {
    pub accounts_added: u32,
    pub accounts_updated: u32,
    /// Gizli değeri olmayan ya da yazılamayan hesapların e-posta adresleri;
    /// bunlar için parola yeniden istenir.
    pub accounts_without_secret: Vec<String>,
    pub rules: u32,
    /// Hesabı bu cihazda olmadığı için atlanan kurallar.
    pub rules_skipped: u32,
    pub tags: u32,
    pub settings: bool,
}

/// Aktarım teklifinin durumu (`mh:transfer` olayı; bilgisayar tarafı).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferStateKind {
    /// QR gösteriliyor, telefon bekleniyor.
    #[default]
    Waiting,
    /// Telefon bağlandı ve kimliğini doğruladı; paket gönderiliyor.
    Connected,
    /// Paket telefona ulaştı; telefonda uygulanması bekleniyor.
    Delivered,
    /// Telefon uyguladığını bildirdi (bitiş).
    Applied,
    /// Hata: çok sayıda hatalı deneme, gönderim hatası ya da telefon
    /// uygulayamadı (bitiş).
    Failed,
    /// Süre doldu: telefon bağlanmadı ya da onay gelmedi (bitiş).
    Expired,
    /// Kullanıcı vazgeçti (bitiş).
    Cancelled,
}

impl TransferStateKind {
    /// Bu durumdan sonra teklif kapanmıştır.
    pub fn is_final(self) -> bool {
        matches!(
            self,
            Self::Applied | Self::Failed | Self::Expired | Self::Cancelled
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferState {
    pub state: TransferStateKind,
    /// Kullanıcıya gösterilebilir Türkçe açıklama (gizli değer içermez).
    pub message: Option<String>,
}

impl TransferState {
    pub fn new(state: TransferStateKind) -> Self {
        Self {
            state,
            message: None,
        }
    }

    pub fn with_message(state: TransferStateKind, message: impl Into<String>) -> Self {
        Self {
            state,
            message: Some(message.into()),
        }
    }
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
            auto_update: true,
            trusted_certs: Vec::new(),
            quick_replies: None,
            dark_mail: true,
            templates: Vec::new(),
            conversation_view: true,
            screener: false,
            saved_searches: Vec::new(),
            after_remove: AfterRemove::List,
            swipe_left: SwipeAction::Archive,
            swipe_right: SwipeAction::Snooze,
            notify_actions: default_notify_actions(),
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
    /// `UpdateInfo`: yeni sürüm bulundu.
    pub const UPDATE: &str = "mh:update";
    /// `TransferState`: QR aktarım teklifinin durumu değişti.
    pub const TRANSFER: &str = "mh:transfer";
    /// `NavigateEvent`: tek pencereli kabuk (Android) bu rotayı yığına itmeli
    /// (`index.html#/<rota>`; ör. yazma: `compose/reply/7`).
    pub const NAVIGATE: &str = "mh:navigate";
    /// `LockStatus`: parola kilidinin durumu değişti (kilitlendi, açıldı,
    /// ayar değişti); tüm pencereler kilit ekranını buna göre gösterir.
    pub const LOCK: &str = "mh:lock";
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotifyEvent {
    pub message: MessageSummary,
    pub account_name: String,
    pub account_color: String,
    /// Postadaki doğrulama kodu ("Kodu kopyala" düğmesi); yoksa `None`.
    #[serde(default)]
    pub otp: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenEvent {
    pub id: MessageId,
}

/// Tek pencereli kabukta (Android) ayrı pencere yerine rota değişimi
/// (`Platform::open_compose`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NavigateEvent {
    /// `index.html#/<route>` biçimindeki rota.
    pub route: String,
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
    /// Sertifika güvenilir değil; `cert_inspect` ile incelenip onaylanabilir.
    Untrusted,
    Discovery,
    Keyring,
    Storage,
    Invalid,
    Internal,
    /// Parola kilidi kapalı değil: çekirdek parola girilene kadar açılmaz ya
    /// da arayüz kilitli; `lock_unlock` ile açılır.
    Locked,
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
    fn hesap_isteklerinde_parola_gizlenir() {
        let server = ServerConfig {
            host: "imap.ornek.com.tr".into(),
            port: 993,
            security: Security::Tls,
            username: "furkan".into(),
        };
        let a = NewAccount {
            name: "İş".into(),
            email: "furkan@ornek.com.tr".into(),
            display_name: "Furkan".into(),
            color: None,
            imap: server.clone(),
            smtp: server,
            password: "cokgizliparola".into(),
        };
        let j = NewJmapAccount {
            name: "İş".into(),
            email: "furkan@ornek.com.tr".into(),
            display_name: "Furkan".into(),
            color: None,
            session_url: "https://posta.ornek.com.tr/.well-known/jmap".into(),
            username: "furkan".into(),
            password: "cokgizliparola".into(),
        };
        for shown in [format!("{a:?}"), format!("{j:?}"), format!("{a:#?}")] {
            assert!(!shown.contains("cokgizliparola"), "{shown}");
            assert!(shown.contains("furkan@ornek.com.tr"));
            assert!(shown.contains("***"));
        }
    }

    #[test]
    fn view_json_bicimi() {
        let v = serde_json::to_value(View::Mailbox {
            id: "a:INBOX".into(),
        })
        .unwrap();
        assert_eq!(v, serde_json::json!({"kind": "mailbox", "id": "a:INBOX"}));
        let v = serde_json::to_value(View::Inbox).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "inbox"}));
        let v = serde_json::to_value(View::Tag { id: "e1".into() }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "tag", "id": "e1"}));
    }

    #[test]
    fn etiket_ve_kayitli_arama_json_bicimi() {
        let tag = Tag {
            id: "e1".into(),
            name: "Fatura".into(),
            color: TAG_COLORS[0].into(),
        };
        assert_eq!(
            serde_json::to_value(&tag).unwrap(),
            serde_json::json!({"id": "e1", "name": "Fatura", "color": "#4C6FD8"})
        );
        let s: Settings = serde_json::from_str(
            r#"{"savedSearches":[{"id":"k1","name":"Elif'ten ekli","query":"from:elif has:ek"}]}"#,
        )
        .unwrap();
        assert_eq!(s.saved_searches.len(), 1);
        assert_eq!(s.saved_searches[0].query, "from:elif has:ek");
        assert!(Settings::default().saved_searches.is_empty());
        assert_eq!(
            serde_json::to_value(Settings::default()).unwrap()["savedSearches"],
            serde_json::json!([])
        );
        for c in TAG_COLORS {
            assert!(c.len() == 7 && c.starts_with('#'), "{c}");
        }
    }

    #[test]
    fn kesif_baska_alan_adi_alani() {
        let server = |host: &str| ServerConfig {
            host: host.into(),
            port: 993,
            security: Security::Tls,
            username: "a@ornek.com".into(),
        };
        let c = DiscoveredConfig {
            domain: "ornek.com".into(),
            imap: server("imap.baska.net"),
            smtp: server("smtp.baska.net"),
            source: DiscoverySource::Srv,
            other_domain: true,
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["otherDomain"], serde_json::json!(true));
        // Alanı bilmeyen eski veri `false` okunur.
        let mut old = v.clone();
        old.as_object_mut().unwrap().remove("otherDomain");
        let back: DiscoveredConfig = serde_json::from_value(old).unwrap();
        assert!(!back.other_domain);
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
            follow_up: None,
            pinned: false,
            tags: Vec::new(),
        };
        let d = MessageDetail {
            summary: s,
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            message_id_header: None,
            references: vec![],
            body_text: Some("x".into()),
            body_html: None,
            has_remote_content: false,
            remote_allowed: false,
            attachments: vec![],
            thread: vec![],
            unsubscribe: Some(Unsubscribe {
                url: Some("https://bulten.ornek.com/cik?u=1".into()),
                mailto: None,
                one_click: true,
            }),
            invite: Some(Invite {
                uid: "etkinlik-1@ornek.com".into(),
                summary: "Sprint planlama".into(),
                start: 1_000,
                end: Some(4_600_000),
                all_day: false,
                location: Some("Toplantı odası 2".into()),
                organizer: Some(Address::new(Some("Kerem"), "kerem@ornek.com")),
                attendees: 3,
                method: InviteMethod::Request,
                sequence: 2,
            }),
            trackers: 3,
            cleaned_links: 2,
            sender_warning: Some(SenderWarning::NameHasOtherAddress),
            otp: Some("482913".into()),
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["trackers"], 3);
        assert_eq!(v["cleanedLinks"], 2);
        assert_eq!(v["senderWarning"], "nameHasOtherAddress");
        assert_eq!(v["otp"], "482913");
        assert_eq!(
            serde_json::to_value(SenderWarning::LookalikeDomain).unwrap(),
            "lookalikeDomain"
        );
        assert_eq!(
            serde_json::to_value(SenderWarning::AuthFailed).unwrap(),
            "authFailed"
        );
        assert_eq!(v["subject"], "Merhaba");
        assert_eq!(v["hasRemoteContent"], false);
        assert_eq!(v["category"], "people");
        assert_eq!(v["pinned"], false);
        assert_eq!(v["tags"], serde_json::json!([]));
        assert_eq!(
            v["unsubscribe"],
            serde_json::json!({"url": "https://bulten.ornek.com/cik?u=1", "mailto": null, "oneClick": true})
        );
        assert_eq!(v["invite"]["method"], "request");
        assert_eq!(v["invite"]["allDay"], false);
        assert_eq!(v["invite"]["organizer"]["email"], "kerem@ornek.com");
        // Alanı bilmeyen eski veri `None` okunur; sabitleme ve etiket boş.
        let mut old = v.clone();
        for k in [
            "unsubscribe",
            "invite",
            "pinned",
            "tags",
            "trackers",
            "cleanedLinks",
            "senderWarning",
            "otp",
        ] {
            old.as_object_mut().unwrap().remove(k);
        }
        let back: MessageDetail = serde_json::from_value(old).unwrap();
        assert_eq!(back.unsubscribe, None);
        assert_eq!(back.invite, None);
        assert_eq!((back.trackers, back.cleaned_links), (0, 0));
        assert_eq!((back.sender_warning, back.otp), (None, None));
        assert!(!back.summary.pinned);
        assert!(back.summary.tags.is_empty());
        assert_eq!(
            serde_json::to_value(InviteResponse::Tentative).unwrap(),
            "tentative"
        );
        assert_eq!(
            serde_json::to_value(UnsubscribeResult::Browser).unwrap(),
            "browser"
        );
        assert_eq!(
            serde_json::to_value(UnsubscribeResult::Done).unwrap(),
            "done"
        );
    }

    #[test]
    fn imza_ve_sablon_json_bicimi() {
        let a = Account {
            id: "a".into(),
            name: "İş".into(),
            email: "a@ornek.com".into(),
            display_name: "A".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: None,
            smtp: None,
            jmap_url: None,
            signatures: vec![Signature {
                id: "s1".into(),
                name: "Kısa".into(),
                body: "-- \nA".into(),
            }],
            signature_new: Some("s1".into()),
            signature_reply: None,
            created_at: 0,
            auth: AuthKind::Password,
            notify: true,
        };
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(
            v["signatures"],
            serde_json::json!([{"id": "s1", "name": "Kısa", "body": "-- \nA"}])
        );
        assert_eq!(v["signatureNew"], "s1");
        assert_eq!(v["signatureReply"], serde_json::Value::Null);
        assert!(v.get("signature").is_none());
        // İmza alanlarını bilmeyen veri boş liste okunur.
        let mut old = v.clone();
        for k in ["signatures", "signatureNew", "signatureReply"] {
            old.as_object_mut().unwrap().remove(k);
        }
        let back: Account = serde_json::from_value(old).unwrap();
        assert!(back.signatures.is_empty());
        assert_eq!(back.signature_new, None);

        let s: Settings = serde_json::from_str(
            r#"{"templates":[{"id":"t1","name":"Teşekkür","subject":"","body":"Teşekkürler!"}]}"#,
        )
        .unwrap();
        assert_eq!(s.templates.len(), 1);
        assert_eq!(s.templates[0].name, "Teşekkür");
        assert!(Settings::default().templates.is_empty());
    }

    #[test]
    fn otomatik_yanit_json_bicimi() {
        let vac = Vacation {
            enabled: true,
            from_date: Some(1_000),
            to_date: None,
            subject: "İzindeyim".into(),
            body: "Pazartesi dönüyorum.".into(),
        };
        assert_eq!(
            serde_json::to_value(&vac).unwrap(),
            serde_json::json!({"enabled": true, "fromDate": 1000, "toDate": null, "subject": "İzindeyim", "body": "Pazartesi dönüyorum."})
        );
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
        assert!(s.conversation_view, "eski ayarda konuşma görünümü açık");
        assert_eq!(
            serde_json::to_value(&s).unwrap()["conversationView"],
            serde_json::json!(true)
        );
        // Kişi kartından açılan yazma penceresi alıcı taşır; eski istek boş liste.
        let p: ComposePrefill =
            serde_json::from_value(serde_json::json!({"mode": "new", "messageId": null})).unwrap();
        assert!(p.to.is_empty());
        let p: ComposePrefill = serde_json::from_value(serde_json::json!({
            "mode": "new", "messageId": null, "to": [{"name": "Ayşe", "email": "ayse@ornek.com"}]
        }))
        .unwrap();
        assert_eq!(p.to, vec![Address::new(Some("Ayşe"), "ayse@ornek.com")]);
        let c = ContactInfo {
            email: "ayse@ornek.com".into(),
            name: Some("Ayşe".into()),
            total: 4,
            last_date: Some(9),
            recent: vec![],
        };
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::json!({"email": "ayse@ornek.com", "name": "Ayşe", "total": 4, "lastDate": 9, "recent": []})
        );
        assert_eq!(
            serde_json::to_value(CardPosition::BottomRight).unwrap(),
            "bottomRight"
        );
    }

    #[test]
    fn kural_json_bicimi() {
        let json = serde_json::json!({
            "id": "k1",
            "name": "Faturalar",
            "enabled": true,
            "accountId": "a",
            "match": "any",
            "conditions": [
                {"kind": "from", "op": "contains", "value": "fatura"},
                {"kind": "subject", "op": "notContains", "value": "reklam"},
                {"kind": "hasAttachment"},
                {"kind": "category", "value": "notifications"}
            ],
            "actions": [
                {"kind": "move", "mailboxId": "a:Faturalar"},
                {"kind": "archive"},
                {"kind": "trash"},
                {"kind": "markRead"},
                {"kind": "star"},
                {"kind": "mute"},
                {"kind": "forward", "to": "muhasebe@ornek.com"},
                {"kind": "tag", "tagId": "e1"}
            ],
            "stop": true
        });
        let rule: Rule = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(rule.match_, RuleMatch::Any);
        assert_eq!(
            rule.actions[0],
            RuleAction::Move {
                mailbox_id: "a:Faturalar".into()
            }
        );
        assert_eq!(
            rule.conditions[1],
            RuleCondition::Subject {
                op: RuleOp::NotContains,
                value: "reklam".into()
            }
        );
        assert_eq!(
            rule.actions[7],
            RuleAction::Tag {
                tag_id: "e1".into()
            }
        );
        assert_eq!(serde_json::to_value(&rule).unwrap(), json);
        // Tüm hesaplar: `accountId` null.
        let mut all = rule.clone();
        all.account_id = None;
        assert_eq!(
            serde_json::to_value(&all).unwrap()["accountId"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn planli_gonderim_ve_liste_suzgeci() {
        let item = ScheduledItem {
            id: "o1".into(),
            account_id: "a".into(),
            send_at: 5,
            to: vec![Address::new(None, "ayse@ornek.com")],
            subject: "Selam".into(),
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            serde_json::json!({
                "id": "o1", "accountId": "a", "sendAt": 5,
                "to": [{"name": null, "email": "ayse@ornek.com"}],
                "subject": "Selam", "error": null
            })
        );
        // `only` alanı olmayan eski istek de okunur.
        let q: ListQuery = serde_json::from_value(serde_json::json!({
            "view": {"kind": "inbox"}, "accountId": null, "category": null,
            "cursor": null, "limit": 50
        }))
        .unwrap();
        assert_eq!(q.only, None);
        let q: ListQuery = serde_json::from_value(serde_json::json!({
            "view": {"kind": "inbox"}, "accountId": null, "category": null,
            "only": "attachments", "cursor": null, "limit": 50
        }))
        .unwrap();
        assert_eq!(q.only, Some(ListOnly::Attachments));
        assert_eq!(
            serde_json::to_value(Counts::default()).unwrap()["scheduled"],
            0
        );
    }

    #[test]
    fn yanit_hatirlatmasi_ve_elek_json_bicimi() {
        // Yeni görünümler.
        assert_eq!(
            serde_json::to_value(View::Followups).unwrap(),
            serde_json::json!({"kind": "followups"})
        );
        assert_eq!(
            serde_json::to_value(View::Screener).unwrap(),
            serde_json::json!({"kind": "screener"})
        );
        // Özette hatırlatma anı; alanı bilmeyen eski veri `None` okunur.
        let v = serde_json::json!({
            "id": 1, "accountId": "a", "mailboxId": "a:Sent", "threadId": "t",
            "from": {"name": null, "email": "a@ornek.com"}, "to": [], "subject": "x", "snippet": "",
            "date": 0, "unread": false, "starred": false, "hasAttachments": false,
            "category": "people", "snoozedUntil": null
        });
        let s: MessageSummary = serde_json::from_value(v).unwrap();
        assert_eq!(s.follow_up, None);
        assert_eq!(
            serde_json::to_value(&s).unwrap()["followUp"],
            serde_json::Value::Null
        );
        // Giden postada hatırlatma anı isteğe bağlı.
        let m: OutgoingMessage = serde_json::from_value(serde_json::json!({
            "accountId": "a", "to": [], "cc": [], "bcc": [], "subject": "", "bodyText": "",
            "inReplyTo": null, "attachments": [], "sendAt": null
        }))
        .unwrap();
        assert_eq!(m.follow_up_at, None);
        let f = FollowUp {
            id: 7,
            account_id: "a".into(),
            at: 10,
            created_at: 5,
            subject: "Teklif".into(),
            to: vec![Address::new(Some("Ayşe"), "ayse@ornek.com")],
        };
        assert_eq!(
            serde_json::to_value(&f).unwrap(),
            serde_json::json!({
                "id": 7, "accountId": "a", "at": 10, "createdAt": 5, "subject": "Teklif",
                "to": [{"name": "Ayşe", "email": "ayse@ornek.com"}]
            })
        );
        let s = ScreenedSender {
            account_id: "a".into(),
            email: "yeni@ornek.com".into(),
            name: None,
            count: 2,
            latest: 99,
            subject: "Merhaba".into(),
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({
                "accountId": "a", "email": "yeni@ornek.com", "name": null, "count": 2,
                "latest": 99, "subject": "Merhaba"
            })
        );
        // Sayaçlar ve ayar varsayılanı.
        let c = serde_json::to_value(Counts::default()).unwrap();
        assert_eq!(
            (c["followUps"].as_u64(), c["screener"].as_u64()),
            (Some(0), Some(0))
        );
        assert!(!Settings::default().screener);
        let s: Settings = serde_json::from_str(r#"{"screener":true}"#).unwrap();
        assert!(s.screener);
    }

    #[test]
    fn bildirim_dugmeleri() {
        // Eski kayıtta alan yok: Yanıtla, Sil, İstenmeyen.
        let s: Settings = serde_json::from_str(r#"{"screener":true}"#).unwrap();
        assert_eq!(s.notify_actions, default_notify_actions());
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            v["notifyActions"],
            serde_json::json!(["reply", "trash", "spam"])
        );
        // Bilinmeyen değer ayarı düşürmez; sadeleştirmede atılır.
        let s: Settings = serde_json::from_str(
            r#"{"notifyActions":["markRead","snoozeHour","archive"],"screener":true}"#,
        )
        .unwrap();
        assert!(s.screener);
        assert_eq!(
            normalize_notify_actions(s.notify_actions),
            vec![NotifyAction::MarkRead, NotifyAction::Archive]
        );
        // Yinelenen atılır, sıra korunur, en çok üç; boş liste geçerli.
        assert_eq!(
            normalize_notify_actions(vec![
                NotifyAction::Spam,
                NotifyAction::Reply,
                NotifyAction::Spam,
                NotifyAction::Trash,
                NotifyAction::Archive,
            ]),
            vec![NotifyAction::Spam, NotifyAction::Reply, NotifyAction::Trash]
        );
        assert!(normalize_notify_actions(vec![]).is_empty());
    }

    #[test]
    fn mobil_kaydirma_ve_okuma_ayarlari() {
        // Eski kayıtta alan yok: bugünkü davranış (listeye dön, sola arşivle, sağa ertele).
        let s: Settings = serde_json::from_str(r#"{"screener":true}"#).unwrap();
        assert_eq!(s.after_remove, AfterRemove::List);
        assert_eq!(
            (s.swipe_left, s.swipe_right),
            (SwipeAction::Archive, SwipeAction::Snooze)
        );
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            (&v["afterRemove"], &v["swipeLeft"], &v["swipeRight"]),
            (
                &serde_json::json!("list"),
                &serde_json::json!("archive"),
                &serde_json::json!("snooze")
            )
        );
        let s: Settings = serde_json::from_str(
            r#"{"afterRemove":"previous","swipeLeft":"toggleRead","swipeRight":"none"}"#,
        )
        .unwrap();
        assert_eq!(s.after_remove, AfterRemove::Previous);
        assert_eq!(
            (s.swipe_left, s.swipe_right),
            (SwipeAction::ToggleRead, SwipeAction::None)
        );
        for (a, name) in [
            (SwipeAction::Trash, "trash"),
            (SwipeAction::Spam, "spam"),
            (SwipeAction::Move, "move"),
            (SwipeAction::Pin, "pin"),
            (SwipeAction::Snooze, "snooze"),
        ] {
            assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(name));
        }
        assert_eq!(
            serde_json::to_value(AfterRemove::Next).unwrap(),
            serde_json::json!("next")
        );
        // Bilinmeyen değer (yeni sürümün ayarı) tüm ayarları düşürmez: kaydırma
        // kapanır, okuma listeye döner; diğer alanlar korunur.
        let s: Settings =
            serde_json::from_str(r#"{"swipeLeft":"fly","afterRemove":"jump","screener":true}"#)
                .unwrap();
        assert_eq!(s.swipe_left, SwipeAction::None);
        assert_eq!(s.after_remove, AfterRemove::List);
        assert!(s.screener);
    }

    #[test]
    fn adres_etiketi() {
        assert_eq!(Address::new(None, "kerem@ornek.com").label(), "kerem");
        assert_eq!(
            Address::new(Some("Kerem Aydın"), "k@o.com").label(),
            "Kerem Aydın"
        );
    }

    #[test]
    fn aktarim_json_bicimi() {
        // Teklif: camelCase alanlar, QR matrisi düz satırlar.
        let offer = TransferOffer {
            text: "mailhere://aktar?v=1&h=192.168.1.20&p=51234&t=AA&k=BB".into(),
            qr: QrMatrix {
                size: 2,
                rows: vec!["10".into(), "01".into()],
            },
            hosts: vec!["192.168.1.20".into()],
            port: 51234,
            expires_at: 1_700_000_000_000,
        };
        let v = serde_json::to_value(&offer).unwrap();
        assert_eq!(v["qr"]["size"], 2);
        assert_eq!(v["qr"]["rows"][1], "01");
        assert_eq!(v["expiresAt"], 1_700_000_000_000i64);
        assert_eq!(v["hosts"][0], "192.168.1.20");
        let back: TransferOffer = serde_json::from_value(v).unwrap();
        assert_eq!(back, offer);
        // Debug kodu (anahtarı) göstermez.
        let dbg = format!("{offer:?}");
        assert!(!dbg.contains("k=BB") && dbg.contains("<gizli>"), "{dbg}");

        // Önizleme ve seçim: eksik alanlar varsayılana düşer (ileri uyumluluk).
        let p: TransferPreview = serde_json::from_str(
            r#"{"previewId":"p1","fromVersion":"0.4.0","accounts":[{"id":"a","name":"İş","email":"a@o.com","kind":"jmap","auth":"google","hasSecret":true,"exists":false}],"rules":3,"yeniAlan":1}"#,
        )
        .unwrap();
        assert_eq!(p.preview_id, "p1");
        assert_eq!(p.accounts[0].kind, AccountKind::Jmap);
        assert_eq!(p.accounts[0].auth, AuthKind::Google);
        assert!(p.accounts[0].has_secret && !p.accounts[0].exists);
        assert_eq!((p.rules, p.tags, p.settings), (3, 0, false));
        assert_eq!(p.from_platform, "");
        let c: TransferChoice =
            serde_json::from_str(r#"{"accounts":["a"],"settings":true}"#).unwrap();
        assert_eq!(c.accounts, vec!["a".to_string()]);
        assert!(c.settings && !c.rules && !c.tags);
        let v = serde_json::to_value(TransferPreviewAccount::default()).unwrap();
        assert_eq!(v["kind"], "imap");
        assert_eq!(v["hasSecret"], false);

        // Rapor.
        let r = TransferReport {
            accounts_added: 1,
            accounts_without_secret: vec!["b@o.com".into()],
            rules_skipped: 2,
            ..Default::default()
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["accountsAdded"], 1);
        assert_eq!(v["accountsWithoutSecret"][0], "b@o.com");
        assert_eq!(v["rulesSkipped"], 2);
        assert_eq!(v["settings"], false);

        // Durum olayı: `state` küçük harf; `message` isteğe bağlı.
        let s = TransferState::with_message(TransferStateKind::Failed, "Beş hatalı deneme.");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"state":"failed","message":"Beş hatalı deneme."})
        );
        let s: TransferState = serde_json::from_str(r#"{"state":"waiting"}"#).unwrap();
        assert_eq!(s, TransferState::new(TransferStateKind::Waiting));
        for (kind, name) in [
            (TransferStateKind::Waiting, "waiting"),
            (TransferStateKind::Connected, "connected"),
            (TransferStateKind::Delivered, "delivered"),
            (TransferStateKind::Applied, "applied"),
            (TransferStateKind::Failed, "failed"),
            (TransferStateKind::Expired, "expired"),
            (TransferStateKind::Cancelled, "cancelled"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
        }
        assert!(TransferStateKind::Applied.is_final() && !TransferStateKind::Delivered.is_final());
        assert_eq!(events::TRANSFER, "mh:transfer");
    }
}
