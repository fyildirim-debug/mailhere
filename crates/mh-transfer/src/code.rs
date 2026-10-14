//! Kod metni: `mailhere://aktar?v=1&h=<ip,ip,…>&p=<kapı>&t=<b64url 16B>&k=<b64url 32B>`
//!
//! QR'ın içeriği ve "kodu kopyala/yapıştır" metni aynıdır. `k` tek gizli
//! parçadır: ağdan hiç geçmez. Ayrıştırma bozuk girdiye dayanıklıdır: en çok
//! [`MAX_CODE_CHARS`] karakter, en çok [`MAX_HOSTS`] adres, yalnız yerel ağ
//! adresleri (genel adres içeren kod bütünüyle reddedilir), bilinmeyen
//! parametreler yok sayılır (ileri uyumluluk), `v` 1'den büyükse anlamlı hata.

use std::fmt;
use std::net::IpAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use zeroize::Zeroizing;

use crate::addr::{is_local_network, MAX_HOSTS};
use crate::crypto::{self, Key, Keys, Token, K_LEN, T_LEN};
use crate::error::{TransferError, TransferResult};

/// Protokol sürümü.
pub const VERSION: u32 = 1;
/// Kod metninin üst uzunluğu (en uzun geçerli kod ~200 karakterdir).
pub const MAX_CODE_CHARS: usize = 1024;
const PREFIX: &str = "mailhere://aktar?";

/// Çözümlenmiş aktarım kodu. `k` gizlidir; `Debug` göstermez.
pub struct Code {
    /// Bilgisayarın yerel adresleri (1–6, yinelenmeyen, hepsi yerel ağ).
    pub hosts: Vec<IpAddr>,
    pub port: u16,
    /// Teklif kimliği (açık).
    pub t: Token,
    /// Kök anahtar (yalnız bellekte).
    pub k: Key,
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Code")
            .field("hosts", &self.hosts)
            .field("port", &self.port)
            .field("t", &"<16 B>")
            .field("k", &"<gizli>")
            .finish()
    }
}

fn invalid() -> TransferError {
    TransferError::InvalidCode(
        "Aktarım kodu tanınmadı. Bilgisayardaki QR kodu yeniden okut ya da kodu eksiksiz yapıştır."
            .into(),
    )
}

impl Code {
    /// Yeni teklif için rastgele T ve K ile kod. `hosts` boş olamaz, en çok
    /// [`MAX_HOSTS`] ve hepsi yerel ağ adresi; `port` 0 olamaz.
    pub fn generate(hosts: Vec<IpAddr>, port: u16, allow_loopback: bool) -> TransferResult<Self> {
        if hosts.is_empty() {
            return Err(TransferError::Network(
                "Bu bilgisayarın yerel ağ adresi bulunamadı. Wi-Fi ya da kablolu ağa bağlı olduğundan emin olup yeniden dene."
                    .into(),
            ));
        }
        if hosts.len() > MAX_HOSTS || port == 0 {
            return Err(TransferError::Internal(
                "Aktarım kodu kurulamadı: adres sayısı ya da kapı geçersiz.".into(),
            ));
        }
        if !hosts.iter().all(|h| host_ok(*h, allow_loopback)) {
            return Err(TransferError::Internal(
                "Aktarım kodu kurulamadı: yalnız yerel ağ adresleri duyurulabilir.".into(),
            ));
        }
        Ok(Self {
            hosts,
            port,
            t: *crypto::random::<T_LEN>()?,
            k: crypto::random::<K_LEN>()?,
        })
    }

