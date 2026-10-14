//! Ek dosyası seçimi.
//!
//! Gönderilecek ek yolları yalnız kullanıcının bu oturumda işletim sisteminin
//! dosya penceresinde seçtiklerinden kabul edilir. Pencere kabukta açılır
//! (`Platform::pick_files`); arayüzden gelen başka bir yol (ör.
//! `~/.ssh/id_ed25519`, `/dev/zero`, ağ paylaşımı) postaya eklenemez. Böylece
//! arayüzde çalışabilecek yabancı bir betik bile dosya okuyup dışarı
//! gönderemez.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use mh_core::{AppError, ErrorCode};
use tauri::{State, WebviewWindow};

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

/// Bu oturumda kullanıcının seçtiği dosyalar.
#[derive(Default)]
pub struct Picked(Mutex<HashSet<PathBuf>>);

impl Picked {
    fn insert(&self, path: PathBuf) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(path);
    }

    fn contains(&self, path: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(&PathBuf::from(path))
    }

    /// Eklerin tümü kullanıcının seçtiği dosyalar mı?
    pub fn ensure(&self, paths: &[String]) -> Res<()> {
        if paths.iter().all(|p| self.contains(p)) {
            return Ok(());
        }
        tracing::warn!(
            count = paths.len(),
            "seçilmemiş dosya yolu ek olarak reddedildi"
        );
        Err(AppError::new(
            ErrorCode::Invalid,
            "Ek yalnız dosya seçme penceresinden eklenebilir. Dosyayı yeniden seç.",
        ))
    }
}

/// Kabuğun dosya penceresinden gelen yolları ek olarak kabul eder: yalnız
/// düzenli dosyalar (klasör, aygıt ya da kırık bağlantı ek olamaz) bu oturum
/// için işaretlenir ve dizge olarak döner.
pub fn accept(picked: &Picked, files: impl IntoIterator<Item = PathBuf>) -> Vec<String> {
    let mut out = Vec::new();
    for path in files {
        if !path.is_file() {
            continue;
        }
        out.push(path.to_string_lossy().into_owned());
        picked.insert(path);
    }
    out
}

/// Kullanıcının seçtiği yol `.ext` ile bitmiyorsa uzantı eklenir (dosya
/// başka bir türle açılmasın).
pub fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    let has = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext));
    if has {
        path
    } else {
        let mut name = path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "posta".into());
        name.push(".");
        name.push(ext);
        path.with_file_name(name)
    }
}

/// Kabuğun dosya penceresini açar; seçilen düzenli dosyaların yollarını döner
/// ve bu oturum için ek olarak kabul edilir sayar.
#[tauri::command]
pub async fn attachments_pick(
    window: WebviewWindow,
    state: State<'_, AppState>,
    picked: State<'_, Picked>,
) -> Res<Vec<String>> {
    let files = state.platform.pick_files(&window).await?;
    Ok(accept(&picked, files))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yalniz_secilen_yollar_kabul_edilir() {
        let p = Picked::default();
        p.insert(PathBuf::from("/ev/furkan/rapor.pdf"));
        assert!(p.ensure(&["/ev/furkan/rapor.pdf".into()]).is_ok());
        assert!(p.ensure(&[]).is_ok());
        let err = p
            .ensure(&[
                "/ev/furkan/rapor.pdf".into(),
                "/ev/furkan/.ssh/id_ed25519".into(),
            ])
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Invalid);
        assert!(p.ensure(&["/dev/zero".into()]).is_err());
    }

    #[test]
    fn secimden_yalniz_duzenli_dosyalar_gecer() {
        let p = Picked::default();
        let dir = std::env::temp_dir().join(format!("mh-sec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ek.txt");
        std::fs::write(&file, b"x").unwrap();
        let out = accept(&p, [file.clone(), dir.clone(), dir.join("yok.pdf")]);
        assert_eq!(out, vec![file.to_string_lossy().into_owned()]);
        assert!(p.ensure(&out).is_ok());
        assert!(p.ensure(&[dir.to_string_lossy().into_owned()]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uzanti_eklenir() {
        assert_eq!(
            with_extension(PathBuf::from("/ev/furkan/posta"), "eml"),
            PathBuf::from("/ev/furkan/posta.eml")
        );
        assert_eq!(
            with_extension(PathBuf::from("/ev/furkan/posta.txt"), "eml"),
            PathBuf::from("/ev/furkan/posta.txt.eml")
        );
        assert_eq!(
            with_extension(PathBuf::from("/ev/furkan/Posta.EML"), "eml"),
            PathBuf::from("/ev/furkan/Posta.EML")
        );
    }
}
