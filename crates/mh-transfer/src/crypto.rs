//! Protokol v1'in kripto katmanı ve tel biçimi (bayt düzeni).
//!
//! Girdiler: QR koddaki `K` (32 B, yalnız QR'da; ağdan geçmez) ve `T` (16 B,
//! teklif kimliği; selamda açık gider).
//!
//! Anahtar türetme (HKDF-SHA256, salt = T, IKM = K, her biri 32 B):
//! - `k_enc`  ← info `"mailhere-aktar-v1 sifre"`  (paket şifreleme)
//! - `k_auth` ← info `"mailhere-aktar-v1 kimlik"` (istemci selamı HMAC'i)
//! - `k_onay` ← info `"mailhere-aktar-v1 onay"`   (onay mesajı şifreleme)
//!
//! İstemci selamı (68 B, telefon → bilgisayar, ilk bağlantı):
//! `b"MHT1" | T(16) | nonce_c(16) | HMAC-SHA256(k_auth, b"istek" | T | nonce_c)(32)`
//!
//! Sunucu yanıtı (bilgisayar → telefon, aynı bağlantı):
//! `len: u32 BE | nonce_s(12) | AES-256-GCM(k_enc, nonce_s, aad = b"MHT1" | T | nonce_c, paket)`
//! `len` kendisinden sonraki bayt sayısıdır (12 + şifreli paket + 16 etiket);
//! düz paket en çok 8 MiB. Yanlış selam yanıtsız kapanır.
//!
//! Onay (58 B, telefon → bilgisayar, ikinci bağlantı):
//! `b"MHT1" | T(16) | nonce_a(12) | AES-256-GCM(k_onay, nonce_a, aad = b"onay" | T, b"uygulandi" | durum u8)`
//!
//! Her anahtar tek bir mesaj için kullanılır; GCM nonce'ları yine de rastgele
//! üretilir. Tüm gizli ara değerler `Zeroizing` ile düşerken sıfırlanır.

use std::fmt;

use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use ring::{hkdf, hmac};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{TransferError, TransferResult};

/// Mesaj başı: protokol v1.
pub const MAGIC: &[u8; 4] = b"MHT1";
/// Teklif kimliği T uzunluğu.
pub const T_LEN: usize = 16;
/// Kök anahtar K uzunluğu.
pub const K_LEN: usize = 32;
/// İstemci selamındaki nonce uzunluğu.
pub const NONCE_C_LEN: usize = 16;
/// AES-GCM nonce uzunluğu.
pub const GCM_NONCE_LEN: usize = 12;
/// AES-GCM etiket uzunluğu.
pub const TAG_LEN: usize = 16;
/// HMAC-SHA256 çıktı uzunluğu.
pub const MAC_LEN: usize = 32;
/// İstemci selamının toplam uzunluğu (68).
pub const HELLO_LEN: usize = MAGIC.len() + T_LEN + NONCE_C_LEN + MAC_LEN;
const CONFIRM_TEXT: &[u8] = b"uygulandi";
/// Onay düz metni: `"uygulandi"` + durum baytı (10).
pub const CONFIRM_PLAIN_LEN: usize = CONFIRM_TEXT.len() + 1;
/// Onay mesajının toplam uzunluğu (58).
pub const CONFIRM_LEN: usize = MAGIC.len() + T_LEN + GCM_NONCE_LEN + CONFIRM_PLAIN_LEN + TAG_LEN;
/// Düz paketin üst sınırı (8 MiB).
pub const MAX_PACKAGE: usize = 8 * 1024 * 1024;
/// Yanıt çerçevesinde `len`'in paket dışı payı: nonce + etiket.
pub const FRAME_OVERHEAD: usize = GCM_NONCE_LEN + TAG_LEN;

const INFO_ENC: &[u8] = b"mailhere-aktar-v1 sifre";
const INFO_AUTH: &[u8] = b"mailhere-aktar-v1 kimlik";
const INFO_CONFIRM: &[u8] = b"mailhere-aktar-v1 onay";
const HELLO_LABEL: &[u8] = b"istek";
const CONFIRM_LABEL: &[u8] = b"onay";

