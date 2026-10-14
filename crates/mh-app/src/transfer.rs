//! Bilgisayardan telefona tek QR kodla ayar aktarımının uygulama katmanı:
//! paketin kurulması ([`build_package`]), önizlenmesi ([`preview`]) ve
//! uygulanması ([`apply_package`]); dört komut (`transfer_begin`,
//! `transfer_cancel`, `transfer_receive`, `transfer_apply`) ve `mh:transfer`
//! olayı. Kripto ve ağ `mh-transfer`'dadır; bu modül paketin içeriğini bilir.
//!
//! # Paket (JSON, v1)
//! `{ v, app, appVersion, minReceiver, createdAt, fromPlatform, settings,
//! accounts: [{ account, secret: { kind, value } | null, jmapUsername }],
//! rules, tags }`. Aktarılmayan: postalar, taslaklar, giden kutusu,
//! hatırlatmalar, elek durumu, sabitleme ve posta etiketleri, klasör
//! gizleme. Cihaza özgü kalan ayarlar: kart konumu ve süresi, yerel bildirim,
//! otomatik güncelleme ve onaylı sertifikalar ([`merge_settings`]).
//!
//! # Uygulama sırası ([`apply_package`])
//! ayarlar → etiketler (ad çakışırsa yereldekine eşlenir, kuralların etiket
//! kimlikleri yeniden yazılır) → hesaplar (kimlik korunur; varsa güncellenir,
//! yoksa eklenir; gizli değer anahtar zincirine; parolalı hesapta 8 s'lik
//! giriş denemesi, başarısızlık engellemez) → kurallar (hesabı bu cihazda
//! olmayan atlanır). Sonuç [`TransferReport`] ve bilgisayara giden onay
//! durumu ([`ApplyStatus`]).
//!
//! # Güvenlik
//! Gizli değerler (`TransferSecret::value`) `Zeroizing` ile tutulur ve
//! `Debug` çıktısında gizlenir; paket diske yazılmaz. Alınan paket en çok
//! 5 dakika bellekte bekler ([`PREVIEW_TTL`]); süresi dolan ya da uygulanan
//! paket düşürülür. Onaylı sertifikalar aktarılmaz: sertifika sabitleme
//! cihaz başına, inceleme sonrası verilen bir karardır.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mh_core::{
    events, Account, AccountKind, AppError, AppResult, AuthKind, ErrorCode, Millis, Rule,
    RuleAction, Settings, Tag, TransferChoice, TransferOffer, TransferPreview,
    TransferPreviewAccount, TransferReport, TAGS_MAX, TAG_COLORS,
};
use mh_store::{fold_loose, normalize_tag_name, Store, MAX_RULES};
use mh_sync::SecretStore;
use mh_transfer::{ApplyStatus, ClientOptions, Code, Offer, OfferHandle, OfferOptions};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroizing;

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Paket biçiminin sürümü.
pub const PACKAGE_VERSION: u32 = 1;
/// Paketin `app` alanı.
pub const APP_NAME: &str = "mailhere";
/// Pakette en çok bu kadar hesap.
pub const MAX_ACCOUNTS: usize = 20;
/// Alınan paketin uygulanmadan bellekte bekleyebileceği süre.
pub const PREVIEW_TTL: Duration = Duration::from_secs(5 * 60);
/// Hesap uygulanırken giriş denemesinin süresi.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(8);

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Invalid, message)
}

// ------------------------------------------------------------------- paket

/// Gizli değerin türü: parola (IMAP/JMAP) ya da OAuth yenileme jetonu.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    Password,
    OauthRefresh,
}

/// Bir hesabın gizli değeri. `value` düşerken sıfırlanır; `Debug` göstermez.
#[derive(Clone)]
pub struct TransferSecret {
    pub kind: SecretKind,
    pub value: Zeroizing<String>,
}

impl TransferSecret {
    pub fn new(kind: SecretKind, value: String) -> Self {
        Self {
            kind,
            value: Zeroizing::new(value),
        }
    }
}

impl fmt::Debug for TransferSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransferSecret")
            .field("kind", &self.kind)
            .field("value", &"<gizli>")
            .finish()
    }
}

impl PartialEq for TransferSecret {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && *self.value == *other.value
    }
}

impl Serialize for TransferSecret {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw<'a> {
            kind: SecretKind,
            value: &'a str,
        }
        Raw {
            kind: self.kind,
            value: &self.value,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TransferSecret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            kind: SecretKind,
            value: String,
        }
        let raw = Raw::deserialize(d)?;
        Ok(Self::new(raw.kind, raw.value))
    }
}

/// Paketteki bir hesap: kayıt, (varsa) gizli değeri ve JMAP kullanıcı adı.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransferAccount {
    pub account: Account,
    #[serde(default)]
    pub secret: Option<TransferSecret>,
    /// JMAP hesabının oturum kullanıcı adı (adresten farklı olabilir).
    #[serde(default)]
    pub jmap_username: Option<String>,
}

/// Aktarım paketi (sürüm 1). Bilinmeyen alanlar yok sayılır, eksik alanlar
/// varsayılan alır; [`validate_package`] başlığı ve sınırları denetler.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferPackage {
    pub v: u32,
    pub app: String,
    /// Paketi hazırlayan Mailhere sürümü.
    pub app_version: String,
    /// Alıcının anlaması gereken en düşük paket sürümü.
    pub min_receiver: u32,
    pub created_at: Millis,
    /// "windows", "linux", "android", "macos".
    pub from_platform: String,
    pub settings: Option<Settings>,
    pub accounts: Vec<TransferAccount>,
    pub rules: Vec<Rule>,
    pub tags: Vec<Tag>,
}

/// Bu cihazdaki hesapları, ayarları, kuralları ve etiketleri paketler.
/// `include_secrets` ile parolalar ve OAuth yenileme jetonları da eklenir;
/// anahtar zincirinden okunamayan gizli değer hesabı düşürmez, yalnız
/// günlüğe yazılır (telefonda parola yeniden istenir). Anahtar zinciri
/// engelleyici olabilir: komut bunu ayrı iş parçacığında çağırır.
pub fn build_package(
    store: &Store,
    secrets: &dyn SecretStore,
    include_secrets: bool,
) -> AppResult<TransferPackage> {
    let mut accounts = Vec::new();
    for account in store.accounts()? {
        let secret = include_secrets
            .then(|| read_secret(secrets, &account))
            .flatten();
        let jmap_username = match account.kind {
            AccountKind::Jmap => store
                .jmap_account_state(&account.id)?
                .username
                .filter(|u| !u.trim().is_empty()),
            AccountKind::Imap => None,
        };
        accounts.push(TransferAccount {
            account,
            secret,
            jmap_username,
        });
    }
    Ok(TransferPackage {
        v: PACKAGE_VERSION,
        app: APP_NAME.to_owned(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        min_receiver: PACKAGE_VERSION,
        created_at: now_ms(),
        from_platform: std::env::consts::OS.to_owned(),
        settings: Some(store.settings()?),
        accounts,
        rules: store.rules()?,
        tags: store.tags()?,
    })
}

/// Hesabın gizli değerinin anahtar zincirindeki adı ve türü.
fn secret_slot(account: &Account) -> (String, SecretKind) {
    match account.auth {
        AuthKind::Password => (account.id.clone(), SecretKind::Password),
        AuthKind::Google | AuthKind::Microsoft => (
            mh_sync::oauth_secret_key(&account.id),
            SecretKind::OauthRefresh,
        ),
    }
}

fn read_secret(secrets: &dyn SecretStore, account: &Account) -> Option<TransferSecret> {
    let (key, kind) = secret_slot(account);
    match secrets.get(&key) {
        Ok(Some(value)) => Some(TransferSecret::new(kind, value)),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(account = %account.id, error = %e.message, "gizli değer okunamadı; paket onsuz hazırlanıyor");
            None
        }
    }
}

