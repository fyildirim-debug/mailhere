//! HTTP uçları.
//!
//! | Uç | Kim çağırır | İş |
//! |---|---|---|
//! | `GET /saglik` | izleme | canlılık |
//! | `GET /v1/yapilandirma` | uygulama | Firebase istemci ayarları ve etkin sağlayıcılar |
//! | `POST /v1/abone` | uygulama | abone açar; yönetim anahtarı ve sağlayıcı adresi döner |
//! | `PUT /v1/abone/{id}` | uygulama | Firebase jetonu güncelleme / canlılık |
//! | `DELETE /v1/abone/{id}` | uygulama | aboneyi siler |
//! | `POST /v1/jmap/{id}/{anahtar}` | posta sunucusu (Stalwart) | JMAP itmesi |
//! | `POST /v1/graph/{id}` | Microsoft Graph | doğrulama ve değişiklik bildirimi |
//! | `POST /v1/gmail` | Google Pub/Sub | Gmail değişikliği (Google imzalı) |
//!
//! Kapalı sağlayıcının uçları 404 döner. Gövde üst sınırı 64 KB. Abone açma
//! IP başına saatte [`SAATLIK_KAYIT`] ile sınırlı. Aynı aboneye art arda gelen
//! değişiklikler [`BIRLESTIRME`] içinde tek iletiye birleşir (sonda bir ileti
//! garanti: son değişiklik kaçmaz). Günlüğe yalnız abone kimliğinin başı
//! yazılır; jeton, adres ve gövde yazılmaz.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::fcm::{Gonderici, Sonuc};
use crate::saglayici::{self, JmapItme, Jwk};
use crate::store::{ozet, Abone, Depo, KayitHata, Saglayici};

pub const GOVDE_SINIRI: usize = 64 * 1024;
pub const SAATLIK_KAYIT: u32 = 30;
pub const BIRLESTIRME: Duration = Duration::from_secs(2);
const JWKS_ADRESI: &str = "https://www.googleapis.com/oauth2/v3/certs";

/// İleti gönderici (testte sahte).
pub trait Itici: Send + Sync + 'static {
    fn gonder<'a>(
        &'a self,
        fcm: &'a str,
        veri: BTreeMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = Sonuc> + Send + 'a>>;
}

impl Itici for Gonderici {
    fn gonder<'a>(
        &'a self,
        fcm: &'a str,
        veri: BTreeMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = Sonuc> + Send + 'a>> {
        Box::pin(async move { Gonderici::gonder(self, fcm, &veri).await })
    }
}

/// Gmail Pub/Sub itmesinin doğrulanacağı değerler.
#[derive(Debug, Clone)]
pub struct GmailAyar {
    /// OIDC jetonunun hedef kitlesi (Pub/Sub aboneliğinde verilen).
    pub kitle: String,
    /// Pub/Sub'ın itme için kullandığı hizmet hesabı e-postası.
    pub eposta: String,
    /// Uygulamanın `users.watch`ta vereceği konu (`projects/…/topics/…`).
    pub konu: String,
}

pub struct Ayar {
    /// Dışarıdan görünen kök adres (`https://…`, sonda `/` yok).
    pub genel_url: String,
    pub saglayicilar: Vec<Saglayici>,
    /// Uygulamaya verilen Firebase istemci ayarları (gizli değildir).
    pub firebase: Value,
    pub gmail: Option<GmailAyar>,
}

#[derive(Default)]
struct Birlestirme {
    son: Option<tokio::time::Instant>,
    bekliyor: bool,
}

pub struct Durum {
    pub ayar: Ayar,
    depo: Mutex<Depo>,
    itici: Arc<dyn Itici>,
    hiz: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    birlestirme: Mutex<HashMap<String, Birlestirme>>,
    jwks: tokio::sync::Mutex<Option<(Instant, Vec<Jwk>)>>,
    http: reqwest::Client,
}

pub struct Istek<'a> {
    pub yontem: &'a str,
    pub yol: &'a str,
    pub sorgu: Option<&'a str>,
    pub yetki: Option<&'a str>,
    pub ip: IpAddr,
    pub govde: &'a [u8],
}

#[derive(Debug)]
pub struct Yanit {
    pub durum: u16,
    pub tur: &'static str,
    pub govde: Vec<u8>,
}

