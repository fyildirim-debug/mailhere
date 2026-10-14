//! Klasör ayarları: kenar çubuğunda gizleme, elle rol atama ve sıfırlama.
//!
//! Gizli özel klasör ve gizli Gereksiz eşitlenmez; rol kutuları (gelen,
//! gönderilen, taslak, arşiv, çöp) gizli olsa da eşitlenir. Değişiklikten sonra
//! hesabın eşitlemesi hemen tetiklenir: IMAP'te ilgili kutu (ya da sıfırlamada
//! tüm hesap), JMAP'te baştan eşitleme (izlenen kutular değişti).

use mh_core::{AppError, ErrorCode, MailboxRole};
use tauri::{AppHandle, State};

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

fn not_found() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Klasör bulunamadı; sunucuda silinmiş ya da yeniden adlandırılmış olabilir.",
    )
}

/// Klasörü kenar çubuğunda gizler ya da gösterir. Gelen kutusu gizlenemez.
/// Gösterilen klasör hemen eşitlenir.
#[tauri::command]
pub fn mailbox_set_hidden(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    hidden: bool,
) -> Res<()> {
    let before = state.store.mailbox(&id)?.ok_or_else(not_found)?;
    let after = state.store.set_mailbox_hidden(&id, hidden)?;
    let account = &after.mailbox.account_id;
    if crate::cmd_jmap::is_jmap_account(&state, account)? {
        let was = mh_jmap::tracked(before.mailbox.role, before.mailbox.hidden);
        if was != mh_jmap::tracked(after.mailbox.role, after.mailbox.hidden) {
            state.jmap.resync(account);
        }
    } else if !hidden {
        // Rol kutusu zaten eşitleniyordu; kutu eşitlemesi yine de zararsız.
        state.sync.sync_path(account, &after.mailbox.path);
    }
    crate::sink::changed(&app, vec![id]);
    Ok(())
}

/// Klasörü Gönderilenler, Taslaklar, Arşiv, Çöp ya da Gereksiz yapar; rolün
/// önceki sahibi sıradan klasöre döner. Seçim sonraki eşitlemelerde korunur.
#[tauri::command]
pub fn mailbox_set_role(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    role: MailboxRole,
) -> Res<()> {
    let touched = state.store.set_mailbox_role(&id, role)?;
    let info = state.store.mailbox(&id)?.ok_or_else(not_found)?;
    let account = &info.mailbox.account_id;
    if crate::cmd_jmap::is_jmap_account(&state, account)? {
        state.jmap.resync(account);
    } else {
        state.sync.sync_path(account, &info.mailbox.path);
    }
    crate::sink::changed(&app, touched);
    Ok(())
}

/// Hesabın elle atanan klasör görevlerini kaldırır; roller hemen başlayan
/// eşitlemede sunucudan yeniden bulunur.
#[tauri::command]
pub fn mailbox_roles_reset(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Res<()> {
    if state.store.account(&account_id)?.is_none() {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "Hesap bulunamadı; kaldırılmış olabilir.",
        ));
    }
    let touched = state.store.reset_mailbox_roles(&account_id)?;
    if crate::cmd_jmap::is_jmap_account(&state, &account_id)? {
        state.jmap.resync(&account_id);
    } else {
        state.sync.sync_account(&account_id);
    }
    crate::sink::changed(&app, touched);
    Ok(())
}
