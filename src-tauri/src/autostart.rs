//! Açılışta başlama (masaüstü): oturum açılınca Mailhere `--tepside`
//! argümanıyla başlar; ana pencere açılmaz, yalnız tepsi simgesi görünür,
//! eşitleme ve bildirim kartları her zamanki gibi çalışır. Elle açılışta
//! (argümansız) pencere açılır.
//!
//! Kayıt:
//! - **Windows:** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
//!   değer adı `Mailhere` (NSIS kaldırıcısı aynı adı siler, güncellemede
//!   korur). Görev Yöneticisi › Başlangıç'tan kapatılmışsa
//!   (`Explorer\StartupApproved\Run`) kapalı sayılır; ayardan açınca o işaret
//!   de kaldırılır.
//! - **Linux:** XDG `~/.config/autostart/mailhere.desktop`
//!   (`X-GNOME-Autostart-enabled=true`, `TryExec` ile kaldırılmış ikili
//!   atlanır). AppImage'da yol, geçici bağlama noktası (`/tmp/.mount_*`)
//!   değil `APPIMAGE` dosyasıdır.
//!
//! Seçim `kabuk.json`da tutulur (`prefs.rs`): kullanıcı hiç seçmediyse ilk
//! sürüm çalıştırmasında bir kez açılır; kapattıysa bir daha açılmaz. Her
//! açılışta kayıt varsa yolu bu ikiliye göre tazelenir (güncelleme, taşınan
//! AppImage). Geliştirme derlemesinde ve bellek kipinde kendiliğinden hiçbir
//! şey yazılmaz; bellek kipinde kayıt yalnız bellektedir.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use mh_core::{AppError, ErrorCode};
use tauri::{AppHandle, Manager};

use crate::prefs::PrefsStore;

type Res<T> = Result<T, AppError>;

/// Açılış kaydının verdiği argüman: pencere açma, tepside başla.
pub const HIDDEN_ARG: &str = "--tepside";
/// Aynı anlamda İngilizce eş ad (elle kurulan kısayollar için).
const HIDDEN_ALIAS: &str = "--hidden";
/// Windows değer adı ve Linux masaüstü girdisinin adı (`productName`).
pub const ENTRY_NAME: &str = "Mailhere";

/// Argümanlardan biri gizli başlatma mı (`args` program adını içermez).
pub fn is_hidden_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|a| a.as_ref() == HIDDEN_ARG || a.as_ref() == HIDDEN_ALIAS)
}

/// Bu süreç açılış kaydından (`--tepside`) mı başladı.
pub fn hidden_start() -> bool {
    static HIDDEN: OnceLock<bool> = OnceLock::new();
    *HIDDEN.get_or_init(|| is_hidden_args(std::env::args_os().skip(1)))
}

/// Açılışta başlatılacak dosya. AppImage çalışırken `current_exe` geçici
/// bağlama noktasının içini gösterir (her açılışta değişir): o durumda
/// `APPIMAGE` (çalıştırılan `.AppImage` dosyası) yazılır. Ortamdan miras
/// kalmış `APPIMAGE` (ör. başka bir AppImage'dan açılan uçbirim) yalnız bu
/// ikili gerçekten o bağlama noktasının içindeyse kullanılır.
pub fn launch_path(current_exe: &Path, appimage: Option<&Path>, appdir: Option<&Path>) -> PathBuf {
    if let Some(image) = appimage.filter(|p| p.is_absolute()) {
        let inside = match appdir {
            Some(dir) if dir.is_absolute() => current_exe.starts_with(dir),
            _ => current_exe.components().any(|c| {
                c.as_os_str()
                    .to_str()
                    .is_some_and(|s| s.starts_with(".mount_"))
            }),
        };
        if inside {
            return image.to_path_buf();
        }
    }
    current_exe.to_path_buf()
}

fn current_launch_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let image = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let dir = std::env::var_os("APPDIR").map(PathBuf::from);
    Ok(launch_path(&exe, image.as_deref(), dir.as_deref()))
}

// ------------------------------------------------------------------ arka uçlar