impl Yanit {
    fn json(durum: u16, v: Value) -> Self {
        Self {
            durum,
            tur: "application/json",
            govde: v.to_string().into_bytes(),
        }
    }
    fn bos(durum: u16) -> Self {
        Self {
            durum,
            tur: "text/plain; charset=utf-8",
            govde: Vec::new(),
        }
    }
    fn metin(durum: u16, m: &str) -> Self {
        Self {
            durum,
            tur: "text/plain; charset=utf-8",
            govde: m.as_bytes().to_vec(),
        }
    }
    fn hata(durum: u16, m: &str) -> Self {
        Self::json(durum, json!({ "hata": m }))
    }
}

pub fn simdi() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn kisa(id: &str) -> &str {
    id.get(..6).unwrap_or(id)
}

impl Durum {
    pub fn yeni(ayar: Ayar, depo: Depo, itici: Arc<dyn Itici>) -> Arc<Self> {
        Arc::new(Self {
            ayar,
            depo: Mutex::new(depo),
            itici,
            hiz: Mutex::new(HashMap::new()),
            birlestirme: Mutex::new(HashMap::new()),
            jwks: tokio::sync::Mutex::new(None),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        })
    }

    fn depo(&self) -> std::sync::MutexGuard<'_, Depo> {
        self.depo.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn etkin(&self, s: Saglayici) -> bool {
        self.ayar.saglayicilar.contains(&s)
    }

    pub fn temizle(&self) -> usize {
        self.depo().temizle(simdi())
    }

    /// Saatlik kayıt sınırı aşıldı mı (aşılmadıysa sayar)?
    fn hiz_asildi(&self, ip: IpAddr) -> bool {
        let mut h = self.hiz.lock().unwrap_or_else(|e| e.into_inner());
        let simdi = Instant::now();
        h.retain(|_, (bas, _)| simdi.duration_since(*bas) < Duration::from_secs(3600));
        let giris = h.entry(ip).or_insert((simdi, 0));
        giris.1 += 1;
        giris.1 > SAATLIK_KAYIT
    }

    /// İletiyi arka planda yollar; jeton kayıtsızsa aboneyi düşürür.
    fn yolla(self: &Arc<Self>, fcm: String, veri: BTreeMap<String, String>) {
        let d = self.clone();
        tokio::spawn(async move {
            match d.itici.gonder(&fcm, veri).await {
                Sonuc::Gitti => {}
                Sonuc::Kayitsiz => {
                    let n = d.depo().jetonu_sil(&fcm);
                    tracing::info!(n, "kayıtsız Firebase jetonu: aboneler silindi");
                }
                Sonuc::Hata(e) => tracing::warn!(error = %e, "ileti gönderilemedi"),
            }
        });
    }

    /// "Değişti" iletisi, birleştirerek.
    fn degisti(self: &Arc<Self>, id: &str) {
        let hemen = {
            let mut m = self.birlestirme.lock().unwrap_or_else(|e| e.into_inner());
            let b = m.entry(id.to_owned()).or_default();
            let simdi = tokio::time::Instant::now();
            match b.son {
                Some(son) if simdi.duration_since(son) < BIRLESTIRME => {
                    if b.bekliyor {
                        return;
                    }
                    b.bekliyor = true;
                    Some(BIRLESTIRME - simdi.duration_since(son))
                }
                _ => {
                    b.son = Some(simdi);
                    None
                }
            }
        };
        match hemen {
            None => self.degisti_yolla(id),
            Some(bekle) => {
                let (d, id) = (self.clone(), id.to_owned());
                tokio::spawn(async move {
                    tokio::time::sleep(bekle).await;
                    {
                        let mut m = d.birlestirme.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(b) = m.get_mut(&id) {
                            b.bekliyor = false;
                            b.son = Some(tokio::time::Instant::now());
                        }
                    }
                    d.degisti_yolla(&id);
                });
            }
        }
    }

    fn degisti_yolla(self: &Arc<Self>, id: &str) {
        let Some(fcm) = self.depo().al(id).map(|a| a.fcm.clone()) else {
            return;
        };
        let veri = BTreeMap::from([
            ("t".to_owned(), "degisti".to_owned()),
            ("a".to_owned(), id.to_owned()),
        ]);
        tracing::info!(abone = kisa(id), "değişiklik iletildi");
        self.yolla(fcm, veri);
    }