/// Paketin başlığını ve sınırlarını denetler: `app`, sürüm, hesap ≤ 20,
/// kural ≤ 200, etiket ≤ 50, hesap başına imza ≤ 20.
pub fn validate_package(p: &TransferPackage) -> AppResult<()> {
    if p.app != APP_NAME {
        return Err(invalid(
            "Alınan paket Mailhere'ye ait değil. Bilgisayarda Mailhere'nin aktarım ekranındaki kodu okut.",
        ));
    }
    if p.v > PACKAGE_VERSION || p.min_receiver > PACKAGE_VERSION {
        return Err(invalid(format!(
            "Bu paket daha yeni bir Mailhere sürümüyle (v{}) hazırlanmış. Önce bu cihazdaki Mailhere'yi güncelle.",
            p.app_version
        )));
    }
    if p.v != PACKAGE_VERSION {
        return Err(invalid(
            "Alınan paket tanınmadı. İki cihazdaki Mailhere'yi güncelleyip yeniden dene.",
        ));
    }
    if p.accounts.len() > MAX_ACCOUNTS {
        return Err(invalid(format!(
            "Pakette {} hesap var; en çok {MAX_ACCOUNTS} aktarılabilir. Bilgisayarda kullanmadığın hesapları kaldırıp yeniden dene.",
            p.accounts.len()
        )));
    }
    if p.rules.len() > MAX_RULES {
        return Err(invalid(format!(
            "Pakette {} kural var; en çok {MAX_RULES} aktarılabilir. Bilgisayarda kullanmadığın kuralları silip yeniden dene.",
            p.rules.len()
        )));
    }
    if p.tags.len() > TAGS_MAX {
        return Err(invalid(format!(
            "Pakette {} etiket var; en çok {TAGS_MAX} aktarılabilir. Bilgisayarda kullanmadığın etiketleri silip yeniden dene.",
            p.tags.len()
        )));
    }
    if let Some(a) = p
        .accounts
        .iter()
        .find(|a| a.account.signatures.len() > crate::signatures::MAX_SIGNATURES)
    {
        return Err(invalid(format!(
            "“{}” hesabında {} imza var; en çok {} aktarılabilir. Bilgisayarda fazlasını silip yeniden dene.",
            a.account.name,
            a.account.signatures.len(),
            crate::signatures::MAX_SIGNATURES
        )));
    }
    Ok(())
}

/// Gelen ayarları yerel ayarların üstüne yazar; cihaza özgü olanlar
/// (kart konumu ve süresi, yerel bildirim, otomatik güncelleme) ve onaylı
/// sertifikalar yerelde kalır.
pub fn merge_settings(local: &Settings, incoming: Settings) -> Settings {
    Settings {
        card_position: local.card_position,
        card_seconds: local.card_seconds,
        native_notifications: local.native_notifications,
        auto_update: local.auto_update,
        trusted_certs: local.trusted_certs.clone(),
        ..incoming
    }
}

/// Paketin özeti: hesaplar (gizli değer var mı, bu cihazda var mı), kural
/// ve etiket sayısı, ayar var mı. Gizli değer içermez.
pub fn preview(
    preview_id: &str,
    package: &TransferPackage,
    store: &Store,
) -> AppResult<TransferPreview> {
    let mut accounts = Vec::with_capacity(package.accounts.len());
    for a in &package.accounts {
        let id = a.account.id.trim();
        accounts.push(TransferPreviewAccount {
            id: id.to_owned(),
            name: a.account.name.clone(),
            email: a.account.email.clone(),
            kind: a.account.kind,
            auth: a.account.auth,
            has_secret: a
                .secret
                .as_ref()
                .is_some_and(|s| s.kind == secret_slot(&a.account).1),
            exists: !id.is_empty() && store.account(id)?.is_some(),
        });
    }
    Ok(TransferPreview {
        preview_id: preview_id.to_owned(),
        from_version: package.app_version.clone(),
        from_platform: package.from_platform.clone(),
        created_at: package.created_at,
        accounts,
        rules: package.rules.len() as u32,
        tags: package.tags.len() as u32,
        settings: package.settings.is_some(),
    })
}

// ---------------------------------------------------------------- uygulama

/// Paketi uygulayanın ihtiyaçları: depo, anahtar zinciri ve kaydedilen hesabın
/// eşitlemesini başlatan kanca (komutta `SyncManager`/`JmapManager`, testte
/// kayıt). `check_login` kapalıysa giriş denenmez (testler, ağ yok).
pub struct Applier<'a> {
    pub store: &'a Store,
    pub secrets: &'a Arc<dyn SecretStore>,
    pub start: &'a (dyn Fn(&Account) + Sync),
    pub check_login: bool,
}

/// Uygulama sonucu ve bilgisayara bildirilecek durum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub report: TransferReport,
    /// Seçilenlerin tamamı uygulandıysa `Applied`; bir kısmı atlandı ya da
    /// gizli değeri yazılamadıysa `Partial`.
    pub status: ApplyStatus,
}

/// Paketi kullanıcının seçimine göre uygular (sıra: ayarlar → etiketler →
/// hesaplar → kurallar). Geçersiz tek bir öğe bütünü durdurmaz: atlanır ve
/// raporda/günlükte görünür. Depo hatası bütünü durdurur.
pub async fn apply_package(
    applier: &Applier<'_>,
    package: TransferPackage,
    choice: &TransferChoice,
) -> AppResult<Outcome> {
    validate_package(&package)?;
    let store = applier.store;
    let mut report = TransferReport::default();
    let mut partial = false;

    if choice.settings {
        if let Some(incoming) = package.settings {
            let local = store.settings()?;
            let merged = crate::commands::normalize_settings(merge_settings(&local, incoming));
            store.set_settings(&merged)?;
            report.settings = true;
        }
    }

    let tags = apply_tags(store, &package.tags, choice.tags)?;
    report.tags = tags.applied;
    partial |= tags.skipped > 0;

    let wanted: HashSet<&str> = choice.accounts.iter().map(|s| s.trim()).collect();
    for (i, ta) in package
        .accounts
        .into_iter()
        .filter(|a| wanted.contains(a.account.id.trim()))
        .enumerate()
    {
        match apply_account(applier, i, ta, &mut report).await? {
            true => {}
            false => partial = true,
        }
    }

    if choice.rules {
        let mut count = store.rule_count()?;
        for rule in package.rules {
            let name = rule.name.clone();
            match clean_rule(store, rule, &tags.map)? {
                Ok(rule) => {
                    if store.rule(&rule.id)?.is_some() {
                        store.rule_update(&rule)?;
                    } else if count < MAX_RULES {
                        store.rule_insert(&rule)?;
                        count += 1;
                    } else {
                        tracing::warn!(rule = %name, "kural sınırı dolu; aktarılan kural atlandı");
                        report.rules_skipped += 1;
                        continue;
                    }
                    report.rules += 1;
                }
                Err(why) => {
                    tracing::info!(rule = %name, why = ?why, "aktarılan kural atlandı");
                    report.rules_skipped += 1;
                }
            }
        }
        partial |= report.rules_skipped > 0;
    }

    partial |= !report.accounts_without_secret.is_empty();
    Ok(Outcome {
        report,
        status: if partial {
            ApplyStatus::Partial
        } else {
            ApplyStatus::Applied
        },
    })
}

/// Etiket uygulamasının sonucu: paketteki kimlik → yerel kimlik eşlemesi.
struct TagsApplied {
    map: HashMap<String, String>,
    applied: u32,
    skipped: u32,
}

/// Etiketleri uygular. Adı (büyük/küçük harf ve Türkçe harf farkı gözetmeden)
/// yerel bir etiketle çakışan etiket eklenmez, ona eşlenir. `insert`
/// kapalıysa yalnız eşleme kurulur (kurallar yerel etiketleri bulsun).
fn apply_tags(store: &Store, incoming: &[Tag], insert: bool) -> AppResult<TagsApplied> {
    let mut by_name: HashMap<String, String> = store
        .tags()?
        .into_iter()
        .map(|t| (fold_loose(&t.name), t.id))
        .collect();
    let mut count = by_name.len();
    let mut out = TagsApplied {
        map: HashMap::new(),
        applied: 0,
        skipped: 0,
    };
    for tag in incoming {
        let name = normalize_tag_name(&tag.name);
        let id = tag.id.trim();
        if name.is_empty() {
            out.skipped += 1;
            continue;
        }
        if let Some(local) = by_name.get(&fold_loose(&name)) {
            if !id.is_empty() {
                out.map.insert(id.to_owned(), local.clone());
            }
            continue;
        }
        if !insert {
            continue;
        }
        let color = TAG_COLORS
            .iter()
            .find(|c| c.eq_ignore_ascii_case(tag.color.trim()))
            .unwrap_or(&TAG_COLORS[0]);
        let clean = Tag {
            id: if id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                id.to_owned()
            },
            name: name.clone(),
            color: (*color).to_owned(),
        };
        if store.tag(&clean.id)?.is_some() {
            // Aynı kimlik, başka ad: önceki aktarımdan gelen etiket yeniden adlandı.
            let old = store.tag(&clean.id)?.map(|t| fold_loose(&t.name));
            if let Some(old) = old {
                by_name.remove(&old);
            }
            store.tag_update(&clean)?;
        } else if count < TAGS_MAX {
            store.tag_insert(&clean)?;
            count += 1;
        } else {
            tracing::warn!(tag = %name, "etiket sınırı dolu; aktarılan etiket atlandı");
            out.skipped += 1;
            continue;
        }
        by_name.insert(fold_loose(&name), clean.id.clone());
        out.map.insert(id.to_owned(), clean.id);
        out.applied += 1;
    }
    Ok(out)
}

