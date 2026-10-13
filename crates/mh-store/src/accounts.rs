//! Hesaplar.

use mh_core::{Account, AccountId, AccountUpdate, AppResult, AuthKind, ServerConfig, Signature};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::codec::{from_json, kind_from, kind_str, to_json};
use crate::Store;

const COLS: &str =
    "id, name, email, display_name, color, kind, imap, smtp, jmap_url, signature, created_at, auth";

/// Eski düz imzanın okunurken aldığı kimlik ve ad.
const LEGACY_ID: &str = "imza";
const LEGACY_NAME: &str = "İmza";

/// `accounts.signature` sütununun biçimi (sürüm 2): imza listesi ve
/// varsayılanlar tek JSON metninde. Önceki sürümler aynı sütuna düz metin
/// tek imza yazıyordu; geçiş gerekmez, okurken çevrilir.
#[derive(Serialize, Deserialize, Default)]
struct SignatureSet {
    v: u8,
    #[serde(default)]
    signatures: Vec<Signature>,
    #[serde(default)]
    new: Option<String>,
    #[serde(default)]
    reply: Option<String>,
}

/// (imzalar, yeni posta varsayılanı, yanıt varsayılanı)
type Signatures = (Vec<Signature>, Option<String>, Option<String>);

/// Sütun değerini çözer. `{"v":2` ile başlamayan dolu değer eski düz imzadır:
/// tek imza olur ve iki varsayılan da o imzadır.
fn decode_signatures(column: Option<&str>) -> Signatures {
    let Some(text) = column.filter(|t| !t.trim().is_empty()) else {
        return (Vec::new(), None, None);
    };
    if text.starts_with("{\"v\":2") {
        match serde_json::from_str::<SignatureSet>(text) {
            Ok(set) => return (set.signatures, set.new, set.reply),
            // Bozuk kayıt kaybolmasın: düz metin olarak gösterilir.
            Err(e) => tracing::warn!(error = %e, "imza kaydı çözülemedi; düz metin okunuyor"),
        }
    }
    let legacy = Signature {
        id: LEGACY_ID.into(),
        name: LEGACY_NAME.into(),
        body: text.to_owned(),
    };
    (vec![legacy], Some(LEGACY_ID.into()), Some(LEGACY_ID.into()))
}

/// Sütuna yazılacak değer; imza yoksa `NULL`.
fn encode_signatures(
    signatures: &[Signature],
    new: Option<&String>,
    reply: Option<&String>,
) -> Option<String> {
    if signatures.is_empty() {
        return None;
    }
    Some(to_json(&SignatureSet {
        v: 2,
        signatures: signatures.to_vec(),
        new: new.cloned(),
        reply: reply.cloned(),
    }))
}

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
    let (signatures, signature_new, signature_reply) =
        decode_signatures(r.get::<_, Option<String>>(9)?.as_deref());
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
        signatures,
        signature_new,
        signature_reply,
        created_at: r.get(10)?,
        auth: auth_from(&r.get::<_, String>(11)?),
    })
}