/// İşletim sistemindeki kaydın durumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Absent,
    /// `current`: bu ikiliyi başlatıyor; `enabled`: sistemin başlangıç
    /// ayarlarından kapatılmamış.
    Present {
        current: bool,
        enabled: bool,
    },
}

impl Entry {
    pub fn on(self) -> bool {
        matches!(self, Entry::Present { enabled: true, .. })
    }
}

/// Açılış kaydının işletim sistemi tarafı (testlerde sahte).
pub trait Backend: Send + Sync {
    fn read(&self, exe: &Path) -> io::Result<Entry>;
    /// Kaydı yazar; sistem ayarlarından kapatılmışsa yeniden açar.
    fn enable(&self, exe: &Path) -> io::Result<()>;
    /// Yalnız yolu tazeler; açık/kapalı işaretine dokunmaz.
    fn refresh(&self, exe: &Path) -> io::Result<()>;
    fn disable(&self) -> io::Result<()>;
    /// Seçim açıkken kayıt kaybolduysa yeniden yazılır mı. Windows'ta kaydı
    /// yalnız kaldırıcı siler (Görev Yöneticisi kapatır, silmez): yeniden
    /// kurulumda geri gelir. Linux'ta masaüstünün başlangıç ayarı dosyayı
    /// siler: kullanıcının kararıdır, dokunulmaz.
    fn restore_missing(&self) -> bool;
}

// ------------------------------------------------------------------ Linux (XDG)

