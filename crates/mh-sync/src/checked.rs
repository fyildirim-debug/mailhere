//! Etiketli yanıtın durumunu denetleyen IMAP komutları.
//!
//! `async-imap`in `uid_search`, `uid_store` ve `expunge` yöntemleri yalnız
//! etiketsiz veriyi toplar; komutu bitiren etiketli yanıtın durumuna (OK / NO
//! / BAD) bakmaz. Sunucu aramayı reddederse boş küme döner ve bu "kutuda posta
//! yok" diye okunur: eşitleme yereldeki postaları siler, kalıcı silme
//! başkasının işaretlediği postayı koruyamaz, "daha eskisi yok" sanılır.
//! Buradaki yardımcılar reddi hataya çevirir; sonucuna göre posta silinen ya
//! da kalıcı karar verilen her yerde bunlar kullanılır.

use std::collections::HashSet;

use async_imap::error::Error;
use async_imap::imap_proto::{MailboxDatum, Response, Status};

use crate::conn::ImapSession;
use crate::error::{SyncError, SyncResult};

/// `UID SEARCH <query>`: eşleşen UID'ler. Sunucu NO ya da BAD derse hata.
pub(crate) async fn uid_search(session: &mut ImapSession, query: &str) -> SyncResult<HashSet<u32>> {
    let tag = session.run_command(format!("UID SEARCH {query}")).await?;
    let mut uids: HashSet<u32> = HashSet::new();
    loop {
        let Some(resp) = session.read_response().await? else {
            return Err(SyncError::Imap(Error::ConnectionLost));
        };
        match resp.parsed() {
            Response::MailboxData(MailboxDatum::Search(found)) => {
                uids.extend(found.iter().copied())
            }
            Response::Done {
                tag: done,
                status,
                outcome,
            } if *done == tag => {
                let text = || {
                    outcome
                        .information
                        .as_deref()
                        .unwrap_or_default()
                        .to_owned()
                };
                return match status {
                    Status::Ok => Ok(uids),
                    Status::No => Err(SyncError::Imap(Error::No(text()))),
                    _ => Err(SyncError::Imap(Error::Bad(text()))),
                };
            }
            // Araya giren etiketsiz bildirimler (EXISTS, EXPUNGE...) atlanır.
            _ => {}
        }
    }
}

/// Komutu çalıştırır ve sunucunun OK dediğini denetler (etiketsiz yanıtlar
/// atlanır). Sonucu veri değil yalnız onay olan komutlar içindir
/// (`UID STORE ... FLAGS.SILENT`, `EXPUNGE`, `UID EXPUNGE`).
pub(crate) async fn command_ok(session: &mut ImapSession, command: &str) -> SyncResult<()> {
    Ok(session.run_command_and_check_ok(command).await?)
}
