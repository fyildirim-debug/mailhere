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
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: now_ms(),
        auth: Default::default(),
        notify: true,
    };
    state.store.insert_account(&account)?;
    state.store.set_jmap_username(&account.id, &username)?;
    state.jmap.start(account.clone());
    crate::push::tetikle(&state);
    crate::lock::accounts_changed(&app);
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

/// Postaları (IMAP ya da yerel, gerçek JMAP) diye ayırır. Yerelde artık
/// olmayan kimlik, JMAP hesabı varken hiçbir gruba girmez.
pub(crate) fn split(
    state: &AppState,
    ids: &[MessageId],
) -> AppResult<(Vec<MessageId>, Vec<MessageId>)> {
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

#[cfg(test)]
mod tests {
    use mh_core::MailboxRole;
    use mh_store::{Action, NewMessage};

    use super::*;
    use crate::testutil::{deliver, mailbox_of, raw_mail, state};

    fn mail(mid: &str) -> Vec<u8> {
        raw_mail("Ayşe <ayse@eposta.com>", "Selam", mid, "", "Merhaba")
    }

    /// Gerçek sunuculu JMAP hesabı `j` (görevleri başlatılmaz) ve tanıtım
    /// JMAP hesabı `t` (ağa çıkmaz).
    fn with_jmap(st: &AppState) {
        for (id, url) in [
            ("j", "https://jmap.ornek.test/.well-known/jmap"),
            ("t", "https://sunucu.ornek.invalid/.well-known/jmap"),
        ] {
            let mut a = crate::testutil::account(id, &format!("{id}@ornek.test"));
            a.kind = AccountKind::Jmap;
            a.imap = None;
            a.smtp = None;
            a.jmap_url = Some(url.into());
            st.store.insert_account(&a).unwrap();
            for (path, role) in [
                ("mb-in", MailboxRole::Inbox),
                ("mb-ar", MailboxRole::Archive),
                ("mb-tr", MailboxRole::Trash),
            ] {
                st.store
                    .upsert_mailbox(id, path, path, role, None, false)
                    .unwrap();
            }
        }
    }

    fn jmap_mail(st: &AppState, account: &str, jmap_id: &str) -> MessageId {
        let mailbox = format!("{account}:mb-in");
        let m = NewMessage::from_raw(mail(&format!("{jmap_id}@x")), false, account, &mailbox, 10)
            .unwrap();
        st.store.jmap_upsert(&m, jmap_id, None, None).unwrap().id
    }

    #[tokio::test]
    async fn hesap_turu_ve_ayirma() {
        let st = state();
        // JMAP hesabı yokken hepsi IMAP / yerel yoldan gider.
        let a1 = deliver(&st, "a", mail("a1@x"), 10).id;
        assert_eq!(split(&st, &[a1, 77]).unwrap(), (vec![a1, 77], Vec::new()));

        with_jmap(&st);
        assert!(is_jmap_account(&st, "j").unwrap());
        assert!(
            !is_jmap_account(&st, "t").unwrap(),
            "tanıtım JMAP hesabı yereldir"
        );
        assert!(!is_jmap_account(&st, "a").unwrap());
        assert!(!is_jmap_account(&st, "yok").unwrap());

        let j1 = jmap_mail(&st, "j", "M1");
        let t1 = jmap_mail(&st, "t", "M2");
        let (other, jmap) = split(&st, &[j1, a1, t1]).unwrap();
        assert_eq!(jmap, [j1]);
        let mut other = other;
        other.sort_unstable();
        assert_eq!(other, [a1, t1], "IMAP ve tanıtım hesapları yerel yoldan");
        // Yerelde olmayan kimlik hiçbir gruba girmez.
        assert_eq!(split(&st, &[9_999_999]).unwrap(), (Vec::new(), Vec::new()));
        assert_eq!(split(&st, &[]).unwrap(), (Vec::new(), Vec::new()));
    }

    #[tokio::test]
    async fn bayrak_ve_tasima_iki_yola_da_gider() {
        let st = state();
        with_jmap(&st);
        let a1 = deliver(&st, "a", mail("a1@x"), 10).id;
        let j1 = jmap_mail(&st, "j", "M1");
        let unread = |id| st.store.summary(id).unwrap().unwrap().unread;
        let starred = |id| st.store.summary(id).unwrap().unwrap().starred;

        // Bayraklar: iki hesap türünde de yerelde hemen değişir; etkilenen
        // kutular birleşik, sıralı ve yinelenmeden döner.
        let touched = set_flags(&st, &[j1, a1, j1], Some(false), Some(true)).unwrap();
        assert_eq!(touched, ["a:INBOX", "j:mb-in"]);
        for id in [a1, j1] {
            assert!(!unread(id) && starred(id), "{id}");
        }
        assert!(set_flags(&st, &[a1], None, None).unwrap().is_empty());
        // Tanıtım hesabı sunucu kuyruğuna yazmaz.
        assert!(st.store.actions("a").unwrap().is_empty());

        // Taşıma: her posta kendi hesabının hedef kutusuna.
        let touched = move_messages(&st, &[a1, j1], MoveTarget::Archive).unwrap();
        assert_eq!(touched, ["a:Archive", "a:INBOX", "j:mb-ar", "j:mb-in"]);
        assert_eq!(mailbox_of(&st, a1), "a:Archive");
        assert_eq!(mailbox_of(&st, j1), "j:mb-ar");
        // Zaten hedefte: posta yerinde kalır; yalnız bulunduğu kutu bildirilir.
        let touched = move_messages(&st, &[a1, j1], MoveTarget::Archive).unwrap();
        assert_eq!(touched, ["a:Archive", "j:mb-ar"]);
        assert_eq!(mailbox_of(&st, a1), "a:Archive");
        let touched = move_messages(&st, &[j1], MoveTarget::Trash).unwrap();
        assert_eq!(touched, ["j:mb-ar", "j:mb-tr"]);
        // JMAP taşıması IMAP'in kalıcı kuyruğunu kullanmaz.
        assert!(!st
            .store
            .actions("j")
            .unwrap()
            .iter()
            .any(|a| matches!(a.action, Action::Moves)));
        // Yerelde olmayan posta sessizce atlanır.
        assert!(move_messages(&st, &[9_999_999], MoveTarget::Trash)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn kutu_listeleri_birlesir() {
        assert_eq!(
            merge(
                vec!["b:INBOX".into(), "a:INBOX".into()],
                vec!["a:INBOX".into(), "a:Trash".into()]
            ),
            ["a:INBOX", "a:Trash", "b:INBOX"]
        );
        assert!(merge(Vec::new(), Vec::new()).is_empty());
    }
}
