//! Planlanan gönderimler: `outbox_list`, `outbox_send_now`, `outbox_to_draft`.
//!
//! Giden kutusu değişince (ekle, gönderildi, iptal, taslağa çevrildi) giden
//! kutusu `mh:changed` yayar; arayüz sayaçları (`counts.scheduled`) tazeler.

use mh_core::{AppError, AppResult, MessageId, ScheduledItem};
use tauri::State;

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Giden kutusundaki gönderimler, gönderim anı artan.
#[tauri::command]
pub fn outbox_list(state: State<'_, AppState>) -> Res<Vec<ScheduledItem>> {
    state.outbox.scheduled()
}

/// Gönderim anını şimdiye çeker ve göndericiyi uyandırır.
#[tauri::command]
pub fn outbox_send_now(state: State<'_, AppState>, id: String) -> Res<()> {
    state.outbox.send_now(&id, now_ms())
}

/// `outbox_to_draft`ın kendisi (pencere bilmez).
pub async fn to_draft(state: &AppState, id: String) -> AppResult<MessageId> {
    // İletilen postanın ekleri taslağa kopyalanır: içerik yerelde yoksa önce
    // indirilir. İndirilemezse taslak hatayı söyler, gönderim yerinde kalır.
    let forwarded = state
        .store
        .outbox_message(&id)?
        .and_then(|m| m.forward_from);
    if let Some(source) = forwarded {
        if let Err(e) = crate::commands::ensure_body(state, source).await {
            tracing::debug!(error = %e.message, "iletilen postanın içeriği indirilemedi");
        }
    }
    let outbox = state.outbox.clone();
    // Ekler diskten okunur ve posta kurulur: çalışma iş parçacığını tutmasın.
    tokio::task::spawn_blocking(move || outbox.to_draft(&id, now_ms()))
        .await
        .map_err(internal)?
}

/// Gönderimi iptal eder (`send_cancel` ile aynı güvence: gönderim başladıysa
/// düzenlenemez) ve postayı taslağa çevirir; taslağın kimliğini döner.
#[tauri::command]
pub async fn outbox_to_draft(state: State<'_, AppState>, id: String) -> Res<MessageId> {
    to_draft(&state, id).await
}

#[cfg(test)]
mod tests {
    use mh_core::{Address, ErrorCode, MailboxRole, OutgoingMessage};

    use super::*;
    use crate::testutil::state;

    const HOUR: i64 = 3_600_000;

    fn outgoing(subject: &str, send_at: Option<i64>) -> OutgoingMessage {
        OutgoingMessage {
            account_id: "a".into(),
            to: vec![Address::new(Some("Ayşe"), "ayse@eposta.com")],
            cc: vec![Address::new(None, "kerem@ornek.com.tr")],
            bcc: Vec::new(),
            subject: subject.into(),
            body_text: "Rapor ekte değil, aşağıda.".into(),
            in_reply_to: None,
            attachments: Vec::new(),
            send_at,
            forward_from: None,
            draft_id: None,
            follow_up_at: None,
        }
    }

    #[tokio::test]
    async fn planlanan_gonderimler_listelenir_ve_hemen_gonderilir() {
        let st = state();
        let now = now_ms();
        assert!(st.outbox.scheduled().unwrap().is_empty());
        let later = st
            .outbox
            .enqueue(&outgoing("Yarınki rapor", Some(now + 24 * HOUR)), now)
            .unwrap();
        let sooner = st
            .outbox
            .enqueue(&outgoing("Öğleden sonra", Some(now + 2 * HOUR)), now)
            .unwrap();
        // Gönderim anı artan sırada; alıcılar ve konu görünür.
        let list = st.outbox.scheduled().unwrap();
        assert_eq!(
            list.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            [sooner.id.as_str(), later.id.as_str()]
        );
        assert_eq!(list[1].subject, "Yarınki rapor");
        assert_eq!(list[1].send_at, now + 24 * HOUR);
        assert_eq!(list[1].to.len(), 2, "Kime + Bilgi");
        assert_eq!(st.store.counts(now).unwrap().scheduled, 2);

        // Hemen gönder: gönderim anı şimdiye çekilir.
        st.outbox.send_now(&later.id, now + 5).unwrap();
        let list = st.outbox.scheduled().unwrap();
        assert_eq!(list[0].id, later.id);
        assert!(list[0].send_at <= now + 5);
        // Bilinmeyen gönderim: ne oldu söylenir.
        let err = st.outbox.send_now("yok", now).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn planlanan_gonderim_taslaga_cevrilir() {
        let st = state();
        let now = now_ms();
        let item = st
            .outbox
            .enqueue(&outgoing("Vazgeçilen posta", Some(now + HOUR)), now)
            .unwrap();
        let draft = to_draft(&st, item.id.clone()).await.unwrap();
        assert!(st.outbox.scheduled().unwrap().is_empty(), "gönderim kalktı");
        let loc = st.store.location(draft).unwrap().unwrap();
        assert_eq!(loc.role, MailboxRole::Drafts);
        let summary = st.store.summary(draft).unwrap().unwrap();
        assert_eq!(summary.subject, "Vazgeçilen posta");
        assert_eq!(summary.account_id, "a");
        // Artık giden kutusunda olmayan gönderim.
        let err = to_draft(&st, item.id).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(
            to_draft(&st, "yok".into()).await.unwrap_err().code,
            ErrorCode::NotFound
        );
        // Geri alınan gönderim listeden kalkar; ikinci kez geri alınamaz.
        let item = st
            .outbox
            .enqueue(&outgoing("Geri alınacak", Some(now + HOUR)), now)
            .unwrap();
        assert!(st.outbox.cancel(&item.id).unwrap());
        assert!(!st.outbox.cancel(&item.id).unwrap());
        assert!(st.outbox.scheduled().unwrap().is_empty());
    }
}