impl Store {
    /// Hesabı ekler ya da (aynı kimlikle) günceller.
    pub fn insert_account(&self, a: &Account) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                &format!(
                    "INSERT INTO accounts ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, email = excluded.email,
                       display_name = excluded.display_name, color = excluded.color, kind = excluded.kind,
                       imap = excluded.imap, smtp = excluded.smtp, jmap_url = excluded.jmap_url,
                       signature = excluded.signature, auth = excluded.auth"
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
                    encode_signatures(
                        &a.signatures,
                        a.signature_new.as_ref(),
                        a.signature_reply.as_ref()
                    ),
                    a.created_at,
                    auth_str(a.auth)
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
                "UPDATE accounts SET name = ?2, display_name = ?3, color = ?4, signature = ?5 WHERE id = ?1",
                params![
                    id,
                    u.name,
                    u.display_name,
                    u.color,
                    encode_signatures(
                        &u.signatures,
                        u.signature_new.as_ref(),
                        u.signature_reply.as_ref()
                    )
                ],
            )
        })?;
        if changed == 0 {
            return Ok(None);
        }
        self.account(id)
    }

    /// Hesabı ve tüm yerel verisini siler. Silindiyse `true`.
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
    use mh_core::Signature;

    use super::{decode_signatures, encode_signatures};
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

    fn sig(id: &str, name: &str, body: &str) -> Signature {
        Signature {
            id: id.into(),
            name: name.into(),
            body: body.into(),
        }
    }

    #[test]
    fn duzenlenebilir_alanlar() {
        let s = store_with_account("a");
        let u = mh_core::AccountUpdate {
            name: "Kişisel".into(),
            display_name: "Furkan Y.".into(),
            color: "#2E9A61".into(),
            signatures: vec![
                sig("uzun", "Uzun", "Sevgiler,\nFurkan"),
                sig("kisa", "Kısa", "F."),
            ],
            signature_new: Some("uzun".into()),
            signature_reply: Some("kisa".into()),
        };
        let a = s.update_account("a", &u).unwrap().unwrap();
        assert_eq!(
            (a.name.as_str(), a.display_name.as_str(), a.color.as_str()),
            ("Kişisel", "Furkan Y.", "#2E9A61")
        );
        assert_eq!(a.signatures, u.signatures);
        assert_eq!(a.signature_new.as_deref(), Some("uzun"));
        assert_eq!(a.signature_reply.as_deref(), Some("kisa"));
        // Sunucu ayarları ve adres değişmez.
        assert_eq!(a.imap, account("a").imap);
        assert_eq!(a.email, "a@ornek.com.tr");
        assert_eq!(s.account("a").unwrap().unwrap(), a);
        assert!(s.update_account("yok", &u).unwrap().is_none());
        // Ekleme/güncelleme yolu da aynı biçimi yazar.
        s.insert_account(&a).unwrap();
        assert_eq!(s.account("a").unwrap().unwrap(), a);
    }

    #[test]
    fn imza_sutunu_bicimleri() {
        // Eski düz imza tek imza olur, iki varsayılan da odur.
        let (list, new, reply) = decode_signatures(Some("-- \nFurkan"));
        assert_eq!(list, vec![sig("imza", "İmza", "-- \nFurkan")]);
        assert_eq!(
            (new.as_deref(), reply.as_deref()),
            (Some("imza"), Some("imza"))
        );
        // Boş ya da yok: imza yok.
        for empty in [None, Some(""), Some("  \n")] {
            assert_eq!(decode_signatures(empty), (Vec::new(), None, None));
        }
        // Sürüm 2.
        let text =
            encode_signatures(&[sig("x", "X", "gövde")], None, Some(&"x".to_owned())).unwrap();
        assert!(text.starts_with("{\"v\":2"), "{text}");
        assert_eq!(
            decode_signatures(Some(&text)),
            (vec![sig("x", "X", "gövde")], None, Some("x".to_owned()))
        );
        assert_eq!(encode_signatures(&[], None, None), None);
        // Bozuk kayıt kaybolmaz: düz metin okunur.
        let (list, _, _) = decode_signatures(Some("{\"v\":2,bozuk"));
        assert_eq!(list[0].body, "{\"v\":2,bozuk");
    }

    #[test]
    fn eski_duz_imza_veritabanindan_okunur() {
        let s = store_with_account("a");
        s.with(|c| {
            c.execute(
                "UPDATE accounts SET signature = 'Sevgiler' WHERE id = 'a'",
                [],
            )
        })
        .unwrap();
        let a = s.account("a").unwrap().unwrap();
        assert_eq!(a.signatures, vec![sig("imza", "İmza", "Sevgiler")]);
        assert_eq!(a.signature_new.as_deref(), Some("imza"));
        assert_eq!(a.signature_reply.as_deref(), Some("imza"));
    }
}
