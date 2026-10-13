//! Hesaplar.

use mh_core::{Account, AccountId, AccountUpdate, AppResult, AuthKind, ServerConfig};
use rusqlite::{params, OptionalExtension, Row};

use crate::codec::{from_json, kind_from, kind_str, to_json};
use crate::Store;

const COLS: &str = "id, name, email, display_name, color, kind, imap, smtp, jmap_url, signature,
    created_at, auth, notify";

fn auth_str(a: AuthKind) -> &'static str {
    match a {
        AuthKind::Password => "password",
        AuthKind::Google => "google",
        AuthKind::Microsoft => "microsoft",
    }
}

fn auth_from(s: &str) -> AuthKind {
    match s {
        "google" => AuthKind::Google,
        "microsoft" => AuthKind::Microsoft,
        _ => AuthKind::Password,
    }
}

fn row_to_account(r: &Row<'_>) -> rusqlite::Result<Account> {
    let server = |idx: usize| -> rusqlite::Result<Option<ServerConfig>> {
        match r.get::<_, Option<String>>(idx)? {
            Some(s) => from_json(idx, &s).map(Some),
            None => Ok(None),
        }
    };
    Ok(Account {
        id: r.get(0)?,
        name: r.get(1)?,
        email: r.get(2)?,
        display_name: r.get(3)?,
        color: r.get(4)?,
        kind: kind_from(&r.get::<_, String>(5)?),
        imap: server(6)?,
        smtp: server(7)?,
        jmap_url: r.get(8)?,
        signature: r.get(9)?,
        created_at: r.get(10)?,
        auth: auth_from(&r.get::<_, String>(11)?),
        notify: r.get(12)?,
    })
}

