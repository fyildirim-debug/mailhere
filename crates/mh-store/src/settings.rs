//! Ayarlar: tek satırlık JSON.

use mh_core::{AppResult, Settings};
use rusqlite::OptionalExtension;

use crate::codec::to_json;
use crate::Store;

impl Store {
    /// Kayıtlı ayarlar; yoksa ya da okunamazsa varsayılanlar.
    pub fn settings(&self) -> AppResult<Settings> {
        let json: Option<String> = self.with(|c| {
            c.query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0))
                .optional()
        })?;
        Ok(json
            .and_then(|j| match serde_json::from_str(&j) {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!(error = %e, "ayarlar okunamadı, varsayılanlar kullanılıyor");
                    None
                }
            })
            .unwrap_or_default())
    }

    pub fn set_settings(&self, s: &Settings) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings (id, json) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                [to_json(s)],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{NotifyMode, Settings, Theme};

    use crate::Store;

    #[test]
    fn ayarlar_kalici() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.settings().unwrap(), Settings::default());
        let v = Settings {
            theme: Theme::Dark,
            notify: NotifyMode::Vip,
            remote_allow: vec!["ornek.com".into()],
            ..Settings::default()
        };
        s.set_settings(&v).unwrap();
        assert_eq!(s.settings().unwrap(), v);
    }
}