/// Teklif kimliği (açık).
pub type Token = [u8; T_LEN];
/// Kök anahtar (gizli; düşerken sıfırlanır).
pub type Key = Zeroizing<[u8; K_LEN]>;

/// İşletim sisteminin güvenli rastgele kaynağından `N` bayt.
pub fn random<const N: usize>() -> TransferResult<Zeroizing<[u8; N]>> {
    let mut out = Zeroizing::new([0u8; N]);
    SystemRandom::new().fill(&mut out[..]).map_err(|_| {
        TransferError::Internal(
            "Güvenli rastgele değer üretilemedi: işletim sisteminin rastgele sayı kaynağına ulaşılamıyor. Mailhere'yi yeniden başlat."
                .into(),
        )
    })?;
    Ok(out)
}

/// K ve T'den türetilen üç anahtar. `Debug` içeriği göstermez.
pub struct Keys {
    enc: Zeroizing<[u8; 32]>,
    auth: Zeroizing<[u8; 32]>,
    confirm: Zeroizing<[u8; 32]>,
}

impl Keys {
    /// HKDF-SHA256(salt = T, IKM = K) ile üç anahtar.
    pub fn derive(k: &[u8; K_LEN], t: &Token) -> Self {
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, t).extract(k);
        Self {
            enc: expand(&prk, INFO_ENC),
            auth: expand(&prk, INFO_AUTH),
            confirm: expand(&prk, INFO_CONFIRM),
        }
    }

    /// Yalnız testler ve sabit vektör denetimi için ham anahtarlar.
    #[cfg(test)]
    pub(crate) fn raw(&self) -> (&[u8; 32], &[u8; 32], &[u8; 32]) {
        (&self.enc, &self.auth, &self.confirm)
    }
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Keys(<gizli>)")
    }
}

fn expand(prk: &hkdf::Prk, info: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut out = Zeroizing::new([0u8; 32]);
    // HKDF_SHA256 anahtar türü 32 bayttır; bu uzunlukta genişletme hata vermez.
    let parts = [info];
    let okm = prk
        .expand(&parts, hkdf::HKDF_SHA256)
        .expect("HKDF-SHA256 32 bayt genişletme");
    okm.fill(&mut out[..]).expect("HKDF çıktı uzunluğu 32");
    out
}

/// Uzunluk ve içerik karşılaştırması; erken çıkmaz. (Karşılaştırılan
/// değerler gizli değildir; HMAC/GCM doğrulamasını ring yapar.)
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn gcm_key(raw: &[u8; 32]) -> TransferResult<LessSafeKey> {
    UnboundKey::new(&aead::AES_256_GCM, raw)
        .map(LessSafeKey::new)
        .map_err(|_| TransferError::Internal("Şifreleme anahtarı kurulamadı.".into()))
}

// ------------------------------------------------------------ istemci selamı

fn hello_message(
    t: &Token,
    nonce_c: &[u8; NONCE_C_LEN],
) -> [u8; HELLO_LABEL.len() + T_LEN + NONCE_C_LEN] {
    let mut m = [0u8; HELLO_LABEL.len() + T_LEN + NONCE_C_LEN];
    m[..HELLO_LABEL.len()].copy_from_slice(HELLO_LABEL);
    m[HELLO_LABEL.len()..HELLO_LABEL.len() + T_LEN].copy_from_slice(t);
    m[HELLO_LABEL.len() + T_LEN..].copy_from_slice(nonce_c);
    m
}

/// İstemci selamı: `MHT1 | T | nonce_c | HMAC(k_auth, "istek" | T | nonce_c)`.
pub fn build_hello(keys: &Keys, t: &Token, nonce_c: &[u8; NONCE_C_LEN]) -> [u8; HELLO_LEN] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, &keys.auth[..]);
    let tag = hmac::sign(&key, &hello_message(t, nonce_c));
    let mut out = [0u8; HELLO_LEN];
    out[..4].copy_from_slice(MAGIC);
    out[4..4 + T_LEN].copy_from_slice(t);
    out[4 + T_LEN..4 + T_LEN + NONCE_C_LEN].copy_from_slice(nonce_c);
    out[4 + T_LEN + NONCE_C_LEN..].copy_from_slice(tag.as_ref());
    out
}

