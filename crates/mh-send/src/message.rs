//! Giden postayı RFC 5322 biçiminde kurar.

use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use lettre::address::Envelope;
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::Message;
use mh_core::{Account, Address, AppError, AppResult, ErrorCode, Millis, OutgoingMessage};
use mh_mime::AttachmentData;

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
    let name = a.name.as_deref().map(clean_name).filter(|n| !n.is_empty());
    Ok(Mailbox::new(name, email))
}

/// Görünen addaki satır sonu, sekme, diğer denetim karakterleri ve yön
/// değiştiren Unicode karakterleri boşluğa çevrilir. Satır sonu içeren ad
/// başlık eklemeye kapı açar ve lettre'yi çökertir (yabancı bir göndericinin
/// adı yanıtta alıcı olur).
pub fn clean_name(name: &str) -> String {
    mh_mime::clean_display_name(name)
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
    validate_with(msg, 0, false)
}

/// [`validate`]; `extra` bellekten eklenecek eklerin (ör. iletilen postanın)
/// toplam boyutudur. Taslakta alıcı zorunlu değildir.
pub fn validate_with(msg: &OutgoingMessage, extra: u64, draft: bool) -> AppResult<()> {
    if !draft && msg.to.is_empty() && msg.cc.is_empty() && msg.bcc.is_empty() {
        return Err(AppError::new(ErrorCode::Invalid, "En az bir alıcı ekle."));
    }
    for a in msg.to.iter().chain(&msg.cc).chain(&msg.bcc) {
        mailbox(a)?;
    }
    let mut total = extra;
    for path in &msg.attachments {
        total += regular_file(path)?.len();
    }
    if total > MAX_ATTACHMENTS {
        return Err(too_large());
    }
    Ok(())
}

fn too_large() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Eklerin toplamı 25 MB'ı geçiyor. Büyük dosyaları bağlantı olarak paylaş.",
    )
}

/// Ek yalnız düzenli bir dosya olabilir: klasör, aygıt (`/dev/zero`), boru ya
/// da `/proc` gibi boyutu yanıltan dosyalar reddedilir.
fn regular_file(path: &str) -> AppResult<std::fs::Metadata> {
    let missing = || {
        AppError::new(
            ErrorCode::Invalid,
            format!("Ek bulunamadı: {}. Dosyayı yeniden seç.", file_name(path)),
        )
    };
    let meta = std::fs::metadata(path).map_err(|_| missing())?;
    if !meta.is_file() {
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!("{} bir dosya değil; ek olarak eklenemez.", file_name(path)),
        ));
    }
    Ok(meta)
}

/// Eki en fazla `limit` bayt okur; dosya bu arada büyümüş olsa da sınırı aşmaz.
fn read_attachment(path: &str, limit: u64) -> AppResult<Vec<u8>> {
    use std::io::Read;
    regular_file(path)?;
    let unreadable = || {
        AppError::new(
            ErrorCode::Invalid,
            format!("Ek okunamadı: {}", file_name(path)),
        )
    };
    let file = std::fs::File::open(path).map_err(|_| unreadable())?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

fn bracket(id: &str) -> String {
    format!(
        "<{}>",
        id.trim().trim_start_matches('<').trim_end_matches('>')
    )
}

/// Postayı kurmanın seçenekleri.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions<'a> {
    /// Yanıtlanan postanın başlıkları.
    pub reply: Option<&'a ReplyRef>,
    /// Bellekten eklenecek dosyalar (ör. iletilen postanın ekleri).
    pub extra: &'a [AttachmentData],
    /// Taslak: alıcı zorunlu değil, gizli alıcılar başlıkta saklanır.
    pub draft: bool,
}

/// Postayı kurar. Message-ID gönderenin alan adıyla üretilir.
pub fn build(
    account: &Account,
    msg: &OutgoingMessage,
    reply: Option<&ReplyRef>,
    now: Millis,
) -> AppResult<Message> {
    build_with(
        account,
        msg,
        &BuildOptions {
            reply,
            ..BuildOptions::default()
        },
        now,
    )
}

