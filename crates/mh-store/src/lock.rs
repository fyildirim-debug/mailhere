//! Parola kilidi: veritabanı anahtarının parolayla sarılması.
//!
//! Kilit açıkken 256 bitlik ham SQLCipher anahtarı (`DbKey`) anahtar
//! zincirinde düz durmaz; parola ve 16 baytlık tuzdan **Argon2id** ile
//! türetilen bir anahtarla (KEK) **AES-256-GCM** altında sarılır ve
//! sürümlü bir JSON kaydı ([`LockRecord`], anahtar zincirinde
//! [`LOCK_RECORD_SECRET`]) olarak saklanır; düz `db-key` kaydı silinir.
//! Kayıt yalnız parolayla açılır: parola olmadan yerel posta, taslak, ayar
//! ve hesap bilgisi okunamaz.
//!
//! - KDF parametreleri kayıtta durur (ileride artırılabilir; eski kayıt
//!   kendi parametreleriyle açılır). Yazarken OWASP tabanının altına
//!   inilmez ([`KdfParams::MIN`]).
//! - Başlık alanları (sürüm, KDF, parametreler, tuz) GCM'in ek doğrulanan
//!   verisidir (AAD): kaydı değiştiren biri şifreli metni açamaz.
//! - Yanlış parola ile tampere edilmiş kayıt birbirinden ayrılamaz (ikisi de
//!   etiket hatasıdır); ikisi de "parola yanlış" olarak bildirilir, kayıt
//!   biçimi bozuksa (JSON, base64, uzunluk) ayrı hata döner.
//! - Parola, KEK ve açılan anahtar bellekte `Zeroizing` ile tutulur; hata
//!   metinleri ve günlük parola ya da anahtar içermez.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use mh_core::{AppError, AppResult, ErrorCode};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::key::{DbKey, KEY_LEN};

/// Sarılı anahtar kaydının anahtar zincirindeki adı (düz anahtar `db-key`).
pub const LOCK_RECORD_SECRET: &str = "db-key-kilitli";

/// Kayıt biçiminin sürümü.
pub const RECORD_VERSION: u32 = 1;
const KDF_NAME: &str = "argon2id";
const AEAD_NAME: &str = "aes-256-gcm";
const SALT_LEN: usize = 16;
const TAG_LEN: usize = 16;

/// Argon2id parametreleri: bellek (KiB), geçiş sayısı, paralellik.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// Masaüstü: 64 MiB, 3 geçiş (orta bir dizüstünde ~0,2–0,4 s).
    pub const DESKTOP: Self = Self {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };
    /// Telefon: 32 MiB, 3 geçiş (orta sınıf cihazda ~0,3–0,6 s; OWASP
    /// tabanı 19 MiB / 2 geçişin üstünde).
    pub const MOBILE: Self = Self {
        m_kib: 32 * 1024,
        t: 3,
        p: 1,
    };
    /// Yazılan kayıtlarda inilmeyen taban (OWASP: m=19 MiB, t=2, p=1).
    pub const MIN: Self = Self {
        m_kib: 19 * 1024,
        t: 2,
        p: 1,
    };

    /// Tabanın altındaki değerleri tabana çeker.
    pub fn floored(self) -> Self {
        Self {
            m_kib: self.m_kib.max(Self::MIN.m_kib),
            t: self.t.max(Self::MIN.t),
            p: self.p.max(Self::MIN.p),
        }
    }
}

