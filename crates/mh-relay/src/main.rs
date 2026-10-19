//! Mailhere bildirim aktarıcısı (`mailhere-relay`).
//!
//! Posta sunucusu (şimdilik Stalwart, JMAP itmesi; Gmail ve Microsoft Graph
//! uçları hazır, ayarla açılır) yeni posta olunca bu sunucuya haber verir;
//! aktarıcı Firebase Cloud Messaging ile telefona içeriksiz "değişti" iletisi
//! yollar, telefon postayı sunucudan kendisi indirir. Aktarıcı parola, erişim
//! jetonu ya da posta içeriği görmez ve saklamaz. Ayrıntı: `docs/PUSH.md`.
//!
//! Ortam değişkenleri:
//! - `MH_RELAY_PUBLIC_URL` (zorunlu): dışarıdan görünen kök adres.
//! - `MH_FCM_SERVICE_ACCOUNT` (zorunlu): Firebase hizmet hesabı JSON'u (ya da
//!   base64 kodlu hâli).
//! - `MH_FIREBASE_API_KEY`, `MH_FIREBASE_APP_ID`, `MH_FIREBASE_SENDER_ID`:
//!   uygulamaya verilen istemci ayarları (gizli değildir).
//! - `MH_PROVIDERS` (varsayılan `jmap`): virgülle etkin sağlayıcılar.
//! - `MH_GMAIL_AUDIENCE`, `MH_GMAIL_PUSH_SA`, `MH_GMAIL_TOPIC`: Gmail açıksa.
//! - `MH_RELAY_DATA` (varsayılan `/data`), `PORT` (varsayılan 8080).

mod fcm;
mod saglayici;
mod store;
mod sunucu;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::json;

use crate::store::{Depo, Saglayici};
use crate::sunucu::{Ayar, Durum, GmailAyar, Istek};