/// Selam ya da onay doğrulanamadı. Neden söylenmez: çağıran sessizce
/// kapatır ve deneme sayar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejected;

/// Selamı doğrular; geçerliyse `nonce_c` döner. Her hata aynı [`Rejected`].
/// HMAC karşılaştırması sabit sürelidir.
pub fn verify_hello(keys: &Keys, t: &Token, hello: &[u8]) -> Result<[u8; NONCE_C_LEN], Rejected> {
    if hello.len() != HELLO_LEN {
        return Err(Rejected);
    }
    let magic_ok = ct_eq(&hello[..4], MAGIC);
    let t_ok = ct_eq(&hello[4..4 + T_LEN], t);
    let mut nonce_c = [0u8; NONCE_C_LEN];
    nonce_c.copy_from_slice(&hello[4 + T_LEN..4 + T_LEN + NONCE_C_LEN]);
    let key = hmac::Key::new(hmac::HMAC_SHA256, &keys.auth[..]);
    let mac_ok = hmac::verify(
        &key,
        &hello_message(t, &nonce_c),
        &hello[4 + T_LEN + NONCE_C_LEN..],
    )
    .is_ok();
    if magic_ok & t_ok & mac_ok {
        Ok(nonce_c)
    } else {
        Err(Rejected)
    }
}

// -------------------------------------------------------------- paket çerçevesi

fn package_aad(t: &Token, nonce_c: &[u8; NONCE_C_LEN]) -> [u8; 4 + T_LEN + NONCE_C_LEN] {
    let mut a = [0u8; 4 + T_LEN + NONCE_C_LEN];
    a[..4].copy_from_slice(MAGIC);
    a[4..4 + T_LEN].copy_from_slice(t);
    a[4 + T_LEN..].copy_from_slice(nonce_c);
    a
}

/// Paketi şifreler ve tam çerçeveyi (`len | nonce_s | şifreli + etiket`) döner.
/// Çerçeve gizli değildir; düz paket çağıranda kalır.
pub fn seal_package(
    keys: &Keys,
    t: &Token,
    nonce_c: &[u8; NONCE_C_LEN],
    package: &[u8],
) -> TransferResult<Vec<u8>> {
    if package.len() > MAX_PACKAGE {
        return Err(TransferError::Internal(format!(
            "Aktarım paketi çok büyük ({} KB; üst sınır {} KB).",
            package.len() / 1024,
            MAX_PACKAGE / 1024
        )));
    }
    let nonce = random::<GCM_NONCE_LEN>()?;
    let mut buf = Zeroizing::new(Vec::with_capacity(package.len() + TAG_LEN));
    buf.extend_from_slice(package);
    gcm_key(&keys.enc)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(*nonce),
            Aad::from(package_aad(t, nonce_c)),
            &mut *buf,
        )
        .map_err(|_| TransferError::Internal("Paket şifrelenemedi.".into()))?;
    let body_len = GCM_NONCE_LEN + buf.len();
    let mut frame = Vec::with_capacity(4 + body_len);
    frame.extend_from_slice(&(body_len as u32).to_be_bytes());
    frame.extend_from_slice(&nonce[..]);
    frame.extend_from_slice(&buf);
    Ok(frame)
}