/// Anahtar zincirine yazılan kayıt. Yalnız `ct` gizlidir; başlık alanları
/// AAD olarak doğrulanır, `relock` ve `secrets` düz tercihlerdir.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LockRecord {
    pub v: u32,
    pub kdf: String,
    pub m: u32,
    pub t: u32,
    pub p: u32,
    pub aead: String,
    /// 16 bayt, base64.
    pub salt: String,
    /// 12 bayt, base64.
    pub nonce: String,
    /// 32 bayt anahtar + 16 bayt etiket, base64.
    pub ct: String,
    /// "Pencereyi her açışta parola sor" tercihi (varsayılan açık).
    #[serde(default = "default_true")]
    pub relock: bool,
    /// Hesapların anahtar zincirindeki gizli değer adları (parola, OAuth
    /// jetonu). Parola unutulup yerel veri sıfırlanınca bunlar da silinir;
    /// veritabanı açılamadığından adlar başka yerden öğrenilemez.
    #[serde(default)]
    pub secrets: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl LockRecord {
    /// Anahtar zincirinden okunan metinden kayıt; biçim bozuksa hata.
    pub fn parse(json: &str) -> AppResult<Self> {
        let rec: Self = serde_json::from_str(json).map_err(|e| {
            tracing::error!(error = %e, "kilit kaydı okunamadı");
            corrupt()
        })?;
        if rec.v != RECORD_VERSION {
            tracing::error!(v = rec.v, "kilit kaydının sürümü tanınmıyor");
            return Err(AppError::new(
                ErrorCode::Keyring,
                "Parola kilidi kaydı daha yeni bir Mailhere sürümüyle yazılmış. Mailhere'yi güncelleyip yeniden dene.",
            ));
        }
        if rec.kdf != KDF_NAME || rec.aead != AEAD_NAME {
            return Err(corrupt());
        }
        Ok(rec)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn params(&self) -> KdfParams {
        KdfParams {
            m_kib: self.m,
            t: self.t,
            p: self.p,
        }
    }

    /// AAD: sürüm, KDF, parametreler ve tuz; bunlardan biri değişirse
    /// şifreli metin açılmaz.
    fn aad(&self) -> String {
        format!(
            "mailhere-kilit/v{}/{}/{}/{}/{}/{}/{}",
            self.v, self.kdf, self.m, self.t, self.p, self.aead, self.salt
        )
    }
}

/// `unwrap` sonucu.
#[derive(Debug)]
pub enum UnwrapError {
    /// Parola yanlış (ya da kayıt değiştirilmiş; ikisi ayrılamaz).
    WrongPassword,
    /// Kayıt biçimi bozuk ya da KDF parametreleri geçersiz.
    Corrupt(AppError),
}

impl fmt::Display for UnwrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnwrapError::WrongPassword => f.write_str("parola yanlış"),
            UnwrapError::Corrupt(e) => f.write_str(&e.message),
        }
    }
}

fn corrupt() -> AppError {
    AppError::new(
        ErrorCode::Keyring,
        "Parola kilidi kaydı bozuk; yerel postalar açılamıyor. Anahtar zincirini yedekten geri yükleyebilirsin ya da \"Parolamı unuttum\" ile yerel veriyi sıfırlayıp hesapları yeniden ekleyebilirsin.",
    )
}

fn rng_fail() -> AppError {
    AppError::new(
        ErrorCode::Internal,
        "Rastgele değer üretilemedi: işletim sisteminin güvenli rastgele sayı kaynağına ulaşılamadı. Yeniden dene.",
    )
}

/// Parola + tuz → 32 baytlık sarma anahtarı (Argon2id).
fn derive(
    password: &[u8],
    salt: &[u8],
    params: KdfParams,
) -> Result<Zeroizing<[u8; 32]>, AppError> {
    let p = Params::new(params.m_kib, params.t, params.p, Some(32)).map_err(|e| {
        tracing::error!(error = %e, "KDF parametreleri geçersiz");
        corrupt()
    })?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
    let mut kek = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password, salt, &mut kek[..])
        .map_err(|e| {
            tracing::error!(error = %e, "anahtar türetilemedi");
            AppError::new(
                ErrorCode::Internal,
                "Parola anahtarı türetilemedi (bellek yetersiz olabilir). Yeniden dene.",
            )
        })?;
    Ok(kek)
}