    async fn google_anahtarlari(&self, yenile: bool) -> Vec<Jwk> {
        let mut k = self.jwks.lock().await;
        if let Some((zaman, liste)) = k.as_ref() {
            if !yenile && zaman.elapsed() < Duration::from_secs(3600) {
                return liste.clone();
            }
        }
        #[derive(Deserialize)]
        struct Jwks {
            keys: Vec<Jwk>,
        }
        let liste = match self.http.get(JWKS_ADRESI).send().await {
            Ok(y) => y
                .text()
                .await
                .ok()
                .and_then(|t| serde_json::from_str::<Jwks>(&t).ok())
                .map(|j| j.keys)
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!(error = %e, "Google anahtarları alınamadı");
                Vec::new()
            }
        };
        if !liste.is_empty() {
            *k = Some((Instant::now(), liste.clone()));
        }
        liste
    }
}

#[derive(Deserialize)]
struct KayitIstegi {
    fcm: String,
    saglayici: String,
    /// Gmail: posta adresi (aktarıcı yalnız özetini saklar).
    #[serde(default)]
    hesap: Option<String>,
}

#[derive(Deserialize)]
struct GuncelleIstegi {
    #[serde(default)]
    fcm: Option<String>,
}

fn fcm_gecerli(f: &str) -> bool {
    (10..=4096).contains(&f.len()) && f.bytes().all(|b| b.is_ascii_graphic())
}

fn bearer(yetki: Option<&str>) -> Option<&str> {
    let y = yetki?.trim();
    y.strip_prefix("Bearer ")
        .or_else(|| y.strip_prefix("bearer "))
        .map(str::trim)
}

pub async fn yanitla(d: &Arc<Durum>, i: Istek<'_>) -> Yanit {
    if i.govde.len() > GOVDE_SINIRI {
        return Yanit::hata(413, "gövde çok büyük");
    }
    let parca: Vec<&str> = i.yol.trim_matches('/').split('/').collect();
    match (i.yontem, parca.as_slice()) {
        ("GET", ["saglik"]) => Yanit::metin(200, "ok"),
        ("GET", ["v1", "yapilandirma"]) => Yanit::json(
            200,
            json!({
                "firebase": d.ayar.firebase,
                "saglayicilar": d.ayar.saglayicilar.iter().map(|s| s.ad()).collect::<Vec<_>>(),
                "gmailKonu": d.ayar.gmail.as_ref().map(|g| g.konu.clone()),
            }),
        ),
        ("POST", ["v1", "abone"]) => abone_ac(d, &i),
        ("PUT", ["v1", "abone", id]) => {
            let Some(gizli) = bearer(i.yetki) else {
                return Yanit::hata(401, "yetki yok");
            };
            if !d.depo().yetkili(id, gizli) {
                return Yanit::hata(404, "abone yok");
            }
            let g: GuncelleIstegi = match serde_json::from_slice(i.govde) {
                Ok(g) => g,
                Err(_) if i.govde.is_empty() => GuncelleIstegi { fcm: None },
                Err(_) => return Yanit::hata(400, "gövde okunamadı"),
            };
            if g.fcm.as_deref().is_some_and(|f| !fcm_gecerli(f)) {
                return Yanit::hata(400, "geçersiz Firebase jetonu");
            }
            d.depo().guncelle(id, g.fcm, simdi());
            Yanit::bos(204)
        }
        ("DELETE", ["v1", "abone", id]) => {
            let Some(gizli) = bearer(i.yetki) else {
                return Yanit::hata(401, "yetki yok");
            };
            let mut depo = d.depo();
            if !depo.yetkili(id, gizli) {
                return Yanit::hata(404, "abone yok");
            }
            depo.sil(id);
            Yanit::bos(204)
        }
        ("POST", ["v1", "jmap", id, anahtar]) if d.etkin(Saglayici::Jmap) => {
            let abone = d.depo().al(id).cloned();
            let Some(a) = abone.filter(|a| {
                a.saglayici == Saglayici::Jmap && crate::store::esit(&a.url_anahtar, anahtar)
            }) else {
                return Yanit::hata(404, "abone yok");
            };
            match saglayici::jmap_coz(i.govde) {
                Some(JmapItme::Dogrula { abonelik, kod }) => {
                    tracing::info!(abone = kisa(&a.id), "JMAP abonelik doğrulaması iletildi");
                    let veri = BTreeMap::from([
                        ("t".to_owned(), "dogrula".to_owned()),
                        ("a".to_owned(), a.id.clone()),
                        ("s".to_owned(), abonelik),
                        ("k".to_owned(), kod),
                    ]);
                    d.yolla(a.fcm.clone(), veri);
                    Yanit::bos(201)
                }
                Some(JmapItme::Degisti) => {
                    d.degisti(&a.id);
                    Yanit::bos(202)
                }
                None => Yanit::hata(400, "JMAP itmesi okunamadı"),
            }
        }
        ("POST", ["v1", "graph", id]) if d.etkin(Saglayici::Graph) => {
            let abone = d
                .depo()
                .al(id)
                .filter(|a| a.saglayici == Saglayici::Graph)
                .cloned();
            let Some(a) = abone else {
                return Yanit::hata(404, "abone yok");
            };
            if let Some(token) = saglayici::graph_dogrulama(i.sorgu) {
                // Graph aboneliği kurarken adresi sınar: belirteç düz metin geri döner.
                return Yanit::metin(200, &token);
            }
            if saglayici::graph_eslesir(i.govde, &a.url_anahtar) {
                d.degisti(&a.id);
            }
            Yanit::bos(202)
        }
        ("POST", ["v1", "gmail"]) if d.etkin(Saglayici::Gmail) => gmail(d, &i).await,
        _ => Yanit::hata(404, "bulunamadı"),
    }
}

