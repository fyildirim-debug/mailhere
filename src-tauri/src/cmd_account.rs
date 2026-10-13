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
/// çevrilir, boş imza kaldırılır.
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
    let signature = u
        .signature
        .map(|s| s.trim_end().to_owned())
        .filter(|s| !s.trim().is_empty());
    Ok(AccountUpdate {
        name: name.to_owned(),
        display_name: display_name.to_owned(),
        color: color.to_ascii_uppercase(),
        signature,
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

    fn update(name: &str, display: &str, color: &str, sig: Option<&str>) -> AccountUpdate {
        AccountUpdate {
            name: name.into(),
            display_name: display.into(),
            color: color.into(),
            signature: sig.map(str::to_owned),
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
        let u = normalize_update(update("İş", "F", "#2E9A61", Some(" \n "))).unwrap();
        assert_eq!(u.signature, None, "boş imza kaldırılır");
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
