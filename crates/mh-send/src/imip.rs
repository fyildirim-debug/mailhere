//! Takvim davetine yanıt (iMIP, RFC 6047): `METHOD:REPLY` taşıyan posta.
//!
//! Yanıt yalnız kullanıcı isteyince, postayı alan hesaptan, davetin
//! düzenleyenine gider; geri alma süresi ve yeniden deneme yoktur. Gövde
//! `multipart/alternative`: okunur metin ve `text/calendar; method=REPLY`.

use std::time::{Duration, UNIX_EPOCH};

use lettre::message::header::ContentType;
use lettre::message::{MultiPart, SinglePart};
use lettre::Message;
use mh_core::{
    Account, Address, AppError, AppResult, ErrorCode, InviteResponse, Language, MessageId, Millis,
    OutgoingMessage,
};

use crate::message::{bracket, mailbox, validate_with, ReplyRef};
use crate::outbox::{now_ms, Outbox};
use crate::parts::reply_ref;

/// Yanıtın gönderim üst süresi.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Yanıt postasının konusu ve metni, arayüz diline göre.
pub fn reply_texts(
    summary: &str,
    response: InviteResponse,
    lang: Language,
    me: &str,
) -> (String, String) {
    let summary = summary.trim();
    let (prefix, verb) = match (lang, response) {
        (Language::Tr, InviteResponse::Accepted) => ("Kabul edildi", "daveti kabul etti"),
        (Language::Tr, InviteResponse::Tentative) => ("Belki", "daveti \"belki\" olarak yanıtladı"),
        (Language::Tr, InviteResponse::Declined) => ("Katılmıyor", "daveti reddetti"),
        (Language::En, InviteResponse::Accepted) => ("Accepted", "accepted the invitation"),
        (Language::En, InviteResponse::Tentative) => {
            ("Tentative", "tentatively accepted the invitation")
        }
        (Language::En, InviteResponse::Declined) => ("Declined", "declined the invitation"),
    };
    let subject = if summary.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}: {summary}")
    };
    let body = if summary.is_empty() {
        format!("{me} {verb}.")
    } else {
        format!("{me} {verb}: {summary}")
    };
    (subject, body)
}

/// Düzenleyene gidecek yanıt postası (zarf ve metin); takvim parçası ayrı
/// verilir. `invite_id` davet postasıdır: yanıt aynı konuşmaya bağlanır.
pub fn invite_reply_message(
    account: &Account,
    organizer: &Address,
    invite_id: MessageId,
    summary: &str,
    response: InviteResponse,
    lang: Language,
) -> OutgoingMessage {
    let (subject, body_text) = reply_texts(summary, response, lang, &account.display_name);
    OutgoingMessage {
        account_id: account.id.clone(),
        to: vec![organizer.clone()],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject,
        body_text,
        in_reply_to: Some(invite_id),
        attachments: Vec::new(),
        send_at: None,
        forward_from: None,
        draft_id: None,
        follow_up_at: None,
    }
}

/// iMIP yanıt postasını kurar: düz metin + `text/calendar; method=REPLY`.
pub fn build_imip(
    account: &Account,
    msg: &OutgoingMessage,
    ics: &str,
    reply: Option<&ReplyRef>,
    now: Millis,
) -> AppResult<Message> {
    let from = mailbox(&Address::new(Some(&account.display_name), &account.email))?;
    let domain = account
        .email
        .rsplit_once('@')
        .map(|(_, d)| d)
        .unwrap_or("mailhere.local");
    let mut b = Message::builder()
        .from(from)
        .subject(msg.subject.trim())
        .date(UNIX_EPOCH + Duration::from_millis(now.max(0) as u64))
        .message_id(Some(format!("<{}@{domain}>", uuid::Uuid::new_v4())))
        .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")).to_owned());
    for a in &msg.to {
        b = b.to(mailbox(a)?);
    }
    if let Some(mid) = reply.and_then(|r| r.message_id.as_deref()) {
        b = b.in_reply_to(bracket(mid));
        let mut refs: Vec<String> = reply
            .map(|r| r.references.iter().map(|x| bracket(x)).collect())
            .unwrap_or_default();
        let own = bracket(mid);
        if !refs.contains(&own) {
            refs.push(own);
        }
        b = b.references(refs.join(" "));
    }
    let calendar = ContentType::parse("text/calendar; charset=utf-8; method=REPLY")
        .map_err(|e| AppError::new(ErrorCode::Internal, format!("Takvim türü: {e}")))?;
    b.multipart(
        MultiPart::alternative()
            .singlepart(SinglePart::plain(msg.body_text.clone()))
            .singlepart(SinglePart::builder().header(calendar).body(ics.to_owned())),
    )
    .map_err(|e| {
        AppError::new(
            ErrorCode::Invalid,
            format!("Yanıt postası oluşturulamadı: {e}"),
        )
    })
}