/// XDG masaüstü girdisi biçimi ve dosya arka ucu. Biçim işlevleri her
/// sistemde birim testlidir; yalnız Linux kullanır.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub mod xdg {
    use std::io;
    use std::path::{Path, PathBuf};

    use super::{Backend, Entry, ENTRY_NAME, HIDDEN_ARG};

    /// Masaüstü girdisi dize değeri kaçışı (`\` → `\\`).
    fn desktop_string(s: &str) -> String {
        s.replace('\\', "\\\\")
    }

    /// `Exec` için tek argüman: tırnaklanır; `"`, `` ` ``, `$`, `\` kaçışlanır,
    /// `%` ikilenir, ardından dize kaçışı uygulanır (Desktop Entry belirtimi).
    fn exec_arg(s: &str) -> String {
        let mut quoted = String::with_capacity(s.len() + 2);
        quoted.push('"');
        for c in s.chars() {
            match c {
                '"' | '`' | '$' | '\\' => {
                    quoted.push('\\');
                    quoted.push(c);
                }
                '%' => quoted.push_str("%%"),
                _ => quoted.push(c),
            }
        }
        quoted.push('"');
        desktop_string(&quoted)
    }

    /// Yolun masaüstü girdisine yazılabilir hali; UTF-8 değilse ya da denetim
    /// karakteri taşıyorsa `None` (girdi satır temellidir).
    fn entry_path(exe: &Path) -> Option<&str> {
        exe.to_str().filter(|s| !s.chars().any(char::is_control))
    }

    fn exec_line(exe: &str) -> String {
        format!("Exec={} {HIDDEN_ARG}", exec_arg(exe))
    }

    fn try_exec_line(exe: &str) -> String {
        format!("TryExec={}", desktop_string(exe))
    }

    /// `~/.config/autostart/mailhere.desktop` içeriği.
    pub fn desktop_entry(exe: &Path) -> Option<String> {
        let exe = entry_path(exe)?;
        Some(format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name={ENTRY_NAME}\n\
             Comment=Starts in the system tray and checks mail; no window opens\n\
             Comment[tr]=Sistem tepsisinde başlar ve postaları denetler; pencere açılmaz\n\
             {}\n\
             {}\n\
             Icon=mailhere\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n\
             X-KDE-autostart-after=panel\n",
            exec_line(exe),
            try_exec_line(exe),
        ))
    }

    /// `[Desktop Entry]` grubundaki bir anahtarın değeri (yerelleştirilmemiş).
    fn entry_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
        let mut in_group = false;
        for line in content.lines() {
            let line = line.trim_end_matches('\r');
            if line.starts_with('[') {
                in_group = line == "[Desktop Entry]";
                continue;
            }
            if !in_group {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim());
                }
            }
        }
        None
    }

    /// Masaüstünün kendi başlangıç ayarlarından kapatılmış mı (GNOME:
    /// `X-GNOME-Autostart-enabled=false`; XDG: `Hidden=true`).
    pub fn entry_enabled(content: &str) -> bool {
        let hidden = entry_value(content, "Hidden").is_some_and(|v| v == "true");
        let gnome_off =
            entry_value(content, "X-GNOME-Autostart-enabled").is_some_and(|v| v == "false");
        !hidden && !gnome_off
    }

    /// Girdi bu ikiliyi mi başlatıyor (`Exec` ve `TryExec` aynı).
    pub fn entry_current(content: &str, exe: &Path) -> bool {
        let Some(exe) = entry_path(exe) else {
            return false;
        };
        let exec = exec_line(exe);
        let try_exec = try_exec_line(exe);
        entry_value(content, "Exec").map(|v| format!("Exec={v}")) == Some(exec)
            && entry_value(content, "TryExec").map(|v| format!("TryExec={v}")) == Some(try_exec)
    }

    /// Yolu tazelenmiş girdi: kullanıcının (masaüstünün) eklediği satırlar ve
    /// açık/kapalı işareti korunur, yalnız `Exec` ve `TryExec` yeniden yazılır.
    pub fn refreshed_entry(content: &str, exe: &Path) -> Option<String> {
        let path = entry_path(exe)?;
        let mut out = String::with_capacity(content.len() + 64);
        let (mut in_group, mut exec_done, mut try_done, mut saw_group) =
            (false, false, false, false);
        for line in content.lines() {
            let bare = line.trim_end_matches('\r');
            if bare.starts_with('[') {
                if in_group {
                    push_missing(&mut out, path, &mut exec_done, &mut try_done);
                }
                in_group = bare == "[Desktop Entry]";
                saw_group |= in_group;
            } else if in_group {
                let key = bare.split_once('=').map(|(k, _)| k.trim());
                if key == Some("Exec") {
                    if !exec_done {
                        out.push_str(&exec_line(path));
                        out.push('\n');
                        exec_done = true;
                    }
                    continue;
                }
                if key == Some("TryExec") {
                    if !try_done {
                        out.push_str(&try_exec_line(path));
                        out.push('\n');
                        try_done = true;
                    }
                    continue;
                }
            }
            out.push_str(bare);
            out.push('\n');
        }
        if !saw_group {
            return desktop_entry(exe);
        }
        if in_group {
            push_missing(&mut out, path, &mut exec_done, &mut try_done);
        }
        Some(out)
    }

    fn push_missing(out: &mut String, path: &str, exec_done: &mut bool, try_done: &mut bool) {
        if !*exec_done {
            out.push_str(&exec_line(path));
            out.push('\n');
            *exec_done = true;
        }
        if !*try_done {
            out.push_str(&try_exec_line(path));
            out.push('\n');
            *try_done = true;
        }
    }

    fn invalid_path() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "yol masaüstü girdisine yazılamıyor",
        )
    }

    /// XDG masaüstü girdisi (Linux).
    pub struct DesktopFile {
        pub path: PathBuf,
    }

    impl DesktopFile {
        fn content(&self) -> io::Result<Option<String>> {
            match std::fs::read_to_string(&self.path) {
                Ok(s) => Ok(Some(s)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        fn write(&self, content: &str) -> io::Result<()> {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = self.path.with_extension("desktop.tmp");
            std::fs::write(&tmp, content)?;
            std::fs::rename(&tmp, &self.path)
        }
    }

    impl Backend for DesktopFile {
        fn read(&self, exe: &Path) -> io::Result<Entry> {
            Ok(match self.content()? {
                None => Entry::Absent,
                Some(c) => Entry::Present {
                    current: entry_current(&c, exe),
                    enabled: entry_enabled(&c),
                },
            })
        }

        fn enable(&self, exe: &Path) -> io::Result<()> {
            self.write(&desktop_entry(exe).ok_or_else(invalid_path)?)
        }

        fn refresh(&self, exe: &Path) -> io::Result<()> {
            let fresh = match self.content()? {
                Some(c) => refreshed_entry(&c, exe),
                None => desktop_entry(exe),
            };
            self.write(&fresh.ok_or_else(invalid_path)?)
        }

        fn disable(&self) -> io::Result<()> {
            match std::fs::remove_file(&self.path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        }

        fn restore_missing(&self) -> bool {
            false
        }
    }
}

// ------------------------------------------------------------------ Windows

/// `HKCU\...\Run` değeri biçimi ve kayıt defteri arka ucu.
#[cfg_attr(not(windows), allow(dead_code))]
pub mod windows_run {
    #[cfg(windows)]
    use std::io;
    use std::path::Path;

    use super::HIDDEN_ARG;
    #[cfg(windows)]
    use super::{Backend, Entry, ENTRY_NAME};

    /// Windows `Run` değeri: tırnaklı yol + gizli başlatma argümanı.
    pub fn run_command(exe: &Path) -> String {
        format!("\"{}\" {HIDDEN_ARG}", exe.display())
    }

    /// `StartupApproved\Run` değeri: ilk bayt tekse (03, 07) kullanıcı Görev
    /// Yöneticisi ya da Ayarlar › Başlangıç'tan kapatmıştır; çift (02, 06) ya
    /// da değer yoksa açıktır.
    pub fn approved_enabled(value: Option<&[u8]>) -> bool {
        match value.and_then(|v| v.first()) {
            Some(b) => b & 1 == 0,
            None => true,
        }
    }

    /// Windows `Run` anahtarı.
    #[cfg(windows)]
    pub struct RunKey;

    #[cfg(windows)]
    impl RunKey {
        const RUN: &'static str = r"Software\Microsoft\Windows\CurrentVersion\Run";
        const APPROVED: &'static str =
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

        fn win(e: impl std::fmt::Display) -> io::Error {
            io::Error::other(e.to_string())
        }

        fn clear_approval() {
            if let Ok(key) = windows_registry::CURRENT_USER.create(Self::APPROVED) {
                let _ = key.remove_value(ENTRY_NAME);
            }
        }
    }

    #[cfg(windows)]
    impl Backend for RunKey {
        fn read(&self, exe: &Path) -> io::Result<Entry> {
            let Some(value) = windows_registry::CURRENT_USER
                .open(Self::RUN)
                .and_then(|k| k.get_string(ENTRY_NAME))
                .ok()
            else {
                return Ok(Entry::Absent);
            };
            let approval = windows_registry::CURRENT_USER
                .open(Self::APPROVED)
                .and_then(|k| k.get_value(ENTRY_NAME))
                .ok();
            Ok(Entry::Present {
                current: value == run_command(exe),
                enabled: approved_enabled(approval.as_deref()),
            })
        }

        fn enable(&self, exe: &Path) -> io::Result<()> {
            self.refresh(exe)?;
            Self::clear_approval();
            Ok(())
        }

        fn refresh(&self, exe: &Path) -> io::Result<()> {
            windows_registry::CURRENT_USER
                .create(Self::RUN)
                .and_then(|k| k.set_string(ENTRY_NAME, run_command(exe)))
                .map_err(Self::win)
        }

        fn disable(&self) -> io::Result<()> {
            if let Ok(key) = windows_registry::CURRENT_USER.create(Self::RUN) {
                if key.get_string(ENTRY_NAME).is_ok() {
                    key.remove_value(ENTRY_NAME).map_err(Self::win)?;
                }
            }
            Self::clear_approval();
            Ok(())
        }

        fn restore_missing(&self) -> bool {
            true
        }
    }
}

/// Bellek kipi (tanıtım, test) ve desteklenmeyen sistem: işletim sistemine
/// dokunulmaz.
pub struct Memory {
    on: Mutex<bool>,
    supported: bool,
}

impl Memory {
    pub fn new(on: bool) -> Self {
        Self {
            on: Mutex::new(on),
            supported: true,
        }
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub fn unsupported() -> Self {
        Self {
            on: Mutex::new(false),
            supported: false,
        }
    }

    fn set(&self, on: bool) -> io::Result<()> {
        if !self.supported {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "açılışta başlama bu sistemde yok",
            ));
        }
        *self.on.lock().unwrap_or_else(|e| e.into_inner()) = on;
        Ok(())
    }
}

impl Backend for Memory {
    fn read(&self, _exe: &Path) -> io::Result<Entry> {
        Ok(if *self.on.lock().unwrap_or_else(|e| e.into_inner()) {
            Entry::Present {
                current: true,
                enabled: true,
            }
        } else {
            Entry::Absent
        })
    }
    fn enable(&self, _exe: &Path) -> io::Result<()> {
        self.set(true)
    }
    fn refresh(&self, _exe: &Path) -> io::Result<()> {
        Ok(())
    }
    fn disable(&self) -> io::Result<()> {
        self.set(false)
    }
    fn restore_missing(&self) -> bool {
        false
    }
}

// ------------------------------------------------------------------ karar

/// Açılışta yapılacak iş.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Enable,
    Refresh,
}

/// Açılış kararı: ne yapılacak ve kaydedilecek seçim. `auto` yalnız sürüm
/// derlemesinde ve gerçek kipte doğrudur; geliştirme ikilisi kurulu sürümün
/// kaydını kendi yoluna çevirmesin.
pub fn plan(
    choice: Option<bool>,
    entry: Entry,
    auto: bool,
    restore_missing: bool,
) -> (Step, Option<bool>) {
    if !auto {
        return (Step::Nothing, None);
    }
    let refresh = |current: bool| {
        if current {
            Step::Nothing
        } else {
            Step::Refresh
        }
    };
    match (choice, entry) {
        // İlk çalıştırma: varsayılan açık, bir kez.
        (None, Entry::Absent) => (Step::Enable, Some(true)),
        (None, Entry::Present { current, .. }) => (refresh(current), Some(true)),
        // Kullanıcı kapattı: bir daha kendiliğinden açılmaz.
        (Some(false), _) => (Step::Nothing, None),
        (Some(true), Entry::Absent) if restore_missing => (Step::Enable, None),
        (Some(true), Entry::Absent) => (Step::Nothing, None),
        (Some(true), Entry::Present { current, .. }) => (refresh(current), None),
    }
}

// ------------------------------------------------------------------ durum

pub struct Autostart {
    backend: Box<dyn Backend>,
    exe: PathBuf,
}

impl Autostart {
    fn read(&self) -> io::Result<Entry> {
        self.backend.read(&self.exe)
    }
}

fn backend(app: &AppHandle) -> Box<dyn Backend> {
    if mh_app::state::memory_mode() {
        return Box::new(Memory::new(true));
    }
    #[cfg(windows)]
    {
        let _ = app;
        Box::new(windows_run::RunKey)
    }
    #[cfg(target_os = "linux")]
    {
        match app.path().config_dir() {
            Ok(dir) => Box::new(xdg::DesktopFile {
                path: dir.join("autostart").join("mailhere.desktop"),
            }),
            Err(e) => {
                tracing::warn!(error = %e, "yapılandırma klasörü bulunamadı; açılışta başlama yok");
                Box::new(Memory::unsupported())
            }
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = app;
        Box::new(Memory::unsupported())
    }
}

/// Açılış kaydını kurar: tercihleri ve arka ucu Tauri durumuna koyar, ilk
/// çalıştırma varsayılanını ve yol tazelemeyi uygular. Veritabanından
/// bağımsızdır; çekirdekten önce çağrılır. Hata yalnız günlüğe gider.
pub fn init(app: &AppHandle) {
    crate::prefs::init(app);
    let exe = match current_launch_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "çalışan dosyanın yolu bulunamadı; açılışta başlama yok");
            return;
        }
    };
    let auto = !cfg!(debug_assertions) && !mh_app::state::memory_mode();
    let state = Autostart {
        backend: backend(app),
        exe,
    };
    let prefs = app.state::<PrefsStore>();
    match state.read() {
        Ok(entry) => {
            let (step, record) = plan(
                prefs.get().autostart,
                entry,
                auto,
                state.backend.restore_missing(),
            );
            let done = match step {
                Step::Nothing => Ok(()),
                Step::Enable => state.backend.enable(&state.exe),
                Step::Refresh => state.backend.refresh(&state.exe),
            };
            match done {
                Ok(()) => {
                    if step != Step::Nothing {
                        tracing::info!(?step, "açılış kaydı güncellendi");
                    }
                    if let Some(on) = record {
                        if let Err(e) = prefs.update(|p| p.autostart = Some(on)) {
                            tracing::warn!(error = %e, "açılış seçimi kaydedilemedi");
                        }
                    }
                }
                // Seçim kaydedilmez: sonraki açılışta yeniden denenir.
                Err(e) => tracing::warn!(error = %e, ?step, "açılış kaydı yazılamadı"),
            }
        }
        Err(e) => tracing::warn!(error = %e, "açılış kaydı okunamadı"),
    }
    app.manage(state);
}