fn abone_ac(d: &Arc<Durum>, i: &Istek<'_>) -> Yanit {
    let k: KayitIstegi = match serde_json::from_slice(i.govde) {
        Ok(k) => k,
        Err(_) => return Yanit::hata(400, "gövde okunamadı"),
    };
    let Some(s) = Saglayici::from_ad(&k.saglayici).filter(|s| d.etkin(*s)) else {
        return Yanit::hata(400, "bu sağlayıcı kapalı");
    };
    if !fcm_gecerli(&k.fcm) {
        return Yanit::hata(400, "geçersiz Firebase jetonu");
    }
    let hesap_ozet = match s {
        Saglayici::Gmail => match k
            .hesap
            .as_deref()
            .map(str::trim)
            .filter(|h| h.contains('@'))
        {
            Some(h) => Some(ozet(&h.to_ascii_lowercase())),
            None => return Yanit::hata(400, "Gmail için posta adresi gerekli"),
        },
        _ => None,
    };
    if d.hiz_asildi(i.ip) {
        return Yanit::hata(429, "çok fazla kayıt; bir saat sonra yeniden dene");
    }
    let sonuc = d.depo().ekle(s, k.fcm, hesap_ozet, simdi());
    let (a, gizli): (Abone, String) = match sonuc {
        Ok(x) => x,
        Err(KayitHata::Dolu) => return Yanit::hata(503, "aktarıcı dolu"),
    };
    tracing::info!(abone = kisa(&a.id), saglayici = s.ad(), "abone açıldı");
    let kok = &d.ayar.genel_url;
    let mut v = json!({ "id": a.id, "gizli": gizli, "saglayici": s.ad() });
    match s {
        Saglayici::Jmap => {
            v["url"] = json!(format!("{kok}/v1/jmap/{}/{}", a.id, a.url_anahtar));
        }
        Saglayici::Graph => {
            v["url"] = json!(format!("{kok}/v1/graph/{}", a.id));
            v["clientState"] = json!(a.url_anahtar);
        }
        Saglayici::Gmail => {
            v["konu"] = json!(d.ayar.gmail.as_ref().map(|g| g.konu.clone()));
        }
    }
    Yanit::json(201, v)
}

