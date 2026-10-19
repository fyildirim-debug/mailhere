//! Sunucu itmesi (push): posta sunucusu yeni postayı Mailhere bildirim
//! aktarıcısına (`crates/mh-relay`), aktarıcı Firebase ile telefona haber
//! verir; telefon uykudayken de hemen uyanıp eşitler. Ayrıntı: `docs/PUSH.md`.
//!
//! Bu modül platformdan bağımsızdır: Firebase kayıt jetonunu kabuk kasaya
//! [`TOKEN_KEY`] adıyla yazar (yalnız Android'de vardır; masaüstünde jeton
//! olmadığından her şey sessizce boş geçer). Çekirdek açılınca ve 6 saatte
//! bir [`refresh`] aboneleri düzenler:
//!
//! 1. Jeton değiştiyse aktarıcıdaki aboneler yeni jetona taşınır.
//! 2. Silinen hesabın abonesi aktarıcıdan silinir.
//! 3. Her JMAP hesabı için aktarıcıda abone açılır ve posta sunucusunda
//!    `PushSubscription` kurulur (yalnız `EmailDelivery`: okundu/taşındı gibi
//!    değişiklikler telefonu uyandırmaz). Sunucu doğrulama kodunu aktarıcı
//!    üzerinden yollar; [`handle`] kodu sunucuya geri verir.
//! 4. Bitmesine 2 günden az kalan abonelik 7 gün uzatılır; aktarıcıya haftada
//!    bir canlılık bildirilir (30 gün sessiz abone orada silinir).
//!
//! Sağlayıcılar: şimdilik yalnız JMAP (Stalwart) açık. Gmail (`users.watch`)
//! ve Microsoft Graph (abonelik) için aktarıcı uçları hazırdır; uygulama
//! tarafı [`Saglayici`] ile genişler (OAuth izinleri gelince).
//!
//! Gizlilik: aktarıcıya ne parola ne posta adresi (JMAP'te) gider; aktarıcı
//! yalnız "şu abone değişti" bilir. Yönetim anahtarı ve durum kasada
//! ([`STATE_KEY`]) durur, günlüğe yazılmaz.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use mh_core::{Account, AccountKind, AppError, AppResult, ErrorCode};
use mh_jmap::JmapManager;
use mh_store::Store;
use mh_sync::SecretStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::{now_ms, AppState};

/// Varsayılan aktarıcı; geliştirmede `MAILHERE_RELAY_URL` ile değişir.
pub const RELAY_URL: &str = "https://mailhere-push.arnexlab.com";
/// Kabuğun Firebase kayıt jetonunu yazdığı kasa adı.
pub const TOKEN_KEY: &str = "push-jeton";
/// Abonelerin durumu (kasada; aktarıcı yönetim anahtarlarını taşır).
pub const STATE_KEY: &str = "push-durum";
/// Çekirdek yokken gelen doğrulama iletisi (kabuk yazar, düzenleme uygular).
pub const PENDING_KEY: &str = "push-bekleyen";

const GUN_MS: i64 = 24 * 3600 * 1000;
/// İstenen abonelik süresi (sunucu kısaltabilir).
const SURE_MS: i64 = 7 * GUN_MS;
/// Bundan az kalınca uzatılır.
const UZAT_ESIGI_MS: i64 = 2 * GUN_MS;
/// Aktarıcıya canlılık aralığı.
const CANLILIK_MS: i64 = 6 * GUN_MS;
/// Düzenleme aralığı (çekirdek açıkken).
const ARALIK: Duration = Duration::from_secs(6 * 3600);
const HTTP_SURE: Duration = Duration::from_secs(15);

/// Sunucu itmesi sağlayıcısı (aktarıcıdaki adlarla).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saglayici {
    Jmap,
    Gmail,
    Graph,
}

impl Saglayici {
    pub fn ad(self) -> &'static str {
        match self {
            Self::Jmap => "jmap",
            Self::Gmail => "gmail",
            Self::Graph => "graph",
        }
    }
}