fn read_error() -> AppError {
    AppError::new(
        ErrorCode::Internal,
        "Açılışta başlama ayarı okunamadı. Mailhere'yi yeniden başlatıp tekrar dene.",
    )
}

fn write_error(on: bool) -> AppError {
    let how = if cfg!(windows) {
        if on {
            "Yeniden dene; sürerse Windows Ayarlar › Uygulamalar › Başlangıç'tan Mailhere'yi aç."
        } else {
            "Yeniden dene; sürerse Görev Yöneticisi › Başlangıç uygulamaları'ndan Mailhere'yi kapat."
        }
    } else if on {
        "~/.config/autostart klasörünün yazılabilir olduğunu denetleyip yeniden dene."
    } else {
        "Yeniden dene; sürerse ~/.config/autostart/mailhere.desktop dosyasını sil."
    };
    AppError::new(
        ErrorCode::Storage,
        format!("Açılış kaydı değiştirilemedi. {how}"),
    )
}

fn unavailable() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Açılışta başlama bu sistemde kullanılamıyor.",
    )
}

/// Açılışta başlama açık mı (kayıt var ve sistemin başlangıç ayarlarından
/// kapatılmamış).
#[tauri::command]
pub async fn autostart_get(app: AppHandle) -> Res<bool> {
    let state = app.try_state::<Autostart>().ok_or_else(unavailable)?;
    state.read().map(Entry::on).map_err(|e| {
        tracing::warn!(error = %e, "açılış kaydı okunamadı");
        read_error()
    })
}