/// Gelen hesabı denetler ve sadeleştirir. Adres geçerli ve kimlik dolu olmalı;
/// boş ad ve gönderen adı adresten türetilir; geçersiz renk paletten alınır;
/// IMAP hesabında sunucu ayarları, JMAP hesabında oturum adresi zorunludur.
fn clean_account(mut a: Account, index: usize) -> AppResult<Account> {
    a.id = a.id.trim().to_owned();
    if a.id.is_empty() {
        return Err(invalid("Hesabın kimliği boş."));
    }
    let (email, domain) = mh_sync::discover::split_email(&a.email)?;
    a.email = email;
    a.name = match mh_send::clean_name(&a.name).as_str() {
        "" => domain,
        n => n.to_owned(),
    };
    a.display_name = match mh_send::clean_name(&a.display_name).as_str() {
        "" => a.email.split('@').next().unwrap_or("").to_owned(),
        d => d.to_owned(),
    };
    let color = a.color.trim().to_ascii_uppercase();
    a.color = if crate::cmd_account::is_hex_color(&color) {
        color
    } else {
        crate::commands::PALETTE[index % crate::commands::PALETTE.len()].to_owned()
    };
    let (signatures, new, reply) = crate::signatures::normalize_signatures(
        std::mem::take(&mut a.signatures),
        a.signature_new.take(),
        a.signature_reply.take(),
    )?;
    a.signatures = signatures;
    a.signature_new = new;
    a.signature_reply = reply;
    match a.kind {
        AccountKind::Imap => {
            let imap = a
                .imap
                .as_ref()
                .ok_or_else(|| invalid("IMAP hesabının gelen posta sunucusu ayarı yok."))?;
            let smtp = a
                .smtp
                .as_ref()
                .ok_or_else(|| invalid("IMAP hesabının giden posta sunucusu ayarı yok."))?;
            crate::commands::validate_server(imap, "Gelen posta")?;
            crate::commands::validate_server(smtp, "Giden posta")?;
            a.jmap_url = None;
        }
        AccountKind::Jmap => {
            if a.auth != AuthKind::Password {
                return Err(invalid("JMAP hesabı yalnız parolayla bağlanır."));
            }
            let url = a
                .jmap_url
                .as_deref()
                .ok_or_else(|| invalid("JMAP hesabının oturum adresi yok."))?;
            a.jmap_url = Some(mh_jmap::normalize_session_url(url)?.to_string());
            a.imap = None;
            a.smtp = None;
        }
    }
    if a.created_at <= 0 {
        a.created_at = now_ms();
    }
    Ok(a)
}

/// Gizli değeri engellemeden yazar/okur (anahtar zinciri sistem çağrısı yapar).
async fn secret_set(
    secrets: &Arc<dyn SecretStore>,
    key: String,
    value: Zeroizing<String>,
) -> AppResult<()> {
    let s = secrets.clone();
    tokio::task::spawn_blocking(move || s.set(&key, &value))
        .await
        .map_err(internal)?
}

async fn secret_present(secrets: &Arc<dyn SecretStore>, key: String) -> bool {
    let s = secrets.clone();
    tokio::task::spawn_blocking(move || s.get(&key))
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten()
        .is_some()
}

/// Parolayı sunucuda dener; `Some(true)` kabul edildi, `Some(false)` sunucu
/// reddetti (yanlış parola), `None` karar verilemedi (ağ, zaman aşımı,
/// sertifika): parola yine de yazılır, durum eşitleme sırasında görünür.
async fn login_ok(account: &Account, jmap_username: &str, password: &str) -> Option<bool> {
    let attempt = async {
        match account.kind {
            AccountKind::Imap => {
                let imap = account.imap.as_ref()?;
                Some(mh_sync::test_login(imap, password).await)
            }
            AccountKind::Jmap => {
                let url = account.jmap_url.as_deref()?;
                Some(
                    mh_jmap::test_login(url, jmap_username, password)
                        .await
                        .map(|_| ()),
                )
            }
        }
    };
    let result = tokio::time::timeout(LOGIN_TIMEOUT, attempt).await.ok()??;
    match result {
        Ok(()) => Some(true),
        Err(e) if e.code == ErrorCode::Auth => Some(false),
        Err(e) => {
            tracing::info!(account = %account.id, code = %e.code, "aktarılan hesabın girişi denenemedi; parola yine de kaydedildi");
            None
        }
    }
}

/// Tek hesabı uygular. `Ok(false)`: hesap geçersiz olduğu için atlandı.
async fn apply_account(
    applier: &Applier<'_>,
    index: usize,
    ta: TransferAccount,
    report: &mut TransferReport,
) -> AppResult<bool> {
    let store = applier.store;
    let account = match clean_account(ta.account, index) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %e.message, "aktarılan hesap geçersiz; atlandı");
            return Ok(false);
        }
    };
    let exists = store.account(&account.id)?.is_some();
    // JMAP kullanıcı adı: paketten; yoksa yerel kayıttan; o da yoksa adres.
    let jmap_username = match account.kind {
        AccountKind::Jmap => ta
            .jmap_username
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                store
                    .jmap_account_state(&account.id)
                    .ok()
                    .and_then(|s| s.username)
                    .filter(|u| !u.trim().is_empty())
            }),
        AccountKind::Imap => None,
    };
    let (key, kind) = secret_slot(&account);
    let secret = ta.secret.filter(|s| s.kind == kind);
    let mut without_secret = false;
    match secret {
        Some(secret) => {
            let rejected = applier.check_login
                && kind == SecretKind::Password
                && login_ok(
                    &account,
                    jmap_username.as_deref().unwrap_or(&account.email),
                    &secret.value,
                )
                .await
                    == Some(false);
            if rejected {
                tracing::warn!(account = %account.id, "sunucu aktarılan parolayı reddetti; parola kaydedilmedi");
                without_secret = true;
            } else if let Err(e) = secret_set(applier.secrets, key, secret.value.clone()).await {
                tracing::warn!(account = %account.id, error = %e.message, "gizli değer anahtar zincirine yazılamadı");
                without_secret = true;
            }
        }
        None => {
            // Yerelde zaten gizli değeri olan hesap korunur; yoksa parola istenir.
            if !(exists && secret_present(applier.secrets, key).await) {
                without_secret = true;
            }
        }
    }
    if without_secret {
        report.accounts_without_secret.push(account.email.clone());
    }
    store.insert_account(&account)?;
    if let Some(u) = jmap_username.as_deref() {
        store.set_jmap_username(&account.id, u)?;
    }
    if exists {
        report.accounts_updated += 1;
    } else {
        report.accounts_added += 1;
    }
    (applier.start)(&account);
    Ok(true)
}

/// Kuralın atlanma nedeni.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipRule {
    /// Kuralın hesabı bu cihazda yok.
    NoAccount,
    /// Koşul ya da eylem kalmadı, sayı sınırı aşıldı.
    Invalid,
}

