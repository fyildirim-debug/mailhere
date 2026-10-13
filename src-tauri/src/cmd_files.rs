//! Ek dosyası seçimi.
//!
//! Gönderilecek ek yolları yalnız kullanıcının bu oturumda işletim sisteminin
//! dosya penceresinde seçtiklerinden kabul edilir. Pencere Rust'ta açılır;
//! arayüzden gelen başka bir yol (ör. `~/.ssh/id_ed25519`, `/dev/zero`, ağ
//! paylaşımı) postaya eklenemez. Böylece arayüzde çalışabilecek yabancı bir
//! betik bile dosya okuyup dışarı gönderemez.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use mh_core::{AppError, ErrorCode};
use tauri::{State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::state::internal;

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

/// İşletim sisteminin dosya penceresini açar; seçilen düzenli dosyaların
/// yollarını döner ve bu oturum için ek olarak kabul edilir sayar.
#[tauri::command]
pub async fn attachments_pick(
    window: WebviewWindow,
    picked: State<'_, Picked>,
) -> Res<Vec<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .pick_files(move |files| {
            let _ = tx.send(files);
        });
    let files = rx.await.map_err(internal)?.unwrap_or_default();
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let Ok(path) = f.into_path() else { continue };
        // Klasör, aygıt ya da kırık bağlantı ek olamaz.
        if !path.is_file() {
            continue;
        }
        out.push(path.to_string_lossy().into_owned());
        picked.insert(path);
    }
    Ok(out)
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
}
