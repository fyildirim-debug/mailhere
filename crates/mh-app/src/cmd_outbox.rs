//! Planlanan gönderimler: `outbox_list`, `outbox_send_now`, `outbox_to_draft`.
//!
//! Giden kutusu değişince (ekle, gönderildi, iptal, taslağa çevrildi) giden
//! kutusu `mh:changed` yayar; arayüz sayaçları (`counts.scheduled`) tazeler.

use mh_core::{AppError, MessageId, ScheduledItem};
use tauri::State;

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Giden kutusundaki gönderimler, gönderim anı artan.
#[tauri::command]
pub fn outbox_list(state: State<'_, AppState>) -> Res<Vec<ScheduledItem>> {
    state.outbox.scheduled()
}

/// Gönderim anını şimdiye çeker ve göndericiyi uyandırır.
#[tauri::command]
pub fn outbox_send_now(state: State<'_, AppState>, id: String) -> Res<()> {
    state.outbox.send_now(&id, now_ms())
}

/// Gönderimi iptal eder (`send_cancel` ile aynı güvence: gönderim başladıysa
/// düzenlenemez) ve postayı taslağa çevirir; taslağın kimliğini döner.
#[tauri::command]
pub async fn outbox_to_draft(state: State<'_, AppState>, id: String) -> Res<MessageId> {
    // İletilen postanın ekleri taslağa kopyalanır: içerik yerelde yoksa önce
    // indirilir. İndirilemezse taslak hatayı söyler, gönderim yerinde kalır.
    let forwarded = state
        .store
        .outbox_message(&id)?
        .and_then(|m| m.forward_from);
    if let Some(source) = forwarded {
        if let Err(e) = crate::commands::ensure_body(&state, source).await {
            tracing::debug!(error = %e.message, "iletilen postanın içeriği indirilemedi");
        }
    }
    let outbox = state.outbox.clone();
    // Ekler diskten okunur ve posta kurulur: çalışma iş parçacığını tutmasın.
    tokio::task::spawn_blocking(move || outbox.to_draft(&id, now_ms()))
        .await
        .map_err(internal)?
}
