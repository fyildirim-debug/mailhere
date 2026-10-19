//! Abone kayıtları: bellekte tutulur, her değişiklikte JSON dosyasına
//! atomik yazılır (geçici dosya + yeniden adlandırma). Birkaç yüz cihaz için
//! veritabanı gerekmez.
//!
//! Gizli değerler düz tutulmaz: cihazın yönetim anahtarı (`gizli`) yalnız
//! SHA-256 özetiyle saklanır. Sağlayıcının çağıracağı adresteki anahtar
//! (`url_anahtar`) sunucunun posta sunucusuna verdiği adresin parçasıdır;
//! karşılaştırmak için düz tutulması gerekir (aboneyi tetiklemekten öte yetki
//! vermez: en fazla "yeni posta var" sinyali yollatır).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

/// Desteklenen sağlayıcılar. Yalnız etkin olanlara kayıt açılır
/// (`MH_PROVIDERS`); kapalı sağlayıcının uçları 404 döner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Saglayici {
    /// JMAP PushSubscription (RFC 8620 §7.2): Stalwart vb.
    Jmap,
    /// Gmail API `users.watch` + Google Cloud Pub/Sub itmesi.
    Gmail,
    /// Microsoft Graph değişiklik bildirimi (Outlook, Microsoft 365).
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

    pub fn from_ad(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "jmap" => Some(Self::Jmap),
            "gmail" => Some(Self::Gmail),
            "graph" => Some(Self::Graph),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Abone {
    pub id: String,
    /// Cihazın yönetim anahtarının SHA-256 özeti (onaltılık).
    pub gizli_ozet: String,
    pub saglayici: Saglayici,
    /// Sağlayıcının çağıracağı adresteki rastgele anahtar (JMAP, Graph
    /// `clientState`). Gmail'de kullanılmaz.
    pub url_anahtar: String,
    /// Gmail: posta adresinin SHA-256 özeti (Pub/Sub iletisi adresle gelir).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hesap_ozet: Option<String>,
    /// Firebase kayıt jetonu.
    pub fcm: String,
    pub olusturma: i64,
    /// Cihazın son görülmesi (kayıt, jeton güncelleme, canlılık).
    pub son_gorulme: i64,
}

/// Uzun süre görülmeyen abone silinir: cihaz haftada bir canlılık bildirir.
pub const BAYAT_SN: i64 = 30 * 24 * 3600;
/// Toplam abone üst sınırı (kötüye kullanıma karşı).
pub const EN_COK_ABONE: usize = 20_000;

pub fn rastgele(bayt: usize) -> String {
    let mut b = vec![0u8; bayt];
    // SystemRandom yalnız işletim sistemi rastgelesi bozuksa hata verir.
    SystemRandom::new()
        .fill(&mut b)
        .expect("işletim sistemi rastgele kaynağı");
    URL_SAFE_NO_PAD.encode(b)
}

pub fn ozet(s: &str) -> String {
    digest::digest(&digest::SHA256, s.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Sabit zamanlı karşılaştırma (anahtar tahmini zamanlamadan sızmasın).
pub fn esit(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, PartialEq)]
pub enum KayitHata {
    Dolu,
}

pub struct Depo {
    yol: Option<PathBuf>,
    aboneler: HashMap<String, Abone>,
}

impl Depo {
    /// Bellek içi (testler).
    #[cfg(test)]
    pub fn bellek() -> Self {
        Self {
            yol: None,
            aboneler: HashMap::new(),
        }
    }

    /// Dosyadan açar; dosya yoksa boş başlar. Bozuk dosya hatadır (sessizce
    /// silinip tüm cihazlar düşmesin).
    pub fn ac(yol: &Path) -> std::io::Result<Self> {
        let aboneler = match std::fs::read(yol) {
            Ok(b) => {
                let liste: Vec<Abone> = serde_json::from_slice(&b)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                liste.into_iter().map(|a| (a.id.clone(), a)).collect()
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            yol: Some(yol.to_owned()),
            aboneler,
        })
    }

    fn yaz(&self) {
        let Some(yol) = &self.yol else { return };
        let mut liste: Vec<&Abone> = self.aboneler.values().collect();
        liste.sort_by(|a, b| a.id.cmp(&b.id));
        let gecici = yol.with_extension("json.yeni");
        let sonuc = serde_json::to_vec(&liste)
            .map_err(std::io::Error::other)
            .and_then(|b| std::fs::write(&gecici, b))
            .and_then(|_| std::fs::rename(&gecici, yol));
        if let Err(e) = sonuc {
            tracing::error!(error = %e, "abone dosyası yazılamadı");
        }
    }

    pub fn sayi(&self) -> usize {
        self.aboneler.len()
    }

    /// Yeni abone; (id, gizli) döner. `gizli` yalnız bu yanıtta görünür.
    pub fn ekle(
        &mut self,
        saglayici: Saglayici,
        fcm: String,
        hesap_ozet: Option<String>,
        simdi: i64,
    ) -> Result<(Abone, String), KayitHata> {
        if self.aboneler.len() >= EN_COK_ABONE {
            return Err(KayitHata::Dolu);
        }
        let gizli = rastgele(32);
        let a = Abone {
            id: rastgele(16),
            gizli_ozet: ozet(&gizli),
            saglayici,
            url_anahtar: rastgele(32),
            hesap_ozet,
            fcm,
            olusturma: simdi,
            son_gorulme: simdi,
        };
        self.aboneler.insert(a.id.clone(), a.clone());
        self.yaz();
        Ok((a, gizli))
    }

    pub fn al(&self, id: &str) -> Option<&Abone> {
        self.aboneler.get(id)
    }

    /// Cihaz yetkisi: id ve gizli eşleşiyor mu?
    pub fn yetkili(&self, id: &str, gizli: &str) -> bool {
        self.aboneler
            .get(id)
            .is_some_and(|a| esit(&a.gizli_ozet, &ozet(gizli)))
    }

    /// Jeton güncelleme ya da canlılık (fcm `None`).
    pub fn guncelle(&mut self, id: &str, fcm: Option<String>, simdi: i64) -> bool {
        let Some(a) = self.aboneler.get_mut(id) else {
            return false;
        };
        if let Some(f) = fcm {
            a.fcm = f;
        }
        a.son_gorulme = simdi;
        self.yaz();
        true
    }

    pub fn sil(&mut self, id: &str) -> bool {
        let var = self.aboneler.remove(id).is_some();
        if var {
            self.yaz();
        }
        var
    }

    /// Firebase "kayıtsız jeton" dedi: bu jetonu taşıyan tüm aboneler düşer.
    pub fn jetonu_sil(&mut self, fcm: &str) -> usize {
        let once = self.aboneler.len();
        self.aboneler.retain(|_, a| a.fcm != fcm);
        let n = once - self.aboneler.len();
        if n > 0 {
            self.yaz();
        }
        n
    }

    /// Gmail iletisi: posta adresinin özetine bağlı aboneler.
    pub fn hesaba_gore(&self, saglayici: Saglayici, hesap_ozet: &str) -> Vec<Abone> {
        self.aboneler
            .values()
            .filter(|a| a.saglayici == saglayici && a.hesap_ozet.as_deref() == Some(hesap_ozet))
            .cloned()
            .collect()
    }

    /// Bayat aboneleri siler; silinen sayısı.
    pub fn temizle(&mut self, simdi: i64) -> usize {
        let once = self.aboneler.len();
        self.aboneler
            .retain(|_, a| simdi.saturating_sub(a.son_gorulme) < BAYAT_SN);
        let n = once - self.aboneler.len();
        if n > 0 {
            self.yaz();
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ekle_yetki_guncelle_sil() {
        let mut d = Depo::bellek();
        let (a, gizli) = d
            .ekle(Saglayici::Jmap, "jeton-1".into(), None, 100)
            .unwrap();
        assert_eq!(a.id.len(), 22);
        assert_eq!(a.url_anahtar.len(), 43);
        assert!(d.yetkili(&a.id, &gizli));
        assert!(!d.yetkili(&a.id, "yanlis"));
        assert!(!d.yetkili("yok", &gizli));
        // Gizli düz saklanmaz.
        assert_ne!(d.al(&a.id).unwrap().gizli_ozet, gizli);
        assert!(d.guncelle(&a.id, Some("jeton-2".into()), 200));
        assert_eq!(d.al(&a.id).unwrap().fcm, "jeton-2");
        assert_eq!(d.al(&a.id).unwrap().son_gorulme, 200);
        assert!(d.sil(&a.id));
        assert!(!d.sil(&a.id));
        assert_eq!(d.sayi(), 0);
    }

    #[test]
    fn bayat_ve_kayitsiz_jeton_temizlenir() {
        let mut d = Depo::bellek();
        let (eski, _) = d.ekle(Saglayici::Jmap, "a".into(), None, 0).unwrap();
        let (yeni, _) = d.ekle(Saglayici::Jmap, "b".into(), None, BAYAT_SN).unwrap();
        assert_eq!(d.temizle(BAYAT_SN + 1), 1);
        assert!(d.al(&eski.id).is_none() && d.al(&yeni.id).is_some());
        d.ekle(Saglayici::Graph, "b".into(), None, BAYAT_SN)
            .unwrap();
        assert_eq!(d.jetonu_sil("b"), 2);
        assert_eq!(d.sayi(), 0);
    }

    #[test]
    fn gmail_hesap_ozetiyle_bulunur() {
        let mut d = Depo::bellek();
        let h = ozet("kisi@gmail.com");
        d.ekle(Saglayici::Gmail, "a".into(), Some(h.clone()), 0)
            .unwrap();
        d.ekle(
            Saglayici::Gmail,
            "b".into(),
            Some(ozet("baska@gmail.com")),
            0,
        )
        .unwrap();
        d.ekle(Saglayici::Jmap, "c".into(), Some(h.clone()), 0)
            .unwrap();
        let bul = d.hesaba_gore(Saglayici::Gmail, &h);
        assert_eq!(bul.len(), 1);
        assert_eq!(bul[0].fcm, "a");
    }

    #[test]
    fn dosyaya_yazilir_ve_geri_okunur() {
        let dir = std::env::temp_dir().join(format!("mh-relay-test-{}", rastgele(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let yol = dir.join("aboneler.json");
        let id = {
            let mut d = Depo::ac(&yol).unwrap();
            d.ekle(Saglayici::Jmap, "j".into(), None, 5).unwrap().0.id
        };
        let d = Depo::ac(&yol).unwrap();
        assert_eq!(d.al(&id).unwrap().fcm, "j");
        // Bozuk dosya sessizce boşaltılmaz.
        std::fs::write(&yol, b"{bozuk").unwrap();
        assert!(Depo::ac(&yol).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sabit_zamanli_esitlik() {
        assert!(esit("abc", "abc"));
        assert!(!esit("abc", "abd"));
        assert!(!esit("abc", "ab"));
    }

    #[test]
    fn saglayici_adlari() {
        for s in [Saglayici::Jmap, Saglayici::Gmail, Saglayici::Graph] {
            assert_eq!(Saglayici::from_ad(s.ad()), Some(s));
        }
        assert_eq!(Saglayici::from_ad(" JMAP "), Some(Saglayici::Jmap));
        assert_eq!(Saglayici::from_ad("imap"), None);
    }
}
