//! Firebase Cloud Messaging HTTP v1 gönderici.
//!
//! Hizmet hesabıyla RS256 imzalı JWT kurulur, Google'dan bir saatlik erişim
//! jetonu alınır (önbellekte tutulur, bitmesine 5 dk kala yenilenir) ve
//! iletiler `projects/{proje}/messages:send` ile yollanır. İleti yalnız veri
//! taşır (bildirim değil): telefon postayı kendisi indirip bildirimi kendisi
//! gösterir. Öncelik yüksek: Android uykudaki uygulamayı hemen uyandırır.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

const KAPSAM: &str = "https://www.googleapis.com/auth/firebase.messaging";
const ZAMAN_ASIMI: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
pub struct HizmetHesabi {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    #[serde(default = "varsayilan_token_uri")]
    pub token_uri: String,
}

fn varsayilan_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_owned()
}

impl std::fmt::Debug for HizmetHesabi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HizmetHesabi")
            .field("project_id", &self.project_id)
            .field("client_email", &self.client_email)
            .finish_non_exhaustive()
    }
}

/// PEM içindeki PKCS#8 özel anahtarın DER baytları.
fn pem_der(pem: &str) -> Option<Vec<u8>> {
    let govde: String = pem
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("-----"))
        .collect();
    STANDARD.decode(govde).ok()
}

/// RS256 imzalı JWT (`başlık.yük.imza`).
pub fn jwt_imzala(anahtar: &RsaKeyPair, yuk: &serde_json::Value) -> Result<String, String> {
    let baslik = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let govde = URL_SAFE_NO_PAD.encode(serde_json::to_vec(yuk).map_err(|e| e.to_string())?);
    let imzalanan = format!("{baslik}.{govde}");
    let mut imza = vec![0u8; anahtar.public().modulus_len()];
    anahtar
        .sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            imzalanan.as_bytes(),
            &mut imza,
        )
        .map_err(|_| "JWT imzalanamadı".to_owned())?;
    Ok(format!("{imzalanan}.{}", URL_SAFE_NO_PAD.encode(imza)))
}

/// Gönderim sonucu.
#[derive(Debug, PartialEq, Eq)]
pub enum Sonuc {
    Gitti,
    /// Jeton artık geçersiz (uygulama kaldırıldı, veri silindi): abone düşer.
    Kayitsiz,
    /// Geçici ya da yapılandırma hatası (günlüğe yazılır, abone kalır).
    Hata(String),
}

/// HTTP durumu ve hata gövdesinden sonuç: 404 ya da `UNREGISTERED` kayıtsız jeton.
pub fn sonuc_coz(durum: u16, govde: &str) -> Sonuc {
    if (200..300).contains(&durum) {
        return Sonuc::Gitti;
    }
    if durum == 404 || govde.contains("UNREGISTERED") {
        return Sonuc::Kayitsiz;
    }
    if durum == 400 && govde.contains("registration token is not a valid") {
        return Sonuc::Kayitsiz;
    }
    Sonuc::Hata(format!("FCM {durum}"))
}

/// Veri iletisi gövdesi: yüksek öncelik, 10 dk yaşam (cihaz kapalıysa sonra gelir).
pub fn ileti(fcm: &str, veri: &BTreeMap<String, String>) -> serde_json::Value {
    json!({
        "message": {
            "token": fcm,
            "data": veri,
            "android": { "priority": "HIGH", "ttl": "600s" }
        }
    })
}

struct Jeton {
    deger: String,
    bitis: Instant,
}

pub struct Gonderici {
    hesap: HizmetHesabi,
    anahtar: RsaKeyPair,
    http: reqwest::Client,
    jeton: Mutex<Option<Jeton>>,
}

