//! Tam ekran algısı: oyun, sunum ya da tam ekran video sırasında köşe kartı
//! açılmaz (yalnız tepsi rozeti).
//!
//! - Windows: `SHQueryUserNotificationState` (meşgul, Direct3D tam ekran,
//!   sunum kipi).
//! - Linux X11: etkin pencerenin (`_NET_ACTIVE_WINDOW`) durumunda
//!   `_NET_WM_STATE_FULLSCREEN` var mı (saf Rust X11 istemcisi).
//! - Wayland ve diğer sistemler ya da herhangi bir hata: tam ekran değil.

/// Kullanıcı şu an tam ekran bir uygulamada mı?
pub fn is_fullscreen() -> bool {
    platform::is_fullscreen()
}

/// `QUERY_USER_NOTIFICATION_STATE` değeri bildirim göstermemeyi mi istiyor?
/// (2 meşgul, 3 Direct3D tam ekran, 4 sunum kipi.)
#[cfg_attr(not(windows), allow(dead_code))]
fn quns_suppresses(state: i32) -> bool {
    matches!(state, 2..=4)
}

/// Oturum Wayland mı? (XWayland'de X11 etkin pencereyi göremez.)
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_wayland(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    session_type.is_some_and(|t| t.trim().eq_ignore_ascii_case("wayland"))
        || wayland_display.is_some_and(|d| !d.trim().is_empty())
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_BUSY,
        QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };

    // Taşınabilir eşleme (`quns_suppresses`) Windows değerleriyle aynı kalsın.
    const _: () =
        assert!(QUNS_BUSY == 2 && QUNS_RUNNING_D3D_FULL_SCREEN == 3 && QUNS_PRESENTATION_MODE == 4);

    pub fn is_fullscreen() -> bool {
        let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
        // SAFETY: tek parametre, bu yığın değişkenine yazılan bir çıktıdır.
        let hr = unsafe { SHQueryUserNotificationState(&mut state) };
        hr >= 0 && super::quns_suppresses(state)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    pub fn is_fullscreen() -> bool {
        let session = std::env::var("XDG_SESSION_TYPE").ok();
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        if super::is_wayland(session.as_deref(), wayland.as_deref()) {
            return false;
        }
        if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) {
            return false;
        }
        // Yanıt vermeyen X sunucusu eşitlemeyi (çağıran) kilitlemesin: sorgu
        // ayrı iş parçacığında, kısa süre sınırıyla. Ulaşılamazsa ya da yanıt
        // beklenmedikse: tam ekran değil.
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("mailhere-tam-ekran".into())
            .spawn(move || {
                let _ = tx.send(x11_fullscreen());
            });
        if spawned.is_err() {
            return false;
        }
        rx.recv_timeout(std::time::Duration::from_millis(500))
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    fn x11_fullscreen() -> Option<bool> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &[u8]| -> Option<u32> {
            Some(conn.intern_atom(true, name).ok()?.reply().ok()?.atom)
        };
        let (active, state, full) = (
            atom(b"_NET_ACTIVE_WINDOW")?,
            atom(b"_NET_WM_STATE")?,
            atom(b"_NET_WM_STATE_FULLSCREEN")?,
        );
        // Pencere yöneticisi EWMH desteklemiyor.
        if active == 0 || state == 0 || full == 0 {
            return Some(false);
        }
        let window = conn
            .get_property(false, root, active, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()?;
        if window == 0 {
            return Some(false);
        }
        let states = conn
            .get_property(false, window, state, AtomEnum::ATOM, 0, 64)
            .ok()?
            .reply()
            .ok()?;
        let fullscreen = super::has_atom(states.value32()?, full);
        Some(fullscreen)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    pub fn is_fullscreen() -> bool {
        false
    }
}

/// Atom listesinde aranan atom var mı?
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn has_atom(mut atoms: impl Iterator<Item = u32>, wanted: u32) -> bool {
    atoms.any(|a| a == wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_bildirim_durumu() {
        assert!(quns_suppresses(2), "meşgul");
        assert!(quns_suppresses(3), "Direct3D tam ekran");
        assert!(quns_suppresses(4), "sunum kipi");
        for s in [0, 1, 5, 6, 7] {
            assert!(!quns_suppresses(s), "{s}");
        }
    }

    #[test]
    fn wayland_algisi() {
        assert!(is_wayland(Some("wayland"), None));
        assert!(is_wayland(Some("x11"), Some("wayland-0")));
        assert!(!is_wayland(Some("x11"), None));
        assert!(!is_wayland(None, Some("")));
        assert!(!is_wayland(None, None));
    }

    #[test]
    fn tam_ekran_atomu() {
        assert!(has_atom([5, 9, 12].into_iter(), 9));
        assert!(!has_atom([5, 12].into_iter(), 9));
        assert!(!has_atom(std::iter::empty(), 9));
    }

    #[test]
    fn hata_tam_ekran_sayilmaz() {
        // Test ortamında ekran yoksa ya da Wayland ise panik yok, `false`.
        let _ = is_fullscreen();
    }
}