/// Açılışta başlamayı açar ya da kapatır; seçim kaydedilir (kapalıysa bir
/// daha kendiliğinden açılmaz). Son durumu döner.
#[tauri::command]
pub async fn autostart_set(app: AppHandle, on: bool) -> Res<bool> {
    let state = app.try_state::<Autostart>().ok_or_else(unavailable)?;
    let done = if on {
        state.backend.enable(&state.exe)
    } else {
        state.backend.disable()
    };
    done.map_err(|e| {
        tracing::warn!(error = %e, on, "açılış kaydı değiştirilemedi");
        write_error(on)
    })?;
    if let Some(prefs) = app.try_state::<PrefsStore>() {
        if let Err(e) = prefs.update(|p| p.autostart = Some(on)) {
            tracing::warn!(error = %e, "açılış seçimi kaydedilemedi");
        }
    }
    state.read().map(Entry::on).map_err(|e| {
        tracing::warn!(error = %e, "açılış kaydı okunamadı");
        read_error()
    })
}

#[cfg(test)]
mod tests {
    use super::windows_run::*;
    use super::xdg::*;
    use super::*;

    #[test]
    fn gizli_baslatma_argumani() {
        assert!(is_hidden_args(["--tepside"]));
        assert!(is_hidden_args(["x", "--hidden"]));
        assert!(!is_hidden_args(Vec::<String>::new()));
        assert!(!is_hidden_args(["--tepside=1", "tepside"]));
    }

