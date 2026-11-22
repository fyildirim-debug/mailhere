//! Paket kimliği: Mailhere Microsoft Store'dan (MSIX) kurulduysa Windows ona
//! bir paket kimliği verir. O kurulumda güncellemeyi Mağaza yapar (uygulama
//! içi güncelleyici kapalıdır) ve açılışta başlama kayıt defteri yerine
//! paketteki `StartupTask` ile yönetilir (bkz. `autostart.rs`,
//! `packaging/msix/AppxManifest.xml`).

/// Bu süreç MSIX paket kimliğiyle mi çalışıyor (Microsoft Store kurulumu).
#[cfg(windows)]
pub fn packaged() -> bool {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        let mut len = 0u32;
        // Ara bellek verilmeden çağrılır: paketliyse gereken uzunluğu bildirip
        // ERROR_INSUFFICIENT_BUFFER, paketsizse APPMODEL_ERROR_NO_PACKAGE döner.
        let rc = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
        rc == ERROR_INSUFFICIENT_BUFFER
    })
}

/// Windows dışında paket kimliği yoktur.
#[cfg(not(windows))]
pub fn packaged() -> bool {
    false
}

/// Mağaza kurulumunda güncelleme düğmesinin yerine gösterilen neden.
pub const STORE_UPDATES: &str =
    "Mailhere Microsoft Store'dan kuruldu; yeni sürümleri Microsoft Store kendisi kurar.";
