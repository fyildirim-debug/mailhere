//! Veritabanı şifreleme anahtarı ve anahtarın saklandığı yer.
//!
//! Anahtar ilk açılışta rastgele üretilen 256 bitlik ham SQLCipher anahtarıdır
//! ve yalnız işletim sisteminin anahtar zincirinde (bellek kipinde süreç
//! belleğinde) durur. Veritabanına, günlüğe ya da hata mesajına yazılmaz;
//! bellekteki kopyaları düşerken sıfırlanır.

use std::fmt;
use std::path::Path;

use mh_core::{AppError, AppResult, ErrorCode};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::{Zeroize, Zeroizing};

use crate::encryption::{self, DbFile};
use crate::Store;

/// Anahtarın anahtar zincirindeki kullanıcı adı (hizmet adı parolalarla aynı).
pub const DB_KEY_SECRET: &str = "db-key";

pub(crate) const KEY_LEN: usize = 32;

/// 256 bitlik ham veritabanı anahtarı.
#[derive(Clone)]
pub struct DbKey(Zeroizing<[u8; KEY_LEN]>);

impl DbKey {
    /// İşletim sisteminin güvenli rastgele kaynağından yeni anahtar.
    pub fn generate() -> AppResult<Self> {
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        SystemRandom::new().fill(&mut key[..]).map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "Veritabanı anahtarı üretilemedi: işletim sisteminin güvenli rastgele sayı kaynağına ulaşılamadı. Mailhere'yi yeniden başlat.",
            )
        })?;
        Ok(Self(key))
    }

    /// Anahtar zincirinden okunan onaltılık metinden anahtar. Metin, sonuç ne
    /// olursa olsun sıfırlanır.
    pub fn from_secret(mut secret: String) -> AppResult<Self> {
        let key = parse_hex(secret.trim());
        secret.zeroize();
        key.map(Self).ok_or_else(|| {
            AppError::new(
                ErrorCode::Keyring,
                format!(
                    "Anahtar zincirindeki veritabanı anahtarı (Mailhere / {DB_KEY_SECRET}) bozuk; yerel postalar açılamıyor. Kaydı değiştirdiysen eski hâline getir. Veriler silinmedi."
                ),
            )
        })
    }

    /// Anahtar zincirine yazılacak biçim: 64 küçük onaltılık karakter.
    pub fn to_secret(&self) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::with_capacity(KEY_LEN * 2));
        for b in self.0.iter() {
            out.push(hex_digit(b >> 4));
            out.push(hex_digit(b & 0x0f));
        }
        out
    }

    /// Ham baytlar (parola kilidi anahtarı sarar; `lock.rs`).
    pub(crate) fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// Ham baytlardan anahtar (parola kilidi açılınca; `lock.rs`).
    pub(crate) fn from_bytes(bytes: Zeroizing<[u8; KEY_LEN]>) -> Self {
        Self(bytes)
    }

    /// SQLCipher'in ham anahtar biçimi: `x'<64 onaltılık>'` (PBKDF2 uygulanmaz).
    pub(crate) fn raw_key_literal(&self) -> Zeroizing<String> {
        let hex = self.to_secret();
        let mut out = Zeroizing::new(String::with_capacity(hex.len() + 3));
        out.push_str("x'");
        out.push_str(&hex);
        out.push('\'');
        out
    }
}

impl PartialEq for DbKey {
    fn eq(&self, other: &Self) -> bool {
        // Sabit süreli karşılaştırma; anahtarlar yalnız doğrulamada karşılaştırılır.
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

impl Eq for DbKey {}

impl fmt::Debug for DbKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DbKey(gizli)")
    }
}

fn hex_digit(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'a' + n - 10 })
}

fn parse_hex(s: &str) -> Option<Zeroizing<[u8; KEY_LEN]>> {
    let bytes = s.as_bytes();
    if bytes.len() != KEY_LEN * 2 {
        return None;
    }
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out[i] = (hi << 4 | lo) as u8;
    }
    Some(out)
}

/// Anahtarın saklandığı yer: işletim sisteminin anahtar zinciri ya da (tanıtım
/// ve deneme kipinde) süreç belleği.
pub trait KeyVault {
    /// Kayıtlı anahtar metni; kayıt yoksa `None`. Erişim hatası `Err` döner.
    fn load(&self) -> AppResult<Option<String>>;
    /// Anahtar metnini kaydeder.
    fn save(&self, secret: &str) -> AppResult<()>;
    /// Anahtar süreç bitince kayboluyorsa (bellek kipi) `false`.
    fn persistent(&self) -> bool;
}

