//! Sistem panosuna metin yazma (`Platform::copy_text`): yeni postadaki
//! doğrulama kodu, kullanıcı "kendiliğinden kopyala"yı açtıysa.
//!
//! Ek bir pano kitaplığı bağlanmaz. Linux'ta kabuğa zaten bağlı GTK'nın
//! panosu, Windows'ta bağımlılık ağacında zaten bulunan `windows-sys`
//! (MIT / Apache-2.0) ile Win32 pano çağrıları kullanılır. `arboard` gibi
//! hazır bir kitaplık seçilmedi: Windows'ta `clipboard-win`e (BSL-1.0)
//! dayanır ve bu lisans depo kuralındaki listenin (MIT / Apache-2.0 / BSD /
//! OFL) dışındadır; ayrıca Linux'ta ikinci bir X11/Wayland istemcisi açardı.

use tauri::AppHandle;

/// Metni panoya yazar; yazıldıysa `true`. Hata günlüğe (metin olmadan)
/// yazılır; çağıran yalnız sonucu görür.
pub fn set_text(app: &AppHandle, text: &str) -> bool {
    imp::set_text(app, text)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::mpsc;
    use std::time::Duration;

    use tauri::AppHandle;

    /// Ana iş parçacığının panoyu yazması en çok bu kadar beklenir.
    const WAIT: Duration = Duration::from_secs(2);

    /// GTK yalnız ana iş parçacığından çağrılır; sonuç kanalla beklenir.
    /// Wayland'da pencere odakta değilken oturum yöneticisi pano yazımını
    /// yok sayabilir (GTK bunu bildirmez); X11'de her zaman yazılır.
    pub fn set_text(app: &AppHandle, text: &str) -> bool {
        let (tx, rx) = mpsc::channel();
        let text = text.to_owned();
        let sent = app.run_on_main_thread(move || {
            let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
            clipboard.set_text(&text);
            // Pano yöneticisi varsa içerik uygulama kapansa da kalır.
            clipboard.store();
            let _ = tx.send(());
        });
        match sent {
            Ok(()) => rx.recv_timeout(WAIT).is_ok(),
            Err(e) => {
                tracing::warn!(error = %e, "pano ana iş parçacığına verilemedi");
                false
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use tauri::AppHandle;

    pub fn set_text(_app: &AppHandle, text: &str) -> bool {
        let ok = super::win32::set_text(text);
        if !ok {
            tracing::warn!("panoya yazılamadı");
        }
        ok
    }
}

/// Win32 panosu: `CF_UNICODETEXT` biçiminde, sonu sıfırlı UTF-16 metin.
#[cfg(windows)]
#[path = "clipboard_win32.rs"]
mod win32;

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use tauri::AppHandle;

    /// Bu işletim sisteminde pano bağlanmadı; kod kartta "Kodu kopyala" ile
    /// kopyalanır.
    pub fn set_text(_app: &AppHandle, _text: &str) -> bool {
        false
    }
}