/// `len`'den sonraki gövdeyi (`nonce_s | şifreli + etiket`) çözer. Gövde
/// tüketilir ve sıfırlanır; düz paket `Zeroizing` döner.
pub fn open_package(
    keys: &Keys,
    t: &Token,
    nonce_c: &[u8; NONCE_C_LEN],
    mut body: Vec<u8>,
) -> TransferResult<Zeroizing<Vec<u8>>> {
    let fail = || {
        TransferError::Protocol(
            "Bilgisayardan gelen paket çözülemedi: kod eşleşmiyor ya da veri yolda bozuldu. Bilgisayarda yeni kod oluşturup yeniden dene."
                .into(),
        )
    };
    if body.len() < FRAME_OVERHEAD || body.len() > FRAME_OVERHEAD + MAX_PACKAGE {
        body.zeroize();
        return Err(fail());
    }
    let mut nonce = [0u8; GCM_NONCE_LEN];
    nonce.copy_from_slice(&body[..GCM_NONCE_LEN]);
    let key = gcm_key(&keys.enc)?;
    let result = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(package_aad(t, nonce_c)),
            &mut body[GCM_NONCE_LEN..],
        )
        .map(|plain| Zeroizing::new(plain.to_vec()));
    body.zeroize();
    result.map_err(|_| fail())
}

// ----------------------------------------------------------------------- onay

fn confirm_aad(t: &Token) -> [u8; CONFIRM_LABEL.len() + T_LEN] {
    let mut a = [0u8; CONFIRM_LABEL.len() + T_LEN];
    a[..CONFIRM_LABEL.len()].copy_from_slice(CONFIRM_LABEL);
    a[CONFIRM_LABEL.len()..].copy_from_slice(t);
    a
}

/// Onay mesajı: `MHT1 | T | nonce_a | GCM(k_onay, "uygulandi" | durum)`.
pub fn build_confirm(keys: &Keys, t: &Token, status: u8) -> TransferResult<[u8; CONFIRM_LEN]> {
    let nonce = random::<GCM_NONCE_LEN>()?;
    let mut buf = Vec::with_capacity(CONFIRM_PLAIN_LEN + TAG_LEN);
    buf.extend_from_slice(CONFIRM_TEXT);
    buf.push(status);
    gcm_key(&keys.confirm)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(*nonce),
            Aad::from(confirm_aad(t)),
            &mut buf,
        )
        .map_err(|_| TransferError::Internal("Onay mesajı şifrelenemedi.".into()))?;
    let mut out = [0u8; CONFIRM_LEN];
    out[..4].copy_from_slice(MAGIC);
    out[4..4 + T_LEN].copy_from_slice(t);
    out[4 + T_LEN..4 + T_LEN + GCM_NONCE_LEN].copy_from_slice(&nonce[..]);
    out[4 + T_LEN + GCM_NONCE_LEN..].copy_from_slice(&buf);
    Ok(out)
}

