//! Microsoft Store (MSIX) paketi: süreç paket kimliğiyle mi çalışıyor.
//!
//! Mağaza paketinde üç şey değişir: güncellemeyi Store kurar (uygulama içi
//! güncelleyici kapalıdır, `updater.rs`), açılışta başlama kayıt defteri
//! yerine paketin başlangıç göreviyle yapılır (`autostart.rs`; paket içinde
//! `HKCU\...\Run` yazımı sisteme görünmez) ve WebView2 kurucu tarafından
//! indirilemez (yoksa açılışta söylenir). Paketin kendisi
//! `src-tauri/msix/` ve `scripts/msix-paketle.ps1` ile üretilir
//! (`docs/MAGAZA.md`).

use std::sync::OnceLock;

/// Süreç bir MSIX paketinin içinden mi çalışıyor (Microsoft Store kurulumu).
/// Windows dışında hep `false`.
pub fn packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(detect)
}

#[cfg(windows)]
fn detect() -> bool {
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    let mut len = 0u32;
    // SAFETY: uzunluk göstericisi geçerli; ara bellek verilmediği için işlev
    // yalnız uzunluğu yazar. Paket kimliği yoksa `APPMODEL_ERROR_NO_PACKAGE`,
    // varsa (ara bellek küçük olduğundan) `ERROR_INSUFFICIENT_BUFFER` döner.
    let rc = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
    rc == ERROR_INSUFFICIENT_BUFFER
}

#[cfg(not(windows))]
fn detect() -> bool {
    false
}

/// WinRT çağrısını ayrı iş parçacığında yapar: ana iş parçacığı tek iş
/// parçacıklı COM dairesindedir (WebView2), orada eşzamansız işlemi beklemek
/// kilitlenebilir.
#[cfg(windows)]
pub fn off_thread<T: Send + 'static>(
    f: impl FnOnce() -> ::windows::core::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    std::thread::spawn(f)
        .join()
        .map_err(|_| std::io::Error::other("WinRT çağrısı çöktü"))?
        .map_err(std::io::Error::other)
}

/// Bu süreç paketin başlangıç görevinden mi (oturum açılışında) başlatıldı.
/// Başlangıç görevi argüman taşımaz; etkinleştirme türünden anlaşılır.
pub fn startup_activation() -> bool {
    #[cfg(windows)]
    {
        use ::windows::ApplicationModel::Activation::ActivationKind;
        use ::windows::ApplicationModel::AppInstance;

        if !packaged() {
            return false;
        }
        match off_thread(|| AppInstance::GetActivatedEventArgs()?.Kind()) {
            Ok(kind) => kind == ActivationKind::StartupTask,
            Err(e) => {
                tracing::debug!(error = %e, "etkinleştirme türü okunamadı; elle açılış sayıldı");
                false
            }
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Mağaza paketinde WebView2 çalışma zamanı yoksa kullanıcıya söyler ve
/// `true` döner (çağıran çıkar). Paket kurulurken bir şey indirilemediği için
/// kurucu bunu tamamlayamaz; Windows 11'de ve güncel Windows 10'da hazır gelir.
pub fn webview_missing() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

        if !packaged() || tauri::webview_version().is_ok() {
            return false;
        }
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let text = wide(
            "Mailhere'nin çalışması için Microsoft Edge WebView2 Runtime gerekiyor ve bu bilgisayarda kurulu değil.\n\n\
             Kurmak için: https://go.microsoft.com/fwlink/p/?LinkId=2124703 adresinden indirip kur, sonra Mailhere'yi yeniden aç.\n\n\
             Mailhere needs the Microsoft Edge WebView2 Runtime, which is not installed on this PC. \
             Install it from the address above, then open Mailhere again.",
        );
        let title = wide("Mailhere");
        // SAFETY: iki dize de sıfırla biten UTF-16 ve çağrı boyunca yaşıyor;
        // sahip pencere yok.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONWARNING,
            );
        }
        true
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sureci_paketli_degil() {
        // Test ikilisi paket kimliği taşımaz; Windows dışında hep `false`.
        assert!(!packaged());
        assert!(!startup_activation());
        assert!(!webview_missing());
    }
}
