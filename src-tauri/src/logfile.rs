//! Kalıcı günlük dosyası: `<veri klasörü>/logs/mailhere.log` (2 MB'ı aşınca
//! `mailhere.log.1`'e döner, en çok iki dosya). Pencereli uygulamanın
//! standart hata akışı kimsenin görmediği bir yere gider; posta gecikmesi ya
//! da kaçan bildirim gibi sorunlar bu dosyadan izlenir. Süzgeç ortaktır
//! (`mh_app::logging`): parola, jeton ve ham protokol trafiği yazılmaz.
//!
//! Dosya klasör bilindiğinde açılır (`open`); o ana kadarki kayıtlar yalnız
//! standart hata akışına gider.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

struct Open {
    file: File,
    path: PathBuf,
    size: u64,
}

static FILE: Mutex<Option<Open>> = Mutex::new(None);

/// Günlük dosyasını açar (klasör yoksa oluşturur).
pub fn open(data_dir: &Path) {
    let dir = data_dir.join("logs");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(error = %e, "günlük klasörü oluşturulamadı");
        return;
    }
    let path = dir.join("mailhere.log");
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            let size = file.metadata().map(|m| m.len()).unwrap_or(0);
            *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Open { file, path, size });
            tracing::info!(surum = env!("CARGO_PKG_VERSION"), "günlük dosyası açıldı");
        }
        Err(e) => tracing::warn!(error = %e, "günlük dosyası açılamadı"),
    }
}

/// `tracing_subscriber` yazıcısı: dosya açıksa ona yazar, değilse yutar.
pub struct Writer;

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = FILE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(open) = guard.as_mut() else {
            return Ok(buf.len());
        };
        if open.size + buf.len() as u64 > MAX_BYTES {
            let old = open.path.with_extension("log.1");
            let _ = std::fs::rename(&open.path, &old);
            if let Ok(file) = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&open.path)
            {
                open.file = file;
                open.size = 0;
            }
        }
        open.file.write_all(buf)?;
        open.size += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            Some(open) => open.file.flush(),
            None => Ok(()),
        }
    }
}
