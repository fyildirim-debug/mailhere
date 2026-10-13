//! Giden postayı RFC 5322 biçiminde kurar.

use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::Message;
use mh_core::{Account, Address, AppError, AppResult, ErrorCode, Millis, OutgoingMessage};

/// Yanıtlanan postanın başlıkları (köşeli parantezsiz).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyRef {
    pub message_id: Option<String>,
    pub references: Vec<String>,
}

/// Eklerin toplam üst sınırı (çoğu sağlayıcı 25 MB'ı geçeni reddeder).
pub const MAX_ATTACHMENTS: u64 = 25 * 1024 * 1024;

fn mailbox(a: &Address) -> AppResult<Mailbox> {
    let email = a.email.trim().parse().map_err(|_| {
        AppError::new(
            ErrorCode::Invalid,
            format!("Adres geçersiz: {}", a.email.trim()),
        )
    })?;
    let name = a
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
    Ok(Mailbox::new(name, email))
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ek")
        .to_owned()
}

/// Gönderim öncesi denetim: alıcı var mı, ekler okunabilir ve sınır içinde mi?
pub fn validate(msg: &OutgoingMessage) -> AppResult<()> {
    if msg.to.is_empty() && msg.cc.is_empty() && msg.bcc.is_empty() {
        return Err(AppError::new(ErrorCode::Invalid, "En az bir alıcı ekle."));
    }
    for a in msg.to.iter().chain(&msg.cc).chain(&msg.bcc) {
        mailbox(a)?;
    }
    let mut total = 0u64;
    for path in &msg.attachments {
        let meta = std::fs::metadata(path).map_err(|_| {
            AppError::new(
                ErrorCode::Invalid,
                format!("Ek bulunamadı: {}. Dosyayı yeniden seç.", file_name(path)),
            )
        })?;
        total += meta.len();
    }
    if total > MAX_ATTACHMENTS {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Eklerin toplamı 25 MB'ı geçiyor. Büyük dosyaları bağlantı olarak paylaş.",
        ));
    }
    Ok(())
}

fn bracket(id: &str) -> String {
    format!(
        "<{}>",
        id.trim().trim_start_matches('<').trim_end_matches('>')
    )
}

/// Postayı kurar. Message-ID gönderenin alan adıyla üretilir.
pub fn build(
    account: &Account,
    msg: &OutgoingMessage,
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
    for a in &msg.cc {
        b = b.cc(mailbox(a)?);
    }
    for a in &msg.bcc {
        b = b.bcc(mailbox(a)?);
    }
    if let Some(r) = reply {
        if let Some(mid) = r.message_id.as_deref() {
            b = b.in_reply_to(bracket(mid));
            let mut refs: Vec<String> = r.references.iter().map(|x| bracket(x)).collect();
            let own = bracket(mid);
            if !refs.contains(&own) {
                refs.push(own);
            }
            b = b.references(refs.join(" "));
        }
    }
    let text = SinglePart::plain(msg.body_text.clone());
    let built = if msg.attachments.is_empty() {
        b.singlepart(text)
    } else {
        let mut mp = MultiPart::mixed().singlepart(text);
        for path in &msg.attachments {
            let bytes = std::fs::read(path).map_err(|_| {
                AppError::new(
                    ErrorCode::Invalid,
                    format!("Ek okunamadı: {}", file_name(path)),
                )
            })?;
            let ct = ContentType::parse(crate::mime::for_path(path)).unwrap_or_else(|_| {
                ContentType::parse("application/octet-stream").expect("geçerli tür")
            });
            mp = mp.singlepart(Attachment::new(file_name(path)).body(bytes, ct));
        }
        b.multipart(mp)
    };
    built.map_err(|e| AppError::new(ErrorCode::Invalid, format!("Posta oluşturulamadı: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::AccountKind;

    pub fn account() -> Account {
        Account {
            id: "a".into(),
            name: "İş".into(),
            email: "furkan@ornek.com.tr".into(),
            display_name: "Furkan Yıldırım".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: None,
            smtp: None,
            jmap_url: None,
            signature: None,
            created_at: 0,
        }
    }

    pub fn out() -> OutgoingMessage {
        OutgoingMessage {
            account_id: "a".into(),
            to: vec![Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com")],
            cc: vec![Address::new(None, "kerem@ornek.com.tr")],
            bcc: vec![Address::new(None, "gizli@ornek.com.tr")],
            subject: "Re: Cumartesi kahvaltı?".into(),
            body_text: "Geliyorum, masayı ayırt lütfen.\nSevgiler".into(),
            in_reply_to: None,
            attachments: vec![],
            send_at: None,
        }
    }

    #[test]
    fn yanit_basliklari_ve_turkce() {
        let reply = ReplyRef {
            message_id: Some("kahvalti@eposta.com".into()),
            references: vec!["kok@eposta.com".into()],
        };
        let m = build(&account(), &out(), Some(&reply), 1_791_782_100_000).unwrap();
        let raw = m.formatted();
        let p = mh_mime::parse(&raw, false).unwrap();
        assert_eq!(
            p.from,
            Address::new(Some("Furkan Yıldırım"), "furkan@ornek.com.tr")
        );
        assert_eq!(p.subject, "Re: Cumartesi kahvaltı?");
        assert_eq!(p.in_reply_to.as_deref(), Some("kahvalti@eposta.com"));
        assert_eq!(p.references, vec!["kok@eposta.com", "kahvalti@eposta.com"]);
        assert_eq!(p.cc.len(), 1);
        assert!(p.message_id.unwrap().ends_with("@ornek.com.tr"));
        assert_eq!(p.date, Some(1_791_782_100_000));
        assert!(p.body_text.unwrap().contains("masayı ayırt"));
        let text = String::from_utf8_lossy(&raw);
        assert!(
            !text.contains("gizli@ornek.com.tr"),
            "Gizli alıcı başlıkta görünmez"
        );
        assert_eq!(m.envelope().to().len(), 3);
    }

    #[test]
    fn ekli_posta() {
        let dir = std::env::temp_dir().join(format!("mh-send-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Sözleşme_v3.pdf");
        std::fs::write(&path, b"%PDF-1.4 deneme").unwrap();
        let mut o = out();
        o.attachments = vec![path.to_string_lossy().into_owned()];
        validate(&o).unwrap();
        let raw = build(&account(), &o, None, 0).unwrap().formatted();
        let r = mh_mime::render(&raw, false).unwrap();
        assert_eq!(r.attachments.len(), 1);
        assert_eq!(r.attachments[0].name, "Sözleşme_v3.pdf");
        assert_eq!(r.attachments[0].mime, "application/pdf");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn denetim_hatalari() {
        let mut o = out();
        o.to.clear();
        o.cc.clear();
        o.bcc.clear();
        assert_eq!(validate(&o).unwrap_err().code, ErrorCode::Invalid);
        let mut o = out();
        o.to = vec![Address::new(None, "gecersiz adres")];
        assert!(validate(&o).unwrap_err().message.contains("gecersiz adres"));
        let mut o = out();
        o.attachments = vec!["/yok/boyle/bir/dosya.pdf".into()];
        assert!(validate(&o).unwrap_err().message.contains("dosya.pdf"));
    }
}