/// Hesabın itme sağlayıcısı; yalnız etkin olanlar. Gmail ve Graph, OAuth
/// izinleri (Gmail API `users.watch`, Graph `Mail.ReadBasic`) eklenince
/// burada açılır.
pub fn saglayici(account: &Account) -> Option<Saglayici> {
    match account.kind {
        AccountKind::Jmap if mh_jmap::is_syncable(account) => Some(Saglayici::Jmap),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Durum {
    /// Abonelerin bağlı olduğu Firebase jetonu.
    #[serde(default)]
    pub jeton: Option<String>,
    #[serde(default)]
    pub hesaplar: BTreeMap<String, Abone>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Abone {
    pub saglayici: String,
    pub relay_id: String,
    pub relay_gizli: String,
    /// Sağlayıcıya verilen adres (aktarıcının bu aboneye ayırdığı uç).
    pub url: String,
    /// Posta sunucusundaki `PushSubscription` kimliği.
    #[serde(default)]
    pub uzak_id: Option<String>,
    /// Aboneliğin bitişi (ms).
    #[serde(default)]
    pub bitis: Option<i64>,
    #[serde(default)]
    pub dogrulandi: bool,
    /// Aktarıcıya son canlılık (ms).
    #[serde(default)]
    pub canlilik: i64,
}

/// Bir hesap için yapılacak iş.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Is {
    /// Aktarıcıda abone yok: aç, sonra sunucuda abonelik kur.
    Ac(String),
    /// Aktarıcıda abone var, sunucuda abonelik yok: kur.
    Kur(String),
    /// Abonelik bitmek üzere: uzat.
    Uzat(String),
    /// Aktarıcıya canlılık bildir.
    Canlilik(String),
    /// Hesap gitti: aktarıcıdan sil.
    Sil(String),
}

/// Saf karar: durum, itme açılabilen hesaplar ve şimdiki zamandan yapılacak işler.
pub fn planla(d: &Durum, hesaplar: &[String], simdi: i64) -> Vec<Is> {
    let mut isler = Vec::new();
    for id in d.hesaplar.keys() {
        if !hesaplar.contains(id) {
            isler.push(Is::Sil(id.clone()));
        }
    }
    for id in hesaplar {
        match d.hesaplar.get(id) {
            None => isler.push(Is::Ac(id.clone())),
            Some(a) => {
                match (&a.uzak_id, a.bitis) {
                    (None, _) => isler.push(Is::Kur(id.clone())),
                    (Some(_), Some(b)) if b - simdi < UZAT_ESIGI_MS => {
                        isler.push(Is::Uzat(id.clone()))
                    }
                    (Some(_), None) => isler.push(Is::Uzat(id.clone())),
                    _ => {}
                }
                if simdi - a.canlilik >= CANLILIK_MS {
                    isler.push(Is::Canlilik(id.clone()));
                }
            }
        }
    }
    isler
}

/// Aktarıcının telefona yolladığı iletiler (Firebase veri iletisi).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ileti {
    /// Sunucu aboneliği doğrulama kodu yolladı.
    Dogrula {
        abone: String,
        abonelik: String,
        kod: String,
    },
    /// Posta kutusunda değişiklik: eşitle.
    Degisti { abone: String },
    /// Firebase jetonu yenilendi (kabuk yollar).
    Jeton,
}

/// Kabuğun verdiği JSON (Firebase `data` alanları ya da `{"t":"jeton"}`).
pub fn ileti_coz(json: &str) -> Option<Ileti> {
    let v: Value = serde_json::from_str(json).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    match s("t")?.as_str() {
        "dogrula" => Some(Ileti::Dogrula {
            abone: s("a")?,
            abonelik: s("s")?,
            kod: s("k")?,
        }),
        "degisti" => Some(Ileti::Degisti { abone: s("a")? }),
        "jeton" => Some(Ileti::Jeton),
        _ => None,
    }
}

/// JMAP UTCDate (RFC 3339, saniye, `Z`).
fn utc_tarih(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn utc_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// `PushSubscription/set` yanıtındaki sonuç.
#[derive(Debug, PartialEq)]
pub enum SetSonuc {
    Olustu { id: String, bitis: Option<i64> },
    Guncellendi { bitis: Option<i64> },
    Yok,
    Hata(String),
}

/// `create` yanıtı (`k` anahtarıyla).
pub fn olusturma_coz(v: &Value, k: &str) -> SetSonuc {
    if let Some(c) = v.get("created").and_then(|c| c.get(k)) {
        if let Some(id) = c.get("id").and_then(Value::as_str) {
            return SetSonuc::Olustu {
                id: id.to_owned(),
                bitis: c.get("expires").and_then(Value::as_str).and_then(utc_ms),
            };
        }
    }
    hata_coz(v.get("notCreated").and_then(|n| n.get(k)))
}

/// `update` yanıtı (abonelik kimliğiyle).
pub fn guncelleme_coz(v: &Value, id: &str) -> SetSonuc {
    if let Some(u) = v.get("updated").and_then(|u| u.get(id)) {
        return SetSonuc::Guncellendi {
            bitis: u.get("expires").and_then(Value::as_str).and_then(utc_ms),
        };
    }
    hata_coz(v.get("notUpdated").and_then(|n| n.get(id)))
}

fn hata_coz(e: Option<&Value>) -> SetSonuc {
    match e.and_then(|e| e.get("type")).and_then(Value::as_str) {
        Some("notFound") => SetSonuc::Yok,
        Some(t) => SetSonuc::Hata(t.to_owned()),
        None => SetSonuc::Hata("yanıt yok".to_owned()),
    }
}

// ------------------------------------------------------------------ aktarıcı

fn relay_url() -> String {
    crate::state::dev_env("MAILHERE_RELAY_URL")
        .unwrap_or_else(|| RELAY_URL.to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(HTTP_SURE)
        .https_only(!cfg!(debug_assertions))
        .build()
        .unwrap_or_default()
}

fn ag_hatasi(e: impl std::fmt::Display) -> AppError {
    AppError::new(
        ErrorCode::Network,
        format!("Bildirim aktarıcısına ulaşılamadı: {e}"),
    )
}

#[derive(Deserialize)]
struct RelayKayit {
    id: String,
    gizli: String,
    url: String,
}

async fn relay_ac(fcm: &str, s: Saglayici) -> AppResult<RelayKayit> {
    let govde = json!({ "fcm": fcm, "saglayici": s.ad() }).to_string();
    let y = http()
        .post(format!("{}/v1/abone", relay_url()))
        .header("content-type", "application/json")
        .body(govde)
        .send()
        .await
        .map_err(ag_hatasi)?;
    let durum = y.status();
    let metin = y.text().await.map_err(ag_hatasi)?;
    if !durum.is_success() {
        return Err(ag_hatasi(format!("{durum}")));
    }
    serde_json::from_str(&metin).map_err(ag_hatasi)
}

/// Jeton güncelleme ya da canlılık. `Ok(false)`: aktarıcıda abone yok.
async fn relay_guncelle(a: &Abone, fcm: Option<&str>) -> AppResult<bool> {
    let govde = match fcm {
        Some(f) => json!({ "fcm": f }).to_string(),
        None => "{}".to_owned(),
    };
    let y = http()
        .put(format!("{}/v1/abone/{}", relay_url(), a.relay_id))
        .bearer_auth(&a.relay_gizli)
        .header("content-type", "application/json")
        .body(govde)
        .send()
        .await
        .map_err(ag_hatasi)?;
    match y.status().as_u16() {
        200..=299 => Ok(true),
        404 => Ok(false),
        s => Err(ag_hatasi(s)),
    }
}

async fn relay_sil(a: &Abone) {
    let r = http()
        .delete(format!("{}/v1/abone/{}", relay_url(), a.relay_id))
        .bearer_auth(&a.relay_gizli)
        .send()
        .await;
    if let Err(e) = r {
        tracing::debug!(error = %e, "aktarıcı abonesi silinemedi");
    }
}

// ------------------------------------------------------------------ çekirdek

/// Push'un kullandığı çekirdek parçaları (pencereli ve etkinliksiz çekirdekte aynı).
#[derive(Clone)]
pub struct Parcalar {
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
    pub jmap: Arc<JmapManager>,
}

impl Parcalar {
    pub fn of(state: &AppState) -> Self {
        Self {
            store: state.store.clone(),
            secrets: state.secrets.clone(),
            jmap: state.jmap.clone(),
        }
    }

    fn durum_oku(&self) -> Durum {
        match self.secrets.get(STATE_KEY) {
            Ok(Some(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Durum::default(),
        }
    }

    fn durum_yaz(&self, d: &Durum) {
        let metin = serde_json::to_string(d).unwrap_or_default();
        if let Err(e) = self.secrets.set(STATE_KEY, &metin) {
            tracing::warn!(error = %e.message, "itme durumu kaydedilemedi");
        }
    }

    fn jeton(&self) -> Option<String> {
        self.secrets
            .get(TOKEN_KEY)
            .ok()
            .flatten()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    }

    async fn abonelik_kur(&self, account: &Account, a: &mut Abone, simdi: i64) -> AppResult<()> {
        let mut turler = json!(["EmailDelivery"]);
        for deneme in 0..2 {
            let args = json!({
                "create": { "k": {
                    "deviceClientId": format!("mailhere:{}", a.relay_id),
                    "url": a.url,
                    "types": turler,
                    "expires": utc_tarih(simdi + SURE_MS),
                }}
            });
            let v = self
                .jmap
                .session_call(account, "PushSubscription/set", args)
                .await?;
            match olusturma_coz(&v, "k") {
                SetSonuc::Olustu { id, bitis } => {
                    a.uzak_id = Some(id);
                    a.bitis = Some(bitis.unwrap_or(simdi + SURE_MS));
                    a.dogrulandi = false;
                    return Ok(());
                }
                // Sunucu EmailDelivery türünü tanımıyorsa posta değişikliklerine düşülür.
                SetSonuc::Hata(t) if deneme == 0 && t == "invalidProperties" => {
                    turler = json!(["Email"]);
                }
                SetSonuc::Hata(t) => {
                    return Err(AppError::new(
                        ErrorCode::Internal,
                        format!("Sunucu bildirim aboneliğini kabul etmedi ({t})."),
                    ))
                }
                _ => break,
            }
        }
        Err(AppError::new(
            ErrorCode::Internal,
            "Sunucu bildirim aboneliğini kabul etmedi.",
        ))
    }

    /// Aboneliği uzatır; sunucuda yoksa yeniden kurar.
    async fn uzat(&self, account: &Account, a: &mut Abone, simdi: i64) -> AppResult<()> {
        let Some(id) = a.uzak_id.clone() else {
            return self.abonelik_kur(account, a, simdi).await;
        };
        let args = json!({ "update": { (id.clone()): { "expires": utc_tarih(simdi + SURE_MS) } } });
        let v = self
            .jmap
            .session_call(account, "PushSubscription/set", args)
            .await?;
        match guncelleme_coz(&v, &id) {
            SetSonuc::Guncellendi { bitis } => {
                a.bitis = Some(bitis.unwrap_or(simdi + SURE_MS));
                Ok(())
            }
            SetSonuc::Yok => {
                a.uzak_id = None;
                self.abonelik_kur(account, a, simdi).await
            }
            SetSonuc::Hata(t) => Err(AppError::new(
                ErrorCode::Internal,
                format!("Bildirim aboneliği uzatılamadı ({t})."),
            )),
            SetSonuc::Olustu { .. } => Ok(()),
        }
    }

    async fn abonelik_sil(&self, account: &Account, a: &Abone) {
        if let Some(id) = &a.uzak_id {
            let args = json!({ "destroy": [id] });
            if let Err(e) = self
                .jmap
                .session_call(account, "PushSubscription/set", args)
                .await
            {
                tracing::debug!(error = %e.message, "eski abonelik silinemedi");
            }
        }
    }

    /// Aboneleri düzenler (bkz. modül belgesi). Jeton yoksa hiçbir şey yapmaz.
    pub async fn refresh(&self) -> AppResult<()> {
        let _kilit = KILIT.lock().await;
        let Some(jeton) = self.jeton() else {
            return Ok(());
        };
        let mut d = self.durum_oku();
        let simdi = now_ms();

        // 0. Çekirdek kapalıyken gelmiş doğrulama kodu.
        if let Ok(Some(bekleyen)) = self.secrets.get(PENDING_KEY) {
            let _ = self.secrets.delete(PENDING_KEY);
            if let Some(Ileti::Dogrula {
                abone,
                abonelik,
                kod,
            }) = ileti_coz(&bekleyen)
            {
                if let Err(e) = self.dogrula(&mut d, &abone, abonelik, &kod).await {
                    tracing::warn!(error = %e.message, "bekleyen doğrulama uygulanamadı");
                }
                self.durum_yaz(&d);
            }
        }

        // 1. Jeton değişti: aboneler taşınır; aktarıcıda olmayan baştan açılır.
        if d.jeton.as_deref() != Some(jeton.as_str()) {
            let mut hepsi = true;
            let ids: Vec<String> = d.hesaplar.keys().cloned().collect();
            for id in ids {
                let a = d.hesaplar[&id].clone();
                match relay_guncelle(&a, Some(&jeton)).await {
                    Ok(true) => {
                        if let Some(a) = d.hesaplar.get_mut(&id) {
                            a.canlilik = simdi;
                        }
                    }
                    Ok(false) => {
                        d.hesaplar.remove(&id);
                    }
                    Err(e) => {
                        hepsi = false;
                        tracing::warn!(error = %e.message, "itme jetonu aktarıcıya taşınamadı");
                    }
                }
            }
            if hepsi {
                d.jeton = Some(jeton.clone());
            }
            self.durum_yaz(&d);
        }

        let hesaplar: Vec<Account> = self
            .store
            .accounts()?
            .into_iter()
            .filter(|a| saglayici(a).is_some())
            .collect();
        let ids: Vec<String> = hesaplar.iter().map(|a| a.id.clone()).collect();
        let bul = |id: &str| hesaplar.iter().find(|a| a.id == id);

        for is in planla(&d, &ids, simdi) {
            let sonuc: AppResult<()> = match &is {
                Is::Sil(id) => {
                    if let Some(a) = d.hesaplar.remove(id) {
                        relay_sil(&a).await;
                    }
                    Ok(())
                }
                Is::Ac(id) => {
                    let (Some(acc), Some(s)) = (bul(id), bul(id).and_then(saglayici)) else {
                        continue;
                    };
                    match relay_ac(&jeton, s).await {
                        Ok(k) => {
                            let mut a = Abone {
                                saglayici: s.ad().to_owned(),
                                relay_id: k.id,
                                relay_gizli: k.gizli,
                                url: k.url,
                                uzak_id: None,
                                bitis: None,
                                dogrulandi: false,
                                canlilik: simdi,
                            };
                            // Aktarıcı abonesi önce kaydedilir: kurulum yarıda kalırsa
                            // bir sonraki düzenleme yalnız sunucu aboneliğini kurar.
                            d.hesaplar.insert(id.clone(), a.clone());
                            d.jeton = Some(jeton.clone());
                            self.durum_yaz(&d);
                            let r = self.abonelik_kur(acc, &mut a, simdi).await;
                            d.hesaplar.insert(id.clone(), a);
                            r
                        }
                        Err(e) => Err(e),
                    }
                }
                Is::Kur(id) | Is::Uzat(id) => {
                    let (Some(acc), Some(mut a)) = (bul(id), d.hesaplar.get(id).cloned()) else {
                        continue;
                    };
                    let r = if matches!(is, Is::Kur(_)) {
                        self.abonelik_kur(acc, &mut a, simdi).await
                    } else {
                        self.uzat(acc, &mut a, simdi).await
                    };
                    d.hesaplar.insert(id.clone(), a);
                    r
                }
                Is::Canlilik(id) => {
                    let Some(a) = d.hesaplar.get(id).cloned() else {
                        continue;
                    };
                    match relay_guncelle(&a, None).await {
                        Ok(true) => {
                            if let Some(a) = d.hesaplar.get_mut(id) {
                                a.canlilik = simdi;
                            }
                            Ok(())
                        }
                        // Aktarıcı aboneyi unutmuş: eski abonelik silinir, sonraki
                        // düzenlemede baştan açılır.
                        Ok(false) => {
                            if let (Some(acc), Some(a)) = (bul(id), d.hesaplar.remove(id)) {
                                self.abonelik_sil(acc, &a).await;
                            }
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
            };
            if let Err(e) = sonuc {
                tracing::warn!(error = %e.message, "itme aboneliği düzenlenemedi");
            }
            self.durum_yaz(&d);
        }
        Ok(())
    }

    /// Sunucunun doğrulama kodunu aboneliğe verir (kilit çağırandadır).
    async fn dogrula(
        &self,
        d: &mut Durum,
        abone: &str,
        abonelik: String,
        kod: &str,
    ) -> AppResult<&'static str> {
        let Some((id, mut a)) = d
            .hesaplar
            .iter()
            .find(|(_, a)| a.relay_id == abone)
            .map(|(id, a)| (id.clone(), a.clone()))
        else {
            return Ok("abone yok");
        };
        let Some(acc) = self.store.accounts()?.into_iter().find(|x| x.id == id) else {
            return Ok("hesap yok");
        };
        let args = json!({ "update": { (abonelik.clone()): { "verificationCode": kod } } });
        let v = self
            .jmap
            .session_call(&acc, "PushSubscription/set", args)
            .await?;
        match guncelleme_coz(&v, &abonelik) {
            SetSonuc::Guncellendi { .. } => {
                a.uzak_id = Some(abonelik);
                a.dogrulandi = true;
                d.hesaplar.insert(id, a);
                tracing::info!("sunucu bildirim aboneliği doğrulandı");
                Ok("doğrulandı")
            }
            SetSonuc::Hata(t) => Err(AppError::new(
                ErrorCode::Internal,
                format!("Bildirim aboneliği doğrulanamadı ({t})."),
            )),
            _ => Ok("abonelik yok"),
        }
    }

    /// Kabuktan gelen ileti. Dönen değer günlük içindir.
    pub async fn handle(&self, json: &str) -> AppResult<&'static str> {
        let Some(ileti) = ileti_coz(json) else {
            return Ok("tanınmadı");
        };
        match ileti {
            Ileti::Jeton => {
                self.refresh().await?;
                Ok("jeton")
            }
            Ileti::Degisti { abone } => {
                let d = self.durum_oku();
                let hesap = d
                    .hesaplar
                    .iter()
                    .find(|(_, a)| a.relay_id == abone)
                    .map(|(id, _)| id.clone());
                match hesap {
                    Some(id) if self.jmap.sync_account(&id) => Ok("eşitleniyor"),
                    _ => {
                        self.jmap.sync_now();
                        Ok("tümü eşitleniyor")
                    }
                }
            }
            Ileti::Dogrula {
                abone,
                abonelik,
                kod,
            } => {
                let _kilit = KILIT.lock().await;
                let mut d = self.durum_oku();
                let r = self.dogrula(&mut d, &abone, abonelik, &kod).await;
                self.durum_yaz(&d);
                r
            }
        }
    }
}

/// Düzenleme ve doğrulama aynı anda çalışmasın (aynı abone iki kez açılmasın).
static KILIT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Çekirdek açıkken düzenleme döngüsü: hemen (tek seferlik çekirdek kısa
/// yaşar; JMAP oturumu eşitlemeden bağımsız açılır), sonra 6 saatte bir.
pub fn start(state: &AppState, rt: &tokio::runtime::Handle) -> tokio::task::JoinHandle<()> {
    let p = Parcalar::of(state);
    rt.spawn(async move {
        loop {
            if let Err(e) = p.refresh().await {
                tracing::warn!(error = %e.message, "itme aboneleri düzenlenemedi");
            }
            tokio::time::sleep(ARALIK).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abone(uzak: Option<&str>, bitis: Option<i64>, canlilik: i64) -> Abone {
        Abone {
            saglayici: "jmap".into(),
            relay_id: "r".into(),
            relay_gizli: "g".into(),
            url: "https://relay/v1/jmap/r/k".into(),
            uzak_id: uzak.map(str::to_owned),
            bitis,
            dogrulandi: true,
            canlilik,
        }
    }

    #[test]
    fn plan_ac_kur_uzat_canlilik_sil() {
        let simdi = 100 * GUN_MS;
        let mut d = Durum::default();
        d.hesaplar.insert(
            "taze".into(),
            abone(Some("S"), Some(simdi + 5 * GUN_MS), simdi),
        );
        d.hesaplar
            .insert("kurulmamis".into(), abone(None, None, simdi));
        d.hesaplar.insert(
            "bitiyor".into(),
            abone(Some("S"), Some(simdi + GUN_MS), simdi),
        );
        d.hesaplar.insert(
            "sessiz".into(),
            abone(Some("S"), Some(simdi + 5 * GUN_MS), simdi - CANLILIK_MS),
        );
        d.hesaplar.insert(
            "gitti".into(),
            abone(Some("S"), Some(simdi + 5 * GUN_MS), simdi),
        );
        let hesaplar: Vec<String> = ["taze", "kurulmamis", "bitiyor", "sessiz", "yeni"]
            .map(String::from)
            .to_vec();
        let mut p = planla(&d, &hesaplar, simdi);
        p.sort_by_key(|i| format!("{i:?}"));
        assert_eq!(
            p,
            vec![
                Is::Ac("yeni".into()),
                Is::Canlilik("sessiz".into()),
                Is::Kur("kurulmamis".into()),
                Is::Sil("gitti".into()),
                Is::Uzat("bitiyor".into()),
            ]
        );
    }

    #[test]
    fn iletiler() {
        assert_eq!(
            ileti_coz(r#"{"t":"dogrula","a":"R","s":"S","k":"K"}"#),
            Some(Ileti::Dogrula {
                abone: "R".into(),
                abonelik: "S".into(),
                kod: "K".into()
            })
        );
        assert_eq!(
            ileti_coz(r#"{"t":"degisti","a":"R"}"#),
            Some(Ileti::Degisti { abone: "R".into() })
        );
        assert_eq!(ileti_coz(r#"{"t":"jeton"}"#), Some(Ileti::Jeton));
        assert_eq!(ileti_coz(r#"{"t":"dogrula","a":"R"}"#), None);
        assert_eq!(ileti_coz("bozuk"), None);
    }

    #[test]
    fn set_yanitlari() {
        let v = json!({"created": {"k": {"id": "P1", "expires": "2026-10-26T10:00:00Z"}}});
        assert_eq!(
            olusturma_coz(&v, "k"),
            SetSonuc::Olustu {
                id: "P1".into(),
                bitis: utc_ms("2026-10-26T10:00:00Z")
            }
        );
        let v = json!({"notCreated": {"k": {"type": "invalidProperties"}}});
        assert_eq!(
            olusturma_coz(&v, "k"),
            SetSonuc::Hata("invalidProperties".into())
        );
        let v = json!({"updated": {"P1": null}});
        assert_eq!(
            guncelleme_coz(&v, "P1"),
            SetSonuc::Guncellendi { bitis: None }
        );
        let v = json!({"notUpdated": {"P1": {"type": "notFound"}}});
        assert_eq!(guncelleme_coz(&v, "P1"), SetSonuc::Yok);
        assert_eq!(
            guncelleme_coz(&json!({}), "P1"),
            SetSonuc::Hata("yanıt yok".into())
        );
    }

    #[test]
    fn utc_tarih_gidis_donus() {
        let ms = 1_792_400_000_000;
        assert_eq!(utc_ms(&utc_tarih(ms)), Some(ms));
        assert!(utc_tarih(ms).ends_with('Z'));
    }

    #[test]
    fn durum_kasada_eski_surumle_okunur() {
        // Alanı eksik kayıt da okunur (ileride alan eklenirse).
        let d: Durum = serde_json::from_str(
            r#"{"hesaplar":{"h":{"saglayici":"jmap","relay_id":"r","relay_gizli":"g","url":"u"}}}"#,
        )
        .unwrap();
        assert_eq!(d.hesaplar["h"].uzak_id, None);
        assert!(!d.hesaplar["h"].dogrulandi);
    }
}