/// Veritabanı anahtarını parolayla sarar; yeni tuz ve nonce üretir.
/// `params` tabanın altındaysa tabana çekilir.
pub fn wrap(
    key: &DbKey,
    password: &[u8],
    params: KdfParams,
    relock: bool,
) -> AppResult<LockRecord> {
    let params = params.floored();
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut salt).map_err(|_| rng_fail())?;
    rng.fill(&mut nonce).map_err(|_| rng_fail())?;
    let mut rec = LockRecord {
        v: RECORD_VERSION,
        kdf: KDF_NAME.to_owned(),
        m: params.m_kib,
        t: params.t,
        p: params.p,
        aead: AEAD_NAME.to_owned(),
        salt: B64.encode(salt),
        nonce: B64.encode(nonce),
        ct: String::new(),
        relock,
        secrets: Vec::new(),
    };
    let kek = derive(password, &salt, params)?;
    let aead = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &kek[..]).map_err(|_| corrupt())?);
    let mut buf = Zeroizing::new(key.as_bytes().to_vec());
    aead.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(rec.aad().as_bytes()),
        &mut *buf,
    )
    .map_err(|_| corrupt())?;
    rec.ct = B64.encode(&*buf);
    Ok(rec)
}

/// Kaydı parolayla açar; parola yanlışsa `WrongPassword`.
pub fn unwrap(rec: &LockRecord, password: &[u8]) -> Result<DbKey, UnwrapError> {
    let salt = B64
        .decode(&rec.salt)
        .ok()
        .filter(|s| s.len() == SALT_LEN)
        .ok_or_else(|| UnwrapError::Corrupt(corrupt()))?;
    let nonce: [u8; NONCE_LEN] = B64
        .decode(&rec.nonce)
        .ok()
        .and_then(|n| n.try_into().ok())
        .ok_or_else(|| UnwrapError::Corrupt(corrupt()))?;
    let mut buf = Zeroizing::new(
        B64.decode(&rec.ct)
            .ok()
            .filter(|c| c.len() == KEY_LEN + TAG_LEN)
            .ok_or_else(|| UnwrapError::Corrupt(corrupt()))?,
    );
    let kek = derive(password, &salt, rec.params()).map_err(UnwrapError::Corrupt)?;
    let aead = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &kek[..]).map_err(|_| UnwrapError::Corrupt(corrupt()))?,
    );
    let plain = aead
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(rec.aad().as_bytes()),
            buf.as_mut_slice(),
        )
        .map_err(|_| UnwrapError::WrongPassword)?;
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    out.copy_from_slice(&plain[..KEY_LEN]);
    Ok(DbKey::from_bytes(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Testler hızlı kalsın: taban parametreler (yazarken tabana çekilir).
    const FAST: KdfParams = KdfParams::MIN;

    #[test]
    fn sar_ac_gidip_gelir() {
        let key = DbKey::generate().unwrap();
        let rec = wrap(&key, "çok gizli parola".as_bytes(), FAST, true).unwrap();
        assert_eq!(rec.v, RECORD_VERSION);
        assert_eq!(rec.params(), FAST);
        assert!(rec.relock);
        let json = rec.to_json();
        assert!(
            !json.contains(&*key.to_secret()),
            "düz anahtar kayda yazılmaz"
        );
        let back = LockRecord::parse(&json).unwrap();
        assert_eq!(back, rec);
        let opened = unwrap(&back, "çok gizli parola".as_bytes()).unwrap();
        assert_eq!(opened, key);
    }

    #[test]
    fn yanlis_parola_reddedilir() {
        let key = DbKey::generate().unwrap();
        let rec = wrap(&key, "doğru parola".as_bytes(), FAST, true).unwrap();
        assert!(matches!(
            unwrap(&rec, "yanlış parola".as_bytes()),
            Err(UnwrapError::WrongPassword)
        ));
        assert!(matches!(unwrap(&rec, b""), Err(UnwrapError::WrongPassword)));
    }

    #[test]
    fn her_sarma_farkli_tuz_ve_nonce_uretir() {
        let key = DbKey::generate().unwrap();
        let a = wrap(&key, b"parola1234", FAST, true).unwrap();
        let b = wrap(&key, b"parola1234", FAST, true).unwrap();
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ct, b.ct);
    }

    #[test]
    fn degistirilen_kayit_acilmaz() {
        let key = DbKey::generate().unwrap();
        let rec = wrap(&key, b"parola1234", FAST, true).unwrap();
        // Parametreler AAD'dedir: zayıflatılmış kayıt doğru parolayla da açılmaz.
        let mut weak = rec.clone();
        weak.t = 1;
        assert!(matches!(
            unwrap(&weak, b"parola1234"),
            Err(UnwrapError::WrongPassword)
        ));
        // Şifreli metnin bir baytı değişince etiket tutmaz.
        let mut ct = B64.decode(&rec.ct).unwrap();
        ct[3] ^= 0x01;
        let mut tampered = rec.clone();
        tampered.ct = B64.encode(ct);
        assert!(matches!(
            unwrap(&tampered, b"parola1234"),
            Err(UnwrapError::WrongPassword)
        ));
        // Düz tercihler AAD dışındadır: değişmeleri açmayı bozmaz.
        let mut prefs = rec.clone();
        prefs.relock = false;
        prefs.secrets = vec!["hesap-1".into()];
        assert!(unwrap(&prefs, b"parola1234").is_ok());
    }

    #[test]
    fn bozuk_kayit_ayri_hata_verir() {
        let key = DbKey::generate().unwrap();
        let rec = wrap(&key, b"parola1234", FAST, true).unwrap();
        let mut short = rec.clone();
        short.ct = B64.encode([1u8; 10]);
        assert!(matches!(
            unwrap(&short, b"parola1234"),
            Err(UnwrapError::Corrupt(_))
        ));
        let mut bad_salt = rec.clone();
        bad_salt.salt = "***".into();
        assert!(matches!(
            unwrap(&bad_salt, b"parola1234"),
            Err(UnwrapError::Corrupt(_))
        ));
        let mut bad_params = rec.clone();
        bad_params.m = 1;
        assert!(matches!(
            unwrap(&bad_params, b"parola1234"),
            Err(UnwrapError::Corrupt(_))
        ));
        assert_eq!(LockRecord::parse("{").unwrap_err().code, ErrorCode::Keyring);
        let mut other = rec.clone();
        other.kdf = "pbkdf2".into();
        assert!(LockRecord::parse(&other.to_json()).is_err());
    }

    #[test]
    fn yeni_surum_kaydi_guncelleme_ister() {
        let key = DbKey::generate().unwrap();
        let mut rec = wrap(&key, b"parola1234", FAST, true).unwrap();
        rec.v = RECORD_VERSION + 1;
        let e = LockRecord::parse(&rec.to_json()).unwrap_err();
        assert_eq!(e.code, ErrorCode::Keyring);
        assert!(e.message.contains("güncelle"));
    }

    #[test]
    fn eski_kayitta_eksik_tercihler_varsayilani_alir() {
        let key = DbKey::generate().unwrap();
        let rec = wrap(&key, b"parola1234", FAST, false).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&rec.to_json()).unwrap();
        v.as_object_mut().unwrap().remove("relock");
        v.as_object_mut().unwrap().remove("secrets");
        let back = LockRecord::parse(&v.to_string()).unwrap();
        assert!(back.relock, "tercih yoksa yeniden kilitleme açık");
        assert!(back.secrets.is_empty());
    }

    #[test]
    fn parametreler_tabanin_altina_inmez() {
        let weak = KdfParams {
            m_kib: 8,
            t: 1,
            p: 0,
        };
        assert_eq!(weak.floored(), KdfParams::MIN);
        assert_eq!(KdfParams::DESKTOP.floored(), KdfParams::DESKTOP);
        assert_eq!(KdfParams::MOBILE.floored(), KdfParams::MOBILE);
        let key = DbKey::generate().unwrap();
        let rec = wrap(&key, b"parola1234", weak, true).unwrap();
        assert_eq!(rec.params(), KdfParams::MIN);
    }

    #[test]
    fn hata_metinleri_parola_ve_anahtar_tasimaz() {
        let key = DbKey::generate().unwrap();
        let rec = wrap(&key, b"parola1234", FAST, true).unwrap();
        let e = format!("{}", unwrap(&rec, b"yanlis").unwrap_err());
        assert!(!e.contains("yanlis"));
        assert!(!e.contains(&*key.to_secret()));
        assert!(!corrupt().message.contains(&rec.ct));
    }
}
