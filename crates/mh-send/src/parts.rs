//! Giden postanın depodan gelen parçaları: yanıt başlıkları ve iletilen ekler.

use mh_core::{AppError, AppResult, ErrorCode, MailboxRole, MessageId, OutgoingMessage};
use mh_mime::AttachmentData;
use mh_store::Store;

use crate::message::ReplyRef;

/// Yanıt başlıkları: `in_reply_to` verilmişse o posta; verilmemiş ama bir
/// taslaktan gönderiliyorsa taslağın yanıtladığı posta.
pub(crate) fn reply_ref(store: &Store, msg: &OutgoingMessage) -> AppResult<Option<ReplyRef>> {
    let source = match (msg.in_reply_to, msg.draft_id) {
        (Some(id), _) => Some(id),
        (None, Some(d)) => store
            .location(d)?
            .filter(|l| l.role == MailboxRole::Drafts)
            .map(|l| l.id),
        (None, None) => None,
    };
    let Some(id) = source else {
        return Ok(None);
    };
    Ok(store
        .reply_refs(id)?
        .map(|(message_id, references)| ReplyRef {
            message_id,
            references,
        }))
}

/// İletilen postanın görünen ekleri; içerik yerelde yoksa `None`.
pub(crate) fn forwarded_local(
    store: &Store,
    id: MessageId,
) -> AppResult<Option<Vec<AttachmentData>>> {
    if store.location(id)?.is_none() {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "İletilen posta bulunamadı; silinmiş ya da taşınmış olabilir. Ekleri yeniden ekle.",
        ));
    }
    Ok(store.raw(id)?.map(|raw| mh_mime::visible_attachments(&raw)))
}

/// Yerelde olması gereken (komut öncesinde indirilen) iletilen ekler.
pub(crate) fn forwarded_required(store: &Store, id: MessageId) -> AppResult<Vec<AttachmentData>> {
    forwarded_local(store, id)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::Network,
            "İletilen postanın ekleri henüz indirilmedi. Bağlantını denetleyip yeniden dene.",
        )
    })
}

/// Eklerin toplam boyutu.
pub(crate) fn total_size(list: &[AttachmentData]) -> u64 {
    list.iter().map(|a| a.bytes.len() as u64).sum()
}
