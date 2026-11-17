//! Daha eski postalar: `messages_older`.
//!
//! İlk eşitleme kutu başına en yeni 300 postayı indirir. Liste sonuna
//! gelindiğinde arayüz bu komutla görünümün arkasındaki sunucu kutularından
//! bir sonraki eski partiyi (kutu başına 300) ister. Postalar olağan alım
//! yolundan geçer (ayrıştırma, önizleme, kategori, kod ve istenmeyen puanı)
//! ama yeni posta sayılmaz: bildirim, kural ve elek çalışmaz.

use mh_core::{AccountId, AppError, AppResult, ErrorCode, MailboxRole, OlderResult, View};
use tauri::{AppHandle, State};

use crate::state::{internal, AppState};

type Res<T> = Result<T, AppError>;

/// Daha eskisi istenecek bir sunucu kutusu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub mailbox_id: String,
    /// Kutu gerçek bir JMAP hesabında mı (değilse IMAP)?
    pub jmap: bool,
}

/// Görünümün karşılık geldiği rol; sunucu kutusu olmayan görünümlerde `None`.
fn view_role(view: &View) -> Option<MailboxRole> {
    Some(match view {
        View::Inbox => MailboxRole::Inbox,
        View::Trash => MailboxRole::Trash,
        View::Sent => MailboxRole::Sent,
        View::Archive => MailboxRole::Archive,
        View::Drafts => MailboxRole::Drafts,
        _ => return None,
    })
}

/// Görünümün arkasındaki sunucu kutuları: gelen kutusu ve rol görünümlerinde
/// seçili hesabın (yoksa tüm hesapların) o roldeki kutuları, klasör
/// görünümünde o klasör. Sunucuyla eşitlenmeyen (tanıtım) hesaplar ve sanal
/// görünümler (yıldızlı, ertelenen, etiket, elek...) boş döner.
pub(crate) fn targets(
    state: &AppState,
    view: &View,
    account_id: Option<&str>,
) -> AppResult<Vec<Target>> {
    let store = &state.store;
    // Hesap: (kimlik, JMAP mi); eşitlenmeyen hesap yok sayılır.
    let kind = |account_id: &str| -> AppResult<Option<bool>> {
        Ok(store.account(account_id)?.and_then(|a| {
            if mh_jmap::is_syncable(&a) {
                Some(true)
            } else if mh_sync::is_syncable(&a) {
                Some(false)
            } else {
                None
            }
        }))
    };
    if let View::Mailbox { id } = view {
        let Some(info) = store.mailbox(id)? else {
            return Ok(Vec::new());
        };
        return Ok(kind(&info.mailbox.account_id)?
            .map(|jmap| Target {
                mailbox_id: info.mailbox.id,
                jmap,
            })
            .into_iter()
            .collect());
    }
    let Some(role) = view_role(view) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for account in store.accounts()? {
        if account_id.is_some_and(|id| id != account.id) {
            continue;
        }
        let Some(jmap) = kind(&account.id)? else {
            continue;
        };
        out.extend(
            store
                .mailboxes_for(&account.id)?
                .into_iter()
                .filter(|m| m.mailbox.role == role)
                .map(|m| Target {
                    mailbox_id: m.mailbox.id,
                    jmap,
                }),
        );
    }
    Ok(out)
}

/// Bağlantı hatası arayüze "ne oldu + ne yapılabilir" diye söylenir; öteki
/// hatalar (kimlik, sertifika...) kendi metniyle kalır.
fn explain(e: AppError) -> AppError {
    if e.code == ErrorCode::Network {
        AppError::new(
            ErrorCode::Network,
            "Daha eski postalar indirilemedi: posta sunucusuna ulaşılamıyor. İnternet bağlantını denetleyip yeniden dene.",
        )
    } else {
        e
    }
}

