//! Taslak komutları: `draft_save`, `draft_delete`.
//!
//! Taslak yerelde hemen yazılır ve kimliği döner; sunucuya ekleme ve eski
//! kopyanın silinmesi hesabın komut kuyruğunda yapılır (çevrimdışıyken de
//! kayıt kaybolmaz).

use mh_core::{AppError, MessageId, OutgoingMessage};
use tauri::State;

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Taslağı kaydeder (`msg.draft_id` bir taslaksa onun yerine, aynı kimlikle)
/// ve taslağın kimliğini döner. İletilen postanın ekleri taslağa kopyalanır.
#[tauri::command]
pub async fn draft_save(
    state: State<'_, AppState>,
    picked: State<'_, crate::cmd_files::Picked>,
    msg: OutgoingMessage,
) -> Res<MessageId> {
    picked.ensure(&msg.attachments)?;
    if let Some(id) = msg.forward_from {
        crate::commands::ensure_body(&state, id).await?;
    }
    let outbox = state.outbox.clone();
    // Ekler diskten okunur ve posta kurulur: çalışma iş parçacığını tutmasın.
    tokio::task::spawn_blocking(move || outbox.save_draft(&msg, now_ms()))
        .await
        .map_err(internal)?
}

/// Taslağı yerelden ve sunucudan siler (IMAP ya da JMAP); zaten silinmişse
/// sessizce geçer.
#[tauri::command]
pub fn draft_delete(state: State<'_, AppState>, id: MessageId) -> Res<()> {
    state.outbox.delete_draft(id)
}
