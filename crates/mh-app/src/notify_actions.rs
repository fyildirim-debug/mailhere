//! Bildirimden yapılan işler: Okundu, Arşivle, Sil, İstenmeyen ve satır içi
//! yanıt. Masaüstü köşe kartı (`notify_action`, `notify_reply` komutları) ve
//! Android bildirim düğmeleri (kabuğun `eylem.rs`i) aynı yolu kullanır.
//!
//! İşler önce yerelde uygulanır, sunucuya kalıcı işlem kuyruğuyla gider (ağ
//! yoksa bağlantı gelince). Yanıt giden kutusuna girer; kullanıcının
//! "gönderimi geri al" süresi dolunca gönderilir.

use std::time::Duration;

use chrono::TimeZone;
use mh_core::{
    Account, Address, AppError, AppResult, ErrorCode, Language, MailboxRole, MessageDetail,
    MessageId, Millis, MoveTarget, NotifyAction, OutboxItem, OutgoingMessage,
};
use tauri::{AppHandle, Manager, State};

use crate::state::{now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Yanıtlanacak postanın gövdesi inmemişse en çok bu kadar beklenir; sonra
/// önizleme alıntılanır.
const BODY_WAIT: Duration = Duration::from_secs(8);

/// Bildirim düğmesinin işini yapar (yanıt hariç: [`reply`]) ve listeleri
/// yeniler.
pub fn apply(
    app: &AppHandle,
    state: &AppState,
    id: MessageId,
    action: NotifyAction,
) -> AppResult<()> {
    let touched = apply_local(state, id, action)?;
    crate::sink::changed(app, touched);
    Ok(())
}

/// İşin kendisi (pencere bilmez): yerelde uygular, sunucu işlemini kuyruğa
/// koyar; etkilenen kutuları döner. Penceresiz çekirdek (`crate::headless`)
/// de bunu kullanır.
pub fn apply_local(
    state: &AppState,
    id: MessageId,
    action: NotifyAction,
) -> AppResult<Vec<String>> {
    match action {
        NotifyAction::MarkRead => crate::cmd_jmap::set_flags(state, &[id], Some(false), None),
        NotifyAction::Archive => crate::cmd_jmap::move_messages(state, &[id], MoveTarget::Archive),
        NotifyAction::Trash => crate::cmd_jmap::move_messages(state, &[id], MoveTarget::Trash),
        NotifyAction::Spam => {
            let account = state.store.summary(id)?.ok_or_else(gone)?.account_id;
            let junk = state
                .store
                .mailbox_by_role(&account, MailboxRole::Junk)?
                .ok_or_else(no_junk)?;
            crate::rules::Engines::of(state).move_to_mailbox(&[id], &junk.mailbox.id)
        }
        NotifyAction::Reply | NotifyAction::Unknown => Err(AppError::new(
            ErrorCode::Invalid,
            "Bu düğme bildirimden çalıştırılamaz; postayı uygulamada aç.",
        )),
    }
}

/// Hesabın İstenmeyen kutusu var mı (bildirimde düğme gösterilsin mi)?
pub fn has_junk(state: &AppState, account_id: &str) -> AppResult<bool> {
    Ok(state
        .store
        .mailbox_by_role(account_id, MailboxRole::Junk)?
        .is_some())
}

fn gone() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Posta artık yok; başka bir cihazda taşınmış ya da silinmiş olabilir.",
    )
}

fn no_junk() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Bu hesapta İstenmeyen klasörü yok. Postayı uygulamada aç ve bir klasöre taşı.",
    )
}