    /// Kod metnini çözümler.
    pub fn parse(text: &str) -> TransferResult<Self> {
        let text = text.trim();
        if text.chars().count() > MAX_CODE_CHARS {
            return Err(TransferError::InvalidCode(
                "Aktarım kodu çok uzun; yalnız bilgisayarın gösterdiği kodu yapıştır.".into(),
            ));
        }
        let query = text
            .get(..PREFIX.len())
            .filter(|p| p.eq_ignore_ascii_case(PREFIX))
            .and_then(|_| text.get(PREFIX.len()..))
            .ok_or_else(invalid)?;

        let mut v: Option<String> = None;
        let mut h: Option<String> = None;
        let mut p: Option<String> = None;
        let mut t: Option<Zeroizing<String>> = None;
        let mut k: Option<Zeroizing<String>> = None;
        for pair in query.split('&').filter(|s| !s.is_empty()) {
            let (key, value) = pair.split_once('=').ok_or_else(invalid)?;
            let value = percent_decode(value).ok_or_else(invalid)?;
            let slot_plain = match key {
                "v" => Some(&mut v),
                "h" => Some(&mut h),
                "p" => Some(&mut p),
                _ => None,
            };
            if let Some(slot) = slot_plain {
                if slot.replace(value).is_some() {
                    return Err(invalid()); // yinelenen parametre
                }
                continue;
            }
            let slot_secret = match key {
                "t" => Some(&mut t),
                "k" => Some(&mut k),
                _ => None,
            };
            // Eşleşmeyen anahtar bilinmeyen parametredir: ileri uyumluluk için yok sayılır.
            if let Some(slot) = slot_secret {
                if slot.replace(Zeroizing::new(value)).is_some() {
                    return Err(invalid());
                }
            }
        }

        let version: u32 = v
            .as_deref()
            .and_then(|s| s.parse().ok())
            .ok_or_else(invalid)?;
        if version > VERSION {
            return Err(TransferError::NewerVersion(version));
        }
        if version != VERSION {
            return Err(invalid());
        }

        let hosts = parse_hosts(h.as_deref().ok_or_else(invalid)?)?;
        let port: u16 = p
            .as_deref()
            .and_then(|s| s.parse().ok())
            .filter(|p| *p != 0)
            .ok_or_else(invalid)?;

        let t_bytes = decode_b64(t.as_deref().ok_or_else(invalid)?).ok_or_else(invalid)?;
        let mut token = [0u8; T_LEN];
        if t_bytes.len() != T_LEN {
            return Err(invalid());
        }
        token.copy_from_slice(&t_bytes);

        let k_bytes = decode_b64(k.as_deref().ok_or_else(invalid)?).ok_or_else(invalid)?;
        if k_bytes.len() != K_LEN {
            return Err(invalid());
        }
        let mut key = Zeroizing::new([0u8; K_LEN]);
        key.copy_from_slice(&k_bytes);

        Ok(Self {
            hosts,
            port,
            t: token,
            k: key,
        })
    }

    /// Kod metni (QR içeriği). Anahtarı içerir; `Zeroizing` döner.
    pub fn to_text(&self) -> Zeroizing<String> {
        let hosts: Vec<String> = self.hosts.iter().map(|h| h.to_string()).collect();
        let mut out = Zeroizing::new(String::with_capacity(220));
        out.push_str(PREFIX);
        out.push_str("v=");
        out.push_str(&VERSION.to_string());
        out.push_str("&h=");
        out.push_str(&hosts.join(","));
        out.push_str("&p=");
        out.push_str(&self.port.to_string());
        out.push_str("&t=");
        out.push_str(&URL_SAFE_NO_PAD.encode(self.t));
        out.push_str("&k=");
        let k = Zeroizing::new(URL_SAFE_NO_PAD.encode(&self.k[..]));
        out.push_str(&k);
        out
    }

    /// Bu kodun türetilmiş anahtarları.
    pub fn keys(&self) -> Keys {
        Keys::derive(&self.k, &self.t)
    }
}

fn host_ok(ip: IpAddr, allow_loopback: bool) -> bool {
    if ip.is_loopback() {
        return allow_loopback;
    }
    !mh_core::net::is_public_ip(ip) && is_local_network(ip)
}

/// `h` değeri: virgülle ayrılmış 1–6 adres; genel, belirtilmemiş, çoklu
/// yayın ya da ayrılmış adres içeren kod reddedilir. Loopback sözdizimsel
/// olarak kabul edilir; istemci kendi ilkesine göre süzer.
fn parse_hosts(s: &str) -> TransferResult<Vec<IpAddr>> {
    let mut hosts: Vec<IpAddr> = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(invalid());
        }
        let ip: IpAddr = part
            .strip_prefix('[')
            .and_then(|p| p.strip_suffix(']'))
            .unwrap_or(part)
            .parse()
            .map_err(|_| invalid())?;
        let ip = ip.to_canonical();
        if !host_ok(ip, true) {
            return Err(TransferError::InvalidCode(
                "Aktarım kodu yerel ağ dışında bir adres içeriyor; güvenlik için kullanılmadı. Bilgisayarda yeni kod oluştur."
                    .into(),
            ));
        }
        if !hosts.contains(&ip) {
            hosts.push(ip);
        }
        if hosts.len() > MAX_HOSTS {
            return Err(invalid());
        }
    }
    if hosts.is_empty() {
        return Err(invalid());
    }
    Ok(hosts)
}

/// base64url; sondaki `=` dolgusu varsa atılır. Çıktı `Zeroizing`.
fn decode_b64(s: &str) -> Option<Zeroizing<Vec<u8>>> {
    let s = s.trim_end_matches('=');
    if s.len() > 64 {
        return None;
    }
    URL_SAFE_NO_PAD.decode(s).ok().map(Zeroizing::new)
}