impl Store {
    /// Diskteki şifreli veritabanını açar; anahtarı `vault`tan alır.
    ///
    /// - İlk açılışta (veritabanı yok ya da önceki sürümden kalma şifresiz)
    ///   anahtar yoksa üretilir, kaydedilir ve geri okunarak doğrulanır.
    /// - Anahtar zincirine ulaşılamazsa veritabanı şifresiz açılmaz; hata döner.
    /// - Veritabanı şifreli ama anahtar yoksa ya da uymuyorsa hata döner;
    ///   hiçbir dosya silinmez.
    pub fn open_with_vault(path: &Path, vault: &dyn KeyVault) -> AppResult<Self> {
        encryption::recover(path)?;
        let state = encryption::probe(path).map_err(|e| encryption::io_error(path, &e))?;
        if !vault.persistent() && state != DbFile::Missing {
            return Err(AppError::new(
                ErrorCode::Invalid,
                format!(
                    "Bellek kipinde veritabanı anahtarı saklanmadığı için var olan veritabanı ({}) açılamaz ya da şifrelenemez. MAILHERE_DATA_DIR ile boş bir klasör göster ya da MAILHERE_SECRETS=memory ayarını kaldır.",
                    path.display()
                ),
            ));
        }
        let key = match load_key(vault)? {
            Some(key) => key,
            None if state == DbFile::Encrypted => return Err(key_missing_error(path)),
            None => {
                let key = DbKey::generate()?;
                vault
                    .save(&key.to_secret())
                    .map_err(|e| keyring_unavailable(&e))?;
                // Bazı anahtar zinciri arka uçları yazmayı kabul edip saklamaz;
                // geri okunamayan anahtarla veri şifrelenirse sonra açılamaz.
                match load_key(vault)? {
                    Some(back) if back == key => {}
                    _ => {
                        tracing::error!("veritabanı anahtarı anahtar zincirinden geri okunamadı");
                        return Err(keyring_unavailable(&AppError::new(
                            ErrorCode::Keyring,
                            "geri okuma tutmadı",
                        )));
                    }
                }
                tracing::info!(
                    persistent = vault.persistent(),
                    "yeni veritabanı anahtarı oluşturuldu"
                );
                key
            }
        };
        Self::open(path, &key)
    }
}

fn load_key(vault: &dyn KeyVault) -> AppResult<Option<DbKey>> {
    match vault.load() {
        Ok(Some(secret)) => DbKey::from_secret(secret).map(Some),
        Ok(None) => Ok(None),
        Err(e) => Err(keyring_unavailable(&e)),
    }
}

pub(crate) fn keyring_unavailable(cause: &AppError) -> AppError {
    // Asıl hata metni anahtar içermez; tanı için günlüğe yazılır.
    tracing::error!(error = %cause, "veritabanı anahtarına ulaşılamadı");
    AppError::new(
        ErrorCode::Keyring,
        "Yerel veritabanının şifreleme anahtarına ulaşılamadı: işletim sisteminin anahtar zinciri yanıt vermiyor. Linux'ta GNOME Anahtarlık ya da KWallet gibi bir Secret Service hizmetinin çalıştığından emin olup Mailhere'yi yeniden başlat. Mailhere postalarını şifresiz saklamaz.",
    )
}

pub(crate) fn key_missing_error(path: &Path) -> AppError {
    tracing::error!(path = %path.display(), "veritabanı şifreli ama anahtar yok");
    AppError::new(
        ErrorCode::Keyring,
        format!(
            "Yerel veritabanı şifreli, ama anahtarı işletim sisteminin anahtar zincirinde (Mailhere / {DB_KEY_SECRET}) yok; postalar bu yüzden açılamıyor. Anahtar zincirini yedekten geri yükleyebilirsin. Baştan başlamak istersen {} dosyasını başka bir klasöre taşıyıp Mailhere'yi yeniden başlat; hesaplarını yeniden eklemen gerekir. Dosya silinmedi.",
            path.display()
        ),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    /// Testler için bellek içi kasa; hata ve "yazar ama saklamaz" taklidi yapar.
    #[derive(Default)]
    pub struct TestVault {
        pub secret: RefCell<Option<String>>,
        pub fail_load: Cell<bool>,
        pub fail_save: Cell<bool>,
        pub forget_on_save: Cell<bool>,
        pub ephemeral: Cell<bool>,
    }

    impl KeyVault for TestVault {
        fn load(&self) -> AppResult<Option<String>> {
            if self.fail_load.get() {
                return Err(AppError::new(ErrorCode::Keyring, "hizmet yok"));
            }
            Ok(self.secret.borrow().clone())
        }

        fn save(&self, secret: &str) -> AppResult<()> {
            if self.fail_save.get() {
                return Err(AppError::new(ErrorCode::Keyring, "hizmet yok"));
            }
            if !self.forget_on_save.get() {
                *self.secret.borrow_mut() = Some(secret.to_owned());
            }
            Ok(())
        }

        fn persistent(&self) -> bool {
            !self.ephemeral.get()
        }
    }

    #[test]
    fn anahtar_metni_gidip_gelir() {
        let k = DbKey::generate().unwrap();
        let hex = k.to_secret();
        assert_eq!(hex.len(), 64);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        let hex: &str = &hex;
        let back = DbKey::from_secret(format!("  {hex}\n")).unwrap();
        assert_eq!(back, k);
        assert_ne!(DbKey::generate().unwrap(), k, "her anahtar rastgele");
        assert_eq!(*k.raw_key_literal(), format!("x'{hex}'"));
        let upper = DbKey::from_secret(hex.to_uppercase()).unwrap();
        assert_eq!(upper, k);
    }

    #[test]
    fn bozuk_anahtar_metni_reddedilir() {
        let wrong = "b".repeat(64);
        for bad in ["", "abc", &"g".repeat(64), &"a".repeat(63), &"a".repeat(66)] {
            let e = DbKey::from_secret(bad.to_owned()).unwrap_err();
            assert_eq!(e.code, ErrorCode::Keyring);
        }
        // Geçerli biçim kabul edilir; hata mesajları anahtar metnini içermez.
        assert!(DbKey::from_secret(wrong.clone()).is_ok());
        let e = DbKey::from_secret(format!("{wrong}z")).unwrap_err();
        assert!(!e.message.contains(&wrong));
    }

    #[test]
    fn hata_ayiklama_ciktisi_anahtari_gostermez() {
        let k = DbKey::generate().unwrap();
        let shown = format!("{k:?}");
        assert_eq!(shown, "DbKey(gizli)");
        assert!(!shown.contains(&*k.to_secret()));
    }
}
