//! Kalıcı silme: `messages_delete` ve `mailbox_empty`.
//!
//! İkisi de geri alınamaz ve yalnız Çöp ile İstenmeyen kutularında çalışır;
//! başka kutudaki posta burada silinemez (önce Çöp'e taşınır). Posta yerelden
//! hemen kalkar; sunucudaki silme hesabın kalıcı işlem kuyruğuna yazılır
//! (çevrimdışıyken ve uygulama kapanıp açılınca da kaybolmaz, bağlanınca
//! uygulanır). Denetim üç katmandadır: burada (hiçbir şeye dokunmadan önce),
//! eşitleme yöneticilerinde ve işlem sunucuda uygulanırken.

use mh_core::{AccountId, AppError, AppResult, ErrorCode, MailboxRole, MessageId};
use tauri::{AppHandle, State};

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

/// Kalıcı silme yalnız Çöp ve İstenmeyen kutularında yapılır.
fn is_purgeable(role: MailboxRole) -> bool {
    matches!(role, MailboxRole::Trash | MailboxRole::Junk)
}

/// Kalıcı silinen postaların açılmış eklerini önbellekten kaldırır (en iyi
/// çaba; kalan olursa bir gün içinde zaten silinir).
fn forget_attachments(state: &AppState, ids: &[MessageId]) {
    let root = state.cache_dir.join("ekler");
    for id in ids {
        let dir = root.join(id.to_string());
        if dir.is_dir() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// `messages_delete`in kendisi (pencere bilmez): içeriği değişen kutuları
/// döner. Kimliklerden biri Çöp ya da İstenmeyen dışındaysa hiçbirine
/// dokunulmaz (`Invalid`); yerelde artık olmayan kimlikler sessizce atlanır.
pub fn delete_forever(state: &AppState, ids: &[MessageId]) -> AppResult<Vec<String>> {
    let locations = state.store.locations(ids)?;
    if locations.iter().any(|l| !is_purgeable(l.role)) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Yalnız Çöp ya da İstenmeyen kutusundaki postalar kalıcı silinebilir. Postayı önce Çöp'e taşı.",
        ));
    }
    let known: Vec<MessageId> = locations.iter().map(|l| l.id).collect();
    let (other, jmap) = crate::cmd_jmap::split(state, &known)?;
    let mut touched = state.sync.purge_messages(&other)?;
    touched.extend(state.jmap.purge_messages(&jmap)?);
    touched.sort();
    touched.dedup();
    forget_attachments(state, &known);
    tracing::info!(silinen = known.len(), "postalar kalıcı silindi");
    Ok(touched)
}

/// `mailbox_empty`nin kendisi (pencere bilmez): hesabın (`None`: tüm
/// hesapların) `role` kutusundaki bütün postaları kalıcı siler; (yerelden
/// silinen posta sayısı, içeriği değişen kutular) döner. Sunucuda kutunun
/// yerelde hiç indirilmemiş eski postaları da silinir; kullanıcının son
/// gördüğü durumdan sonra gelenlere dokunulmaz.
pub fn empty_role(
    state: &AppState,
    account_id: Option<&str>,
    role: MailboxRole,
) -> AppResult<(u32, Vec<String>)> {
    if !is_purgeable(role) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Yalnız Çöp ve İstenmeyen kutuları boşaltılabilir. Başka bir klasördeki postaları önce Çöp'e taşı.",
        ));
    }
    let accounts = match account_id {
        Some(id) => vec![state.store.account(id)?.ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "Hesap bulunamadı; kaldırılmış olabilir. Listeyi yenileyip yeniden dene.",
            )
        })?],
        None => state.store.accounts()?,
    };
    let mut total = 0u32;
    let mut touched: Vec<String> = Vec::new();
    for account in accounts {
        let jmap = mh_jmap::is_syncable(&account);
        for info in state.store.mailboxes_for(&account.id)? {
            if info.mailbox.role != role {
                continue;
            }
            let ids = state.store.message_ids_in(&info.mailbox.id)?;
            let (n, t) = if jmap {
                state.jmap.empty_mailbox(&info.mailbox.id)?
            } else {
                state.sync.empty_mailbox(&info.mailbox.id)?
            };
            forget_attachments(state, &ids);
            total = total.saturating_add(n);
            touched.extend(t);
        }
    }
    touched.sort();
    touched.dedup();
    tracing::info!(silinen = total, "kutu boşaltıldı");
    Ok((total, touched))
}