    #[test]
    fn appimage_yolu_baglama_noktasi_yerine_dosya() {
        let mounted = Path::new("/tmp/.mount_MailheAbC123/usr/bin/mailhere");
        let image = Path::new("/home/ali/Uygulamalar/Mailhere_0.8.0_amd64.AppImage");
        let appdir = Path::new("/tmp/.mount_MailheAbC123");
        assert_eq!(launch_path(mounted, Some(image), Some(appdir)), image);
        // APPDIR yoksa bağlama noktası adından anlaşılır.
        assert_eq!(launch_path(mounted, Some(image), None), image);
        // Miras kalmış APPIMAGE: bu ikili o bağlama noktasında değil.
        let deb = Path::new("/usr/bin/mailhere");
        assert_eq!(launch_path(deb, Some(image), Some(appdir)), deb);
        assert_eq!(launch_path(deb, Some(image), None), deb);
        assert_eq!(launch_path(deb, None, None), deb);
        // Göreli APPIMAGE yok sayılır.
        assert_eq!(
            launch_path(mounted, Some(Path::new("Mailhere.AppImage")), Some(appdir)),
            mounted
        );
    }

    #[test]
    fn windows_deger_bicimi() {
        let exe = Path::new(r"C:\Users\Ali Veli\AppData\Local\Mailhere\mailhere.exe");
        assert_eq!(
            run_command(exe),
            r#""C:\Users\Ali Veli\AppData\Local\Mailhere\mailhere.exe" --tepside"#
        );
    }

