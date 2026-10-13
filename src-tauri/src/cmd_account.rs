//! Hesap düzenleme: `account_update`.

use mh_core::{Account, AccountUpdate, AppError, AppResult, ErrorCode};
use tauri::{AppHandle, State};

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::Invalid, message)
}

/// "#RRGGBB" biçiminde mi?
fn is_hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Düzenlemeyi denetler ve sadeleştirir: adlar kırpılır, renk büyük harfe
/// çevrilir, imzalar `signatures::normalize_signatures` ile denetlenir.
pub fn normalize_update(u: AccountUpdate) -> AppResult<AccountUpdate> {
    let name = mh_send::clean_name(&u.name);
    let name = name.as_str();
    if name.is_empty() {
        return Err(invalid("Hesap adı boş olamaz."));
    }
    let display_name = mh_send::clean_name(&u.display_name);
    let display_name = display_name.as_str();
    if display_name.is_empty() {
        return Err(invalid("Gönderen adı boş olamaz."));
    }
    let color = u.color.trim();
    if !is_hex_color(color) {
        return Err(invalid("Renk #RRGGBB biçiminde olmalı (ör. #4C6FD8)."));
    }
    let (signatures, signature_new, signature_reply) =
        crate::signatures::normalize_signatures(u.signatures, u.signature_new, u.signature_reply)?;
    Ok(AccountUpdate {
        name: name.to_owned(),
        display_name: display_name.to_owned(),
        color: color.to_ascii_uppercase(),
        signatures,
        signature_new,
        signature_reply,
    })
}

/// Hesabın adını, gönderen adını, rengini ve imzasını değiştirir.
#[tauri::command]
pub fn account_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    update: AccountUpdate,
) -> Res<Account> {
    let update = normalize_update(update)?;
    let account = state.store.update_account(&id, &update)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Hesap bulunamadı; kaldırılmış olabilir.",
        )
    })?;
    crate::sink::changed(&app, Vec::new());
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tek imzalı (iki varsayılanı da o olan) düzenleme.
    fn update(name: &str, display: &str, color: &str, sig: Option<&str>) -> AccountUpdate {
        AccountUpdate {
            name: name.into(),
            display_name: display.into(),
            color: color.into(),
            signatures: sig
                .map(|body| mh_core::Signature {
                    id: "s".into(),
                    name: "İmza".into(),
                    body: body.into(),
                })
                .into_iter()
                .collect(),
            signature_new: sig.map(|_| "s".into()),
            signature_reply: sig.map(|_| "s".into()),
        }
    }

    #[test]
    fn duzenleme_sadelestirilir() {
        let u = normalize_update(update(
            "  İş ",
            " Furkan Yıldırım ",
            "#4c6fd8",
            Some("-- \nFurkan\n\n"),
        ))
        .unwrap();
        assert_eq!(
            u,
            update("İş", "Furkan Yıldırım", "#4C6FD8", Some("-- \nFurkan"))
        );
        let mut u = update("İş", "F", "#2E9A61", None);
        u.signature_new = Some("silinmis".into());
        let u = normalize_update(u).unwrap();
        assert_eq!(u.signature_new, None, "listede olmayan varsayılan kalkar");
    }

    #[test]
    fn gecersiz_duzenleme() {
        let err = |u| normalize_update(u).unwrap_err();
        let e = err(update("  ", "F", "#2E9A61", None));
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("Hesap adı"));
        assert!(err(update("İş", "\t", "#2E9A61", None))
            .message
            .contains("Gönderen adı"));
        for color in ["2E9A61", "#2E9A6", "#2E9A611", "#GG9A61", "", "#2e9a6ı"] {
            assert!(
                err(update("İş", "F", color, None))
                    .message
                    .contains("#RRGGBB"),
                "{color}"
            );
        }
    }
}
