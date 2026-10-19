//! Dış bağlantı ve dosya açma.
//!
//! AppImage'ın GTK kancası süreç ortamına kendi yollarını yazar (`GTK_PATH`,
//! `GIO_MODULE_DIR`, `XDG_DATA_DIRS`…). Bu ortam sistemin `xdg-open`ına ve
//! tarayıcısına geçerse tarayıcı açılmaz, bağlantıya tıklamak hiçbir şey
//! yapmaz. AppImage içindeyken açıcı bu değişkenlerden arındırılmış ortamla
//! başlatılır; diğer paketlerde opener eklentisi kullanılır.

use mh_core::{AppError, ErrorCode};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

type Res<T> = Result<T, AppError>;

/// Arayüzden dış bağlantı: yalnız http, https ve mailto.
#[tauri::command]
pub fn link_open(app: AppHandle, url: String) -> Res<()> {
    if !allowed(&url) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Bu bağlantı açılamaz; yalnız web ve e-posta bağlantıları açılır.",
        ));
    }
    open_url(&app, &url).map_err(|e| {
        tracing::warn!(error = %e, "bağlantı açılamadı");
        AppError::new(
            ErrorCode::Internal,
            "Bağlantı tarayıcıda açılamadı. Varsayılan tarayıcının ayarlı olduğunu denetle.",
        )
    })
}

fn allowed(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
        && url.len() <= 8192
        && !url.chars().any(char::is_control)
}

/// Bağlantıyı varsayılan uygulamayla açar.
pub fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if let Some(dir) = appimage::dir() {
        return appimage::open(url, &dir);
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Dosyayı varsayılan uygulamasıyla açar.
pub fn open_path(app: &AppHandle, path: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if let Some(dir) = appimage::dir() {
        return appimage::open(path, &dir);
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
mod appimage {
    use std::process::{Command, Stdio};

    /// AppImage kancalarının yazdığı, sistem uygulamalarını bozan değişkenler.
    const BOZUCU: &[&str] = &[
        "APPDIR",
        "APPIMAGE",
        "ARGV0",
        "OWD",
        "GTK_DATA_PREFIX",
        "GTK_THEME",
        "GTK_EXE_PREFIX",
        "GTK_PATH",
        "GTK_IM_MODULE_FILE",
        "GDK_PIXBUF_MODULE_FILE",
        "GSETTINGS_SCHEMA_DIR",
        "GI_TYPELIB_PATH",
        "GIO_MODULE_DIR",
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "GST_PLUGIN_SCANNER",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
    ];

    /// AppImage içinde çalışılıyorsa paket kökü.
    pub fn dir() -> Option<String> {
        std::env::var("APPIMAGE").ok().filter(|v| !v.is_empty())?;
        std::env::var("APPDIR").ok().filter(|v| !v.is_empty())
    }

    /// Ortamdaki yol listesinden paket köküne düşen girdileri çıkarır.
    pub fn temiz_liste(deger: &str, kok: &str) -> String {
        let kok = kok.trim_end_matches('/');
        deger
            .split(':')
            .filter(|p| !p.is_empty() && !(p.starts_with(kok) && !kok.is_empty()))
            .collect::<Vec<_>>()
            .join(":")
    }

    fn komut(program: &str, kok: &str) -> Command {
        let mut c = Command::new(program);
        for k in BOZUCU {
            c.env_remove(k);
        }
        for k in ["XDG_DATA_DIRS", "PATH"] {
            if let Ok(v) = std::env::var(k) {
                let t = temiz_liste(&v, kok);
                if t.is_empty() {
                    c.env_remove(k);
                } else {
                    c.env(k, t);
                }
            }
        }
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    }

    /// `xdg-open`, yoksa `gio open` ile açar; süreç arka planda beklenir.
    pub fn open(hedef: &str, kok: &str) -> Result<(), String> {
        // Tire ile başlayan hedef seçenek sanılmasın.
        let hedef = if hedef.starts_with('-') {
            format!("./{hedef}")
        } else {
            hedef.to_owned()
        };
        let mut son = String::from("açıcı bulunamadı");
        for (program, on) in [("xdg-open", None), ("gio", Some("open"))] {
            let mut c = komut(program, kok);
            if let Some(a) = on {
                c.arg(a);
            }
            match c.arg(&hedef).spawn() {
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Ok(());
                }
                Err(e) => son = format!("{program}: {e}"),
            }
        }
        Err(son)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yalniz_web_ve_posta_baglantisi() {
        assert!(allowed("https://ornek.com/a?b=1"));
        assert!(allowed("HTTP://ornek.com"));
        assert!(allowed("mailto:a@ornek.com"));
        assert!(!allowed("file:///etc/passwd"));
        assert!(!allowed("javascript:alert(1)"));
        assert!(!allowed("https://ornek.com/\nx"));
        assert!(!allowed("-https://x"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_yollari_ayiklanir() {
        assert_eq!(
            appimage::temiz_liste(
                "/tmp/.mount_Mailx/usr/share:/usr/share::/usr/local/share",
                "/tmp/.mount_Mailx"
            ),
            "/usr/share:/usr/local/share"
        );
        assert_eq!(appimage::temiz_liste("/usr/share", ""), "/usr/share");
    }
}