/// Yanıtı giden kutusuna koyar ve postayı okundu sayar. Gövde inmemişse kısa
/// süre beklenir; ağ yoksa önizleme alıntılanır.
pub async fn reply(app: &AppHandle, id: MessageId, text: &str) -> AppResult<OutboxItem> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Yanıt boş. Bir şey yaz ya da postayı uygulamada aç.",
        ));
    }
    let state = app.try_state::<AppState>().ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            "Mailhere henüz hazır değil; birazdan yeniden dene.",
        )
    })?;
    let detail = match tokio::time::timeout(
        BODY_WAIT,
        crate::commands::message_get(app.state(), id),
    )
    .await
    {
        Ok(Ok(d)) => d,
        _ => {
            let store = state.store.clone();
            blocking(move || {
                let allow = store.settings()?.remote_allow;
                store.detail(id, &allow)
            })
            .await?
        }
    };
    let h = app.clone();
    let item = blocking(move || {
        let state = h.state::<AppState>();
        let msg = build_reply(&state, &detail, &text)?;
        state.outbox.enqueue(&msg, now_ms())
    })
    .await?;
    // Yanıtlanan posta okunmuş sayılır; olmazsa yanıt yine gider.
    let h = app.clone();
    let read = blocking(move || {
        let state = h.state::<AppState>();
        apply(&h, &state, id, NotifyAction::MarkRead)
    })
    .await;
    if let Err(e) = read {
        tracing::warn!(error = %e.message, "yanıtlanan posta okundu sayılamadı");
    }
    Ok(item)
}

/// Yanıt postasını kurar: hesabın yanıt imzası, dil, alıntı (depo okur; ağ
/// yok). Pencereli ve penceresiz yanıt aynı postayı üretir.
pub fn build_reply(
    state: &AppState,
    detail: &MessageDetail,
    text: &str,
) -> AppResult<OutgoingMessage> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Yanıt boş. Bir şey yaz ya da postayı uygulamada aç.",
        ));
    }
    let account = state
        .store
        .account(&detail.summary.account_id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Bu postanın hesabı artık yok."))?;
    let lang = state.store.settings()?.language;
    let input = ReplyInput::from_detail(detail);
    Ok(reply_message(
        &input,
        &account,
        text,
        lang,
        &format_date(input.date),
    ))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(crate::state::internal)?
}

/// Köşe kartındaki düğme (yanıt hariç).
#[tauri::command]
pub fn notify_action(
    app: AppHandle,
    state: State<'_, AppState>,
    id: MessageId,
    action: NotifyAction,
) -> Res<()> {
    apply(&app, &state, id, action)
}

/// Köşe kartından satır içi yanıt: giden kutusuna girer, geri alınabilir
/// (`send_cancel`).
#[tauri::command]
pub async fn notify_reply(app: AppHandle, id: MessageId, text: String) -> Res<OutboxItem> {
    reply(&app, id, &text).await
}

// ------------------------------------------------------------ yanıt postası

/// Yanıt için gereken alanlar (çekirdeğin `MessageDetail`inden).
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyInput {
    pub id: MessageId,
    pub account_id: String,
    pub from: Address,
    pub reply_to: Vec<Address>,
    pub to: Vec<Address>,
    pub subject: String,
    /// Alıntılanacak metin: gövde, yoksa önizleme.
    pub quote: String,
    pub date: Millis,
}

impl ReplyInput {
    pub fn from_detail(d: &MessageDetail) -> Self {
        let s = &d.summary;
        Self {
            id: s.id,
            account_id: s.account_id.clone(),
            from: s.from.clone(),
            reply_to: d.reply_to.clone(),
            to: s.to.clone(),
            subject: s.subject.clone(),
            quote: d
                .body_text
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| s.snippet.clone()),
            date: s.date,
        }
    }
}

fn format_date(ms: Millis) -> String {
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_default()
}

