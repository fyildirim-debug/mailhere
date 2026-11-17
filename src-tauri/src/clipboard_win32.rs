//! Win32 panosuna metin yazma (yalnız Windows; `clipboard.rs` kullanır).
//!
//! Tauri'ye ya da uygulamanın geri kalanına bağlı değildir: yalnız
//! `windows-sys` çağrıları. Pano herhangi bir iş parçacığından açılabilir.

use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// Pano biçimi: sonu sıfırlı UTF-16 metin (`winuser.h`).
const CF_UNICODETEXT: u32 = 13;
/// Pano başka bir süreçte açıksa bu kadar kez yeniden denenir.
const OPEN_TRIES: u32 = 5;
const OPEN_WAIT: Duration = Duration::from_millis(20);

/// Açık panoyu düşerken kapatır.
struct Open;

impl Open {
    /// Panoyu açar; başka bir süreç tutuyorsa kısa aralıklarla yeniden dener.
    fn new() -> Option<Self> {
        for attempt in 0..OPEN_TRIES {
            // SAFETY: sahip penceresi olmadan (null) açmak geçerlidir; pano
            // bu iş parçacığına bağlanır ve `Drop` içinde kapatılır.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Some(Self);
            }
            if attempt + 1 < OPEN_TRIES {
                std::thread::sleep(OPEN_WAIT);
            }
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: pano bu iş parçacığında `OpenClipboard` ile açıldı.
        unsafe {
            CloseClipboard();
        }
    }
}

/// Metni panoya yazar; yazıldıysa `true`.
pub fn set_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let Some(_open) = Open::new() else {
        return false;
    };
    // SAFETY: pano açık (`_open`). Bellek `GMEM_MOVEABLE` ile ayrılır (pano
    // bunu ister), kilitlenince `wide.len()` adet `u16` sığacak boyuttadır ve
    // kopyalama bu sınırı aşmaz. `SetClipboardData` başarılıysa belleğin
    // sahibi pano olur ve bırakılmaz; başarısızsa burada bırakılır.
    unsafe {
        if EmptyClipboard() == 0 {
            return false;
        }
        let mem = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of_val(wide.as_slice()));
        if mem.is_null() {
            return false;
        }
        let dst = GlobalLock(mem).cast::<u16>();
        if dst.is_null() {
            GlobalFree(mem);
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
        GlobalUnlock(mem);
        if SetClipboardData(CF_UNICODETEXT, mem as HANDLE).is_null() {
            GlobalFree(mem);
            return false;
        }
        true
    }
}
