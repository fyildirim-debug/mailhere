//! Taslak komutları: `draft_save`, `draft_delete`.
//!
//! Taslak yerelde hemen yazılır ve kimliği döner; sunucuya ekleme ve eski
//! kopyanın silinmesi hesabın komut kuyruğunda yapılır (çevrimdışıyken de
//! kayıt kaybolmaz).

use mh_core::{AppError, AppResult, MessageId, OutgoingMessage};
use tauri::State;

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// `draft_save`in kendisi (pencere bilmez): ekler yalnız bu oturumda seçilmiş
/// dosyalar olabilir; iletilen postanın içeriği yerelde yoksa önce indirilir.
pub async fn save(
    state: &AppState,
    picked: &crate::files::Picked,
    msg: OutgoingMessage,
) -> AppResult<MessageId> {
    picked.ensure(&msg.attachments)?;
    if let Some(id) = msg.forward_from {
        crate::commands::ensure_body(state, id).await?;
    }
    let outbox = state.outbox.clone();
    // Ekler diskten okunur ve posta kurulur: çalışma iş parçacığını tutmasın.
    tokio::task::spawn_blocking(move || outbox.save_draft(&msg, now_ms()))
        .await
        .map_err(internal)?
}

/// Taslağı kaydeder (`msg.draft_id` bir taslaksa onun yerine, aynı kimlikle)
/// ve taslağın kimliğini döner. İletilen postanın ekleri taslağa kopyalanır.
#[tauri::command]
pub async fn draft_save(
    state: State<'_, AppState>,
    picked: State<'_, crate::files::Picked>,
    msg: OutgoingMessage,
) -> Res<MessageId> {
    save(&state, &picked, msg).await
}

/// Taslağı yerelden ve sunucudan siler (IMAP ya da JMAP); zaten silinmişse
/// sessizce geçer.
#[tauri::command]
pub fn draft_delete(state: State<'_, AppState>, id: MessageId) -> Res<()> {
    state.outbox.delete_draft(id)
}

#[cfg(test)]
mod tests {
    use mh_core::{Address, ErrorCode, ListQuery, MessageSummary, View};

    use super::*;
    use crate::files::Picked;
    use crate::testutil::{deliver, raw_mail, state};

    fn outgoing(account: &str, subject: &str) -> OutgoingMessage {
        OutgoingMessage {
            account_id: account.into(),
            to: vec![Address::new(Some("Ayşe"), "ayse@eposta.com")],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.into(),
            body_text: "Merhaba, taslak gövdesi.".into(),
            in_reply_to: None,
            attachments: Vec::new(),
            send_at: None,
            forward_from: None,
            draft_id: None,
            follow_up_at: None,
        }
    }

    fn drafts(st: &AppState) -> Vec<MessageSummary> {
        st.store
            .list(
                &ListQuery {
                    view: View::Drafts,
                    account_id: None,
                    category: None,
                    only: None,
                    cursor: None,
                    limit: 50,
                },
                now_ms(),
            )
            .unwrap()
            .items
    }

    #[tokio::test]
    async fn taslak_kaydedilir_yerine_yazilir_ve_silinir() {
        let st = state();
        let picked = Picked::default();
        let id = save(&st, &picked, outgoing("a", "İlk taslak"))
            .await
            .unwrap();
        let list = drafts(&st);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].id, list[0].subject.as_str()), (id, "İlk taslak"));
        assert_eq!(list[0].mailbox_id, "a:Drafts");
        assert!(!list[0].unread, "taslak okunmuş yazılır");

        // Aynı taslak yeniden kaydedilir: kimlik korunur, içerik yenilenir.
        let mut again = outgoing("a", "Düzeltilmiş taslak");
        again.draft_id = Some(id);
        assert_eq!(save(&st, &picked, again).await.unwrap(), id);
        let list = drafts(&st);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].subject, "Düzeltilmiş taslak");

        // Gönderen hesap değişirse taslak yeni hesabın kutusuna geçer.
        let mut moved = outgoing("b", "Düzeltilmiş taslak");
        moved.draft_id = Some(id);
        let other = save(&st, &picked, moved).await.unwrap();
        let list = drafts(&st);
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].id, list[0].mailbox_id.as_str()),
            (other, "b:Drafts")
        );

        // Silme: taslak kalkar; yeniden silmek sessizce geçer.
        st.outbox.delete_draft(other).unwrap();
        assert!(drafts(&st).is_empty());
        st.outbox.delete_draft(other).unwrap();
        // Taslak olmayan posta taslak olarak silinemez.
        let inbox = deliver(
            &st,
            "a",
            raw_mail("Ayşe <ayse@eposta.com>", "Selam", "t1@x", "", "Merhaba"),
            10,
        );
        assert_eq!(
            st.outbox.delete_draft(inbox.id).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert!(st.store.summary(inbox.id).unwrap().is_some());
    }

    #[tokio::test]
    async fn taslakta_yalniz_secilmis_dosya_ek_olur() {
        let st = state();
        let picked = Picked::default();
        let dir = std::env::temp_dir().join(format!("mh-taslak-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not.txt");
        std::fs::write(&file, b"ek icerigi").unwrap();
        let path = file.to_string_lossy().into_owned();

        // Dosya seçme penceresinden gelmemiş yol reddedilir; taslak yazılmaz.
        let mut msg = outgoing("a", "Ekli taslak");
        msg.attachments = vec![path.clone()];
        let err = save(&st, &picked, msg.clone()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Invalid);
        assert!(drafts(&st).is_empty());

        // Seçilmiş dosya kabul edilir ve taslağa eklenir.
        assert_eq!(crate::files::accept(&picked, [file.clone()]), [path]);
        let id = save(&st, &picked, msg).await.unwrap();
        assert!(st.store.summary(id).unwrap().unwrap().has_attachments);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn taslak_bilinmeyen_hesapta_ve_icerigi_olmayan_iletide_kaydedilmez() {
        let st = state();
        let picked = Picked::default();
        let err = save(&st, &picked, outgoing("yok", "Taslak"))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        // İletilecek posta yok: içerik indirilemez, taslak yazılmaz.
        let mut fwd = outgoing("a", "Fwd: Rapor");
        fwd.forward_from = Some(9_999_999);
        let err = save(&st, &picked, fwd).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(drafts(&st).is_empty());
    }
}