/// Konudan yanıt/iletme öneklerini atar (`Re:`, `Fwd:`, `Ynt:`, `İlt:`,
/// `Aw:`, `Wg:`, `Cevap:`; `Re[2]:` gibi sayılı biçimler dahil).
pub fn base_subject(subject: &str) -> String {
    const PREFIXES: [&str; 8] = ["re", "fw", "fwd", "ynt", "ilt", "cevap", "aw", "wg"];
    let mut s = subject.trim();
    while let Some(colon) = s.find(':') {
        let head = s[..colon].trim();
        // `Re[2]` → `re`
        let head = match head.find('[') {
            Some(b)
                if head.ends_with(']')
                    && head[b + 1..head.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_digit()) =>
            {
                head[..b].trim_end()
            }
            _ => head,
        };
        let folded: String = head
            .chars()
            .map(|c| match c {
                'İ' | 'I' | 'ı' => 'i',
                c => c.to_ascii_lowercase(),
            })
            .collect();
        if !PREFIXES.contains(&folded.as_str()) {
            break;
        }
        s = s[colon + 1..].trim_start();
    }
    s.trim().to_owned()
}

/// Yanıtın alıcıları: Reply-To ya da gönderen, hesabın kendi adresi
/// çıkarılmış; kimse kalmazsa (kendi gönderdiğimiz posta) postanın
/// alıcıları; o da boşsa yanıt adresleri. Arayüzdeki hızlı yanıtla aynı kural.
pub fn reply_targets(input: &ReplyInput, own_email: &str) -> Vec<Address> {
    let me = own_email.trim().to_lowercase();
    let not_me = |a: &&Address| a.email.trim().to_lowercase() != me;
    let targets: Vec<Address> = if input.reply_to.is_empty() {
        vec![input.from.clone()]
    } else {
        input.reply_to.clone()
    };
    let others: Vec<Address> = targets.iter().filter(not_me).cloned().collect();
    if !others.is_empty() {
        return others;
    }
    let to: Vec<Address> = input.to.iter().filter(not_me).cloned().collect();
    if to.is_empty() {
        targets
    } else {
        to
    }
}