impl Gonderici {
    pub fn yeni(hesap: HizmetHesabi) -> Result<Self, String> {
        let der = pem_der(&hesap.private_key).ok_or("hizmet hesabı anahtarı PEM değil")?;
        let anahtar =
            RsaKeyPair::from_pkcs8(&der).map_err(|_| "hizmet hesabı anahtarı okunamadı")?;
        let http = reqwest::Client::builder()
            .timeout(ZAMAN_ASIMI)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            hesap,
            anahtar,
            http,
            jeton: Mutex::new(None),
        })
    }

    pub fn proje(&self) -> &str {
        &self.hesap.project_id
    }

    async fn erisim_jetonu(&self) -> Result<String, String> {
        let mut kilit = self.jeton.lock().await;
        if let Some(j) = kilit.as_ref() {
            if j.bitis > Instant::now() + Duration::from_secs(300) {
                return Ok(j.deger.clone());
            }
        }
        let simdi = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let assertion = jwt_imzala(
            &self.anahtar,
            &json!({
                "iss": self.hesap.client_email,
                "scope": KAPSAM,
                "aud": self.hesap.token_uri,
                "iat": simdi,
                "exp": simdi + 3600,
            }),
        )?;
        let govde = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
        );
        let yanit = self
            .http
            .post(&self.hesap.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(govde)
            .send()
            .await
            .map_err(|e| format!("Google jetonu alınamadı: {e}"))?;
        let durum = yanit.status();
        let metin = yanit.text().await.unwrap_or_default();
        if !durum.is_success() {
            return Err(format!("Google jetonu reddetti: {durum}"));
        }
        #[derive(Deserialize)]
        struct Yanit {
            access_token: String,
            expires_in: u64,
        }
        let y: Yanit = serde_json::from_str(&metin).map_err(|e| e.to_string())?;
        let deger = y.access_token.clone();
        *kilit = Some(Jeton {
            deger: y.access_token,
            bitis: Instant::now() + Duration::from_secs(y.expires_in),
        });
        Ok(deger)
    }

    pub async fn gonder(&self, fcm: &str, veri: &BTreeMap<String, String>) -> Sonuc {
        let jeton = match self.erisim_jetonu().await {
            Ok(j) => j,
            Err(e) => return Sonuc::Hata(e),
        };
        let adres = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.hesap.project_id
        );
        let sonuc = self
            .http
            .post(adres)
            .bearer_auth(jeton)
            .header("content-type", "application/json")
            .body(ileti(fcm, veri).to_string())
            .send()
            .await;
        match sonuc {
            Ok(y) => {
                let durum = y.status().as_u16();
                let govde = y.text().await.unwrap_or_default();
                sonuc_coz(durum, &govde)
            }
            Err(e) => Sonuc::Hata(format!("FCM'e ulaşılamadı: {e}")),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ring::signature::{UnparsedPublicKey, RSA_PKCS1_2048_8192_SHA256};

    /// Yalnız test için üretilmiş 2048 bit anahtar (PKCS#8, PEM).
    pub(crate) const TEST_PEM: &str = include_str!("../testdata/test-anahtar.pem");

    #[test]
    fn jwt_imzasi_acik_anahtarla_dogrulanir() {
        let der = pem_der(TEST_PEM).unwrap();
        let k = RsaKeyPair::from_pkcs8(&der).unwrap();
        let jwt = jwt_imzala(&k, &json!({"iss": "a@b", "exp": 1})).unwrap();
        let parca: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parca.len(), 3);
        let baslik = URL_SAFE_NO_PAD.decode(parca[0]).unwrap();
        assert_eq!(baslik, br#"{"alg":"RS256","typ":"JWT"}"#);
        let imza = URL_SAFE_NO_PAD.decode(parca[2]).unwrap();
        let acik = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, k.public().as_ref());
        acik.verify(format!("{}.{}", parca[0], parca[1]).as_bytes(), &imza)
            .expect("imza geçerli");
    }

    #[test]
    fn sonuclar() {
        assert_eq!(sonuc_coz(200, "{}"), Sonuc::Gitti);
        assert_eq!(sonuc_coz(404, ""), Sonuc::Kayitsiz);
        assert_eq!(
            sonuc_coz(
                400,
                r#"{"error":{"details":[{"errorCode":"UNREGISTERED"}]}}"#
            ),
            Sonuc::Kayitsiz
        );
        assert!(matches!(sonuc_coz(503, ""), Sonuc::Hata(_)));
        assert!(matches!(sonuc_coz(401, ""), Sonuc::Hata(_)));
    }

    #[test]
    fn ileti_yalniz_veri_ve_yuksek_oncelik() {
        let mut v = BTreeMap::new();
        v.insert("t".to_owned(), "degisti".to_owned());
        let m = ileti("jeton", &v);
        assert_eq!(m["message"]["token"], "jeton");
        assert_eq!(m["message"]["data"]["t"], "degisti");
        assert_eq!(m["message"]["android"]["priority"], "HIGH");
        assert!(m["message"].get("notification").is_none());
    }

    #[test]
    fn hizmet_hesabi_ozel_anahtari_debugda_gorunmez() {
        let h: HizmetHesabi = serde_json::from_value(json!({
            "project_id": "p", "client_email": "e@p", "private_key": "GIZLI"
        }))
        .unwrap();
        assert!(!format!("{h:?}").contains("GIZLI"));
        assert_eq!(h.token_uri, "https://oauth2.googleapis.com/token");
    }
}
