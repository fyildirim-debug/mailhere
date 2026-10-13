//! Rol kutusu dışında, adı bilinen bir klasöre taşıma (kurallar için).
//!
//! Rol kutusuna taşımayla (`move_messages`) aynı yoldan gider: posta yerelde
//! hemen taşınır, sunucudaki yeri onaya kadar saklanır ve taşıma kalıcı
//! işlem kuyruğuna (`Action::Moves`) yazılır.

use mh_core::{AppError, AppResult, ErrorCode, MessageId};
use mh_store::Action;

use super::SyncManager;

impl SyncManager {
    /// Postaları `mailbox_id` klasörüne taşır. Klasör başka bir hesaptaysa o
    /// hesabın olmayan postalarına dokunulmaz. Etkilenen kutuları döner.
    pub fn move_to_mailbox(&self, ids: &[MessageId], mailbox_id: &str) -> AppResult<Vec<String>> {
        let dest = self.store.mailbox(mailbox_id)?.ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "Taşınacak klasör bulunamadı; silinmiş olabilir.",
            )
        })?;
        let mut touched = Vec::new();
        for (account, syncable, list) in self.by_account(ids)? {
            if account != dest.mailbox.account_id {
                continue;
            }
            let ids: Vec<MessageId> = list.iter().map(|l| l.id).collect();
            if syncable {
                touched.extend(self.store.move_local(&ids, &dest.mailbox.id)?);
                self.queue(&account, &Action::Moves, None)?;
            } else {
                touched.extend(self.store.move_local_untracked(&ids, &dest.mailbox.id)?);
            }
        }
        touched.sort();
        touched.dedup();
        Ok(touched)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mh_core::{Account, Address, Category, MailboxRole, MessageSummary, SyncStatus};
    use mh_store::{NewMessage, Store};

    use crate::{EventSink, MemorySecrets, SyncManager};

    struct Quiet;

    impl EventSink for Quiet {
        fn sync_status(&self, _: SyncStatus) {}
        fn changed(&self, _: Vec<String>) {}
        fn new_mail(&self, _: &Account, _: Vec<MessageSummary>) {}
    }

    fn message(account: &str, mailbox: &str, uid: u32) -> NewMessage {
        NewMessage {
            account_id: account.into(),
            mailbox_id: mailbox.into(),
            uid: Some(uid),
            uidvalidity: Some(1),
            message_id_header: Some(format!("{uid}@ornek.test")),
            in_reply_to: None,
            references: vec![],
            from: Address::new(None, "ayse@ornek.test"),
            to: vec![],
            cc: vec![],
            reply_to: vec![],
            subject: format!("Posta {uid}"),
            snippet: String::new(),
            body_text: None,
            date: i64::from(uid),
            unread: true,
            starred: false,
            has_attachments: false,
            category: Category::People,
            size: 10,
            raw: None,
        }
    }

    fn account(id: &str, host: &str) -> Account {
        Account {
            id: id.into(),
            name: id.into(),
            email: format!("{id}@ornek.test"),
            display_name: id.into(),
            color: "#4C6FD8".into(),
            kind: mh_core::AccountKind::Imap,
            imap: Some(mh_core::ServerConfig {
                host: host.into(),
                port: 993,
                security: mh_core::Security::Tls,
                username: id.into(),
            }),
            smtp: None,
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
        }
    }

    #[tokio::test]
    async fn klasore_tasima_kuyruga_yazilir() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        // Gerçek sunuculu hesap (görevler başlatılmaz) ve yerel tanıtım hesabı.
        store
            .insert_account(&account("a", "imap.ornek.test"))
            .unwrap();
        store
            .insert_account(&account("d", "imap.ornek.invalid"))
            .unwrap();
        for acc in ["a", "d"] {
            store
                .upsert_mailbox(acc, "INBOX", "INBOX", MailboxRole::Inbox, None, false)
                .unwrap();
            store
                .upsert_mailbox(
                    acc,
                    "Faturalar",
                    "Faturalar",
                    MailboxRole::Other,
                    None,
                    false,
                )
                .unwrap();
        }
        let x = store
            .upsert_message(&message("a", "a:INBOX", 1))
            .unwrap()
            .id;
        let y = store
            .upsert_message(&message("d", "d:INBOX", 2))
            .unwrap()
            .id;
        let mgr = SyncManager::new(
            store.clone(),
            Arc::new(MemorySecrets::new()),
            Arc::new(Quiet),
            tokio::runtime::Handle::current(),
        );

        // Yalnız klasörün hesabındaki posta taşınır.
        let touched = mgr.move_to_mailbox(&[x, y], "a:Faturalar").unwrap();
        assert_eq!(touched, vec!["a:Faturalar".to_string(), "a:INBOX".into()]);
        let loc = store.location(x).unwrap().unwrap();
        assert_eq!(loc.mailbox_id, "a:Faturalar");
        assert!(loc.pending, "sunucu onayı bekler");
        assert_eq!(loc.server_path, "INBOX");
        assert_eq!(store.location(y).unwrap().unwrap().mailbox_id, "d:INBOX");
        let actions = store.actions("a").unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0].action, mh_store::Action::Moves));

        // Tanıtım hesabı: yalnız yerel, kuyruk yok.
        mgr.move_to_mailbox(&[y], "d:Faturalar").unwrap();
        let loc = store.location(y).unwrap().unwrap();
        assert_eq!(
            (loc.mailbox_id.as_str(), loc.pending),
            ("d:Faturalar", false)
        );
        assert!(store.actions("d").unwrap().is_empty());

        assert_eq!(
            mgr.move_to_mailbox(&[x], "a:Yok").unwrap_err().code,
            mh_core::ErrorCode::NotFound
        );
    }
}
