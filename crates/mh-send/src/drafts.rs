//! Taslak kaydı ve silme. Taslak, gönderimle aynı kurucudan geçer (taslak
//! kipinde: alıcı zorunlu değil, gizli alıcılar saklanır).

use std::sync::Arc;

use mh_core::{
    Account, AccountKind, AppError, AppResult, ErrorCode, MessageId, Millis, OutgoingMessage,
};
use mh_jmap::JmapManager;

use crate::message::{build_with, validate_with, BuildOptions};
use crate::outbox::Outbox;
use crate::parts::{forwarded_required, reply_ref, total_size};

impl Outbox {
    /// Taslağı kurar, yerelde hemen kaydeder ve sunucuya eklenmesini sıraya
    /// alır. `msg.draft_id` bir taslaksa onun yerine (aynı kimlikle) yazılır.
    /// İletilen postanın ekleri (`forward_from`) yerelde olmalıdır.
    pub fn save_draft(&self, msg: &OutgoingMessage, now: Millis) -> AppResult<MessageId> {
        let account = self
            .store
            .account(&msg.account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Hesap bulunamadı."))?;
        let extra = match msg.forward_from {
            Some(id) => forwarded_required(&self.store, id)?,
            None => Vec::new(),
        };
        validate_with(msg, total_size(&extra), true)?;
        let reply = reply_ref(&self.store, msg)?;
        let opts = BuildOptions {
            reply: reply.as_ref(),
            extra: &extra,
            draft: true,
        };
        let raw = build_with(&account, msg, &opts, now)?.formatted();
        // Gönderen hesap başka türdeyse (IMAP ↔ JMAP) eski taslağı kendi
        // eşitleyicisi siler; yenisi baştan yazılır. Ekler yukarıda okundu.
        let mut replace = msg.draft_id;
        if let Some(old) = replace {
            if self.kind_of(old)?.is_some_and(|k| k != account.kind) {
                self.delete_draft(old)?;
                replace = None;
            }
        }
        let (id, touched) = match self.jmap_for(&account)? {
            Some(jmap) => jmap.save_draft(&account.id, raw, replace, now)?,
            None => self.sync.save_draft(&account.id, raw, replace, now)?,
        };
        self.sink.changed(touched);
        Ok(id)
    }

    /// Taslağı yerelden ve sunucudan siler; zaten yoksa sessizce geçer.
    pub fn delete_draft(&self, id: MessageId) -> AppResult<()> {
        let jmap = match self.store.location(id)? {
            Some(loc) => match self.store.account(&loc.account_id)? {
                Some(account) => self.jmap_for(&account)?,
                None => None,
            },
            None => return Ok(()),
        };
        let touched = match jmap {
            Some(jmap) => jmap.delete_draft(id)?,
            None => self.sync.delete_draft(id)?,
        };
        if !touched.is_empty() {
            self.sink.changed(touched);
        }
        Ok(())
    }

    /// JMAP hesabıysa taslakları yöneten JMAP eşitleyicisi; değilse `None` (IMAP).
    fn jmap_for(&self, account: &Account) -> AppResult<Option<&Arc<JmapManager>>> {
        if account.kind != AccountKind::Jmap {
            return Ok(None);
        }
        self.jmap
            .get()
            .map(Some)
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "JMAP eşitleyicisi hazır değil."))
    }

    /// Postanın bulunduğu hesabın türü (posta ya da hesap yoksa `None`).
    fn kind_of(&self, id: MessageId) -> AppResult<Option<AccountKind>> {
        let Some(loc) = self.store.location(id)? else {
            return Ok(None);
        };
        Ok(self.store.account(&loc.account_id)?.map(|a| a.kind))
    }
}
