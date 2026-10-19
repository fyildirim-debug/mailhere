//! Sağlayıcıların aktarıcıya yolladığı gövdelerin ayrıştırılması.
//!
//! - JMAP (RFC 8620 §7.2): önce `PushVerification` (doğrulama kodu cihaza
//!   iletilir, cihaz aboneliği onaylar), sonra her değişiklikte
//!   `StateChange`. İçerik yalnız tür/durum dizgileridir; aktarıcı içeriği
//!   cihaza taşımaz, yalnız "değişti" der.
//! - Microsoft Graph: kurulumda `validationToken` sorgusu düz metin olarak
//!   geri yollanır; bildirimlerde `clientState` aboneye verilen anahtarla
//!   eşleşmelidir.
//! - Gmail: Google Cloud Pub/Sub itmesi; `message.data` base64 JSON
//!   `{emailAddress, historyId}`. İstek Google'ın OIDC jetonuyla imzalıdır
//!   (`Authorization: Bearer`), doğrulanmadan işlenmez.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde::Deserialize;
use serde_json::Value;

/// JMAP itmesinin cihaza söyleyeceği şey.
#[derive(Debug, PartialEq, Eq)]
pub enum JmapItme {
    /// Abonelik doğrulaması: kod cihaza gider.
    Dogrula { abonelik: String, kod: String },
    /// Bir şey değişti: cihaz eşitlesin.
    Degisti,
}

pub fn jmap_coz(govde: &[u8]) -> Option<JmapItme> {
    let v: Value = serde_json::from_slice(govde).ok()?;
    match v.get("@type").and_then(Value::as_str)? {
        "PushVerification" => {
            let kod = v.get("verificationCode")?.as_str()?;
            let abonelik = v.get("pushSubscriptionId")?.as_str()?;
            // Kod ve kimlik kısa dizgilerdir; aşırı uzun değer reddedilir
            // (FCM veri iletisi 4 KB ile sınırlı).
            if kod.is_empty() || kod.len() > 512 || abonelik.len() > 256 {
                return None;
            }
            Some(JmapItme::Dogrula {
                abonelik: abonelik.to_owned(),
                kod: kod.to_owned(),
            })
        }
        "StateChange" => Some(JmapItme::Degisti),
        _ => None,
    }
}

/// Graph bildirim gövdesinde verilen anahtarla eşleşen bildirim var mı?
pub fn graph_eslesir(govde: &[u8], anahtar: &str) -> bool {
    let Ok(v) = serde_json::from_slice::<Value>(govde) else {
        return false;
    };
    v.get("value")
        .and_then(Value::as_array)
        .is_some_and(|liste| {
            liste.iter().any(|n| {
                n.get("clientState")
                    .and_then(Value::as_str)
                    .is_some_and(|s| crate::store::esit(s, anahtar))
            })
        })
}

/// Graph kurulum doğrulaması: `?validationToken=…` (URL kodlu) çözülmüş değeri.
pub fn graph_dogrulama(sorgu: Option<&str>) -> Option<String> {
    let sorgu = sorgu?;
    for parca in sorgu.split('&') {
        if let Some(deger) = parca.strip_prefix("validationToken=") {
            return Some(url_coz(deger));
        }
    }
    None
}