/// Gelen kuralı sadeleştirir: ad ve değerler kırpılır; hesabı bu cihazda
/// olmayan kural atlanır; etiket eylemleri yerel kimliğe çevrilir (karşılığı
/// yoksa eylem düşer); klasöre taşıma kuralın hesabındaki bir klasörü
/// göstermeli (klasörler ilk eşitlemeyle gelir, varlığı burada aranmaz);
/// geçersiz iletme adresi düşer. Eylemi kalmayan kural atlanır.
fn clean_rule(
    store: &Store,
    mut rule: Rule,
    tag_map: &HashMap<String, String>,
) -> AppResult<Result<Rule, SkipRule>> {
    use crate::cmd_rules::{clip, valid_email, ACTIONS_MAX, CONDITIONS_MAX, NAME_MAX, VALUE_MAX};
    use mh_core::RuleCondition;

    rule.id = rule.id.trim().to_owned();
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    rule.name = clip(&rule.name, NAME_MAX);
    if rule.name.is_empty() {
        rule.name = "Kural".to_owned();
    }
    rule.account_id = rule
        .account_id
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty());
    if let Some(a) = &rule.account_id {
        if store.account(a)?.is_none() {
            return Ok(Err(SkipRule::NoAccount));
        }
    }
    if rule.conditions.is_empty() || rule.conditions.len() > CONDITIONS_MAX {
        return Ok(Err(SkipRule::Invalid));
    }
    for c in &mut rule.conditions {
        let value = match c {
            RuleCondition::From { value, .. }
            | RuleCondition::To { value, .. }
            | RuleCondition::Subject { value, .. }
            | RuleCondition::Body { value, .. } => value,
            RuleCondition::HasAttachment | RuleCondition::Category { .. } => continue,
        };
        *value = clip(value, VALUE_MAX);
        if value.is_empty() {
            return Ok(Err(SkipRule::Invalid));
        }
    }
    if rule.actions.len() > ACTIONS_MAX {
        return Ok(Err(SkipRule::Invalid));
    }
    let mut actions = Vec::with_capacity(rule.actions.len());
    for a in rule.actions {
        let keep = match a {
            RuleAction::Move { mailbox_id } => {
                let mailbox_id = mailbox_id.trim().to_owned();
                let ok = rule
                    .account_id
                    .as_deref()
                    .is_some_and(|acc| mailbox_id.starts_with(&format!("{acc}:")))
                    && mailbox_id.len() > rule.account_id.as_deref().map_or(0, |a| a.len() + 1);
                ok.then_some(RuleAction::Move { mailbox_id })
            }
            RuleAction::Forward { to } => {
                let to = to.trim().to_owned();
                valid_email(&to).then_some(RuleAction::Forward { to })
            }
            RuleAction::Tag { tag_id } => {
                let tag_id = tag_id.trim();
                let local = match tag_map.get(tag_id) {
                    Some(l) => Some(l.clone()),
                    None => store.tag(tag_id)?.map(|t| t.id),
                };
                local.map(|tag_id| RuleAction::Tag { tag_id })
            }
            other => Some(other),
        };
        if let Some(a) = keep {
            actions.push(a);
        }
    }
    if actions.is_empty() {
        return Ok(Err(SkipRule::Invalid));
    }
    rule.actions = actions;
    Ok(Ok(rule))
}

// --------------------------------------------------------------- ağ köprüsü

/// Paketi JSON'a çevirip teklifi açar (bilgisayar tarafı).
pub async fn open_offer(package: &TransferPackage, opts: OfferOptions) -> AppResult<Offer> {
    let bytes = Zeroizing::new(serde_json::to_vec(package).map_err(internal)?);
    Ok(mh_transfer::start(bytes, opts).await?)
}

/// Koddaki bilgisayardan paketi alır, çözer ve denetler (telefon tarafı).
pub async fn fetch_package(code: &Code, opts: &ClientOptions) -> AppResult<TransferPackage> {
    let bytes = mh_transfer::fetch(code, opts).await?;
    let package: TransferPackage = serde_json::from_slice(&bytes).map_err(|e| {
        tracing::warn!(error = %e, "aktarım paketi çözülemedi");
        invalid(
            "Bilgisayardan gelen paket çözülemedi; iki cihazdaki Mailhere sürümleri uyumlu olmayabilir. İkisini de güncelleyip yeniden dene.",
        )
    })?;
    validate_package(&package)?;
    Ok(package)
}

// ------------------------------------------------------------------- durum

struct OpenOffer {
    handle: OfferHandle,
    /// Durumları `mh:transfer` olayına ileten görev.
    forwarder: tokio::task::JoinHandle<()>,
}

struct Pending {
    package: TransferPackage,
    code: Code,
    created: Instant,
}

/// Aktarım durumu (`AppState::transfer`): açık teklif (bilgisayar) ve
/// uygulanmayı bekleyen paketler (telefon). Aynı anda tek teklif; yeni teklif
/// eskisini sessizce iptal eder. Bekleyen paket [`PREVIEW_TTL`] sonra düşer.
#[derive(Default)]
pub struct TransferHub {
    offer: Mutex<Option<OpenOffer>>,
    pending: Mutex<HashMap<String, Pending>>,
}

impl TransferHub {
    fn offer(&self) -> std::sync::MutexGuard<'_, Option<OpenOffer>> {
        self.offer.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Yeni teklifi kaydeder; eskisi varsa olay iletmeden iptal edilir
    /// (arayüz yalnız yeni teklifin durumlarını görsün).
    fn replace_offer(&self, handle: OfferHandle, forwarder: tokio::task::JoinHandle<()>) {
        let old = self.offer().replace(OpenOffer { handle, forwarder });
        if let Some(old) = old {
            old.forwarder.abort();
            old.handle.cancel();
        }
    }

    /// Açık teklifi iptal eder; `cancelled` olayı iletilir. Teklif yoksa `false`.
    pub fn cancel_offer(&self) -> bool {
        match self.offer().take() {
            Some(open) => {
                open.handle.cancel();
                true
            }
            None => false,
        }
    }

    fn insert_preview_at(&self, id: String, package: TransferPackage, code: Code, now: Instant) {
        let mut pending = self.pending();
        pending.retain(|_, p| now.saturating_duration_since(p.created) < PREVIEW_TTL);
        pending.insert(
            id,
            Pending {
                package,
                code,
                created: now,
            },
        );
    }

    /// Alınan paketi (ve onay için kodu) bekletir.
    pub fn insert_preview(&self, id: String, package: TransferPackage, code: Code) {
        self.insert_preview_at(id, package, code, Instant::now());
    }

    fn take_preview_at(&self, id: &str, now: Instant) -> Option<(TransferPackage, Code)> {
        let mut pending = self.pending();
        pending.retain(|_, p| now.saturating_duration_since(p.created) < PREVIEW_TTL);
        pending.remove(id).map(|p| (p.package, p.code))
    }

    /// Bekleyen paketi alır (tek kullanımlık); süresi dolmuşsa `None`.
    pub fn take_preview(&self, id: &str) -> Option<(TransferPackage, Code)> {
        self.take_preview_at(id, Instant::now())
    }

    /// Süresi dolan paketleri düşürür.
    pub fn purge(&self) {
        let now = Instant::now();
        self.pending()
            .retain(|_, p| now.saturating_duration_since(p.created) < PREVIEW_TTL);
    }

    /// Bekleyen paket sayısı (testler).
    pub fn pending_len(&self) -> usize {
        self.pending().len()
    }
}

// ----------------------------------------------------------------- komutlar

/// Bu bilgisayarda aktarım teklifi açar: paket hazırlanır, yerel kapı
/// dinlenir, QR ve kod döner. Durumlar `mh:transfer` olayıyla gelir. Açık bir
/// teklif varsa sessizce kapanır.
#[tauri::command]
pub async fn transfer_begin(
    app: AppHandle,
    state: State<'_, AppState>,
    include_secrets: bool,
) -> Res<TransferOffer> {
    let (store, secrets) = (state.store.clone(), state.secrets.clone());
    let package = tokio::task::spawn_blocking(move || {
        build_package(&store, secrets.as_ref(), include_secrets)
    })
    .await
    .map_err(internal)??;
    let offer = open_offer(&package, OfferOptions::default()).await?;
    drop(package);
    let (info, handle, mut events) = offer.into_parts();
    tracing::info!(
        port = info.port,
        hosts = info.hosts.len(),
        include_secrets,
        "aktarım teklifi açıldı"
    );
    let forwarder = tokio::spawn(async move {
        while let Some(st) = events.recv().await {
            if let Err(e) = app.emit(events::TRANSFER, &st) {
                tracing::warn!(error = %e, "aktarım olayı gönderilemedi");
            }
        }
    });
    state.transfer.replace_offer(handle, forwarder);
    Ok(info)
}

/// Açık teklifi kapatır (`cancelled` olayı gelir). Teklif yoksa sessizce geçer.
#[tauri::command]
pub async fn transfer_cancel(state: State<'_, AppState>) -> Res<()> {
    state.transfer.cancel_offer();
    Ok(())
}

