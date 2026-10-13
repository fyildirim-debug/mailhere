//! Taslak komutları: `draft_save`, `draft_delete`.
//!
//! Taslak yerelde hemen yazılır ve kimliği döner; sunucuya ekleme ve eski
//! kopyanın silinmesi hesabın komut kuyruğunda yapılır (çevrimdışıyken de
//! kayıt kaybolmaz).

use mh_core::{AccountKind, AppError, ErrorCode, MessageId, OutgoingMessage};
use tauri::State;

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Taslaklar şimdilik yalnız IMAP hesaplarında.
fn ensure_imap(state: &AppState, account_id: &str) -> Res<()> {
    let account = state
        .store
        .account(account_id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Hesap bulunamadı."))?;
    if account.kind == AccountKind::Jmap {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "JMAP hesaplarında taslak kaydı henüz yok.",
        ));
    }
    Ok(())
}

/// Taslağı kaydeder (`msg.draft_id` bir taslaksa onun yerine, aynı kimlikle)
/// ve taslağın kimliğini döner. İletilen postanın ekleri taslağa kopyalanır.
#[tauri::command]
pub async fn draft_save(state: State<'_, AppState>, msg: OutgoingMessage) -> Res<MessageId> {
    ensure_imap(&state, &msg.account_id)?;
    if let Some(id) = msg.forward_from {
        crate::commands::ensure_body(&state, id).await?;
    }
    let outbox = state.outbox.clone();
    // Ekler diskten okunur ve posta kurulur: çalışma iş parçacığını tutmasın.
    tokio::task::spawn_blocking(move || outbox.save_draft(&msg, now_ms()))
        .await
        .map_err(internal)?
}

/// Taslağı yerelden ve sunucudan siler; zaten silinmişse sessizce geçer.
#[tauri::command]
pub fn draft_delete(state: State<'_, AppState>, id: MessageId) -> Res<()> {
    if let Some(loc) = state.store.location(id)? {
        ensure_imap(&state, &loc.account_id)?;
    }
    state.outbox.delete_draft(id)
}