fn url_coz(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(c) => {
                        out.push(c);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Gmail Pub/Sub iletisindeki posta adresi (küçük harf).
pub fn gmail_adres(govde: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Zarf {
        message: Ileti,
    }
    #[derive(Deserialize)]
    struct Ileti {
        data: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Veri {
        email_address: String,
    }
    let z: Zarf = serde_json::from_slice(govde).ok()?;
    let ham = STANDARD
        .decode(z.message.data.trim())
        .or_else(|_| URL_SAFE_NO_PAD.decode(z.message.data.trim().trim_end_matches('=')))
        .ok()?;
    let v: Veri = serde_json::from_slice(&ham).ok()?;
    Some(v.email_address.trim().to_ascii_lowercase())
}

/// Google'ın açık anahtarı (JWKS `kid`, `n`, `e`).
#[derive(Debug, Clone, Deserialize)]
pub struct Jwk {
    pub kid: String,
    pub n: String,
    pub e: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OidcHata {
    Bicim,
    AnahtarYok,
    Imza,
    Iddia(&'static str),
}

/// Google OIDC jetonu (Pub/Sub itmesi) doğrulaması: RS256 imzası, veren,
/// hedef kitle, e-posta (Pub/Sub'ın kullandığı hizmet hesabı) ve süre.
pub fn oidc_dogrula(
    jwt: &str,
    anahtarlar: &[Jwk],
    kitle: &str,
    eposta: &str,
    simdi: i64,
) -> Result<(), OidcHata> {
    let parca: Vec<&str> = jwt.split('.').collect();
    let [b, y, i] = parca[..] else {
        return Err(OidcHata::Bicim);
    };
    let baslik: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(b).map_err(|_| OidcHata::Bicim)?)
            .map_err(|_| OidcHata::Bicim)?;
    if baslik.get("alg").and_then(Value::as_str) != Some("RS256") {
        return Err(OidcHata::Bicim);
    }
    let kid = baslik
        .get("kid")
        .and_then(Value::as_str)
        .ok_or(OidcHata::Bicim)?;
    let jwk = anahtarlar
        .iter()
        .find(|k| k.kid == kid)
        .ok_or(OidcHata::AnahtarYok)?;
    let n = URL_SAFE_NO_PAD
        .decode(&jwk.n)
        .map_err(|_| OidcHata::Bicim)?;
    let e = URL_SAFE_NO_PAD
        .decode(&jwk.e)
        .map_err(|_| OidcHata::Bicim)?;
    let imza = URL_SAFE_NO_PAD.decode(i).map_err(|_| OidcHata::Bicim)?;
    RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &RSA_PKCS1_2048_8192_SHA256,
            format!("{b}.{y}").as_bytes(),
            &imza,
        )
        .map_err(|_| OidcHata::Imza)?;
    let iddia: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(y).map_err(|_| OidcHata::Bicim)?)
            .map_err(|_| OidcHata::Bicim)?;
    let s = |ad: &str| iddia.get(ad).and_then(Value::as_str);
    match s("iss") {
        Some("https://accounts.google.com") | Some("accounts.google.com") => {}
        _ => return Err(OidcHata::Iddia("iss")),
    }
    if s("aud") != Some(kitle) {
        return Err(OidcHata::Iddia("aud"));
    }
    if s("email") != Some(eposta) || iddia.get("email_verified") != Some(&Value::Bool(true)) {
        return Err(OidcHata::Iddia("email"));
    }
    let exp = iddia
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or(OidcHata::Iddia("exp"))?;
    // Saat kayması için 60 sn pay.
    if exp + 60 < simdi {
        return Err(OidcHata::Iddia("exp"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{KeyPair, RsaKeyPair};
    use serde_json::json;

    #[test]
    fn jmap_dogrulama_ve_degisiklik() {
        let v = br#"{"@type":"PushVerification","pushSubscriptionId":"P1","verificationCode":"kod-42"}"#;
        assert_eq!(
            jmap_coz(v),
            Some(JmapItme::Dogrula {
                abonelik: "P1".into(),
                kod: "kod-42".into()
            })
        );
        let d = br#"{"@type":"StateChange","changed":{"c":{"EmailDelivery":"s1"}}}"#;
        assert_eq!(jmap_coz(d), Some(JmapItme::Degisti));
        assert_eq!(jmap_coz(br#"{"@type":"Baska"}"#), None);
        assert_eq!(jmap_coz(b"bozuk"), None);
        let uzun = format!(
            r#"{{"@type":"PushVerification","pushSubscriptionId":"P","verificationCode":"{}"}}"#,
            "x".repeat(600)
        );
        assert_eq!(jmap_coz(uzun.as_bytes()), None);
    }

    #[test]
    fn graph_dogrulama_ve_eslesme() {
        assert_eq!(
            graph_dogrulama(Some("validationToken=Abc%3A%20d+e")).as_deref(),
            Some("Abc: d e")
        );
        assert_eq!(graph_dogrulama(Some("x=1")), None);
        assert_eq!(graph_dogrulama(None), None);
        let g = br#"{"value":[{"clientState":"anahtar","resource":"me/messages/1"}]}"#;
        assert!(graph_eslesir(g, "anahtar"));
        assert!(!graph_eslesir(g, "baska"));
        assert!(!graph_eslesir(b"{}", "anahtar"));
    }

    #[test]
    fn gmail_iletisi_adresi_verir() {
        let veri = STANDARD.encode(br#"{"emailAddress":"Kisi@Gmail.com","historyId":"9"}"#);
        let g = json!({"message": {"data": veri, "messageId": "1"}, "subscription": "s"});
        assert_eq!(
            gmail_adres(g.to_string().as_bytes()).as_deref(),
            Some("kisi@gmail.com")
        );
        assert_eq!(gmail_adres(b"{}"), None);
    }

    fn jwt(k: &RsaKeyPair, kid: &str, iddia: Value) -> String {
        let b = URL_SAFE_NO_PAD.encode(json!({"alg":"RS256","kid":kid}).to_string());
        let y = URL_SAFE_NO_PAD.encode(iddia.to_string());
        let mut imza = vec![0u8; k.public().modulus_len()];
        k.sign(
            &ring::signature::RSA_PKCS1_SHA256,
            &ring::rand::SystemRandom::new(),
            format!("{b}.{y}").as_bytes(),
            &mut imza,
        )
        .unwrap();
        format!("{b}.{y}.{}", URL_SAFE_NO_PAD.encode(imza))
    }

    fn test_jwk(k: &RsaKeyPair) -> Jwk {
        // Açık anahtar DER (RSAPublicKey: SEQUENCE { n, e }) bileşenlerine ayrılır.
        let der = k.public_key().as_ref();
        let (n, e) = rsa_bilesenleri(der);
        Jwk {
            kid: "k1".into(),
            n: URL_SAFE_NO_PAD.encode(n),
            e: URL_SAFE_NO_PAD.encode(e),
        }
    }

    /// Basit DER okuyucu (yalnız test): SEQUENCE içindeki iki INTEGER.
    fn rsa_bilesenleri(der: &[u8]) -> (Vec<u8>, Vec<u8>) {
        fn uzunluk(b: &[u8], i: &mut usize) -> usize {
            let ilk = b[*i] as usize;
            *i += 1;
            if ilk < 0x80 {
                return ilk;
            }
            let n = ilk & 0x7f;
            let mut l = 0;
            for _ in 0..n {
                l = (l << 8) | b[*i] as usize;
                *i += 1;
            }
            l
        }
        let mut i = 1;
        uzunluk(der, &mut i);
        let oku = |i: &mut usize| {
            assert_eq!(der[*i], 0x02);
            *i += 1;
            let l = uzunluk(der, i);
            let mut v = der[*i..*i + l].to_vec();
            *i += l;
            while v.len() > 1 && v[0] == 0 {
                v.remove(0);
            }
            v
        };
        let n = oku(&mut i);
        let e = oku(&mut i);
        (n, e)
    }

    #[test]
    fn google_oidc_jetonu_dogrulanir() {
        let der = STANDARD
            .decode(
                crate::fcm::tests::TEST_PEM
                    .lines()
                    .filter(|l| !l.starts_with("-----"))
                    .collect::<String>(),
            )
            .unwrap();
        let k = RsaKeyPair::from_pkcs8(&der).unwrap();
        let anahtarlar = vec![test_jwk(&k)];
        let iyi = json!({
            "iss": "https://accounts.google.com", "aud": "https://relay/v1/gmail",
            "email": "pubsub@p.iam.gserviceaccount.com", "email_verified": true, "exp": 2000
        });
        let ok = jwt(&k, "k1", iyi.clone());
        let d = |j: &str, simdi| {
            oidc_dogrula(
                j,
                &anahtarlar,
                "https://relay/v1/gmail",
                "pubsub@p.iam.gserviceaccount.com",
                simdi,
            )
        };
        assert_eq!(d(&ok, 1500), Ok(()));
        assert_eq!(d(&ok, 2100), Err(OidcHata::Iddia("exp")));
        assert_eq!(
            d(&jwt(&k, "k2", iyi.clone()), 1500),
            Err(OidcHata::AnahtarYok)
        );
        let mut baska = iyi.clone();
        baska["aud"] = json!("https://baska");
        assert_eq!(d(&jwt(&k, "k1", baska), 1500), Err(OidcHata::Iddia("aud")));
        let mut baska = iyi.clone();
        baska["email"] = json!("saldirgan@x.com");
        assert_eq!(
            d(&jwt(&k, "k1", baska), 1500),
            Err(OidcHata::Iddia("email"))
        );
        // İmza bozulursa reddedilir.
        let mut bozuk = ok.clone();
        bozuk.pop();
        bozuk.push(if ok.ends_with('A') { 'B' } else { 'A' });
        assert_eq!(d(&bozuk, 1500), Err(OidcHata::Imza));
        assert_eq!(d("a.b", 1500), Err(OidcHata::Bicim));
    }
}