/// `messages_older`in kendisi (pencere bilmez): sonucu ve içeriği değişen
/// kutuları döner. Kutular (hesaplar) aynı anda sorulur. Kutulardan biri
/// indirilemezse öbürlerinin sonucu yine döner ve `more` doğru kalır (o
/// kutuda daha eskisi olabilir); hiçbiri indirilemezse hata döner.
pub async fn fetch_older(
    state: &AppState,
    view: &View,
    account_id: Option<&str>,
) -> AppResult<(OlderResult, Vec<String>)> {
    let mut tasks = Vec::new();
    for t in targets(state, view, account_id)? {
        let (sync, jmap) = (state.sync.clone(), state.jmap.clone());
        tasks.push(tokio::spawn(async move {
            if t.jmap {
                let o = jmap.fetch_older(&t.mailbox_id).await?;
                Ok::<_, AppError>((o.fetched, o.more, o.changed))
            } else {
                let o = sync.fetch_older(&t.mailbox_id).await?;
                let changed = if o.fetched > 0 {
                    vec![t.mailbox_id]
                } else {
                    Vec::new()
                };
                Ok((o.fetched, o.more, changed))
            }
        }));
    }
    let mut result = OlderResult::default();
    let mut touched: Vec<String> = Vec::new();
    let mut failed: Option<AppError> = None;
    let mut succeeded = 0usize;
    for task in tasks {
        match task.await.map_err(internal)? {
            Ok((fetched, more, changed)) => {
                succeeded += 1;
                result.fetched = result.fetched.saturating_add(fetched);
                result.more |= more;
                touched.extend(changed);
            }
            Err(e) => {
                tracing::warn!(code = %e.code, "daha eski postalar indirilemedi");
                failed.get_or_insert(e);
            }
        }
    }
    if let Some(e) = failed {
        if succeeded == 0 {
            return Err(explain(e));
        }
        result.more = true;
    }
    touched.sort();
    touched.dedup();
    tracing::info!(
        indirilen = result.fetched,
        daha_var = result.more,
        "daha eski postalar"
    );
    Ok((result, touched))
}