impl Store {
    /// Hesabı ekler ya da (aynı kimlikle) günceller.
    pub fn insert_account(&self, a: &Account) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                &format!(
                    "INSERT INTO accounts ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, email = excluded.email,
                       display_name = excluded.display_name, color = excluded.color, kind = excluded.kind,
                       imap = excluded.imap, smtp = excluded.smtp, jmap_url = excluded.jmap_url,
                       signature = excluded.signature, auth = excluded.auth,
                       notify = excluded.notify"
                ),
                params![
                    a.id,
                    a.name,
                    a.email,
                    a.display_name,
                    a.color,
                    kind_str(a.kind),
                    a.imap.as_ref().map(to_json),
                    a.smtp.as_ref().map(to_json),
                    a.jmap_url,
                    a.signature,
                    a.created_at,
                    auth_str(a.auth),
                    a.notify
                ],
            )?;
            Ok(())
        })
    }

    /// Hesaplar, eklenme sırasıyla.
    pub fn accounts(&self) -> AppResult<Vec<Account>> {
        self.with(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {COLS} FROM accounts ORDER BY created_at, rowid"
            ))?;
            let rows = st.query_map([], row_to_account)?;
            rows.collect()
        })
    }

    pub fn account(&self, id: &str) -> AppResult<Option<Account>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {COLS} FROM accounts WHERE id = ?1"),
                [id],
                row_to_account,
            )
            .optional()
        })
    }

    /// Hesabın düzenlenebilir alanlarını yazar (doğrulama çağıranın işidir).
    /// Hesap yoksa `None`.
    pub fn update_account(&self, id: &str, u: &AccountUpdate) -> AppResult<Option<Account>> {
        let changed = self.with(|c| {
            c.execute(
                "UPDATE accounts SET name = ?2, display_name = ?3, color = ?4, signature = ?5,
                   notify = ?6 WHERE id = ?1",
                params![id, u.name, u.display_name, u.color, u.signature, u.notify],
            )
        })?;
        if changed == 0 {
            return Ok(None);
        }
        self.account(id)
    }

    /// Hesabın gelen ve giden posta sunucu ayarlarını yazar (doğrulama ve
    /// bağlantı denemesi çağıranın işidir). Hesap yoksa `None`.
    pub fn update_account_servers(
        &self,
        id: &str,
        imap: &ServerConfig,
        smtp: &ServerConfig,
    ) -> AppResult<Option<Account>> {
        let changed = self.with(|c| {
            c.execute(
                "UPDATE accounts SET imap = ?2, smtp = ?3 WHERE id = ?1",
                params![id, to_json(imap), to_json(smtp)],
            )
        })?;
        if changed == 0 {
            return Ok(None);
        }
        self.account(id)
    }

    /// Hesabı ve (zincirleme) tüm postalarını siler; silinen içerik WAL'da da
    /// beklemesin diye ardından WAL sıfırlanır.
    pub fn remove_account(&self, id: &AccountId) -> AppResult<bool> {
        let removed =
            self.with(|c| Ok(c.execute("DELETE FROM accounts WHERE id = ?1", [id])? > 0))?;
        if removed {
            self.purge_freed()?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::{account, store_with_account};

    #[test]
    fn ekle_oku_guncelle_sil() {
        let s = store_with_account("a");
        let mut a = s.account("a").unwrap().unwrap();
        assert_eq!(a, account("a"));
        a.name = "Kişisel".into();
        s.insert_account(&a).unwrap();
        assert_eq!(s.accounts().unwrap()[0].name, "Kişisel");
        assert!(s.remove_account(&"a".to_string()).unwrap());
        assert!(s.account("a").unwrap().is_none());
        assert!(!s.remove_account(&"a".to_string()).unwrap());
    }

    #[test]
    fn duzenlenebilir_alanlar() {
        let s = store_with_account("a");
        let u = mh_core::AccountUpdate {
            name: "Kişisel".into(),
            display_name: "Furkan Y.".into(),
            color: "#2E9A61".into(),
            signature: Some("Sevgiler,\nFurkan".into()),
            notify: false,
        };
        let a = s.update_account("a", &u).unwrap().unwrap();
        assert_eq!(
            (a.name.as_str(), a.display_name.as_str(), a.color.as_str()),
            ("Kişisel", "Furkan Y.", "#2E9A61")
        );
        assert_eq!(a.signature.as_deref(), Some("Sevgiler,\nFurkan"));
        assert!(!a.notify);
        // Sunucu ayarları ve adres değişmez.
        assert_eq!(a.imap, account("a").imap);
        assert_eq!(a.email, "a@ornek.com.tr");
        assert_eq!(s.account("a").unwrap().unwrap(), a);
        assert!(s.update_account("yok", &u).unwrap().is_none());
    }

    #[test]
    fn bildirim_tercihi_okunur_yazilir() {
        let s = store_with_account("a");
        assert!(s.account("a").unwrap().unwrap().notify, "varsayılan açık");
        let mut a = account("a");
        a.notify = false;
        s.insert_account(&a).unwrap();
        assert!(!s.account("a").unwrap().unwrap().notify);
    }

    #[test]
    fn sunucu_ayarlari_yazilir() {
        let s = store_with_account("a");
        let server = |host: &str, port: u16| mh_core::ServerConfig {
            host: host.into(),
            port,
            security: mh_core::Security::Starttls,
            username: "furkan".into(),
        };
        let a = s
            .update_account_servers(
                "a",
                &server("posta.ornek.com", 143),
                &server("smtp.ornek.com", 587),
            )
            .unwrap()
            .unwrap();
        assert_eq!(a.imap, Some(server("posta.ornek.com", 143)));
        assert_eq!(a.smtp, Some(server("smtp.ornek.com", 587)));
        assert_eq!(
            (a.name.as_str(), a.email.as_str()),
            ("İş", "a@ornek.com.tr")
        );
        assert_eq!(s.account("a").unwrap().unwrap(), a);
        assert!(s
            .update_account_servers("yok", &server("x", 1), &server("y", 2))
            .unwrap()
            .is_none());
    }
}
