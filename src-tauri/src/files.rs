//! İşletim sisteminin dosya pencereleri: ek seçme ve kaydetme
//! (`Platform::pick_files` / `Platform::save_file`).

use std::path::{Path, PathBuf};

use mh_app::commands::write_private;
use mh_app::state::internal;
use mh_core::{AppError, AppResult, ErrorCode};
use tauri::WebviewWindow;
use tauri_plugin_dialog::DialogExt;

/// Dosya seçme penceresi; kullanıcının seçtiği yollar (klasör ve aygıt
/// süzmesi ortak katmanda, `files::accept`).
pub async fn pick(window: &WebviewWindow) -> AppResult<Vec<PathBuf>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(window)
        .pick_files(move |files| {
            let _ = tx.send(files);
        });
    let files = rx.await.map_err(internal)?.unwrap_or_default();
    Ok(files
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .collect())
}

/// Kaydet penceresindeki süzgeç adı.
fn filter_label(ext: &str) -> String {
    match ext {
        "eml" => "E-posta (.eml)".to_owned(),
        other => format!(".{other}"),
    }
}

/// Kaydet penceresi: önerilen adın uzantısı korunur, dosya yalnız kullanıcıya
/// okunur (0600) yazılır; vazgeçilirse `None`.
pub async fn save(
    window: &WebviewWindow,
    suggested: &str,
    bytes: Vec<u8>,
) -> AppResult<Option<String>> {
    let ext = Path::new(suggested)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = window
        .dialog()
        .file()
        .set_parent(window)
        .set_file_name(suggested);
    if !ext.is_empty() {
        dialog = dialog.add_filter(filter_label(&ext), &[ext.as_str()]);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(picked) = rx.await.map_err(internal)? else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|_| {
        AppError::new(
            ErrorCode::Invalid,
            "Seçilen konum bir dosya yolu değil. Bilgisayarında bir klasör seç.",
        )
    })?;
    let path = if ext.is_empty() {
        path
    } else {
        mh_app::files::with_extension(path, &ext)
    };
    let written = tokio::task::spawn_blocking(move || write_private(&path, &bytes).map(|_| path))
        .await
        .map_err(internal)?
        .map_err(|e| {
            tracing::warn!(error = %e, "dosya yazılamadı");
            AppError::new(
                ErrorCode::Storage,
                "Dosya yazılamadı. Klasörün yazılabilir olduğunu ve boş alanı denetle.",
            )
        })?;
    Ok(Some(written.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn suzgec_adi() {
        assert_eq!(super::filter_label("eml"), "E-posta (.eml)");
        assert_eq!(super::filter_label("ics"), ".ics");
    }
}