/// Görünümün sunucu kutularından yereldekinden daha eski bir sonraki partiyi
/// indirir; `mh:changed` yayar. Sunucu kutusu olmayan görünümlerde
/// `{ fetched: 0, more: false }`. Çevrimdışıyken `network`.
#[tauri::command]
pub async fn messages_older(
    app: AppHandle,
    state: State<'_, AppState>,
    view: View,
    account_id: Option<AccountId>,
) -> Res<OlderResult> {
    let (result, touched) = fetch_older(&state, &view, account_id.as_deref()).await?;
    if !touched.is_empty() {
        crate::sink::changed(&app, touched);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use mh_core::{Account, AccountKind, Security, ServerConfig};

    use super::*;
    use crate::testutil::{deliver, raw_mail, state};

    /// Gerçek (eşitlenen) sunuculu IMAP hesabı; görevleri başlatılmaz.
    fn imap_account(id: &str, host: &str, port: u16, security: Security) -> Account {
        let mut a = crate::testutil::account(id, &format!("{id}@ornek.test"));
        a.imap = Some(ServerConfig {
            host: host.into(),
            port,
            security,
            username: id.into(),
        });
        a
    }

    fn jmap_account(id: &str) -> Account {
        let mut a = crate::testutil::account(id, &format!("{id}@ornek.test"));
        a.kind = AccountKind::Jmap;
        a.imap = None;
        a.smtp = None;
        a.jmap_url = Some("https://jmap.ornek.test/.well-known/jmap".into());
        a
    }

    fn add(st: &AppState, account: &Account, boxes: &[(&str, MailboxRole)]) {
        st.store.insert_account(account).unwrap();
        for (path, role) in boxes {
            st.store
                .upsert_mailbox(&account.id, path, path, *role, None, false)
                .unwrap();
        }
    }

    fn ids(list: Vec<Target>) -> Vec<(String, bool)> {
        list.into_iter().map(|t| (t.mailbox_id, t.jmap)).collect()
    }

    #[tokio::test]
    async fn gorunumun_sunucu_kutulari() {
        let st = state();
        // Tanıtım hesapları (a, b) eşitlenmez: hedef olmaz.
        assert!(targets(&st, &View::Inbox, None).unwrap().is_empty());
        add(
            &st,
            &imap_account("i", "imap.ornek.test", 993, Security::Tls),
            &[
                ("INBOX", MailboxRole::Inbox),
                ("Trash", MailboxRole::Trash),
                ("Projeler", MailboxRole::Other),
            ],
        );
        add(
            &st,
            &jmap_account("j"),
            &[("mb-in", MailboxRole::Inbox), ("mb-se", MailboxRole::Sent)],
        );
        // Gelen kutusu: tüm hesapların ya da seçili hesabın gelen kutuları.
        assert_eq!(
            ids(targets(&st, &View::Inbox, None).unwrap()),
            [("i:INBOX".to_string(), false), ("j:mb-in".into(), true)]
        );
        assert_eq!(
            ids(targets(&st, &View::Inbox, Some("j")).unwrap()),
            [("j:mb-in".to_string(), true)]
        );
        assert!(targets(&st, &View::Inbox, Some("a")).unwrap().is_empty());
        assert!(targets(&st, &View::Inbox, Some("yok")).unwrap().is_empty());
        // Rol görünümleri: o roldeki kutular.
        assert_eq!(
            ids(targets(&st, &View::Trash, None).unwrap()),
            [("i:Trash".to_string(), false)]
        );
        assert_eq!(
            ids(targets(&st, &View::Sent, None).unwrap()),
            [("j:mb-se".to_string(), true)]
        );
        assert!(targets(&st, &View::Archive, None).unwrap().is_empty());
        assert!(targets(&st, &View::Drafts, Some("i")).unwrap().is_empty());
        // Klasör görünümü: o klasör (hesap kapsamından bağımsız).
        let folder = View::Mailbox {
            id: "i:Projeler".into(),
        };
        assert_eq!(
            ids(targets(&st, &folder, None).unwrap()),
            [("i:Projeler".to_string(), false)]
        );
        assert_eq!(
            ids(targets(&st, &folder, Some("j")).unwrap()),
            [("i:Projeler".to_string(), false)]
        );
        for view in [
            View::Mailbox { id: "i:Yok".into() },
            View::Mailbox {
                id: "a:INBOX".into(),
            },
        ] {
            assert!(targets(&st, &view, None).unwrap().is_empty(), "{view:?}");
        }
        // Sunucu kutusu olmayan görünümler.
        for view in [
            View::Starred,
            View::Snoozed,
            View::Followups,
            View::Screener,
            View::Tag { id: "e1".into() },
        ] {
            assert!(targets(&st, &view, None).unwrap().is_empty(), "{view:?}");
        }
    }

    #[tokio::test]
    async fn sunucusuz_gorunumde_ve_tanitim_hesabinda_bos_sonuc() {
        let st = state();
        deliver(
            &st,
            "a",
            raw_mail("Ayşe <ayse@eposta.com>", "Selam", "o1@x", "", "Merhaba"),
            10,
        );
        for view in [View::Inbox, View::Starred, View::Trash] {
            let (r, touched) = fetch_older(&st, &view, None).await.unwrap();
            assert_eq!(r, OlderResult::default(), "{view:?}");
            assert!(touched.is_empty());
        }
    }

    #[tokio::test]
    async fn cevrimdisiyken_ag_hatasi_ne_oldu_ne_yapilabilir() {
        let st = state();
        // Dinleyen kimse olmayan yerel kapı: bağlantı kurulamaz.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        add(
            &st,
            &imap_account("i", "127.0.0.1", port, Security::None),
            &[("INBOX", MailboxRole::Inbox), ("Trash", MailboxRole::Trash)],
        );
        st.secrets.set("i", "gizli-parola").unwrap();
        st.store
            .set_mailbox_sync_state("i:INBOX", Some(7), Some(11), 10)
            .unwrap();
        let err = fetch_older(&st, &View::Inbox, None).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Network);
        assert!(err.message.contains("Daha eski postalar indirilemedi"));
        assert!(err.message.contains("yeniden dene"));
        assert!(!err.message.contains("gizli-parola"));
        // Başına ulaşılmış kutu için sunucuya gidilmez: çevrimdışıyken de hata yok.
        st.store.set_mailbox_older_done("i:INBOX", true).unwrap();
        let (r, touched) = fetch_older(&st, &View::Inbox, None).await.unwrap();
        assert_eq!(r, OlderResult::default());
        assert!(touched.is_empty());
    }

    #[test]
    fn baglanti_disindaki_hata_kendi_metniyle_kalir() {
        let auth = AppError::new(ErrorCode::Auth, "Parola kabul edilmedi.");
        assert_eq!(explain(auth.clone()), auth);
        let net = explain(AppError::new(ErrorCode::Network, "x:993 bağlanılamadı"));
        assert_eq!(net.code, ErrorCode::Network);
        assert!(!net.message.contains("x:993"));
    }
}
