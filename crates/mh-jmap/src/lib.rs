//! Mailhere JMAP (RFC 8620, RFC 8621): oturum keşfi, kutu ve posta
//! eşitleme, durum değişikliği akışı (EventSource), sunucu işlemleri,
//! gönderim ve taslaklar. Stalwart hedeflenir; yalnız standarda dayanır.
//!
//! Yerel modelde kutunun yolu JMAP kutu kimliğidir (kutu kimliği
//! `"<hesap>:<jmap kutu kimliği>"`); her JMAP postası tek satırdır ve
//! `jmap_messages` tablosuyla sunucu kimliğine bağlanır.

mod client;
mod discover;
mod error;
mod manager;
mod ops;
mod push;
mod send;
mod sync;
mod types;

use std::sync::Arc;

use mh_core::{Account, AccountKind, AppError, AppResult, ErrorCode};
use mh_store::Store;
use mh_sync::SecretStore;

pub use client::{
    expand_template, fetch_session, normalize_session_url, test_login, Call, Client, Credentials,
};
pub use discover::{candidates, discover, probe};
pub use error::{JmapError, JmapResult};
pub use manager::{JmapManager, Timing};
pub use ops::{destroy_emails, ensure_role_mailbox, move_emails, set_keywords};
pub use push::{touches_account, SseEvent, SseParser};
pub use send::{delete_draft, envelope_recipients, save_draft, submit, SavedDraft};
pub use sync::{
    email_properties, fetch_body, full_sync, incremental_sync, prefetch, role_from, sync_mailboxes,
    to_new_message, SyncOutcome, INITIAL_FETCH, SYNC_ROLES,
};
pub use types::{Email, Session, CORE, MAIL, SUBMISSION};

/// Tanıtım hesabı mı (oturum adresi yok ya da `.invalid` alanında)? Ağa
/// hiç çıkılmaz.
pub fn is_demo(account: &Account) -> bool {
    account
        .jmap_url
        .as_deref()
        .and_then(|u| reqwest::Url::parse(u).ok())
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_none_or(|h| h.trim_end_matches('.').ends_with(".invalid"))
}

/// Bu hesap gerçek bir JMAP sunucusuyla eşitlenir mi?
pub fn is_syncable(account: &Account) -> bool {
    account.kind == AccountKind::Jmap && !is_demo(account)
}

/// Hesabın JMAP oturumunu açar (kullanıcı adı depoda, parola anahtar zincirinde).
pub async fn connect(
    store: &Store,
    secrets: &Arc<dyn SecretStore>,
    account: &Account,
) -> JmapResult<Client> {
    if !is_syncable(account) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Bu hesap bir JMAP sunucusuna bağlı değil.",
        )
        .into());
    }
    let url = account.jmap_url.as_deref().unwrap_or_default();
    let username = store
        .jmap_account_state(&account.id)?
        .username
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| account.email.clone());
    let password = mh_sync::password_for(secrets, &account.id).await?;
    Client::connect(url, Credentials::new(username, password)).await
}

/// Giden kutusundan: ham postayı hesabın JMAP sunucusuyla gönderir.
/// Sunucu postayı Gönderilenler'e kendisi taşır.
pub async fn send_raw(
    store: &Store,
    secrets: &Arc<dyn SecretStore>,
    account: &Account,
    raw: &[u8],
    rcpt_to: &[String],
) -> AppResult<()> {
    let client = connect(store, secrets, account)
        .await
        .map_err(|e| e.to_app())?;
    send::submit(&client, &account.email, &account.display_name, raw, rcpt_to)
        .await
        .map_err(|e| e.to_app())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(url: Option<&str>) -> Account {
        Account {
            id: "a".into(),
            name: "Sunucu".into(),
            email: "posta@alanadi.com".into(),
            display_name: "Furkan".into(),
            color: "#D08A1E".into(),
            kind: AccountKind::Jmap,
            imap: None,
            smtp: None,
            jmap_url: url.map(str::to_owned),
            signature: None,
            created_at: 0,
            auth: Default::default(),
        }
    }

    #[test]
    fn tanitim_hesabi_aga_cikmaz() {
        assert!(is_demo(&account(Some("https://demo.invalid/jmap"))));
        assert!(is_demo(&account(None)));
        assert!(is_demo(&account(Some("bozuk"))));
        assert!(!is_demo(&account(Some(
            "https://posta.alanadi.com/.well-known/jmap"
        ))));
        assert!(is_syncable(&account(Some("http://127.0.0.1:8080/jmap"))));
        let mut imap = account(Some("https://posta.alanadi.com/jmap"));
        imap.kind = AccountKind::Imap;
        assert!(!is_syncable(&imap));
    }

    #[tokio::test]
    async fn tanitim_hesabina_baglanilmaz() {
        let store = Store::open_in_memory().unwrap();
        let secrets: Arc<dyn SecretStore> = Arc::new(mh_sync::MemorySecrets::new());
        let err = connect(
            &store,
            &secrets,
            &account(Some("https://demo.invalid/jmap")),
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_app().code, ErrorCode::Invalid);
    }
}