/// QR'dan okunan ya da yapıştırılan kodla bilgisayardan paketi alır ve
/// özetini döner. Paket 5 dakika bellekte bekler; `transfer_apply` ile
/// uygulanır.
#[tauri::command]
pub async fn transfer_receive(
    app: AppHandle,
    state: State<'_, AppState>,
    code: String,
) -> Res<TransferPreview> {
    // Kod metni anahtarı taşır: çözümlenince tampon sıfırlanarak düşer.
    let text = Zeroizing::new(code);
    let code = Code::parse(&text)?;
    drop(text);
    let package = fetch_package(&code, &ClientOptions::default()).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let summary = preview(&id, &package, &state.store)?;
    state.transfer.insert_preview(id, package, code);
    // Uygulanmayan paket gizli değer taşıyabilir: süresi dolunca bellekten düşer.
    tokio::spawn(async move {
        tokio::time::sleep(PREVIEW_TTL + Duration::from_secs(1)).await;
        if let Some(s) = app.try_state::<AppState>() {
            s.transfer.purge();
        }
    });
    Ok(summary)
}

/// Bekleyen paketi seçime göre uygular, sonucu bilgisayara bildirir ve
/// raporu döner.
#[tauri::command]
pub async fn transfer_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    preview_id: String,
    choice: TransferChoice,
) -> Res<TransferReport> {
    let (package, code) = state.transfer.take_preview(&preview_id).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Önizlemenin süresi doldu ya da paket zaten uygulandı. Bilgisayarda yeni kod oluşturup QR'ı yeniden okut.",
        )
    })?;
    let start = |a: &Account| {
        // Yeni yenileme jetonu kullanılsın; eski erişim jetonu düşsün.
        state.sync.tokens().forget(&a.id);
        match a.kind {
            AccountKind::Imap => state.sync.start(a.clone()),
            AccountKind::Jmap => state.jmap.start(a.clone()),
        }
    };
    let applier = Applier {
        store: &state.store,
        secrets: &state.secrets,
        start: &start,
        check_login: true,
    };
    let outcome = apply_package(&applier, package, &choice).await;
    let status = outcome.as_ref().map_or(ApplyStatus::Failed, |o| o.status);
    tokio::spawn(async move {
        if let Err(e) = mh_transfer::confirm(&code, status, &ClientOptions::default()).await {
            tracing::warn!(error = %e, "aktarım onayı bilgisayara iletilemedi");
        }
    });
    let outcome = outcome?;
    if outcome.report.settings {
        if let Ok(settings) = state.store.settings() {
            if let Err(e) = app.emit(events::SETTINGS, &settings) {
                tracing::warn!(error = %e, "ayar olayı gönderilemedi");
            }
        }
    }
    crate::sink::changed(&app, Vec::new());
    tracing::info!(
        added = outcome.report.accounts_added,
        updated = outcome.report.accounts_updated,
        rules = outcome.report.rules,
        tags = outcome.report.tags,
        status = ?outcome.status,
        "aktarım paketi uygulandı"
    );
    Ok(outcome.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::{
        CardPosition, Density, RuleCondition, RuleMatch, RuleOp, Security, ServerConfig, Signature,
        Theme, TransferStateKind,
    };
    use mh_sync::MemorySecrets;

    fn server(host: &str, user: &str) -> ServerConfig {
        ServerConfig {
            host: host.into(),
            port: 993,
            security: Security::Tls,
            username: user.into(),
        }
    }

    fn account(id: &str, auth: AuthKind) -> Account {
        Account {
            id: id.into(),
            name: format!("Hesap {id}"),
            email: format!("{id}@ornek.com"),
            display_name: "Furkan".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: Some(server("imap.ornek.com", id)),
            smtp: Some(server("smtp.ornek.com", id)),
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 1_000,
            auth,
            notify: true,
        }
    }

    fn jmap_account(id: &str) -> Account {
        Account {
            kind: AccountKind::Jmap,
            imap: None,
            smtp: None,
            jmap_url: Some("https://posta.ornek.com/jmap".into()),
            ..account(id, AuthKind::Password)
        }
    }

    /// 32 baytlık, `:` ayraçlı SHA-256 parmak izi (ayar sadeleştirmesi biçimi denetler).
    fn fingerprint(byte: &str) -> String {
        vec![byte; 32].join(":")
    }

    fn tag(id: &str, name: &str, color: &str) -> Tag {
        Tag {
            id: id.into(),
            name: name.into(),
            color: color.into(),
        }
    }

    fn rule(id: &str, account: Option<&str>, actions: Vec<RuleAction>) -> Rule {
        Rule {
            id: id.into(),
            name: format!("Kural {id}"),
            enabled: true,
            account_id: account.map(str::to_owned),
            match_: RuleMatch::All,
            conditions: vec![RuleCondition::From {
                op: RuleOp::Contains,
                value: "fatura".into(),
            }],
            actions,
            stop: false,
        }
    }

    fn package(accounts: Vec<TransferAccount>) -> TransferPackage {
        TransferPackage {
            v: 1,
            app: APP_NAME.into(),
            app_version: "0.4.0".into(),
            min_receiver: 1,
            created_at: 5,
            from_platform: "linux".into(),
            settings: Some(Settings::default()),
            accounts,
            rules: Vec::new(),
            tags: Vec::new(),
        }
    }

    fn with_secret(account: Account, secret: Option<(SecretKind, &str)>) -> TransferAccount {
        TransferAccount {
            account,
            secret: secret.map(|(k, v)| TransferSecret::new(k, v.into())),
            jmap_username: None,
        }
    }

    fn all(package: &TransferPackage) -> TransferChoice {
        TransferChoice {
            accounts: package
                .accounts
                .iter()
                .map(|a| a.account.id.clone())
                .collect(),
            rules: true,
            tags: true,
            settings: true,
        }
    }

    /// Başlatılan hesapları kaydeden kanca.
    #[derive(Default)]
    struct Started(Mutex<Vec<String>>);

    impl Started {
        fn ids(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    struct Side {
        store: Store,
        secrets: Arc<dyn SecretStore>,
        started: Started,
    }

    impl Side {
        fn new() -> Self {
            Self {
                store: Store::open_in_memory().unwrap(),
                secrets: Arc::new(MemorySecrets::new()),
                started: Started::default(),
            }
        }

        async fn apply(&self, package: TransferPackage, choice: &TransferChoice) -> Outcome {
            let start = |a: &Account| self.started.0.lock().unwrap().push(a.id.clone());
            let applier = Applier {
                store: &self.store,
                secrets: &self.secrets,
                start: &start,
                check_login: false,
            };
            apply_package(&applier, package, choice).await.unwrap()
        }
    }

    #[test]
    fn paket_gizli_degerleri_toplar() {
        let s = Store::open_in_memory().unwrap();
        let secrets = MemorySecrets::new();
        s.insert_account(&account("a", AuthKind::Password)).unwrap();
        s.insert_account(&account("g", AuthKind::Google)).unwrap();
        s.insert_account(&jmap_account("j")).unwrap();
        s.set_jmap_username("j", "furkan").unwrap();
        // "x" hesabının gizli değeri yok: paket yine de hazırlanır.
        s.insert_account(&account("x", AuthKind::Password)).unwrap();
        secrets.set("a", "parola-a").unwrap();
        secrets.set("oauth:g", "yenileme-g").unwrap();
        secrets.set("j", "parola-j").unwrap();
        s.tag_insert(&tag("t1", "Fatura", TAG_COLORS[2])).unwrap();
        s.rule_insert(&rule("r1", None, vec![RuleAction::Archive]))
            .unwrap();

        let p = build_package(&s, &secrets, true).unwrap();
        validate_package(&p).unwrap();
        assert_eq!((p.v, p.app.as_str(), p.min_receiver), (1, "mailhere", 1));
        assert_eq!(p.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(p.from_platform, std::env::consts::OS);
        assert!(p.created_at > 0);
        assert_eq!(p.accounts.len(), 4);
        let by_id = |id: &str| p.accounts.iter().find(|a| a.account.id == id).unwrap();
        assert_eq!(
            by_id("a").secret,
            Some(TransferSecret::new(SecretKind::Password, "parola-a".into()))
        );
        assert_eq!(
            by_id("g").secret,
            Some(TransferSecret::new(
                SecretKind::OauthRefresh,
                "yenileme-g".into()
            ))
        );
        assert_eq!(
            by_id("j").secret.as_ref().unwrap().kind,
            SecretKind::Password
        );
        assert_eq!(by_id("j").jmap_username.as_deref(), Some("furkan"));
        assert_eq!(by_id("a").jmap_username, None);
        assert_eq!(by_id("x").secret, None);
        assert_eq!(p.rules.len(), 1);
        assert_eq!(p.tags.len(), 1);
        assert!(p.settings.is_some());

        // Debug gizli değer sızdırmaz; JSON gidip gelir (değeri taşır).
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("parola-a") && !dbg.contains("yenileme-g"));
        assert!(dbg.contains("<gizli>"));
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"appVersion\"") && json.contains("\"minReceiver\""));
        assert!(json.contains("\"kind\":\"oauthRefresh\""));
        assert!(json.contains("\"jmapUsername\":\"furkan\""));
        let back: TransferPackage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);

        // Gizli değerler istenmeyince hiçbiri yok.
        let bare = build_package(&s, &secrets, false).unwrap();
        assert!(bare.accounts.iter().all(|a| a.secret.is_none()));
        assert!(!serde_json::to_string(&bare).unwrap().contains("parola"));

        // Eksik ve bilinmeyen alanlar: ileri uyumluluk.
        let p2: TransferPackage =
            serde_json::from_str(r#"{"v":1,"app":"mailhere","yeniAlan":true}"#).unwrap();
        assert_eq!(p2.accounts.len(), 0);
        assert_eq!(p2.settings, None);
        validate_package(&p2).unwrap();
    }

    #[test]
    fn paket_basligi_ve_sinirlar_denetlenir() {
        let msg = |p: TransferPackage| {
            let e = validate_package(&p).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid);
            e.message
        };
        let base = package(Vec::new());
        assert!(msg(TransferPackage {
            app: "baska".into(),
            ..base.clone()
        })
        .contains("Mailhere'ye ait değil"));
        assert!(msg(TransferPackage {
            v: 2,
            ..base.clone()
        })
        .contains("güncelle"));
        assert!(msg(TransferPackage {
            min_receiver: 2,
            ..base.clone()
        })
        .contains("güncelle"));
        assert!(msg(TransferPackage {
            v: 0,
            ..base.clone()
        })
        .contains("tanınmadı"));
        let many: Vec<TransferAccount> = (0..21)
            .map(|i| with_secret(account(&format!("h{i}"), AuthKind::Password), None))
            .collect();
        assert!(msg(package(many)).contains("en çok 20"));
        assert!(msg(TransferPackage {
            tags: (0..51).map(|i| tag(&i.to_string(), "E", "")).collect(),
            ..base.clone()
        })
        .contains("en çok 50"));
        assert!(msg(TransferPackage {
            rules: (0..201)
                .map(|i| rule(&i.to_string(), None, vec![RuleAction::Star]))
                .collect(),
            ..base.clone()
        })
        .contains("en çok 200"));
        let mut a = account("a", AuthKind::Password);
        a.signatures = (0..21)
            .map(|i| Signature {
                id: i.to_string(),
                name: "İ".into(),
                body: "b".into(),
            })
            .collect();
        assert!(msg(package(vec![with_secret(a, None)])).contains("imza"));
    }

    #[tokio::test]
    async fn uygulama_var_olani_gunceller() {
        let side = Side::new();
        let mut old = account("a", AuthKind::Password);
        old.name = "Eski".into();
        old.created_at = 42;
        side.store.insert_account(&old).unwrap();
        side.secrets.set("a", "eski-parola").unwrap();

        let mut new = account("a", AuthKind::Password);
        new.name = "  Yeni ".into();
        new.color = "#2e9a61".into();
        new.created_at = 999;
        new.signatures = vec![Signature {
            id: "".into(),
            name: " ".into(),
            body: "Sevgiler\n\n".into(),
        }];
        let mut p = package(vec![
            with_secret(new, Some((SecretKind::Password, "yeni-parola"))),
            with_secret(
                account("b", AuthKind::Password),
                Some((SecretKind::Password, "parola-b")),
            ),
            // Seçilmeyen hesap uygulanmaz.
            with_secret(account("c", AuthKind::Password), None),
        ]);
        p.rules.push(rule("r1", Some("c"), vec![RuleAction::Star]));
        let mut choice = all(&p);
        choice.accounts.retain(|id| id != "c");
        let out = side.apply(p, &choice).await;
        assert_eq!(out.report.accounts_updated, 1);
        assert_eq!(out.report.accounts_added, 1);
        assert!(out.report.accounts_without_secret.is_empty());
        assert_eq!(out.report.rules_skipped, 1, "c hesabı bu cihazda yok");
        assert_eq!(out.status, ApplyStatus::Partial);
        let a = side.store.account("a").unwrap().unwrap();
        assert_eq!(a.name, "Yeni");
        assert_eq!(a.color, "#2E9A61");
        assert_eq!(a.created_at, 42, "yerel ekleniş zamanı korunur");
        assert_eq!(a.signatures.len(), 1);
        assert_eq!(a.signatures[0].name, "İmza 1");
        assert_eq!(a.signatures[0].body, "Sevgiler");
        assert_eq!(
            side.secrets.get("a").unwrap().as_deref(),
            Some("yeni-parola")
        );
        assert_eq!(side.secrets.get("b").unwrap().as_deref(), Some("parola-b"));
        assert!(side.store.account("c").unwrap().is_none());
        assert_eq!(side.started.ids(), vec!["a", "b"]);
        assert_eq!(side.store.accounts().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn etiket_adi_cakisinca_eslenir() {
        let side = Side::new();
        side.store
            .tag_insert(&tag("yerel-fatura", "Fatura", TAG_COLORS[0]))
            .unwrap();
        let mut p = package(vec![with_secret(
            account("a", AuthKind::Password),
            Some((SecretKind::Password, "p")),
        )]);
        p.tags = vec![
            tag("uzak-fatura", " FATURA ", TAG_COLORS[3]),
            tag("uzak-seyahat", "Seyahat", "#123456"),
            tag("uzak-bos", "  ", TAG_COLORS[1]),
        ];
        p.rules = vec![
            rule(
                "r1",
                None,
                vec![RuleAction::Tag {
                    tag_id: "uzak-fatura".into(),
                }],
            ),
            rule(
                "r2",
                Some("a"),
                vec![
                    RuleAction::Tag {
                        tag_id: "uzak-seyahat".into(),
                    },
                    RuleAction::Move {
                        mailbox_id: "a:Seyahat".into(),
                    },
                    RuleAction::Move {
                        mailbox_id: "b:Baska".into(),
                    },
                    RuleAction::Forward {
                        to: "bozuk adres".into(),
                    },
                ],
            ),
            rule(
                "r3",
                None,
                vec![RuleAction::Tag {
                    tag_id: "uzak-yok".into(),
                }],
            ),
        ];
        let choice = all(&p);
        let out = side.apply(p, &choice).await;
        // Fatura eşlendi (eklenmedi), Seyahat eklendi, boş ad atlandı.
        assert_eq!(out.report.tags, 1);
        let tags = side.store.tags().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], tag("yerel-fatura", "Fatura", TAG_COLORS[0]));
        assert_eq!(tags[1], tag("uzak-seyahat", "Seyahat", TAG_COLORS[0]));
        // r1 yerel etikete yazıldı; r2'de başka hesabın klasörü ve bozuk
        // adres düştü; r3'ün tek eylemi karşılıksız → atlandı.
        let rules = side.store.rules().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules[0].actions,
            vec![RuleAction::Tag {
                tag_id: "yerel-fatura".into()
            }]
        );
        assert_eq!(
            rules[1].actions,
            vec![
                RuleAction::Tag {
                    tag_id: "uzak-seyahat".into()
                },
                RuleAction::Move {
                    mailbox_id: "a:Seyahat".into()
                },
            ]
        );
        assert_eq!(out.report.rules, 2);
        assert_eq!(out.report.rules_skipped, 1);
        assert_eq!(out.status, ApplyStatus::Partial);

        // Etiketler seçilmeyince eklenmez ama ad eşlemesi yine kurulur.
        let side2 = Side::new();
        side2
            .store
            .tag_insert(&tag("yerel-fatura", "Fatura", TAG_COLORS[0]))
            .unwrap();
        let mut p2 = package(Vec::new());
        p2.tags = vec![tag("uzak-fatura", "fatura", TAG_COLORS[3])];
        p2.rules = vec![rule(
            "r1",
            None,
            vec![RuleAction::Tag {
                tag_id: "uzak-fatura".into(),
            }],
        )];
        let out = side2
            .apply(
                p2,
                &TransferChoice {
                    accounts: vec![],
                    rules: true,
                    tags: false,
                    settings: false,
                },
            )
            .await;
        assert_eq!(out.report.tags, 0);
        assert_eq!(side2.store.tags().unwrap().len(), 1);
        assert_eq!(
            side2.store.rules().unwrap()[0].actions,
            vec![RuleAction::Tag {
                tag_id: "yerel-fatura".into()
            }]
        );
        assert_eq!(out.status, ApplyStatus::Applied);
    }

    #[tokio::test]
    async fn hesabi_olmayan_kural_atlanir() {
        let side = Side::new();
        let mut p = package(vec![
            with_secret(
                account("a", AuthKind::Password),
                Some((SecretKind::Password, "p")),
            ),
            with_secret(
                account("b", AuthKind::Password),
                Some((SecretKind::Password, "p")),
            ),
        ]);
        p.rules = vec![
            rule("r1", Some("yok"), vec![RuleAction::Archive]),
            rule("r2", None, vec![RuleAction::MarkRead]),
            rule("r3", Some("a"), vec![RuleAction::Star]),
            // Pakette var ama seçilmedi: bu cihazda yok sayılır.
            rule("r4", Some("b"), vec![RuleAction::Mute]),
            // Koşulsuz kural geçersiz.
            Rule {
                conditions: vec![],
                ..rule("r5", None, vec![RuleAction::Trash])
            },
        ];
        let mut choice = all(&p);
        choice.accounts = vec!["a".into()];
        let out = side.apply(p, &choice).await;
        assert_eq!(out.report.rules, 2);
        assert_eq!(out.report.rules_skipped, 3);
        let ids: Vec<String> = side
            .store
            .rules()
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["r2", "r3"]);
        assert_eq!(out.status, ApplyStatus::Partial);

        // Var olan kural yerinde güncellenir, sırası korunur.
        let mut p2 = package(Vec::new());
        p2.rules = vec![Rule {
            name: "Yeni ad".into(),
            ..rule("r2", None, vec![RuleAction::Archive])
        }];
        let out = side
            .apply(
                p2,
                &TransferChoice {
                    accounts: vec![],
                    rules: true,
                    tags: false,
                    settings: false,
                },
            )
            .await;
        assert_eq!(out.report.rules, 1);
        let rules = side.store.rules().unwrap();
        assert_eq!(rules[0].id, "r2");
        assert_eq!(rules[0].name, "Yeni ad");
        assert_eq!(rules[0].actions, vec![RuleAction::Archive]);
        assert_eq!(rules.len(), 2);
    }

    #[tokio::test]
    async fn cihaza_ozgu_ayarlar_korunur() {
        let local = Settings {
            card_position: CardPosition::TopLeft,
            card_seconds: 20,
            native_notifications: true,
            auto_update: false,
            theme: Theme::Light,
            trusted_certs: vec![mh_core::TrustedCert {
                host: "imap.ornek.com".into(),
                port: 993,
                sha256: fingerprint("AA"),
            }],
            ..Settings::default()
        };
        let incoming = Settings {
            card_position: CardPosition::BottomRight,
            card_seconds: 5,
            native_notifications: false,
            auto_update: true,
            theme: Theme::Dark,
            density: Density::Compact,
            vip: vec!["ayse@ornek.com".into()],
            trusted_certs: vec![mh_core::TrustedCert {
                host: "baska.ornek.com".into(),
                port: 993,
                sha256: fingerprint("BB"),
            }],
            ..Settings::default()
        };
        let merged = merge_settings(&local, incoming.clone());
        assert_eq!(merged.card_position, CardPosition::TopLeft);
        assert_eq!(merged.card_seconds, 20);
        assert!(merged.native_notifications);
        assert!(!merged.auto_update);
        assert_eq!(merged.trusted_certs, local.trusted_certs);
        assert_eq!(merged.theme, Theme::Dark);
        assert_eq!(merged.density, Density::Compact);
        assert_eq!(merged.vip, incoming.vip);

        // Uygulama yoluyla: depoya yazılan birleşik ve sadeleştirilmiş ayar.
        let side = Side::new();
        side.store.set_settings(&local).unwrap();
        let mut p = package(Vec::new());
        p.settings = Some(Settings {
            vip: vec![" Ayse@Ornek.com".into(), "ayse@ornek.com".into()],
            ..incoming
        });
        let out = side.apply(p, &all(&package(Vec::new()))).await;
        assert!(out.report.settings);
        assert_eq!(out.status, ApplyStatus::Applied);
        let saved = side.store.settings().unwrap();
        assert_eq!(saved.theme, Theme::Dark);
        assert_eq!(saved.card_seconds, 20);
        assert_eq!(saved.vip, vec!["ayse@ornek.com".to_string()]);
        assert_eq!(saved.trusted_certs, local.trusted_certs);

        // Ayar seçilmezse dokunulmaz.
        let side2 = Side::new();
        side2.store.set_settings(&local).unwrap();
        let mut p = package(Vec::new());
        p.settings = Some(Settings {
            theme: Theme::Dark,
            ..Settings::default()
        });
        let out = side2
            .apply(
                p,
                &TransferChoice {
                    accounts: vec![],
                    rules: false,
                    tags: false,
                    settings: false,
                },
            )
            .await;
        assert!(!out.report.settings);
        assert_eq!(side2.store.settings().unwrap().theme, Theme::Light);
    }

    #[tokio::test]
    async fn parolasiz_hesap_raporlanir() {
        let side = Side::new();
        // Yerelde gizli değeri olan hesap: pakette parola yoksa korunur.
        side.store
            .insert_account(&account("a", AuthKind::Password))
            .unwrap();
        side.secrets.set("a", "yerel").unwrap();
        let p = package(vec![
            with_secret(account("a", AuthKind::Password), None),
            with_secret(account("c", AuthKind::Password), None),
            // OAuth hesabına parola türünde gizli değer: uyumsuz, yok sayılır.
            with_secret(
                account("g", AuthKind::Google),
                Some((SecretKind::Password, "yanlis-tur")),
            ),
            with_secret(
                account("m", AuthKind::Microsoft),
                Some((SecretKind::OauthRefresh, "yenileme-m")),
            ),
            // Yerelde parolası olmayan var olan hesap da raporlanır.
            with_secret(account("e", AuthKind::Password), None),
        ]);
        side.store
            .insert_account(&account("e", AuthKind::Password))
            .unwrap();
        let out = side.apply(p, &all(&package(Vec::new()))).await;
        // Hiçbir hesap seçilmedi: boş seçim.
        assert_eq!(out.report.accounts_added + out.report.accounts_updated, 0);

        let p = package(vec![
            with_secret(account("a", AuthKind::Password), None),
            with_secret(account("c", AuthKind::Password), None),
            with_secret(
                account("g", AuthKind::Google),
                Some((SecretKind::Password, "yanlis-tur")),
            ),
            with_secret(
                account("m", AuthKind::Microsoft),
                Some((SecretKind::OauthRefresh, "yenileme-m")),
            ),
            with_secret(account("e", AuthKind::Password), None),
        ]);
        let choice = all(&p);
        let out = side.apply(p, &choice).await;
        assert_eq!(out.report.accounts_updated, 2);
        assert_eq!(out.report.accounts_added, 3);
        assert_eq!(
            out.report.accounts_without_secret,
            vec!["c@ornek.com", "g@ornek.com", "e@ornek.com"]
        );
        assert_eq!(out.status, ApplyStatus::Partial);
        assert_eq!(side.secrets.get("a").unwrap().as_deref(), Some("yerel"));
        assert_eq!(side.secrets.get("g").unwrap(), None);
        assert_eq!(side.secrets.get("oauth:g").unwrap(), None);
        assert_eq!(
            side.secrets.get("oauth:m").unwrap().as_deref(),
            Some("yenileme-m")
        );
        assert_eq!(side.started.ids(), vec!["a", "c", "g", "m", "e"]);
    }

    #[tokio::test]
    async fn gecersiz_hesap_atlanir() {
        let side = Side::new();
        let mut no_smtp = account("s", AuthKind::Password);
        no_smtp.smtp = None;
        let mut bad_mail = account("b", AuthKind::Password);
        bad_mail.email = "bozuk".into();
        let mut jmap_oauth = jmap_account("j");
        jmap_oauth.auth = AuthKind::Google;
        let mut ok_jmap = jmap_account("k");
        ok_jmap.name = "".into();
        ok_jmap.display_name = " ".into();
        ok_jmap.color = "mavi".into();
        let p = package(vec![
            with_secret(no_smtp, None),
            with_secret(bad_mail, None),
            with_secret(jmap_oauth, None),
            TransferAccount {
                jmap_username: Some(" furkan ".into()),
                ..with_secret(ok_jmap, Some((SecretKind::Password, "p")))
            },
        ]);
        let choice = all(&p);
        let out = side.apply(p, &choice).await;
        assert_eq!(out.report.accounts_added, 1);
        assert_eq!(out.status, ApplyStatus::Partial);
        let k = side.store.account("k").unwrap().unwrap();
        assert_eq!(k.name, "ornek.com");
        assert_eq!(k.display_name, "k");
        assert_eq!(k.color, crate::commands::PALETTE[3]);
        assert_eq!(k.jmap_url.as_deref(), Some("https://posta.ornek.com/jmap"));
        assert_eq!(
            side.store
                .jmap_account_state("k")
                .unwrap()
                .username
                .as_deref(),
            Some("furkan")
        );
        assert_eq!(side.started.ids(), vec!["k"]);
    }

    #[test]
    fn onizleme_gizli_deger_icermez() {
        let s = Store::open_in_memory().unwrap();
        s.insert_account(&account("a", AuthKind::Password)).unwrap();
        let mut p = package(vec![
            with_secret(
                account("a", AuthKind::Password),
                Some((SecretKind::Password, "cok-gizli")),
            ),
            with_secret(account("g", AuthKind::Google), None),
            with_secret(
                account("h", AuthKind::Google),
                Some((SecretKind::Password, "uyumsuz")),
            ),
        ]);
        p.tags = vec![tag("t", "Fatura", TAG_COLORS[0])];
        p.rules = vec![rule("r", None, vec![RuleAction::Star])];
        let pv = preview("on-1", &p, &s).unwrap();
        assert_eq!(pv.preview_id, "on-1");
        assert_eq!(pv.from_version, "0.4.0");
        assert_eq!(pv.from_platform, "linux");
        assert_eq!(pv.created_at, 5);
        assert_eq!((pv.rules, pv.tags, pv.settings), (1, 1, true));
        assert_eq!(pv.accounts.len(), 3);
        assert!(pv.accounts[0].exists && pv.accounts[0].has_secret);
        assert!(!pv.accounts[1].exists && !pv.accounts[1].has_secret);
        assert!(
            !pv.accounts[2].has_secret,
            "uyumsuz tür gizli değer sayılmaz"
        );
        assert_eq!(pv.accounts[1].auth, AuthKind::Google);
        let json = serde_json::to_string(&pv).unwrap();
        assert!(!json.contains("cok-gizli") && !json.contains("uyumsuz"));
    }

    #[test]
    fn bekleyen_paket_tek_kullanimlik_ve_sureli() {
        let hub = TransferHub::default();
        let code = || Code::generate(vec!["192.168.1.2".parse().unwrap()], 5000, false).unwrap();
        let t0 = Instant::now();
        hub.insert_preview_at("a".into(), package(Vec::new()), code(), t0);
        hub.insert_preview_at("b".into(), package(Vec::new()), code(), t0);
        assert_eq!(hub.pending_len(), 2);
        assert!(hub
            .take_preview_at("a", t0 + Duration::from_secs(60))
            .is_some());
        assert!(hub
            .take_preview_at("a", t0 + Duration::from_secs(60))
            .is_none());
        assert!(hub.take_preview_at("yok", t0).is_none());
        // Süresi dolan paket alınamaz ve düşer.
        assert!(hub.take_preview_at("b", t0 + PREVIEW_TTL).is_none());
        assert_eq!(hub.pending_len(), 0);
        // Teklif yokken iptal sessiz.
        assert!(!hub.cancel_offer());
    }

    #[tokio::test]
    async fn uctan_uca_loopback() {
        // Bilgisayar: hesaplar, ayarlar, kurallar, etiketler.
        let pc = Side::new();
        pc.store
            .insert_account(&account("a", AuthKind::Password))
            .unwrap();
        pc.store.insert_account(&jmap_account("j")).unwrap();
        pc.store.set_jmap_username("j", "furkan").unwrap();
        pc.store
            .insert_account(&account("g", AuthKind::Google))
            .unwrap();
        pc.secrets.set("a", "parola-a").unwrap();
        pc.secrets.set("j", "parola-j").unwrap();
        pc.secrets.set("oauth:g", "yenileme-g").unwrap();
        pc.store
            .tag_insert(&tag("t-fatura", "Fatura", TAG_COLORS[2]))
            .unwrap();
        pc.store
            .rule_insert(&rule(
                "r-etiket",
                Some("a"),
                vec![RuleAction::Tag {
                    tag_id: "t-fatura".into(),
                }],
            ))
            .unwrap();
        pc.store
            .set_settings(&Settings {
                theme: Theme::Dark,
                card_seconds: 30,
                ..Settings::default()
            })
            .unwrap();
        let package = build_package(&pc.store, pc.secrets.as_ref(), true).unwrap();
        let offer = open_offer(&package, OfferOptions::loopback())
            .await
            .unwrap();
        drop(package);
        let (info, handle, mut events) = offer.into_parts();
        assert_eq!(info.hosts, vec!["127.0.0.1"]);
        assert_eq!(
            events.recv().await.unwrap().state,
            TransferStateKind::Waiting
        );

        // Telefon: kodu çöz, paketi al, önizle.
        let copts = ClientOptions {
            allow_loopback: true,
            ..ClientOptions::default()
        };
        let code = Code::parse(&info.text).unwrap();
        let got = fetch_package(&code, &copts).await.unwrap();
        let phone = Side::new();
        phone
            .store
            .tag_insert(&tag("yerel-fatura", "fatura", TAG_COLORS[0]))
            .unwrap();
        let hub = TransferHub::default();
        hub.insert_preview("on".into(), got.clone(), code);
        let pv = preview("on", &got, &phone.store).unwrap();
        assert_eq!(pv.accounts.len(), 3);
        assert!(pv.accounts.iter().all(|a| a.has_secret && !a.exists));
        assert_eq!(
            events.recv().await.unwrap().state,
            TransferStateKind::Connected
        );
        assert_eq!(
            events.recv().await.unwrap().state,
            TransferStateKind::Delivered
        );

        // Uygula ve onayla.
        let (package, code) = hub.take_preview("on").unwrap();
        let choice = TransferChoice {
            accounts: vec!["a".into(), "j".into(), "g".into()],
            rules: true,
            tags: true,
            settings: true,
        };
        let out = phone.apply(package, &choice).await;
        assert_eq!(out.status, ApplyStatus::Applied);
        assert_eq!(
            out.report,
            TransferReport {
                accounts_added: 3,
                accounts_updated: 0,
                accounts_without_secret: vec![],
                rules: 1,
                rules_skipped: 0,
                tags: 0,
                settings: true,
            }
        );
        mh_transfer::confirm(&code, out.status, &copts)
            .await
            .unwrap();
        let last = events.recv().await.unwrap();
        assert_eq!(last.state, TransferStateKind::Applied);
        assert!(events.recv().await.is_none());
        drop(handle);

        // Telefonun deposu ve anahtar zinciri.
        assert_eq!(phone.secrets.get("a").unwrap().as_deref(), Some("parola-a"));
        assert_eq!(phone.secrets.get("j").unwrap().as_deref(), Some("parola-j"));
        assert_eq!(
            phone.secrets.get("oauth:g").unwrap().as_deref(),
            Some("yenileme-g")
        );
        assert_eq!(phone.store.accounts().unwrap().len(), 3);
        assert_eq!(
            phone
                .store
                .jmap_account_state("j")
                .unwrap()
                .username
                .as_deref(),
            Some("furkan")
        );
        let rules = phone.store.rules().unwrap();
        assert_eq!(
            rules[0].actions,
            vec![RuleAction::Tag {
                tag_id: "yerel-fatura".into()
            }]
        );
        let settings = phone.store.settings().unwrap();
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(settings.card_seconds, 8, "cihaza özgü ayar yerelde kaldı");
        assert_eq!(phone.started.ids(), vec!["a", "j", "g"]);
    }
}