    #[test]
    fn windows_baslangic_onayi() {
        assert!(approved_enabled(None));
        assert!(approved_enabled(Some(&[
            0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ])));
        assert!(approved_enabled(Some(&[0x06, 0, 0, 0])));
        assert!(!approved_enabled(Some(&[
            0x03, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8
        ])));
        assert!(!approved_enabled(Some(&[0x07])));
        assert!(approved_enabled(Some(&[])));
    }

    #[test]
    fn linux_masaustu_girdisi() {
        let exe = Path::new("/usr/bin/mailhere");
        let entry = desktop_entry(exe).unwrap();
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(entry.contains("\nName=Mailhere\n"));
        assert!(entry.contains("\nExec=\"/usr/bin/mailhere\" --tepside\n"));
        assert!(entry.contains("\nTryExec=/usr/bin/mailhere\n"));
        assert!(entry.contains("\nX-GNOME-Autostart-enabled=true\n"));
        assert!(entry.contains("\nComment[tr]=Sistem tepsisinde başlar"));
        assert!(entry.ends_with('\n'));
        assert!(entry_enabled(&entry));
        assert!(entry_current(&entry, exe));
        assert!(!entry_current(&entry, Path::new("/opt/mailhere")));
    }

    #[test]
    fn linux_exec_kacisi() {
        // Boşluk, %, $, ` ve \ taşıyan AppImage yolu.
        let exe = Path::new("/home/a b/%x/$HOME/`c`/d\\e.AppImage");
        let entry = desktop_entry(exe).unwrap();
        assert!(
            entry.contains(
                "\nExec=\"/home/a b/%%x/\\\\$HOME/\\\\`c\\\\`/d\\\\\\\\e.AppImage\" --tepside\n"
            ),
            "{entry}"
        );
        assert!(entry.contains("\nTryExec=/home/a b/%x/$HOME/`c`/d\\\\e.AppImage\n"));
        assert!(entry_current(&entry, exe));
        // Satır sonu taşıyan yol yazılmaz.
        assert!(desktop_entry(Path::new("/tmp/a\nExec=evil")).is_none());
    }

    #[test]
    fn linux_kapatilmis_girdi() {
        let base = desktop_entry(Path::new("/usr/bin/mailhere")).unwrap();
        let gnome = base.replace(
            "X-GNOME-Autostart-enabled=true",
            "X-GNOME-Autostart-enabled=false",
        );
        assert!(!entry_enabled(&gnome));
        assert!(!entry_enabled(&format!("{base}Hidden=true\n")));
        // Başka gruptaki anahtar sayılmaz.
        assert!(entry_enabled(&format!(
            "{base}[Desktop Action x]\nHidden=true\n"
        )));
    }