async fn gmail(d: &Arc<Durum>, i: &Istek<'_>) -> Yanit {
    let Some(ayar) = d.ayar.gmail.clone() else {
        return Yanit::hata(404, "bulunamadı");
    };
    let Some(jwt) = bearer(i.yetki) else {
        return Yanit::hata(401, "yetki yok");
    };
    let mut anahtarlar = d.google_anahtarlari(false).await;
    let mut sonuc = saglayici::oidc_dogrula(jwt, &anahtarlar, &ayar.kitle, &ayar.eposta, simdi());
    if sonuc == Err(saglayici::OidcHata::AnahtarYok) {
        // Google anahtarları döndürmüş olabilir.
        anahtarlar = d.google_anahtarlari(true).await;
        sonuc = saglayici::oidc_dogrula(jwt, &anahtarlar, &ayar.kitle, &ayar.eposta, simdi());
    }
    if let Err(e) = sonuc {
        tracing::warn!(?e, "Gmail itmesi doğrulanamadı");
        return Yanit::hata(401, "imza geçersiz");
    }
    let Some(adres) = saglayici::gmail_adres(i.govde) else {
        // Pub/Sub 2xx almazsa yeniden dener; bozuk ileti için yeniden deneme gereksiz.
        return Yanit::bos(204);
    };
    let aboneler = d.depo().hesaba_gore(Saglayici::Gmail, &ozet(&adres));
    for a in aboneler {
        d.degisti(&a.id);
    }
    Yanit::bos(204)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[derive(Default)]
    struct Sahte {
        gelen: Mutex<Vec<(String, BTreeMap<String, String>)>>,
        kayitsiz: Mutex<Vec<String>>,
    }

    impl Itici for Sahte {
        fn gonder<'a>(
            &'a self,
            fcm: &'a str,
            veri: BTreeMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = Sonuc> + Send + 'a>> {
            Box::pin(async move {
                self.gelen.lock().unwrap().push((fcm.to_owned(), veri));
                if self.kayitsiz.lock().unwrap().iter().any(|k| k == fcm) {
                    Sonuc::Kayitsiz
                } else {
                    Sonuc::Gitti
                }
            })
        }
    }

    fn durum(saglayicilar: Vec<Saglayici>) -> (Arc<Durum>, Arc<Sahte>) {
        let sahte = Arc::new(Sahte::default());
        let d = Durum::yeni(
            Ayar {
                genel_url: "https://relay.ornek".into(),
                saglayicilar,
                firebase: json!({"projectId": "p", "apiKey": "k"}),
                gmail: None,
            },
            Depo::bellek(),
            sahte.clone(),
        );
        (d, sahte)
    }

    fn istek<'a>(yontem: &'a str, yol: &'a str, govde: &'a [u8]) -> Istek<'a> {
        Istek {
            yontem,
            yol,
            sorgu: None,
            yetki: None,
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            govde,
        }
    }

    fn json_of(y: &Yanit) -> Value {
        serde_json::from_slice(&y.govde).unwrap()
    }

    const JETON: &str = "fcm-jetonu-0123456789";

    async fn jmap_abone(d: &Arc<Durum>) -> (String, String, String) {
        let g = json!({"fcm": JETON, "saglayici": "jmap"}).to_string();
        let y = yanitla(d, istek("POST", "/v1/abone", g.as_bytes())).await;
        assert_eq!(y.durum, 201, "{:?}", String::from_utf8_lossy(&y.govde));
        let v = json_of(&y);
        let url = v["url"].as_str().unwrap().to_owned();
        assert!(url.starts_with("https://relay.ornek/v1/jmap/"));
        (
            v["id"].as_str().unwrap().to_owned(),
            v["gizli"].as_str().unwrap().to_owned(),
            url.trim_start_matches("https://relay.ornek").to_owned(),
        )
    }

    async fn bekle() {
        // Arka plan gönderim görevleri çalışsın.
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn yapilandirma_ve_saglik() {
        let (d, _) = durum(vec![Saglayici::Jmap]);
        assert_eq!(yanitla(&d, istek("GET", "/saglik", b"")).await.durum, 200);
        let y = yanitla(&d, istek("GET", "/v1/yapilandirma", b"")).await;
        let v = json_of(&y);
        assert_eq!(v["saglayicilar"], json!(["jmap"]));
        assert_eq!(v["firebase"]["projectId"], "p");
        assert_eq!(yanitla(&d, istek("GET", "/yok", b"")).await.durum, 404);
    }

    #[tokio::test]
    async fn jmap_dogrulama_ve_degisiklik_iletilir() {
        let (d, sahte) = durum(vec![Saglayici::Jmap]);
        let (id, _gizli, yol) = jmap_abone(&d).await;
        let dog =
            br#"{"@type":"PushVerification","pushSubscriptionId":"S1","verificationCode":"K9"}"#;
        assert_eq!(yanitla(&d, istek("POST", &yol, dog)).await.durum, 201);
        bekle().await;
        {
            let g = sahte.gelen.lock().unwrap();
            assert_eq!(g.len(), 1);
            assert_eq!(g[0].0, JETON);
            assert_eq!(g[0].1["t"], "dogrula");
            assert_eq!(g[0].1["a"], id);
            assert_eq!(g[0].1["s"], "S1");
            assert_eq!(g[0].1["k"], "K9");
        }
        let deg = br#"{"@type":"StateChange","changed":{}}"#;
        assert_eq!(yanitla(&d, istek("POST", &yol, deg)).await.durum, 202);
        bekle().await;
        let g = sahte.gelen.lock().unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[1].1["t"], "degisti");
        // İleti posta içeriği taşımaz.
        assert_eq!(g[1].1.len(), 2);
    }

    #[tokio::test]
    async fn yanlis_anahtar_ve_kapali_saglayici_404() {
        let (d, sahte) = durum(vec![Saglayici::Jmap]);
        let (id, _, _) = jmap_abone(&d).await;
        let deg = br#"{"@type":"StateChange"}"#;
        let yol = format!("/v1/jmap/{id}/yanlis");
        assert_eq!(yanitla(&d, istek("POST", &yol, deg)).await.durum, 404);
        assert_eq!(
            yanitla(&d, istek("POST", &format!("/v1/graph/{id}"), b"{}"))
                .await
                .durum,
            404
        );
        assert_eq!(
            yanitla(&d, istek("POST", "/v1/gmail", b"{}")).await.durum,
            404
        );
        let g = json!({"fcm": JETON, "saglayici": "gmail", "hesap": "a@gmail.com"}).to_string();
        assert_eq!(
            yanitla(&d, istek("POST", "/v1/abone", g.as_bytes()))
                .await
                .durum,
            400
        );
        bekle().await;
        assert!(sahte.gelen.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn art_arda_degisiklik_birlestirilir_son_kacmaz() {
        let (d, sahte) = durum(vec![Saglayici::Jmap]);
        let (_, _, yol) = jmap_abone(&d).await;
        let deg = br#"{"@type":"StateChange"}"#;
        for _ in 0..5 {
            yanitla(&d, istek("POST", &yol, deg)).await;
        }
        bekle().await;
        assert_eq!(sahte.gelen.lock().unwrap().len(), 1);
        tokio::time::sleep(BIRLESTIRME + Duration::from_millis(10)).await;
        bekle().await;
        assert_eq!(sahte.gelen.lock().unwrap().len(), 2);
        tokio::time::sleep(BIRLESTIRME * 2).await;
        yanitla(&d, istek("POST", &yol, deg)).await;
        bekle().await;
        assert_eq!(sahte.gelen.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn jeton_guncelleme_silme_ve_yetki() {
        let (d, sahte) = durum(vec![Saglayici::Jmap]);
        let (id, gizli, yol) = jmap_abone(&d).await;
        let yeni = json!({"fcm": "yeni-jeton-0123456789"}).to_string();
        let yol_abone = format!("/v1/abone/{id}");
        let mut i = istek("PUT", &yol_abone, yeni.as_bytes());
        assert_eq!(yanitla(&d, i).await.durum, 401);
        let yetki = format!("Bearer {gizli}");
        i = istek("PUT", &yol_abone, yeni.as_bytes());
        i.yetki = Some("Bearer yanlis");
        assert_eq!(yanitla(&d, i).await.durum, 404);
        i = istek("PUT", &yol_abone, yeni.as_bytes());
        i.yetki = Some(&yetki);
        assert_eq!(yanitla(&d, i).await.durum, 204);
        yanitla(&d, istek("POST", &yol, br#"{"@type":"StateChange"}"#)).await;
        bekle().await;
        assert_eq!(sahte.gelen.lock().unwrap()[0].0, "yeni-jeton-0123456789");
        i = istek("DELETE", &yol_abone, b"");
        i.yetki = Some(&yetki);
        assert_eq!(yanitla(&d, i).await.durum, 204);
        assert_eq!(
            yanitla(&d, istek("POST", &yol, br#"{"@type":"StateChange"}"#))
                .await
                .durum,
            404
        );
    }

    #[tokio::test]
    async fn kayitsiz_jeton_aboneyi_dusurur() {
        let (d, sahte) = durum(vec![Saglayici::Jmap]);
        sahte.kayitsiz.lock().unwrap().push(JETON.to_owned());
        let (_, _, yol) = jmap_abone(&d).await;
        yanitla(&d, istek("POST", &yol, br#"{"@type":"StateChange"}"#)).await;
        bekle().await;
        assert_eq!(d.depo().sayi(), 0);
    }

    #[tokio::test]
    async fn kayit_hiz_siniri_ve_dogrulama() {
        let (d, _) = durum(vec![Saglayici::Jmap]);
        let g = json!({"fcm": JETON, "saglayici": "jmap"}).to_string();
        for _ in 0..SAATLIK_KAYIT {
            assert_eq!(
                yanitla(&d, istek("POST", "/v1/abone", g.as_bytes()))
                    .await
                    .durum,
                201
            );
        }
        assert_eq!(
            yanitla(&d, istek("POST", "/v1/abone", g.as_bytes()))
                .await
                .durum,
            429
        );
        let kotu = json!({"fcm": "kisa", "saglayici": "jmap"}).to_string();
        assert_eq!(
            yanitla(&d, istek("POST", "/v1/abone", kotu.as_bytes()))
                .await
                .durum,
            400
        );
        let buyuk = vec![b' '; GOVDE_SINIRI + 1];
        assert_eq!(
            yanitla(&d, istek("POST", "/v1/abone", &buyuk)).await.durum,
            413
        );
    }

    #[tokio::test]
    async fn graph_dogrulamasi_ve_bildirim() {
        let (d, sahte) = durum(vec![Saglayici::Graph]);
        let g = json!({"fcm": JETON, "saglayici": "graph"}).to_string();
        let v = json_of(&yanitla(&d, istek("POST", "/v1/abone", g.as_bytes())).await);
        let id = v["id"].as_str().unwrap().to_owned();
        let cs = v["clientState"].as_str().unwrap().to_owned();
        let yol = format!("/v1/graph/{id}");
        let mut i = istek("POST", &yol, b"");
        i.sorgu = Some("validationToken=abc%20123");
        let y = yanitla(&d, i).await;
        assert_eq!((y.durum, y.govde.as_slice()), (200, &b"abc 123"[..]));
        let yanlis = json!({"value": [{"clientState": "baska"}]}).to_string();
        yanitla(&d, istek("POST", &yol, yanlis.as_bytes())).await;
        let dogru = json!({"value": [{"clientState": cs}]}).to_string();
        assert_eq!(
            yanitla(&d, istek("POST", &yol, dogru.as_bytes()))
                .await
                .durum,
            202
        );
        bekle().await;
        let gelen = sahte.gelen.lock().unwrap();
        assert_eq!(gelen.len(), 1);
        assert_eq!(gelen[0].1["t"], "degisti");
    }

    #[tokio::test]
    async fn gmail_imzasiz_itme_reddedilir() {
        let (d, sahte) = {
            let sahte = Arc::new(Sahte::default());
            let d = Durum::yeni(
                Ayar {
                    genel_url: "https://relay.ornek".into(),
                    saglayicilar: vec![Saglayici::Gmail],
                    firebase: json!({}),
                    gmail: Some(GmailAyar {
                        kitle: "https://relay.ornek/v1/gmail".into(),
                        eposta: "pubsub@p.iam.gserviceaccount.com".into(),
                        konu: "projects/p/topics/t".into(),
                    }),
                },
                Depo::bellek(),
                sahte.clone(),
            );
            (d, sahte)
        };
        let g = json!({"fcm": JETON, "saglayici": "gmail", "hesap": "Kisi@Gmail.com"}).to_string();
        let v = json_of(&yanitla(&d, istek("POST", "/v1/abone", g.as_bytes())).await);
        assert_eq!(v["konu"], "projects/p/topics/t");
        assert_eq!(
            yanitla(&d, istek("POST", "/v1/gmail", b"{}")).await.durum,
            401
        );
        bekle().await;
        assert!(sahte.gelen.lock().unwrap().is_empty());
    }
}