impl Outbox {
    /// Davet yanıtını hemen gönderir; gönderilen kopya Gönderilenler'e
    /// yazılır. Hata doğrudan döner (taslağa çevrilmez).
    pub async fn send_invite_reply(&self, msg: &OutgoingMessage, ics: &str) -> AppResult<()> {
        validate_with(msg, 0, false)?;
        if msg.to.len() != 1 {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Davet yanıtı yalnız düzenleyene gönderilir.",
            ));
        }
        let account = self
            .store
            .account(&msg.account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Gönderen hesap bulunamadı."))?;
        let reply = reply_ref(&self.store, msg)?;
        let now = now_ms();
        let message = build_imip(&account, msg, ics, reply.as_ref(), now)?;
        match tokio::time::timeout(TIMEOUT, self.transmit(&account, msg, message, now)).await {
            Ok(Ok(note)) => {
                if let Some(note) = note {
                    tracing::warn!(note = %note, "davet yanıtı gönderildi, kopyası yazılamadı");
                }
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AppError::new(
                ErrorCode::Network,
                "Davet yanıtı bir dakikada gönderilemedi. İnternet bağlantını denetleyip yeniden dene.",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:sprint-42@ornek.com.tr\r\nSEQUENCE:1\r\nDTSTART:20261020T060000Z\r\nORGANIZER;CN=Kerem:mailto:kerem.aydin@ornek.com.tr\r\nATTENDEE:mailto:furkan@ornek.com.tr\r\nSUMMARY:Sprint planlama\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn account() -> Account {
        Account {
            id: "a".into(),
            name: "İş".into(),
            email: "furkan@ornek.com.tr".into(),
            display_name: "Furkan Yıldırım".into(),
            color: "#4C6FD8".into(),
            kind: mh_core::AccountKind::Imap,
            imap: None,
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

    #[test]
    fn metinler() {
        let (s, b) = reply_texts(
            "Sprint planlama",
            InviteResponse::Accepted,
            Language::Tr,
            "Furkan",
        );
        assert_eq!(s, "Kabul edildi: Sprint planlama");
        assert_eq!(b, "Furkan daveti kabul etti: Sprint planlama");
        let (s, _) = reply_texts("  ", InviteResponse::Declined, Language::En, "Furkan");
        assert_eq!(s, "Declined");
        let (s, b) = reply_texts("Standup", InviteResponse::Tentative, Language::En, "Furkan");
        assert_eq!(s, "Tentative: Standup");
        assert!(b.contains("tentatively"));
    }

    #[test]
    fn imip_postasi_kurulur() {
        let a = account();
        let organizer = Address::new(Some("Kerem Aydın"), "kerem.aydin@ornek.com.tr");
        let msg = invite_reply_message(
            &a,
            &organizer,
            7,
            "Sprint planlama",
            InviteResponse::Accepted,
            Language::Tr,
        );
        assert_eq!(msg.in_reply_to, Some(7));
        assert_eq!(msg.to, vec![organizer.clone()]);
        let reply = ReplyRef {
            message_id: Some("davet@ornek.com.tr".into()),
            references: vec![],
        };
        let cal = mh_mime::parse_calendar(ICS).unwrap();
        let ics = mh_mime::build_reply(
            &cal,
            &Address::new(Some(&a.display_name), &a.email),
            InviteResponse::Accepted,
            1_792_000_000_000,
        )
        .unwrap();
        let raw = build_imip(&a, &msg, &ics, Some(&reply), 1_792_000_000_000)
            .unwrap()
            .formatted();
        let text = String::from_utf8_lossy(&raw);
        assert!(
            text.contains("Content-Type: multipart/alternative"),
            "{text}"
        );
        assert!(
            text.contains("Content-Type: text/calendar; charset=utf-8; method=REPLY"),
            "{text}"
        );
        let p = mh_mime::parse(&raw, false).unwrap();
        assert_eq!(p.subject, "Kabul edildi: Sprint planlama");
        assert_eq!(p.in_reply_to.as_deref(), Some("davet@ornek.com.tr"));
        assert_eq!(p.to, vec![organizer]);
        assert!(p.body_text.unwrap().contains("daveti kabul etti"));
        // Takvim parçası postadan geri okunur: METHOD:REPLY, aynı UID ve SEQUENCE.
        let back = mh_mime::parse_calendar(&mh_mime::calendar_text(&raw).unwrap()).unwrap();
        assert_eq!(back.method.as_deref(), Some("REPLY"));
        let e = back.event.unwrap();
        assert_eq!(e.uid, "sprint-42@ornek.com.tr");
        assert_eq!(e.sequence, 1);
        assert_eq!(e.attendees[0].partstat.as_deref(), Some("ACCEPTED"));
        // Yanıt postası davet olarak gösterilmez (METHOD REPLY).
        assert!(mh_mime::invite(&raw).is_none());
    }

    struct NoSink;

    impl mh_sync::EventSink for NoSink {
        fn sync_status(&self, _: mh_core::SyncStatus) {}
        fn changed(&self, _: Vec<String>) {}
        fn new_mail(&self, _: &mh_core::Account, _: Vec<mh_core::MessageSummary>) {}
    }

    #[tokio::test]
    async fn yanit_bekletmeden_gonderilir() {
        let store = Arc::new(mh_store::Store::open_in_memory().unwrap());
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-13T10:30:00+03:00").unwrap();
        mh_store::seed_demo(&store, now).unwrap();
        let secrets: Arc<dyn mh_sync::SecretStore> = Arc::new(mh_sync::MemorySecrets::new());
        let sync = Arc::new(mh_sync::SyncManager::new(
            store.clone(),
            secrets.clone(),
            Arc::new(NoSink),
            tokio::runtime::Handle::current(),
        ));
        let outbox = Outbox::new(store.clone(), secrets, sync, Arc::new(NoSink));
        let account = store.accounts().unwrap().remove(0);
        let invite = store
            .find_by_subject("Cumartesi kahvaltı?")
            .unwrap()
            .unwrap();
        let organizer = Address::new(Some("Kerem"), "kerem.aydin@ornek.com.tr");
        let msg = invite_reply_message(
            &account,
            &organizer,
            invite.id,
            "Sprint planlama",
            InviteResponse::Declined,
            Language::Tr,
        );
        outbox.send_invite_reply(&msg, ICS).await.unwrap();
        let sent = store
            .find_by_subject("Katılmıyor: Sprint planlama")
            .unwrap()
            .unwrap();
        assert_eq!(sent.account_id, account.id);
        assert_eq!(sent.to, vec![organizer]);
        assert_eq!(
            store.outbox_next_due().unwrap(),
            None,
            "giden kutusuna girmez"
        );
        // Birden çok alıcıya gitmez.
        let mut bad = msg;
        bad.to.push(Address::new(None, "baska@ornek.com"));
        assert_eq!(
            outbox.send_invite_reply(&bad, ICS).await.unwrap_err().code,
            ErrorCode::Invalid
        );
    }
}
