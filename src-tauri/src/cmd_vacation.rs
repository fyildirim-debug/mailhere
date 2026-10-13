//! Otomatik yanıt: `vacation_get`, `vacation_set`. Yalnız JMAP sunucusu
//! `VacationResponse` yeteneğini bildiren hesaplarda çalışır.

use mh_core::{Account, AppError, AppResult, ErrorCode, Vacation};
use tauri::State;

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

fn account(state: &AppState, account_id: &str) -> AppResult<Account> {
    state.store.account(account_id)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Hesap bulunamadı; kaldırılmış olabilir.",
        )
    })
}

/// Hesabın otomatik yanıtı; sunucu desteklemiyorsa ya da hesap IMAP ise `None`.
#[tauri::command]
pub async fn vacation_get(state: State<'_, AppState>, account_id: String) -> Res<Option<Vacation>> {
    let account = account(&state, &account_id)?;
    mh_jmap::vacation_get(&state.store, &state.secrets, &account).await
}

/// Otomatik yanıtı yazar ve sunucunun kaydettiği hali döner.
#[tauri::command]
pub async fn vacation_set(
    state: State<'_, AppState>,
    account_id: String,
    vacation: Vacation,
) -> Res<Vacation> {
    let account = account(&state, &account_id)?;
    mh_jmap::vacation_set(&state.store, &state.secrets, &account, vacation).await
}