    #[test]
    fn linux_yol_tazeleme_isaretleri_korur() {
        let old = desktop_entry(Path::new("/tmp/eski/Mailhere.AppImage"))
            .unwrap()
            .replace(
                "X-GNOME-Autostart-enabled=true",
                "X-GNOME-Autostart-enabled=false",
            )
            + "X-GNOME-Autostart-Delay=5\n[Desktop Action yeni]\nExec=baska\n";
        let new = Path::new("/home/ali/Mailhere.AppImage");
        let fresh = refreshed_entry(&old, new).unwrap();
        assert!(entry_current(&fresh, new));
        assert!(!entry_enabled(&fresh), "kullanıcının kapatması korunur");
        assert!(fresh.contains("X-GNOME-Autostart-Delay=5\n"));
        assert!(fresh.contains("[Desktop Action yeni]\nExec=baska\n"));
        assert_eq!(fresh.matches("TryExec=").count(), 1);
        // Exec satırı olmayan girdiye eklenir; grup yoksa baştan yazılır.
        let bare = "[Desktop Entry]\nName=Mailhere\n";
        assert!(entry_current(&refreshed_entry(bare, new).unwrap(), new));
        assert_eq!(refreshed_entry("bozuk", new), desktop_entry(new));
    }

    #[test]
    fn linux_dosya_arka_ucu() {
        let dir = std::env::temp_dir().join(format!("mailhere-acilis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = DesktopFile {
            path: dir.join("autostart").join("mailhere.desktop"),
        };
        let exe = Path::new("/usr/bin/mailhere");
        assert_eq!(file.read(exe).unwrap(), Entry::Absent);
        file.enable(exe).unwrap();
        assert_eq!(
            file.read(exe).unwrap(),
            Entry::Present {
                current: true,
                enabled: true
            }
        );
        let other = Path::new("/opt/Mailhere.AppImage");
        assert_eq!(
            file.read(other).unwrap(),
            Entry::Present {
                current: false,
                enabled: true
            }
        );
        file.refresh(other).unwrap();
        assert!(file.read(other).unwrap().on());
        file.disable().unwrap();
        file.disable().unwrap();
        assert_eq!(file.read(exe).unwrap(), Entry::Absent);
        assert!(!file.restore_missing());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn acilis_karari() {
        let absent = Entry::Absent;
        let stale = Entry::Present {
            current: false,
            enabled: true,
        };
        let fine = Entry::Present {
            current: true,
            enabled: true,
        };
        let off_in_os = Entry::Present {
            current: false,
            enabled: false,
        };
        // Geliştirme derlemesi / bellek kipi: hiçbir şey.
        assert_eq!(plan(None, absent, false, true), (Step::Nothing, None));
        assert_eq!(plan(Some(true), stale, false, true), (Step::Nothing, None));
        // İlk çalıştırma: bir kez açılır, seçim kaydedilir.
        assert_eq!(plan(None, absent, true, false), (Step::Enable, Some(true)));
        assert_eq!(plan(None, fine, true, false), (Step::Nothing, Some(true)));
        assert_eq!(plan(None, stale, true, false), (Step::Refresh, Some(true)));
        // Kapattıysa bir daha açılmaz.
        assert_eq!(plan(Some(false), absent, true, true), (Step::Nothing, None));
        assert_eq!(plan(Some(false), stale, true, true), (Step::Nothing, None));
        // Güncelleme sonrası yol değişti: tazelenir, sistemdeki kapalı işareti korunur.
        assert_eq!(plan(Some(true), stale, true, false), (Step::Refresh, None));
        assert_eq!(
            plan(Some(true), off_in_os, true, false),
            (Step::Refresh, None)
        );
        assert_eq!(plan(Some(true), fine, true, false), (Step::Nothing, None));
        // Kayıt kayboldu: Windows'ta (kaldırıp yeniden kurma) geri gelir, Linux'ta dokunulmaz.
        assert_eq!(plan(Some(true), absent, true, true), (Step::Enable, None));
        assert_eq!(plan(Some(true), absent, true, false), (Step::Nothing, None));
    }

    #[test]
    fn bellek_arka_ucu() {
        let m = Memory::new(true);
        let exe = Path::new("/x");
        assert!(m.read(exe).unwrap().on());
        m.disable().unwrap();
        assert!(!m.read(exe).unwrap().on());
        let u = Memory::unsupported();
        assert!(u.enable(exe).is_err());
        assert!(!u.read(exe).unwrap().on());
    }

    #[test]
    fn hata_metni_ne_oldu_ve_ne_yapilir() {
        for on in [true, false] {
            let e = write_error(on);
            assert!(e.message.starts_with("Açılış kaydı değiştirilemedi."));
            assert!(e.message.len() > "Açılış kaydı değiştirilemedi. ".len());
        }
    }
}
