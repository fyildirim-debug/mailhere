//! JMAP komutları (`jmap_discover`, `account_add_jmap`) ve gerçek JMAP
//! hesaplarının postalarını IMAP motoru yerine JMAP sürücüsüne yönlendiren
//! yardımcılar. Tanıtım JMAP hesabı ("Sunucu") yerel kalır, ağa çıkmaz.

use std::collections::HashSet;

use mh_core::{
    Account, AccountKind, AppError, AppResult, ErrorCode, MessageId, MoveTarget, NewJmapAccount,
};
use tauri::{AppHandle, State};

use crate::commands::PALETTE;
use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Adresin alan adında JMAP oturum adresini arar.
#[tauri::command]
pub async fn jmap_discover(email: String) -> Res<String> {
    mh_jmap::discover(&email).await
}

/// JMAP hesabı ekler: oturum kimlik bilgisiyle alınarak doğrulanır, parola
/// anahtar zincirine yazılır, eşitleme başlar.
#[tauri::command]
pub async fn account_add_jmap(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewJmapAccount,
) -> Res<Account> {
    let (email, domain) = mh_sync::discover::split_email(&input.email)?;
    if input.password.is_empty() {
        return Err(AppError::new(ErrorCode::Invalid, "Parola boş olamaz."));
    }
    let url = mh_jmap::normalize_session_url(&input.session_url)?;
    if url
        .host_str()
        .is_some_and(|h| h.to_ascii_lowercase().ends_with(".invalid"))
    {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "JMAP oturum adresi geçersiz. Sunucunun gerçek adresini yaz.",
        ));
    }
    let username = match input.username.trim() {
        "" => email.clone(),
        u => u.to_owned(),
    };
    // Aynı kökendeki yönlendirmelerden sonraki son oturum adresi saklanır.
    let url = mh_jmap::test_login(url.as_str(), &username, &input.password).await?;

    let existing = state.store.accounts()?;
    let id = uuid::Uuid::new_v4().to_string();
    let secrets = state.secrets.clone();
    let (sid, password) = (id.clone(), input.password);
    tokio::task::spawn_blocking(move || secrets.set(&sid, &password))
        .await
        .map_err(internal)??;

    let account = Account {
        id,
        name: match input.name.trim() {
            "" => domain,
            n => n.to_owned(),
        },
        display_name: match input.display_name.trim() {
            "" => email.split('@').next().unwrap_or("").to_owned(),
            d => d.to_owned(),
        },
        email,
        color: input
            .color
            .filter(|c| c.len() == 7 && c.starts_with('#'))
            .unwrap_or_else(|| PALETTE[existing.len() % PALETTE.len()].to_owned()),
        kind: AccountKind::Jmap,
        imap: None,
        smtp: None,
        jmap_url: Some(url.to_string()),
        signature: None,
        created_at: now_ms(),
        auth: Default::default(),
        notify: true,
    };
    state.store.insert_account(&account)?;
    state.store.set_jmap_username(&account.id, &username)?;
    state.jmap.start(account.clone());
    crate::sink::changed(&app, Vec::new());
    Ok(account)
}

/// Hesap gerçek bir JMAP sunucusuyla mı eşitleniyor?
pub fn is_jmap_account(state: &AppState, account_id: &str) -> AppResult<bool> {
    Ok(state
        .store
        .account(account_id)?
        .is_some_and(|a| mh_jmap::is_syncable(&a)))
}

/// Postaları (IMAP ya da yerel, gerçek JMAP) diye ayırır.
fn split(state: &AppState, ids: &[MessageId]) -> AppResult<(Vec<MessageId>, Vec<MessageId>)> {
    let jmap: HashSet<String> = state
        .store
        .accounts()?
        .into_iter()
        .filter(mh_jmap::is_syncable)
        .map(|a| a.id)
        .collect();
    if jmap.is_empty() {
        return Ok((ids.to_vec(), Vec::new()));
    }
    let (mut other, mut via_jmap) = (Vec::new(), Vec::new());
    for loc in state.store.locations(ids)? {
        if jmap.contains(&loc.account_id) {
            via_jmap.push(loc.id);
        } else {
            other.push(loc.id);
        }
    }
    Ok((other, via_jmap))
}

fn merge(mut a: Vec<String>, b: Vec<String>) -> Vec<String> {
    a.extend(b);
    a.sort();
    a.dedup();
    a
}

/// Bayraklar: IMAP postaları IMAP motoruna, JMAP postaları `Email/set`e.
pub fn set_flags(
    state: &AppState,
    ids: &[MessageId],
    unread: Option<bool>,
    starred: Option<bool>,
) -> AppResult<Vec<String>> {
    let (other, jmap) = split(state, ids)?;
    let touched = state.sync.set_flags(&other, unread, starred)?;
    Ok(merge(
        touched,
        state.jmap.set_flags(&jmap, unread, starred)?,
    ))
}

/// Taşıma: IMAP postaları IMAP motoruna, JMAP postaları `Email/set`e.
pub fn move_messages(
    state: &AppState,
    ids: &[MessageId],
    target: MoveTarget,
) -> AppResult<Vec<String>> {
    let (other, jmap) = split(state, ids)?;
    let touched = state.sync.move_messages(&other, target)?;
    Ok(merge(touched, state.jmap.move_messages(&jmap, target)?))
}