/// Yalnız ASCII `%XX` çözer (bazı tarayıcılar `,` ve `:` için kaçışlar).
/// Geçersiz kaçış ya da ASCII dışı karakter → `None`.
fn percent_decode(s: &str) -> Option<String> {
    if !s.is_ascii() {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            let b = u8::from_str_radix(hex, 16).ok()?;
            if !b.is_ascii() || b.is_ascii_control() {
                return None;
            }
            out.push(b);
            i += 3;
        } else if bytes[i].is_ascii_control() || bytes[i] == b' ' {
            return None;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(out).ok()?;
    if s.is_empty() {
        return None;
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn ornek() -> Code {
        let mut k = Zeroizing::new([0u8; K_LEN]);
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        let mut t = [0u8; T_LEN];
        for (i, b) in t.iter_mut().enumerate() {
            *b = 0xa0 + i as u8;
        }
        Code {
            hosts: vec![ip("192.168.1.20"), ip("10.8.0.2"), ip("fd12:3456::20")],
            port: 51234,
            t,
            k,
        }
    }

    #[test]
    fn kod_metni_gidip_gelir() {
        let code = ornek();
        let text = code.to_text();
        assert_eq!(
            &*text,
            "mailhere://aktar?v=1&h=192.168.1.20,10.8.0.2,fd12:3456::20&p=51234&t=oKGio6SlpqeoqaqrrK2urw&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
        );
        let back = Code::parse(&text).unwrap();
        assert_eq!(back.hosts, code.hosts);
        assert_eq!(back.port, code.port);
        assert_eq!(back.t, code.t);
        assert_eq!(&back.k[..], &code.k[..]);
        // Debug anahtarı göstermez.
        let dbg = format!("{back:?}");
        assert!(dbg.contains("192.168.1.20") && dbg.contains("<gizli>"));
        assert!(!dbg.contains("AAECAwQF") && !dbg.contains("oKGio6Sl"));
        // Türetilmiş anahtarlar iki tarafta aynı.
        assert_eq!(
            format!("{:?}", code.keys().raw()),
            format!("{:?}", back.keys().raw())
        );
        // Hoşgörü: boşluk, büyük harfli şema, dolgulu base64, bilinmeyen
        // parametre, sondaki `&`, yüzde kaçışlı virgül, köşeli ayraçlı IPv6.
        let lenient = format!(
            "  MAILHERE://aktar?x=1&v=1&h=192.168.1.20%2C[fd12:3456::20]&p=51234&t={}=&k={}&\n",
            URL_SAFE_NO_PAD.encode(code.t),
            URL_SAFE_NO_PAD.encode(&code.k[..])
        );
        let l = Code::parse(&lenient).unwrap();
        assert_eq!(l.hosts, vec![ip("192.168.1.20"), ip("fd12:3456::20")]);
        assert_eq!(l.port, 51234);
        // IPv4 eşlenmiş IPv6 adresi IPv4'e indirgenir; yinelenen adres tek sayılır.
        let mapped = text.replace(
            "h=192.168.1.20,10.8.0.2",
            "h=::ffff:192.168.1.20,192.168.1.20",
        );
        assert_eq!(
            Code::parse(&mapped).unwrap().hosts,
            vec![ip("192.168.1.20"), ip("fd12:3456::20")]
        );
    }

    #[test]
    fn bozuk_girdiler() {
        let good = ornek().to_text().to_string();
        let t = URL_SAFE_NO_PAD.encode(ornek().t);
        let k = URL_SAFE_NO_PAD.encode(&ornek().k[..]);
        let bad: Vec<(String, &str)> = vec![
            ("".into(), "boş"),
            ("mailhere://aktar".into(), "sorgu yok"),
            ("mailhere://aktar?".into(), "boş sorgu"),
            ("https://ornek.com/?v=1".into(), "başka şema"),
            (
                "mailhere://baska?v=1&h=10.0.0.1&p=1&t=AA&k=BB".into(),
                "başka yol",
            ),
            (good.replace("v=1", "v=0"), "sürüm 0"),
            (good.replace("v=1", "v=bir"), "sürüm sayı değil"),
            (good.replace("v=1&", ""), "sürüm yok"),
            (good.replace("&h=", "&hh="), "adres yok"),
            (good.replace("&p=51234", "&p=0"), "kapı 0"),
            (good.replace("&p=51234", "&p=70000"), "kapı büyük"),
            (good.replace("&p=51234", "&p=-5"), "kapı negatif"),
            (good.replace("&p=51234", ""), "kapı yok"),
            (format!("{good}&p=1"), "yinelenen kapı"),
            (format!("{good}&k={k}"), "yinelenen anahtar"),
            (good.replace(&format!("&k={k}"), ""), "anahtar yok"),
            (good.replace(&format!("&k={k}"), "&k=AAEC"), "anahtar kısa"),
            (
                good.replace(&format!("&k={k}"), &format!("&k={k}AA")),
                "anahtar uzun",
            ),
            (
                good.replace(&format!("&k={k}"), "&k=!!!!"),
                "anahtar base64 değil",
            ),
            (good.replace(&format!("&t={t}"), "&t=oKGio6Sl"), "T kısa"),
            (good.replace(&format!("&t={t}"), ""), "T yok"),
            (good.replace("192.168.1.20", "8.8.8.8"), "genel IPv4"),
            (
                good.replace("fd12:3456::20", "2606:4700::1111"),
                "genel IPv6",
            ),
            (
                good.replace("192.168.1.20", "::ffff:8.8.8.8"),
                "genel eşlenmiş",
            ),
            (good.replace("192.168.1.20", "0.0.0.0"), "belirtilmemiş"),
            (good.replace("192.168.1.20", "224.0.0.1"), "çoklu yayın"),
            (good.replace("192.168.1.20", "192.0.2.1"), "belgeleme bloğu"),
            (good.replace("192.168.1.20", "ornek.com"), "ad, IP değil"),
            (good.replace("192.168.1.20", "192.168.1"), "eksik IPv4"),
            (good.replace("192.168.1.20,", ","), "boş adres"),
            (
                good.replace("192.168.1.20,", "192.168.1.20,,"),
                "boş adres ortada",
            ),
            (
                good.replace(
                    "h=192.168.1.20",
                    "h=10.0.0.1,10.0.0.2,10.0.0.3,10.0.0.4,10.0.0.5,10.0.0.6,10.0.0.7",
                ),
                "yediden çok adres",
            ),
            (good.replace("&h=", "&h=%zz"), "bozuk yüzde kaçışı"),
            (good.replace("&h=", "&h=%0a"), "denetim karakteri kaçışı"),
            (good.replace("&p=", "&p=5 1"), "boşluk"),
            (good.replace("&p=", "&p=5\u{200b}1"), "görünmez karakter"),
            (good.replace("&h=", "&="), "boş anahtar adı"),
            (
                "mailhere://aktar?v=1&h=10.0.0.1&p=1&t&k".into(),
                "değer yok",
            ),
            (format!("{good}{}", "a".repeat(MAX_CODE_CHARS)), "çok uzun"),
            ("mailhere://aktar?\u{1F600}=1".into(), "ASCII dışı"),
        ];
        for (text, why) in bad {
            let r = Code::parse(&text);
            assert!(r.is_err(), "kabul edildi: {why}: {text}");
            let msg = r.err().unwrap().to_string();
            assert!(!msg.contains(&k), "{why}: hata mesajı anahtarı sızdırıyor");
            assert!(!msg.is_empty());
        }
        // Daha yeni sürüm anlamlı hata verir.
        match Code::parse(&good.replace("v=1", "v=2")) {
            Err(TransferError::NewerVersion(2)) => {}
            other => panic!("{other:?}"),
        }
        match Code::parse("mailhere://aktar?v=7") {
            Err(TransferError::NewerVersion(7)) => {}
            other => panic!("{other:?}"),
        }
        // Genel adres için ayrı, açıklayıcı mesaj.
        let e = Code::parse(&good.replace("192.168.1.20", "8.8.8.8")).unwrap_err();
        assert!(e.to_string().contains("yerel ağ dışında"), "{e}");
    }

    #[test]
    fn uretim_denetimleri() {
        assert!(Code::generate(vec![], 5000, false).is_err());
        assert!(Code::generate(vec![ip("192.168.1.2")], 0, false).is_err());
        assert!(Code::generate(vec![ip("8.8.8.8")], 5000, false).is_err());
        assert!(Code::generate(vec![ip("127.0.0.1")], 5000, false).is_err());
        assert!(Code::generate(vec![ip("127.0.0.1")], 5000, true).is_ok());
        let seven: Vec<IpAddr> = (1..=7).map(|i| ip(&format!("10.0.0.{i}"))).collect();
        assert!(Code::generate(seven, 5000, false).is_err());
        let a = Code::generate(vec![ip("192.168.1.2")], 5000, false).unwrap();
        let b = Code::generate(vec![ip("192.168.1.2")], 5000, false).unwrap();
        assert_ne!(a.t, b.t);
        assert_ne!(&a.k[..], &b.k[..]);
        assert!(a.to_text().len() < 200);
        // Loopback kod metninde yazılıp okunabilir (testler için).
        let lo = Code::parse(&a.to_text().replace("192.168.1.2", "127.0.0.1")).unwrap();
        assert_eq!(lo.hosts, vec![ip("127.0.0.1")]);
    }
}
