//! Taslaklar: yerel kayıt hemen yapılır, sunucu işi kalıcı işlem kuyruğuna
//! (`Action::Draft`) girer.
//!
//! Her kayıt yeni bir Message-ID ile Taslaklar kutusuna `\Draft \Seen`
//! bayraklarıyla eklenir; önceki sürüm UID'si biliniyorsa UID ile, henüz
//! bilinmiyorsa (ekleme kuyrukta bekliyordu) Message-ID aramasıyla bulunup
//! silinir. Yerel satır, eşitlemede aynı Message-ID ile yeni UID'yi sahiplenir.

use futures::TryStreamExt;
use mh_core::{AppError, AppResult, ErrorCode, MailboxRole, MessageId, Millis};
use mh_store::{Action, MessageLocation, NewMessage, ServerRef};

use super::{ensure_mailbox, store_flags, Conn, SyncManager};
use crate::error::SyncResult;
use crate::mailbox::uid_set;

/// Sunucudan kaldırılacak eski taslak kopyası (sunucudaki yeri: taşıması
/// bekleyen postada kaynak kutu).
fn old_draft(l: &MessageLocation) -> ServerRef {
    ServerRef {
        path: l.server_path.clone(),
        uid: l.server_uid,
        uidvalidity: l.server_uidvalidity,
        message_id: l.message_id_header.clone(),
    }
}

/// Message-ID için IMAP arama ölçütü. Yalnız ASCII ve satır sonu içermeyen
/// kimlikler aranır.
pub(crate) fn header_search(message_id: &str) -> Option<String> {
    let id = message_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    if id.is_empty() || !id.is_ascii() || id.chars().any(|c| c.is_ascii_control()) {
        return None;
    }
    let quoted = id.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!("HEADER Message-ID \"{quoted}\""))
}

impl SyncManager {
    /// Taslak işini kuyruğa yazar (yalnız sunucuyla eşitlenen hesapta).
    fn queue_draft(
        &self,
        account_id: &str,
        path: String,
        raw: Option<&[u8]>,
        remove: Vec<ServerRef>,
    ) -> AppResult<()> {
        if !self.syncable(account_id)? || (raw.is_none() && remove.is_empty()) {
            return Ok(());
        }
        self.queue(account_id, &Action::Draft { path, remove }, raw)
    }

    /// Taslağı yerelde hemen kaydeder (varsa `replace` taslağının yerine,
    /// aynı kimlikle) ve sunucuya eklenmesini sıraya alır. Taslağın kimliğini
    /// ve değişen kutuları döner.
    pub fn save_draft(
        &self,
        account_id: &str,
        raw: Vec<u8>,
        replace: Option<MessageId>,
        now: Millis,
    ) -> AppResult<(MessageId, Vec<String>)> {
        let dest = self.destination(account_id, MailboxRole::Drafts)?;
        let mut m = NewMessage::from_raw(raw.clone(), false, account_id, &dest.mailbox.id, now)
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "Taslak oluşturulamadı."))?;
        m.unread = false;
        let put = self.store.put_draft(&m, replace)?;
        let mut touched = vec![dest.mailbox.id.clone()];
        let mut remove = Vec::new();
        if let Some(prev) = &put.previous {
            if prev.mailbox_id != dest.mailbox.id {
                touched.push(prev.mailbox_id.clone());
            }
            if prev.account_id == account_id {
                // Aynı sürüm yeniden kaydediliyorsa sunucudaki kopyası kalır.
                if prev.message_id_header.is_none() || prev.message_id_header != m.message_id_header
                {
                    remove.push(old_draft(prev));
                }
            } else {
                // Gönderen hesap değişti: eski hesabın taslağı da kalkar.
                self.queue_draft(
                    &prev.account_id,
                    prev.server_path.clone(),
                    None,
                    vec![old_draft(prev)],
                )?;
            }
        }
        self.queue_draft(account_id, dest.mailbox.path, Some(&raw), remove)?;
        Ok((put.id, touched))
    }

    /// Taslağı yerelden siler, sunucudan silinmesini sıraya alır. Taslak
    /// zaten yoksa hiçbir şey yapmaz. Değişen kutuları döner.
    pub fn delete_draft(&self, id: MessageId) -> AppResult<Vec<String>> {
        let Some(loc) = self.store.take_draft(id)? else {
            return Ok(Vec::new());
        };
        self.queue_draft(
            &loc.account_id,
            loc.server_path.clone(),
            None,
            vec![old_draft(&loc)],
        )?;
        Ok(vec![loc.mailbox_id])
    }
}

/// İşlemi sunucuda uygular; yeniden eşitlenecek kutu yollarını döner.
pub(super) async fn apply(
    conn: &mut Conn,
    dest: &str,
    raw: Option<&[u8]>,
    remove: &[ServerRef],
) -> SyncResult<Vec<String>> {
    let mut touched: Vec<String> = Vec::new();
    if let Some(raw) = raw {
        ensure_mailbox(conn, dest).await?;
        // Bağlantı eklemeden hemen sonra koptuysa işlem yeniden çalışır;
        // aynı sürüm sunucudaysa ikinci kez eklenmez.
        let query = mh_mime::parse(raw, true)
            .and_then(|p| p.message_id)
            .as_deref()
            .and_then(header_search);
        let present = match query {
            Some(q) => {
                conn.session.examine(dest).await?;
                !conn.session.uid_search(q).await?.is_empty()
            }
            None => false,
        };
        if !present {
            conn.session
                .append(dest, Some("(\\Seen \\Draft)"), None, raw)
                .await?;
        }
        touched.push(dest.to_owned());
    }
    let mut paths: Vec<&str> = remove.iter().map(|o| o.path.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    for path in paths {
        let mb = match conn.session.select(path).await {
            Ok(mb) => mb,
            // Kutu sunucuda yok: silinecek kopya da yok.
            Err(async_imap::error::Error::No(m)) => {
                tracing::debug!(path, reply = %m, "taslak kutusu seçilemedi");
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let mut uids = Vec::new();
        for old in remove.iter().filter(|o| o.path == path) {
            match old.uid {
                Some(uid) if old.uidvalidity.is_none() || old.uidvalidity == mb.uid_validity => {
                    uids.push(uid)
                }
                _ => {
                    if let Some(query) = old.message_id.as_deref().and_then(header_search) {
                        uids.extend(conn.session.uid_search(query).await?);
                    }
                }
            }
        }
        if uids.is_empty() {
            continue;
        }
        let set = uid_set(&uids);
        store_flags(conn, &set, "+FLAGS.SILENT (\\Deleted)").await?;
        if conn.has_uidplus {
            let _: Vec<u32> = conn.session.uid_expunge(&set).await?.try_collect().await?;
        } else {
            let _: Vec<u32> = conn.session.expunge().await?.try_collect().await?;
        }
        if !touched.iter().any(|t| t == path) {
            touched.push(path.to_owned());
        }
    }
    Ok(touched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_aramasi() {
        assert_eq!(
            header_search("<a1b2@ornek.com.tr>").as_deref(),
            Some("HEADER Message-ID \"a1b2@ornek.com.tr\"")
        );
        assert_eq!(
            header_search("x\"y\\z@o").as_deref(),
            Some("HEADER Message-ID \"x\\\"y\\\\z@o\"")
        );
        assert_eq!(header_search("<>"), None);
        assert_eq!(header_search("a\r\nb@o"), None);
        assert_eq!(header_search("şık@o"), None);
    }
}