/// Postaları **kalıcı** siler (sunucuda da); geri alınamaz. Yalnız Çöp ya da
/// İstenmeyen kutusundaki postalar için: başka kutudan bir kimlik varsa
/// hiçbirine dokunulmaz, `invalid` döner. `mh:changed` yayar.
#[tauri::command]
pub fn messages_delete(app: AppHandle, state: State<'_, AppState>, ids: Vec<MessageId>) -> Res<()> {
    let touched = delete_forever(&state, &ids)?;
    crate::sink::changed(&app, touched);
    Ok(())
}

/// Hesabın (`None`: tüm hesapların) Çöp ya da İstenmeyen kutusundaki bütün
/// postaları kalıcı siler; silinen posta sayısını döner. Geri alınamaz.
/// `mh:changed` yayar.
#[tauri::command]
pub fn mailbox_empty(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: Option<AccountId>,
    role: MailboxRole,
) -> Res<u32> {
    let (n, touched) = empty_role(&state, account_id.as_deref(), role)?;
    if !touched.is_empty() {
        crate::sink::changed(&app, touched);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use mh_core::{Account, AccountKind, MoveTarget, Security, ServerConfig};
    use mh_store::{Action, NewMessage};

    use super::*;
    use crate::testutil::{deliver, deliver_to, raw_mail, state};

    fn mail(mid: &str) -> Vec<u8> {
        raw_mail("Ayşe <ayse@eposta.com>", "Selam", mid, "", "Merhaba")
    }

    fn count(st: &AppState, mailbox: &str) -> usize {
        st.store.message_ids_in(mailbox).unwrap().len()
    }

    /// Gerçek sunuculu IMAP hesabı `i` ve JMAP hesabı `j` (görevleri
    /// başlatılmaz: işlemler kalıcı kuyrukta bekler).
    fn with_real_accounts(st: &AppState) {
        let mut i = crate::testutil::account("i", "i@ornek.test");
        i.imap = Some(ServerConfig {
            host: "imap.ornek.test".into(),
            port: 993,
            security: Security::Tls,
            username: "i".into(),
        });
        let mut j: Account = crate::testutil::account("j", "j@ornek.test");
        j.kind = AccountKind::Jmap;
        j.imap = None;
        j.smtp = None;
        j.jmap_url = Some("https://jmap.ornek.test/.well-known/jmap".into());
        for (a, boxes) in [
            (
                &i,
                vec![("INBOX", MailboxRole::Inbox), ("Trash", MailboxRole::Trash)],
            ),
            (
                &j,
                vec![("mb-in", MailboxRole::Inbox), ("mb-tr", MailboxRole::Trash)],
            ),
        ] {
            st.store.insert_account(a).unwrap();
            for (path, role) in boxes {
                st.store
                    .upsert_mailbox(&a.id, path, path, role, None, false)
                    .unwrap();
            }
        }
    }

    /// IMAP hesabına, sunucudaki yeri (UID) bilinen posta.
    fn imap_mail(st: &AppState, mailbox: &str, uid: u32) -> MessageId {
        let mut m =
            NewMessage::from_raw(mail(&format!("u{uid}@x")), false, "i", mailbox, 10).unwrap();
        m.uid = Some(uid);
        m.uidvalidity = Some(7);
        st.store.upsert_message(&m).unwrap().id
    }

    /// JMAP hesabına, sunucu kimliği bilinen posta.
    fn jmap_mail(st: &AppState, mailbox: &str, jmap_id: &str) -> MessageId {
        let m =
            NewMessage::from_raw(mail(&format!("{jmap_id}@x")), false, "j", mailbox, 10).unwrap();
        st.store.jmap_upsert(&m, jmap_id, None, None).unwrap().id
    }

    #[tokio::test]
    async fn kalici_silme_yalniz_cop_ve_istenmeyende() {
        let st = state();
        let inbox = deliver(&st, "a", mail("g1@x"), 10).id;
        let trash = deliver_to(&st, "a", "Trash", mail("c1@x"), 11).id;
        let junk = deliver_to(&st, "a", "Junk", mail("i1@x"), 12).id;
        let archive = deliver_to(&st, "a", "Archive", mail("a1@x"), 13).id;
        let other_trash = deliver_to(&st, "b", "Trash", mail("c2@x"), 14).id;

        // Başka kutudan tek bir kimlik bile varsa hiçbirine dokunulmaz.
        for ids in [
            vec![inbox],
            vec![trash, inbox],
            vec![junk, archive, trash],
            vec![other_trash, inbox],
        ] {
            let err = delete_forever(&st, &ids).unwrap_err();
            assert_eq!(err.code, ErrorCode::Invalid, "{ids:?}");
            assert!(err.message.contains("Çöp'e taşı"));
        }
        for id in [inbox, trash, junk, archive, other_trash] {
            assert!(st.store.summary(id).unwrap().is_some(), "{id}");
        }

        // Çöp ve İstenmeyen (birden çok hesap): kalıcı silinir.
        let touched = delete_forever(&st, &[trash, junk, other_trash]).unwrap();
        assert_eq!(touched, ["a:Junk", "a:Trash", "b:Trash"]);
        for id in [trash, junk, other_trash] {
            assert!(st.store.summary(id).unwrap().is_none(), "{id}");
        }
        assert!(st.store.summary(inbox).unwrap().is_some());
        assert!(st.store.summary(archive).unwrap().is_some());
        // Arama dizininden de kalkar.
        assert!(st
            .store
            .search("selam in:çöp", 10, None)
            .unwrap()
            .is_empty());

        // Yerelde olmayan kimlik ve boş liste sessizce geçer.
        assert!(delete_forever(&st, &[9_999_999]).unwrap().is_empty());
        assert!(delete_forever(&st, &[]).unwrap().is_empty());
        // Çöp'e taşınan posta artık kalıcı silinebilir.
        crate::cmd_jmap::move_messages(&st, &[inbox], MoveTarget::Trash).unwrap();
        delete_forever(&st, &[inbox]).unwrap();
        assert!(st.store.summary(inbox).unwrap().is_none());
    }

    #[tokio::test]
    async fn kalici_silme_sunucu_icin_kuyruga_yazilir() {
        let st = state();
        with_real_accounts(&st);
        let t1 = imap_mail(&st, "i:Trash", 4);
        let t2 = imap_mail(&st, "i:Trash", 5);
        let keep = imap_mail(&st, "i:INBOX", 9);
        let j1 = jmap_mail(&st, "j:mb-tr", "M1");
        let jkeep = jmap_mail(&st, "j:mb-in", "M2");
        // Taşıması sunucuya henüz gitmemiş posta: sunucuda hâlâ gelen kutusunda.
        let pending = imap_mail(&st, "i:INBOX", 10);
        st.sync
            .move_messages(&[pending], MoveTarget::Trash)
            .unwrap();

        // Karışık istek (bir posta gelen kutusunda): kuyruğa hiçbir şey yazılmaz.
        assert_eq!(
            delete_forever(&st, &[t1, j1, jkeep]).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert!(st
            .store
            .actions("i")
            .unwrap()
            .iter()
            .all(|a| matches!(a.action, Action::Moves)));
        assert!(st.store.actions("j").unwrap().is_empty());

        let touched = delete_forever(&st, &[t1, t2, pending, j1]).unwrap();
        assert_eq!(touched, ["i:Trash", "j:mb-tr"]);
        for id in [t1, t2, pending, j1] {
            assert!(st.store.summary(id).unwrap().is_none());
        }
        assert!(st.store.summary(keep).unwrap().is_some());
        assert!(st.store.summary(jkeep).unwrap().is_some());

        // IMAP: sunucudaki yerleriyle; bekleyen taşımada kaynak kutu + yedek hedef.
        let deletes: Vec<_> = st
            .store
            .actions("i")
            .unwrap()
            .into_iter()
            .filter_map(|a| match a.action {
                Action::Delete { items } => Some(items),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(deletes.len(), 3);
        let at = |uid: u32| deletes.iter().find(|r| r.uid == Some(uid)).unwrap();
        assert_eq!(
            (at(4).path.as_str(), at(4).fallback_path.as_deref()),
            ("Trash", None)
        );
        assert_eq!(at(5).uidvalidity, Some(7));
        assert_eq!(
            (at(10).path.as_str(), at(10).fallback_path.as_deref()),
            ("INBOX", Some("Trash"))
        );
        // JMAP: sunucu kimliğiyle.
        let purges: Vec<_> = st
            .store
            .actions("j")
            .unwrap()
            .into_iter()
            .map(|a| a.action)
            .collect();
        assert_eq!(
            purges,
            [Action::Purge {
                ids: vec!["M1".into()]
            }]
        );
    }

    #[tokio::test]
    async fn kutu_bosaltma() {
        let st = state();
        for i in 0..3 {
            deliver_to(&st, "a", "Trash", mail(&format!("c{i}@x")), 10 + i);
        }
        deliver_to(&st, "a", "Junk", mail("i1@x"), 20);
        deliver_to(&st, "b", "Trash", mail("c9@x"), 21);
        let inbox = deliver(&st, "a", mail("g1@x"), 22).id;

        // Yalnız Çöp ve İstenmeyen boşaltılabilir.
        for role in [
            MailboxRole::Inbox,
            MailboxRole::Archive,
            MailboxRole::Sent,
            MailboxRole::Drafts,
            MailboxRole::Other,
        ] {
            let err = empty_role(&st, None, role).unwrap_err();
            assert_eq!(err.code, ErrorCode::Invalid, "{role:?}");
        }
        assert_eq!(
            empty_role(&st, Some("yok"), MailboxRole::Trash)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(count(&st, "a:Trash"), 3);

        // Tek hesap: yalnız o hesabın çöpü.
        let (n, touched) = empty_role(&st, Some("a"), MailboxRole::Trash).unwrap();
        assert_eq!((n, touched), (3, vec!["a:Trash".to_string()]));
        assert_eq!(count(&st, "a:Trash"), 0);
        assert_eq!(count(&st, "b:Trash"), 1);
        assert_eq!(count(&st, "a:Junk"), 1);
        // Zaten boş: sıfır, değişen kutu yok.
        assert_eq!(
            empty_role(&st, Some("a"), MailboxRole::Trash).unwrap(),
            (0, Vec::new())
        );
        // Tüm hesaplar; İstenmeyen kutusu olmayan hesap (b) atlanır.
        let (n, touched) = empty_role(&st, None, MailboxRole::Junk).unwrap();
        assert_eq!((n, touched), (1, vec!["a:Junk".to_string()]));
        let (n, _) = empty_role(&st, None, MailboxRole::Trash).unwrap();
        assert_eq!(n, 1);
        assert_eq!(count(&st, "b:Trash"), 0);
        assert!(st.store.summary(inbox).unwrap().is_some());
        assert_eq!(empty_role(&st, None, MailboxRole::Junk).unwrap().0, 0);
    }

    #[tokio::test]
    async fn kutu_bosaltma_sunucu_icin_kuyruga_yazilir() {
        let st = state();
        with_real_accounts(&st);
        imap_mail(&st, "i:Trash", 4);
        imap_mail(&st, "i:Trash", 5);
        st.store
            .set_mailbox_sync_state("i:Trash", Some(7), Some(41), 40)
            .unwrap();
        jmap_mail(&st, "j:mb-tr", "M1");
        st.store.set_jmap_email_state("j", Some("e9")).unwrap();

        let (n, touched) = empty_role(&st, None, MailboxRole::Trash).unwrap();
        assert_eq!(n, 3);
        assert_eq!(touched, ["i:Trash", "j:mb-tr"]);
        // IMAP: bilinen postalar + filigrana kadar tüm kutu (inmemiş eskiler dahil).
        let imap: Vec<_> = st
            .store
            .actions("i")
            .unwrap()
            .into_iter()
            .map(|a| a.action)
            .collect();
        assert_eq!(imap.len(), 2);
        assert!(matches!(&imap[0], Action::Delete { items } if items.len() == 2));
        assert_eq!(
            imap[1],
            Action::Empty {
                path: "Trash".into(),
                upto: 40,
                uidvalidity: Some(7),
                since: None,
            }
        );
        // JMAP: bilinen postalar + son eşitlenen duruma göre kutu taraması.
        let jmap: Vec<_> = st
            .store
            .actions("j")
            .unwrap()
            .into_iter()
            .map(|a| a.action)
            .collect();
        assert_eq!(
            jmap,
            [
                Action::Purge {
                    ids: vec!["M1".into()]
                },
                Action::Empty {
                    path: "mb-tr".into(),
                    upto: 0,
                    uidvalidity: None,
                    since: Some("e9".into()),
                }
            ]
        );
    }

    #[tokio::test]
    async fn kalici_silinen_postanin_acilmis_ekleri_onbellekten_kalkar() {
        let mut st = state();
        let root = std::env::temp_dir().join(format!("mh-purge-{}", std::process::id()));
        st.cache_dir = root.clone();
        let trash = deliver_to(&st, "a", "Trash", mail("c1@x"), 10).id;
        let keep = deliver(&st, "a", mail("g1@x"), 11).id;
        for id in [trash, keep] {
            let dir = root.join("ekler").join(id.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("fatura.pdf"), b"%PDF").unwrap();
        }
        delete_forever(&st, &[trash]).unwrap();
        assert!(!root.join("ekler").join(trash.to_string()).exists());
        assert!(root.join("ekler").join(keep.to_string()).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