/// Seçenekli [`build`]: iletilen ekler ve taslak kipi.
pub fn build_with(
    account: &Account,
    msg: &OutgoingMessage,
    opts: &BuildOptions<'_>,
    now: Millis,
) -> AppResult<Message> {
    let reply = opts.reply;
    let from = mailbox(&Address::new(Some(&account.display_name), &account.email))?;
    let domain = account
        .email
        .rsplit_once('@')
        .map(|(_, d)| d)
        .unwrap_or("mailhere.local");
    let mut b = Message::builder()
        .from(from.clone())
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
    if opts.draft {
        // Taslak gönderilmez; zarf yalnız kurulum için gönderenin kendisidir.
        let envelope = Envelope::new(Some(from.email.clone()), vec![from.email]).map_err(|e| {
            AppError::new(ErrorCode::Invalid, format!("Taslak oluşturulamadı: {e}"))
        })?;
        b = b.keep_bcc().envelope(envelope);
    }
    let text = SinglePart::plain(msg.body_text.clone());
    let built = if msg.attachments.is_empty() && opts.extra.is_empty() {
        b.singlepart(text)
    } else {
        let mut mp = MultiPart::mixed().singlepart(text);
        for a in opts.extra {
            let ct = ContentType::parse(&a.mime).unwrap_or_else(|_| {
                ContentType::parse("application/octet-stream").expect("geçerli tür")
            });
            mp = mp.singlepart(Attachment::new(a.name.clone()).body(a.bytes.clone(), ct));
        }
        let mut budget =
            MAX_ATTACHMENTS.saturating_sub(opts.extra.iter().map(|a| a.bytes.len() as u64).sum());
        for path in &msg.attachments {
            let bytes = read_attachment(path, budget)?;
            budget -= bytes.len() as u64;
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

    #[test]
    fn gorunen_ad_temizlenir_ve_posta_kurulur() {
        assert_eq!(clean_name("Evil\r\nX-Inject: 1"), "Evil X-Inject: 1");
        assert_eq!(clean_name("  Ali\t\u{202E}Veli\u{0}  "), "Ali Veli");
        let mut m = out();
        m.to = vec![Address::new(Some("Evil\r\nX-Inject: 1"), "x@ornek.com")];
        let raw =
            String::from_utf8(build(&account(), &m, None, 1_000).unwrap().formatted()).unwrap();
        assert!(!raw.contains("\r\nX-Inject"), "başlık eklenemez");
    }
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
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
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
            forward_from: None,
            draft_id: None,
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
    fn iletilen_ekler_kopyalanir() {
        let extra = vec![
            AttachmentData {
                name: "Sözleşme_v3.pdf".into(),
                mime: "application/pdf".into(),
                bytes: b"%PDF-1.4 iletilen".to_vec(),
            },
            AttachmentData {
                name: "plan.bilinmez".into(),
                mime: "bozuk tür".into(),
                bytes: vec![0, 1, 2],
            },
        ];
        let mut o = out();
        o.subject = "Fwd: Sözleşme".into();
        validate_with(&o, 20, false).unwrap();
        let opts = BuildOptions {
            extra: &extra,
            ..BuildOptions::default()
        };
        let raw = build_with(&account(), &o, &opts, 0).unwrap().formatted();
        let copied = mh_mime::visible_attachments(&raw);
        assert_eq!(copied.len(), 2);
        assert_eq!(copied[0], extra[0], "ad, tür ve içerik aynen kopyalanır");
        assert_eq!(copied[1].mime, "application/octet-stream");
        assert_eq!(copied[1].bytes, vec![0, 1, 2]);
        let r = mh_mime::render(&raw, false).unwrap();
        assert!(r.text.unwrap().contains("masayı ayırt"));
        assert_eq!(
            validate_with(&o, MAX_ATTACHMENTS + 1, false)
                .unwrap_err()
                .code,
            ErrorCode::Invalid,
            "iletilen eklerle 25 MB sınırı"
        );
    }

    #[test]
    fn taslak_kipi() {
        let mut o = out();
        o.to.clear();
        o.cc.clear();
        let opts = BuildOptions {
            draft: true,
            ..BuildOptions::default()
        };
        // Gizli alıcı taslakta saklanır (yeniden açınca kaybolmasın).
        let raw = build_with(&account(), &o, &opts, 0).unwrap().formatted();
        let r = mh_mime::render(&raw, false).unwrap();
        assert_eq!(r.bcc, vec![Address::new(None, "gizli@ornek.com.tr")]);
        // Alıcısız taslak kurulabilir; gönderim denetimi yine reddeder.
        o.bcc.clear();
        validate_with(&o, 0, true).unwrap();
        assert_eq!(validate(&o).unwrap_err().code, ErrorCode::Invalid);
        let raw = build_with(&account(), &o, &opts, 0).unwrap().formatted();
        let p = mh_mime::parse(&raw, false).unwrap();
        assert!(p.to.is_empty() && p.cc.is_empty());
        assert_eq!(p.subject, "Re: Cumartesi kahvaltı?");
        assert!(p.message_id.unwrap().ends_with("@ornek.com.tr"));
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
