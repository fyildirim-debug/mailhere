//! Taslak kaydı ve silme. Taslak, gönderimle aynı kurucudan geçer (taslak
//! kipinde: alıcı zorunlu değil, gizli alıcılar saklanır).

use mh_core::{AppError, AppResult, ErrorCode, MessageId, Millis, OutgoingMessage};

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
        let (id, touched) = self.sync.save_draft(&account.id, raw, msg.draft_id, now)?;
        self.sink.changed(touched);
        Ok(id)
    }

    /// Taslağı yerelden ve sunucudan siler; zaten yoksa sessizce geçer.
    pub fn delete_draft(&self, id: MessageId) -> AppResult<()> {
        let touched = self.sync.delete_draft(id)?;
        if !touched.is_empty() {
            self.sink.changed(touched);
        }
        Ok(())
    }
}