/// Onay mesajını çözer; durum baytını döner. Her hata [`Rejected`].
pub fn open_confirm(keys: &Keys, t: &Token, msg: &[u8]) -> Result<u8, Rejected> {
    if msg.len() != CONFIRM_LEN {
        return Err(Rejected);
    }
    let magic_ok = ct_eq(&msg[..4], MAGIC);
    let t_ok = ct_eq(&msg[4..4 + T_LEN], t);
    let mut nonce = [0u8; GCM_NONCE_LEN];
    nonce.copy_from_slice(&msg[4 + T_LEN..4 + T_LEN + GCM_NONCE_LEN]);
    let mut buf = msg[4 + T_LEN + GCM_NONCE_LEN..].to_vec();
    let key = gcm_key(&keys.confirm).map_err(|_| Rejected)?;
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(confirm_aad(t)),
            &mut buf,
        )
        .map_err(|_| Rejected)?;
    let text_ok =
        plain.len() == CONFIRM_PLAIN_LEN && ct_eq(&plain[..CONFIRM_TEXT.len()], CONFIRM_TEXT);
    if magic_ok & t_ok & text_ok {
        Ok(plain[CONFIRM_TEXT.len()])
    } else {
        Err(Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn sabit() -> ([u8; K_LEN], Token) {
        let mut k = [0u8; K_LEN];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        let mut t = [0u8; T_LEN];
        for (i, b) in t.iter_mut().enumerate() {
            *b = 0xa0 + i as u8;
        }
        (k, t)
    }

    #[test]
    fn anahtar_turetme_sabit() {
        // Bağımsız bir HKDF/HMAC uygulamasıyla (Python hashlib) üretilen vektörler.
        let (k, t) = sabit();
        let keys = Keys::derive(&k, &t);
        let (enc, auth, onay) = keys.raw();
        assert_eq!(
            hex(enc),
            "3390075e422dc23c000cfa35627175e1cc31ec416ccdc8de11ae71da63359fa9"
        );
        assert_eq!(
            hex(auth),
            "b48e50d3c04aa7f09514ee6ca549fd19c43fc44d0140a9d59f2321a31c7905de"
        );
        assert_eq!(
            hex(onay),
            "bdd218a30e2f2be7eedd862625ef9d61f1b0ad4816a3624ddec5ae975a7d11dc"
        );
        // Selam HMAC'i de sabit: nonce_c = 0x30..0x3f.
        let mut nonce_c = [0u8; NONCE_C_LEN];
        for (i, b) in nonce_c.iter_mut().enumerate() {
            *b = 0x30 + i as u8;
        }
        let hello = build_hello(&keys, &t, &nonce_c);
        assert_eq!(hello.len(), 68);
        assert_eq!(&hello[..4], b"MHT1");
        assert_eq!(&hello[4..20], &t);
        assert_eq!(&hello[20..36], &nonce_c);
        assert_eq!(
            hex(&hello[36..]),
            "b926b39b8dbbfaed99cfecaa637152e884b8affd001b55b1c35117f045983ef7"
        );
        assert_eq!(format!("{keys:?}"), "Keys(<gizli>)");
    }

    #[test]
    fn selam_dogrulanir() {
        let (k, t) = sabit();
        let keys = Keys::derive(&k, &t);
        let nonce_c = *random::<NONCE_C_LEN>().unwrap();
        let hello = build_hello(&keys, &t, &nonce_c);
        assert_eq!(verify_hello(&keys, &t, &hello), Ok(nonce_c));
        // Tek bayt bozulursa reddedilir; kısa ya da uzun girdi de.
        for i in 0..HELLO_LEN {
            let mut bad = hello;
            bad[i] ^= 0x01;
            assert!(verify_hello(&keys, &t, &bad).is_err(), "bayt {i}");
        }
        assert!(verify_hello(&keys, &t, &hello[..67]).is_err());
        let mut uzun = hello.to_vec();
        uzun.push(0);
        assert!(verify_hello(&keys, &t, &uzun).is_err());
        assert!(verify_hello(&keys, &t, &[]).is_err());
    }

    #[test]
    fn yanlis_k_reddedilir() {
        let (k, t) = sabit();
        let keys = Keys::derive(&k, &t);
        let mut k2 = k;
        k2[31] ^= 0x80;
        let keys2 = Keys::derive(&k2, &t);
        let nonce_c = *random::<NONCE_C_LEN>().unwrap();
        let hello = build_hello(&keys2, &t, &nonce_c);
        assert!(verify_hello(&keys, &t, &hello).is_err());
        // Paket de çözülemez.
        let frame = seal_package(&keys, &t, &nonce_c, b"paket").unwrap();
        assert!(open_package(&keys2, &t, &nonce_c, frame[4..].to_vec()).is_err());
        // Onay da.
        let onay = build_confirm(&keys2, &t, 0).unwrap();
        assert!(open_confirm(&keys, &t, &onay).is_err());
    }

    #[test]
    fn yanlis_t_reddedilir() {
        let (k, t) = sabit();
        let mut t2 = t;
        t2[0] ^= 0xff;
        let keys = Keys::derive(&k, &t);
        let nonce_c = *random::<NONCE_C_LEN>().unwrap();
        // Aynı K, başka T ile üretilmiş selam: anahtarlar farklı türetilir.
        let hello = build_hello(&Keys::derive(&k, &t2), &t2, &nonce_c);
        assert!(verify_hello(&keys, &t, &hello).is_err());
        // Doğru anahtar, mesajda T değiştirilmiş: T alanı ve HMAC uyuşmaz.
        let mut hello = build_hello(&keys, &t, &nonce_c);
        hello[4..20].copy_from_slice(&t2);
        assert!(verify_hello(&keys, &t, &hello).is_err());
        // Paket: aad'deki T farklıysa açılmaz.
        let frame = seal_package(&keys, &t, &nonce_c, b"paket").unwrap();
        assert!(open_package(&keys, &t2, &nonce_c, frame[4..].to_vec()).is_err());
        // Onay: T alanı değiştirilmişse açılmaz.
        let mut onay = build_confirm(&keys, &t, 0).unwrap();
        onay[4..20].copy_from_slice(&t2);
        assert!(open_confirm(&keys, &t, &onay).is_err());
    }

    #[test]
    fn paket_gidip_gelir() {
        let (k, t) = sabit();
        let keys = Keys::derive(&k, &t);
        let nonce_c = *random::<NONCE_C_LEN>().unwrap();
        let package = b"{\"v\":1,\"app\":\"mailhere\"}";
        let frame = seal_package(&keys, &t, &nonce_c, package).unwrap();
        let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(len, frame.len() - 4);
        assert_eq!(len, GCM_NONCE_LEN + package.len() + TAG_LEN);
        // Şifreli metin düz metni içermez.
        assert!(!frame.windows(package.len()).any(|w| w == package));
        let plain = open_package(&keys, &t, &nonce_c, frame[4..].to_vec()).unwrap();
        assert_eq!(&plain[..], package);
        // nonce_c (aad) farklıysa açılmaz; etiket bozuksa açılmaz.
        let mut other = nonce_c;
        other[3] ^= 1;
        assert!(open_package(&keys, &t, &other, frame[4..].to_vec()).is_err());
        let mut bozuk = frame[4..].to_vec();
        let last = bozuk.len() - 1;
        bozuk[last] ^= 1;
        assert!(open_package(&keys, &t, &nonce_c, bozuk).is_err());
        // Boş paket de geçerli.
        let frame = seal_package(&keys, &t, &nonce_c, b"").unwrap();
        assert!(open_package(&keys, &t, &nonce_c, frame[4..].to_vec())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn buyuk_paket_reddedilir() {
        let (k, t) = sabit();
        let keys = Keys::derive(&k, &t);
        let nonce_c = [7u8; NONCE_C_LEN];
        let big = vec![0u8; MAX_PACKAGE + 1];
        let e = seal_package(&keys, &t, &nonce_c, &big).unwrap_err();
        assert!(matches!(e, TransferError::Internal(_)));
        assert!(!e.to_string().is_empty());
        // Tam sınır kabul edilir.
        let ok = vec![0u8; MAX_PACKAGE];
        assert!(seal_package(&keys, &t, &nonce_c, &ok).is_ok());
        // Çözücü: sınırın üstündeki gövde hiç denenmeden reddedilir.
        let body = vec![0u8; FRAME_OVERHEAD + MAX_PACKAGE + 1];
        assert!(open_package(&keys, &t, &nonce_c, body).is_err());
        assert!(open_package(&keys, &t, &nonce_c, vec![0u8; FRAME_OVERHEAD - 1]).is_err());
    }

    #[test]
    fn onay_gidip_gelir() {
        let (k, t) = sabit();
        let keys = Keys::derive(&k, &t);
        for status in [0u8, 1, 2, 200] {
            let msg = build_confirm(&keys, &t, status).unwrap();
            assert_eq!(msg.len(), 58);
            assert_eq!(&msg[..4], b"MHT1");
            assert_eq!(&msg[4..20], &t);
            assert!(!msg.windows(9).any(|w| w == b"uygulandi"));
            assert_eq!(open_confirm(&keys, &t, &msg), Ok(status));
        }
        let msg = build_confirm(&keys, &t, 0).unwrap();
        for i in 0..CONFIRM_LEN {
            let mut bad = msg;
            bad[i] ^= 0x01;
            assert!(open_confirm(&keys, &t, &bad).is_err(), "bayt {i}");
        }
        assert!(open_confirm(&keys, &t, &msg[..57]).is_err());
    }
}
