//! Kabuk tercihleri: yalnız bu bilgisayara ait küçük seçimler (açılışta
//! başlama seçimi, tepsi bilgisinin bir kez gösterildiği).
//!
//! Veritabanında değil, yerel veri klasöründe `kabuk.json` dosyasında durur:
//! açılış kaydı veritabanı açılmadan (posta kutusu kilitliyken de) yazılır ve
//! okunur. Gizli bilgi taşımaz. Bellek kipinde (tanıtım, test) dosyaya
//! yazılmaz; seçimler süreçle kaybolur.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FILE: &str = "kabuk.json";

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    /// Açılışta başlama seçimi. `None`: henüz karar verilmedi (ilk sürüm
    /// çalıştırmasında varsayılan "açık" bir kez uygulanır); `Some(false)`:
    /// kullanıcı kapattı, bir daha kendiliğinden açılmaz.
    pub autostart: Option<bool>,
    /// Ana pencere ilk kez tepsiye gizlendi ve bilgi bir kez gösterildi.
    pub tray_hint_shown: bool,
}

/// Tercihler ve (varsa) dosyası. Tauri durumunda yönetilir.
pub struct PrefsStore {
    path: Option<PathBuf>,
    value: Mutex<Prefs>,
}

impl PrefsStore {
    /// Dosyayı okur; yoksa ya da bozuksa varsayılanla başlar (bozuk dosya
    /// ilk yazımda düzelir).
    pub fn open(path: Option<PathBuf>) -> Self {
        let value = path.as_deref().map(read).unwrap_or_default();
        Self {
            path,
            value: Mutex::new(value),
        }
    }

    pub fn get(&self) -> Prefs {
        self.value.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Değiştirir ve dosyaya yazar. Yazılamazsa bellekteki değer yine
    /// değişir (bu oturumda geçerli), hata döner.
    pub fn update(&self, f: impl FnOnce(&mut Prefs)) -> io::Result<()> {
        let snapshot = {
            let mut v = self.value.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut v);
            v.clone()
        };
        match &self.path {
            Some(path) => write(path, &snapshot),
            None => Ok(()),
        }
    }
}

fn read(path: &Path) -> Prefs {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "kabuk tercihleri okunamadı; varsayılanla sürüyor");
            Prefs::default()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Prefs::default(),
        Err(e) => {
            tracing::warn!(error = %e, "kabuk tercihleri okunamadı; varsayılanla sürüyor");
            Prefs::default()
        }
    }
}

/// Geçici dosyaya yazıp yerine taşır: yarım yazılmış dosya kalmaz.
fn write(path: &Path, prefs: &Prefs) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(prefs).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Veri klasörü yoksa yalnız kullanıcıya açık (0700) oluşturulur; varsa
/// izinlerine dokunulmaz (çekirdek kendisi ayarlar).
fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Tercih dosyasının yeri: `MAILHERE_DATA_DIR` (çekirdekle aynı), bellek
/// kipinde yok, değilse işletim sisteminin yerel veri klasörü.
fn location(app: &AppHandle) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MAILHERE_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join(FILE));
    }
    if mh_app::state::memory_mode() {
        return None;
    }
    match app.path().app_local_data_dir() {
        Ok(dir) => Some(dir.join(FILE)),
        Err(e) => {
            tracing::warn!(error = %e, "veri klasörü bulunamadı; kabuk tercihleri bellekte");
            None
        }
    }
}

/// Tercihleri açar ve Tauri durumuna koyar (bir kez).
pub fn init(app: &AppHandle) {
    if app.try_state::<PrefsStore>().is_none() {
        app.manage(PrefsStore::open(location(app)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mailhere-kabuk-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn dosya_yoksa_varsayilan_yazilinca_kalici() {
        let dir = temp_dir("kalici");
        let path = dir.join("alt").join(FILE);
        let store = PrefsStore::open(Some(path.clone()));
        assert_eq!(store.get(), Prefs::default());
        store
            .update(|p| {
                p.autostart = Some(false);
                p.tray_hint_shown = true;
            })
            .unwrap();
        let again = PrefsStore::open(Some(path.clone()));
        assert_eq!(again.get().autostart, Some(false));
        assert!(again.get().tray_hint_shown);
        assert!(!path.with_extension("json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bozuk_dosya_varsayilana_duser_bilinmeyen_alan_yok_sayilir() {
        let dir = temp_dir("bozuk");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(&path, b"{ bozuk").unwrap();
        assert_eq!(PrefsStore::open(Some(path.clone())).get(), Prefs::default());
        std::fs::write(&path, br#"{"autostart":true,"yeniAlan":1}"#).unwrap();
        let p = PrefsStore::open(Some(path)).get();
        assert_eq!(p.autostart, Some(true));
        assert!(!p.tray_hint_shown);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bellek_kipinde_dosya_yazilmaz() {
        let store = PrefsStore::open(None);
        store.update(|p| p.autostart = Some(true)).unwrap();
        assert_eq!(store.get().autostart, Some(true));
    }
}