fn ortam(ad: &str) -> Option<String> {
    std::env::var(ad)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn zorunlu(ad: &str) -> String {
    ortam(ad).unwrap_or_else(|| {
        eprintln!("{ad} ortam değişkeni gerekli");
        std::process::exit(2)
    })
}

/// Hizmet hesabı: JSON ya da (ortam dosyasında tırnak ve satır sonu sorunu
/// çıkmasın diye) base64 kodlu JSON.
fn hizmet_hesabi(deger: &str) -> Result<fcm::HizmetHesabi, String> {
    use base64::Engine;
    let metin = if deger.trim_start().starts_with('{') {
        deger.to_owned()
    } else {
        let b = base64::engine::general_purpose::STANDARD
            .decode(deger.trim())
            .map_err(|e| format!("base64 değil: {e}"))?;
        String::from_utf8(b).map_err(|e| e.to_string())?
    };
    serde_json::from_str(&metin).map_err(|e| e.to_string())
}

/// İstemci adresi: Traefik arkasında `X-Forwarded-For`'un ilk değeri.
fn istemci_ip(req: &Request<Incoming>, esh: SocketAddr) -> IpAddr {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(esh.ip())
}

async fn hizmet(
    d: Arc<Durum>,
    req: Request<Incoming>,
    esh: SocketAddr,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let ip = istemci_ip(&req, esh);
    let yontem = req.method().as_str().to_owned();
    let yol = req.uri().path().to_owned();
    let sorgu = req.uri().query().map(str::to_owned);
    let yetki = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let govde = match Limited::new(req.into_body(), sunucu::GOVDE_SINIRI)
        .collect()
        .await
    {
        Ok(b) => b.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(413)
                .body(Full::new(Bytes::from_static(
                    b"{\"hata\":\"g\xc3\xb6vde \xc3\xa7ok b\xc3\xbcy\xc3\xbck\"}",
                )))
                .unwrap_or_default())
        }
    };
    let y = sunucu::yanitla(
        &d,
        Istek {
            yontem: &yontem,
            yol: &yol,
            sorgu: sorgu.as_deref(),
            yetki: yetki.as_deref(),
            ip,
            govde: &govde,
        },
    )
    .await;
    Ok(Response::builder()
        .status(y.durum)
        .header("content-type", y.tur)
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(y.govde)))
        .unwrap_or_default())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let genel_url = zorunlu("MH_RELAY_PUBLIC_URL")
        .trim_end_matches('/')
        .to_owned();
    let hesap: fcm::HizmetHesabi = hizmet_hesabi(&zorunlu("MH_FCM_SERVICE_ACCOUNT"))
        .unwrap_or_else(|e| {
            eprintln!("MH_FCM_SERVICE_ACCOUNT okunamadı: {e}");
            std::process::exit(2)
        });
    let gonderici = fcm::Gonderici::yeni(hesap).unwrap_or_else(|e| {
        eprintln!("Firebase göndericisi kurulamadı: {e}");
        std::process::exit(2)
    });
    let saglayicilar: Vec<Saglayici> = ortam("MH_PROVIDERS")
        .unwrap_or_else(|| "jmap".into())
        .split(',')
        .filter_map(Saglayici::from_ad)
        .collect();
    let gmail = if saglayicilar.contains(&Saglayici::Gmail) {
        Some(GmailAyar {
            kitle: ortam("MH_GMAIL_AUDIENCE").unwrap_or_else(|| format!("{genel_url}/v1/gmail")),
            eposta: zorunlu("MH_GMAIL_PUSH_SA"),
            konu: zorunlu("MH_GMAIL_TOPIC"),
        })
    } else {
        None
    };
    let firebase = json!({
        "apiKey": ortam("MH_FIREBASE_API_KEY"),
        "appId": ortam("MH_FIREBASE_APP_ID"),
        "projectId": gonderici.proje(),
        "senderId": ortam("MH_FIREBASE_SENDER_ID"),
    });
    let veri = std::path::PathBuf::from(ortam("MH_RELAY_DATA").unwrap_or_else(|| "/data".into()));
    if let Err(e) = std::fs::create_dir_all(&veri) {
        eprintln!("veri klasörü açılamadı: {e}");
        std::process::exit(2)
    }
    let depo = Depo::ac(&veri.join("aboneler.json")).unwrap_or_else(|e| {
        eprintln!("abone dosyası okunamadı: {e}");
        std::process::exit(2)
    });
    tracing::info!(
        aboneler = depo.sayi(),
        saglayicilar = ?saglayicilar.iter().map(|s| s.ad()).collect::<Vec<_>>(),
        "aktarıcı başlıyor"
    );
    let d = Durum::yeni(
        Ayar {
            genel_url,
            saglayicilar,
            firebase,
            gmail,
        },
        depo,
        Arc::new(gonderici),
    );

    {
        let d = d.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_secs(6 * 3600));
            loop {
                t.tick().await;
                let n = d.temizle();
                if n > 0 {
                    tracing::info!(n, "bayat aboneler silindi");
                }
            }
        });
    }

    let port: u16 = ortam("PORT").and_then(|p| p.parse().ok()).unwrap_or(8080);
    let dinleyici = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap_or_else(|e| {
            eprintln!("{port} kapısı açılamadı: {e}");
            std::process::exit(2)
        });
    loop {
        let (soket, esh) = match dinleyici.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(error = %e, "bağlantı kabul edilemedi");
                continue;
            }
        };
        let d = d.clone();
        tokio::spawn(async move {
            let svc = service_fn(move |req| hizmet(d.clone(), req, esh));
            let _ = http1::Builder::new()
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(Duration::from_secs(10))
                .serve_connection(TokioIo::new(soket), svc)
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::hizmet_hesabi;
    use base64::Engine;

    #[test]
    fn hizmet_hesabi_json_ya_da_base64() {
        let j = r#"{"project_id":"p","client_email":"e@p","private_key":"k"}"#;
        assert_eq!(hizmet_hesabi(j).unwrap().project_id, "p");
        let b = base64::engine::general_purpose::STANDARD.encode(j);
        assert_eq!(hizmet_hesabi(&b).unwrap().client_email, "e@p");
        assert!(hizmet_hesabi("!!").is_err());
    }
}