/// Yanıt postası: metin (+ hesabın yanıt imzası) ve alıntı, arayüzün hızlı
/// yanıtıyla aynı biçimde. Ek yok; gönderim zamanı giden kutusunda belirlenir.
pub fn reply_message(
    input: &ReplyInput,
    account: &Account,
    text: &str,
    lang: Language,
    date: &str,
) -> OutgoingMessage {
    let signature = account
        .signature_reply
        .as_ref()
        .and_then(|id| account.signatures.iter().find(|s| &s.id == id))
        .map(|s| s.body.trim_end())
        .filter(|b| !b.trim().is_empty());
    let head = match signature {
        Some(sig) => format!("{}\n\n{sig}", text.trim()),
        None => text.trim().to_owned(),
    };
    let name = input.from.label();
    let wrote = match lang {
        Language::Tr => format!("{date} tarihinde {name} şunu yazdı:"),
        Language::En => format!("On {date}, {name} wrote:"),
    };
    let quoted = input
        .quote
        .split('\n')
        .map(|l| format!("> {}", l.trim_end_matches('\r')))
        .collect::<Vec<_>>()
        .join("\n");
    let subject = format!("Re: {}", base_subject(&input.subject));
    OutgoingMessage {
        account_id: input.account_id.clone(),
        to: reply_targets(input, &account.email),
        cc: vec![],
        bcc: vec![],
        subject: subject.trim_end().to_owned(),
        body_text: format!("{}\n\n{wrote}\n{quoted}\n", head.trim()),
        in_reply_to: Some(input.id),
        attachments: vec![],
        send_at: None,
        forward_from: None,
        draft_id: None,
        follow_up_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: Option<&str>, email: &str) -> Address {
        Address::new(name, email)
    }

    fn input() -> ReplyInput {
        ReplyInput {
            id: 42,
            account_id: "is".into(),
            from: addr(Some("Ayşe Demir"), "ayse@ornek.com"),
            reply_to: vec![],
            to: vec![addr(None, "ben@ornek.com")],
            subject: "Re: Ynt: Toplantı".into(),
            quote: "Merhaba,\r\nyarın 10'da?".into(),
            date: 0,
        }
    }

    fn account() -> Account {
        serde_json::from_value(serde_json::json!({
            "id": "is",
            "name": "İş",
            "email": "Ben@Ornek.com",
            "displayName": "Ben",
            "color": "#4655D4",
            "kind": "imap",
            "imap": null,
            "smtp": null,
            "jmapUrl": null,
            "signatures": [{ "id": "s1", "name": "Kısa", "body": "Sevgiler,\nBen\n\n" }],
            "signatureNew": null,
            "signatureReply": "s1",
            "createdAt": 0
        }))
        .expect("örnek hesap")
    }

    #[test]
    fn konu_onekleri_atilir() {
        assert_eq!(base_subject("Re: Ynt: Toplantı"), "Toplantı");
        assert_eq!(base_subject("RE[2]: fwd:  Rapor "), "Rapor");
        assert_eq!(base_subject("İLT: Cevap: AW: WG: Fw: Tatil"), "Tatil");
        assert_eq!(base_subject("Not: bugün"), "Not: bugün");
        assert_eq!(base_subject("Saat 10:30"), "Saat 10:30");
        assert_eq!(base_subject(""), "");
    }

    #[test]
    fn alicilar_kendi_adresim_cikarilir() {
        let mut i = input();
        assert_eq!(
            reply_targets(&i, "ben@ornek.com"),
            vec![addr(Some("Ayşe Demir"), "ayse@ornek.com")]
        );
        // Reply-To önceliklidir.
        i.reply_to = vec![addr(None, "liste@ornek.com")];
        assert_eq!(
            reply_targets(&i, "ben@ornek.com"),
            vec![addr(None, "liste@ornek.com")]
        );
        // Kendi gönderdiğim posta: alıcılara gider.
        let mut own = input();
        own.from = addr(Some("Ben"), "BEN@ornek.com");
        own.to = vec![addr(None, "ben@ornek.com"), addr(None, "ali@ornek.com")];
        assert_eq!(
            reply_targets(&own, "ben@ornek.com"),
            vec![addr(None, "ali@ornek.com")]
        );
        // Kimse kalmazsa yanıt adresleri.
        own.to = vec![];
        assert_eq!(
            reply_targets(&own, "ben@ornek.com"),
            vec![addr(Some("Ben"), "BEN@ornek.com")]
        );
    }

    #[test]
    fn yanit_metni_imza_ve_alinti() {
        let msg = reply_message(
            &input(),
            &account(),
            "  Olur, görüşürüz. ",
            Language::Tr,
            "15.10.2026 09:30",
        );
        assert_eq!(msg.account_id, "is");
        assert_eq!(msg.subject, "Re: Toplantı");
        assert_eq!(msg.in_reply_to, Some(42));
        assert_eq!(msg.to, vec![addr(Some("Ayşe Demir"), "ayse@ornek.com")]);
        assert!(msg.cc.is_empty() && msg.bcc.is_empty() && msg.attachments.is_empty());
        assert_eq!(msg.send_at, None);
        assert_eq!(
            msg.body_text,
            "Olur, görüşürüz.\n\nSevgiler,\nBen\n\n15.10.2026 09:30 tarihinde Ayşe Demir şunu yazdı:\n> Merhaba,\n> yarın 10'da?\n"
        );
        let mut a = account();
        a.signature_reply = None;
        let en = reply_message(&input(), &a, "OK", Language::En, "15.10.2026 09:30");
        assert_eq!(
            en.body_text,
            "OK\n\nOn 15.10.2026 09:30, Ayşe Demir wrote:\n> Merhaba,\n> yarın 10'da?\n"
        );
    }

    #[test]
    fn eylem_adlari_arayuzle_ayni() {
        for (a, name) in [
            (NotifyAction::Reply, "reply"),
            (NotifyAction::MarkRead, "markRead"),
            (NotifyAction::Archive, "archive"),
            (NotifyAction::Trash, "trash"),
            (NotifyAction::Spam, "spam"),
        ] {
            assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(name));
        }
    }
}
