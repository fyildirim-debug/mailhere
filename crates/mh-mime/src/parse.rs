//! Ham RFC 5322 baytlarından liste/okuma alanı bilgilerini çıkarır.

use std::collections::HashMap;

use base64::Engine as _;
use mail_parser::{HeaderValue, Message, MessageParser, MessagePart, MimeHeaders, PartType};
use mh_core::{Address, Attachment, Category, Millis};

use crate::category::classify;
use crate::html::{normalize_cid, sanitize_html};
use crate::text::{clean_display_name, html_to_text, snippet, tidy_lines};

/// Arama dizinine yazılacak gövde metninin üst sınırı (karakter).
const INDEX_TEXT_LIMIT: usize = 64 * 1024;

/// Listeleme ve dizinleme için gereken alanlar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage {
    /// Köşeli parantezsiz Message-ID.
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: String,
    pub date: Option<Millis>,
    pub snippet: String,
    /// Aramada kullanılan düz metin (kısaltılmış); yalnız başlık geldiyse `None`.
    pub body_text: Option<String>,
    pub has_attachments: bool,
    pub category: Category,
}

/// Okuma alanı için işlenmiş gövde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: Option<String>,
    pub html: Option<String>,
    pub has_remote_content: bool,
    pub attachments: Vec<Attachment>,
    /// Başlıkta saklanan gizli alıcılar (yalnız taslaklarda bulunur).
    pub bcc: Vec<Address>,
}

/// Diske yazılacak bir ek.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentData {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

fn parser() -> MessageParser {
    MessageParser::default()
}

/// Adres listesi. Görünen ad kaynağında temizlenir (`clean_display_name`):
/// çözülmüş ad satır sonu ya da denetim karakteri taşıyabilir.
fn convert_addresses(a: Option<&mail_parser::Address<'_>>) -> Vec<Address> {
    a.map(|a| {
        a.iter()
            .filter_map(|addr| {
                let email = addr.address()?.trim();
                if email.is_empty() {
                    return None;
                }
                let name = addr
                    .name()
                    .map(clean_display_name)
                    .filter(|n| !n.is_empty() && n != email);
                Some(Address::new(name.as_deref(), email))
            })
            .collect()
    })
    .unwrap_or_default()
}

fn id_list(v: &HeaderValue<'_>) -> Vec<String> {
    match v {
        HeaderValue::Text(t) => vec![clean_id(t)],
        HeaderValue::TextList(list) => list.iter().map(|t| clean_id(t)).collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect()
}

fn clean_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_owned()
}

fn mime_of(part: &MessagePart<'_>) -> String {
    match part.content_type() {
        Some(ct) => match &ct.c_subtype {
            Some(sub) => format!("{}/{}", ct.c_type, sub).to_ascii_lowercase(),
            None => ct.c_type.to_ascii_lowercase(),
        },
        None => match part.body {
            PartType::Text(_) => "text/plain".into(),
            PartType::Html(_) => "text/html".into(),
            PartType::Message(_) => "message/rfc822".into(),
            _ => "application/octet-stream".into(),
        },
    }
}

/// Gerçek (çevrilmemiş) metin ve HTML gövdeleri.
fn bodies<'a>(msg: &'a Message<'a>) -> (Option<String>, Option<String>) {
    let text = msg
        .text_body
        .iter()
        .filter_map(|id| msg.parts.get(*id as usize))
        .filter_map(|p| match &p.body {
            PartType::Text(t) => Some(t.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let html = msg
        .html_body
        .iter()
        .filter_map(|id| msg.parts.get(*id as usize))
        .filter_map(|p| match &p.body {
            PartType::Html(t) => Some(t.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let join = |v: Vec<&str>, sep: &str| (!v.is_empty()).then(|| v.join(sep));
    (join(text, "\n\n"), join(html, "\n<hr>\n"))
}

/// HTML gövdede `cid:` ile başvurulan Content-ID'ler (küçük harfli).
fn referenced_cids(html: Option<&str>) -> Vec<String> {
    let Some(html) = html else {
        return Vec::new();
    };
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut rest = lower.as_str();
    while let Some(pos) = rest.find("cid:") {
        let tail = &rest[pos + 4..];
        let end = tail
            .find(|c: char| c == '"' || c == '\'' || c == ')' || c == '>' || c.is_whitespace())
            .unwrap_or(tail.len());
        out.push(normalize_cid(&tail[..end]));
        rest = &tail[end..];
    }
    out
}

fn inline_map(msg: &Message<'_>) -> HashMap<String, String> {
    let engine = base64::engine::general_purpose::STANDARD;
    msg.parts
        .iter()
        .filter_map(|p| {
            let cid = p.content_id()?;
            let mime = mime_of(p);
            if !mime.starts_with("image/") || mime.contains("svg") {
                return None;
            }
            let data = format!("data:{mime};base64,{}", engine.encode(p.contents()));
            Some((normalize_cid(cid), data))
        })
        .collect()
}

fn attachment_name(part: &MessagePart<'_>, index: usize) -> String {
    if let Some(n) = part
        .attachment_name()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        return n.to_owned();
    }
    if let PartType::Message(m) = &part.body {
        if let Some(s) = m.subject().map(str::trim).filter(|s| !s.is_empty()) {
            return format!("{s}.eml");
        }
        return format!("ileti-{}.eml", index + 1);
    }
    let mime = mime_of(part);
    let ext = mime.split('/').nth(1).unwrap_or("bin");
    format!("ek-{}.{ext}", index + 1)
}

/// Kullanıcıya gösterilecek ekler; HTML içinde kullanılan satır içi görseller
/// hariç. `index`, [`attachment`] ile aynı sırayı kullanır.
fn list_attachments(msg: &Message<'_>, html: Option<&str>) -> Vec<Attachment> {
    let used = referenced_cids(html);
    msg.attachments()
        .enumerate()
        .filter(|(_, p)| {
            !p.content_id()
                .is_some_and(|cid| used.contains(&normalize_cid(cid)))
        })
        .map(|(i, p)| Attachment {
            index: i as u32,
            name: attachment_name(p, i),
            mime: mime_of(p),
            size: p.len() as u64,
        })
        .collect()
}

fn headers_of<'a>(msg: &'a Message<'a>) -> Vec<(&'a str, &'a str)> {
    msg.headers_raw().collect()
}

fn top_is_mixed(msg: &Message<'_>) -> bool {
    msg.parts
        .first()
        .and_then(|p| p.content_type())
        .is_some_and(|ct| {
            ct.c_type.eq_ignore_ascii_case("multipart")
                && ct
                    .c_subtype
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case("mixed"))
        })
}

fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((pos, _)) => s[..pos].to_owned(),
        None => s.to_owned(),
    }
}

/// Postayı ayrıştırır. `headers_only` doğruysa yalnız başlık indirilmiştir;
/// önizleme boş kalır, ek bilgisi içerik türünden tahmin edilir.
pub fn parse(raw: &[u8], headers_only: bool) -> Option<ParsedMessage> {
    let msg = parser().parse(raw)?;
    let from = convert_addresses(msg.from().or_else(|| msg.sender()))
        .into_iter()
        .next()
        .unwrap_or_else(|| Address::new(None, ""));
    let (text, html) = if headers_only {
        (None, None)
    } else {
        bodies(&msg)
    };
    let plain = text
        .as_deref()
        .map(tidy_lines)
        .or_else(|| html.as_deref().map(html_to_text));
    let has_attachments = if headers_only {
        top_is_mixed(&msg)
    } else {
        !list_attachments(&msg, html.as_deref()).is_empty()
    };
    let category = classify(&headers_of(&msg), &from.email);
    let mut references = id_list(msg.references());
    let in_reply_to = id_list(msg.in_reply_to()).into_iter().next();
    references.dedup();
    Some(ParsedMessage {
        message_id: msg.message_id().map(clean_id).filter(|s| !s.is_empty()),
        in_reply_to,
        references,
        to: convert_addresses(msg.to()),
        cc: convert_addresses(msg.cc()),
        reply_to: convert_addresses(msg.reply_to()),
        subject: msg.subject().map(str::trim).unwrap_or("").to_owned(),
        date: msg.date().map(|d| d.to_timestamp() * 1000),
        snippet: plain.as_deref().map(snippet).unwrap_or_default(),
        body_text: if headers_only {
            None
        } else {
            Some(truncate_chars(
                plain.as_deref().unwrap_or(""),
                INDEX_TEXT_LIMIT,
            ))
        },
        has_attachments,
        category,
        from,
    })
}

/// Okuma alanı için gövdeyi hazırlar: HTML temizlenir, uzak görseller izne
/// göre engellenir, ekler listelenir.
pub fn render(raw: &[u8], remote_allowed: bool) -> Option<Rendered> {
    let msg = parser().parse(raw)?;
    let (text, html) = bodies(&msg);
    let attachments = list_attachments(&msg, html.as_deref());
    let sanitized = html
        .as_deref()
        .map(|h| sanitize_html(h, remote_allowed, inline_map(&msg)));
    // Sondaki satır sonları atılır: tek parçalı postada gövdeyi bitiren CRLF
    // metne dahildir ve taslak her açılıp kaydedildiğinde bir satır uzardı.
    let text = text
        .map(|t| t.replace("\r\n", "\n").trim_end_matches('\n').to_owned())
        .or_else(|| html.as_deref().map(html_to_text));
    Some(Rendered {
        text,
        has_remote_content: sanitized.as_ref().is_some_and(|s| s.has_remote),
        html: sanitized.map(|s| s.html),
        attachments,
        bcc: convert_addresses(msg.bcc()),
    })
}

/// `index` sıradaki eki çıkarır.
pub fn attachment(raw: &[u8], index: u32) -> Option<AttachmentData> {
    let msg = parser().parse(raw)?;
    let part = msg.attachments().nth(index as usize)?;
    Some(AttachmentData {
        name: attachment_name(part, index as usize),
        mime: mime_of(part),
        bytes: part.contents().to_vec(),
    })
}

/// Okuma alanında gösterilen eklerin tamamı (iletirken kopyalanır); HTML
/// gövdede kullanılan satır içi görseller hariç.
pub fn visible_attachments(raw: &[u8]) -> Vec<AttachmentData> {
    let Some(msg) = parser().parse(raw) else {
        return Vec::new();
    };
    let (_, html) = bodies(&msg);
    let shown = list_attachments(&msg, html.as_deref());
    msg.attachments()
        .enumerate()
        .filter(|(i, _)| shown.iter().any(|a| a.index as usize == *i))
        .map(|(i, part)| AttachmentData {
            name: attachment_name(part, i),
            mime: mime_of(part),
            bytes: part.contents().to_vec(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALTERNATIVE: &str = "From: =?UTF-8?B?QXnFn2UgRGVtaXI=?= <ayse.demir@eposta.com>\r
To: Furkan =?UTF-8?Q?Y=C4=B1ld=C4=B1r=C4=B1m?= <furkan@eposta.com>, kerem@ornek.com.tr\r
Cc: \"Can Öz\" <can@ornek.com>\r
Reply-To: ayse@baska.com\r
Subject: =?UTF-8?Q?Cumartesi_kahvalt=C4=B1=3F?=\r
Date: Mon, 12 Oct 2026 08:15:00 +0300\r
Message-ID: <kahvalti-1@eposta.com>\r
In-Reply-To: <onceki@eposta.com>\r
References: <kok@eposta.com> <onceki@eposta.com>\r
MIME-Version: 1.0\r
Content-Type: multipart/alternative; boundary=\"sinir\"\r
\r
--sinir\r
Content-Type: text/plain; charset=utf-8\r
Content-Transfer-Encoding: quoted-printable\r
\r
Selam!\r
\r
Cumartesi Moda'daki yeni kahvalt=C4=B1 yerine gidelim mi?\r
> eski al=C4=B1nt=C4=B1\r
--sinir\r
Content-Type: text/html; charset=utf-8\r
\r
<p>Selam!</p><p>Cumartesi <b>Moda</b>'daki yeni kahvaltı yerine gidelim mi?</p><img src=\"https://iz.example/p.gif\"><script>x()</script>\r
--sinir--\r
";

    const MIXED: &str = "From: Elif =?ISO-8859-9?Q?=DEahin?= <elif.sahin@hukukbiz.com.tr>\r
To: furkan@ornek.com.tr\r
Subject: =?UTF-8?B?U8O2emxlxZ9tZSBpbXpheWEgaGF6xLFy?=\r
Date: Sun, 11 Oct 2026 16:48:00 +0300\r
Message-ID: <sozlesme@hukukbiz.com.tr>\r
MIME-Version: 1.0\r
Content-Type: multipart/mixed; boundary=\"dis\"\r
\r
--dis\r
Content-Type: multipart/related; boundary=\"ic\"\r
\r
--ic\r
Content-Type: text/html; charset=utf-8\r
\r
<p>Son düzeltmeleri işledim.</p><img src=\"cid:imza@hukukbiz\">\r
--ic\r
Content-Type: image/png\r
Content-ID: <imza@hukukbiz>\r
Content-Disposition: inline\r
Content-Transfer-Encoding: base64\r
\r
iVBORw0KGgo=\r
--ic--\r
--dis\r
Content-Type: application/pdf; name=\"Sozlesme_v3.pdf\"\r
Content-Disposition: attachment; filename=\"Sozlesme_v3.pdf\"\r
Content-Transfer-Encoding: base64\r
\r
JVBERi0xLjQKJcOkw7zDtsOfCg==\r
--dis--\r
";

    #[test]
    fn alternatif_posta_ayristirilir() {
        let p = parse(ALTERNATIVE.as_bytes(), false).unwrap();
        assert_eq!(
            p.from,
            Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com")
        );
        assert_eq!(p.to.len(), 2);
        assert_eq!(p.to[0].name.as_deref(), Some("Furkan Yıldırım"));
        assert_eq!(p.to[1], Address::new(None, "kerem@ornek.com.tr"));
        assert_eq!(p.cc, vec![Address::new(Some("Can Öz"), "can@ornek.com")]);
        assert_eq!(p.reply_to, vec![Address::new(None, "ayse@baska.com")]);
        assert_eq!(p.subject, "Cumartesi kahvaltı?");
        assert_eq!(p.message_id.as_deref(), Some("kahvalti-1@eposta.com"));
        assert_eq!(p.in_reply_to.as_deref(), Some("onceki@eposta.com"));
        assert_eq!(p.references, vec!["kok@eposta.com", "onceki@eposta.com"]);
        assert_eq!(p.date, Some(1_791_782_100_000));
        assert_eq!(
            p.snippet,
            "Selam! Cumartesi Moda'daki yeni kahvaltı yerine gidelim mi?"
        );
        assert!(!p.has_attachments);
        assert_eq!(p.category, Category::People);
        assert!(p.body_text.unwrap().contains("eski alıntı"));
    }

    #[test]
    fn alternatif_posta_gosterilir() {
        let r = render(ALTERNATIVE.as_bytes(), false).unwrap();
        assert!(r.text.unwrap().starts_with("Selam!"));
        let html = r.html.unwrap();
        assert!(html.contains("<b>Moda</b>"));
        assert!(!html.contains("script"));
        assert!(html.contains("data-mh-src=\"https://iz.example/p.gif\""));
        assert!(r.has_remote_content);
        assert!(r.attachments.is_empty());
    }

    #[test]
    fn ekler_ve_cid_gorsel() {
        let p = parse(MIXED.as_bytes(), false).unwrap();
        assert_eq!(p.from.name.as_deref(), Some("Elif Şahin"));
        assert_eq!(p.subject, "Sözleşme imzaya hazır");
        assert!(p.has_attachments);
        assert_eq!(p.snippet, "Son düzeltmeleri işledim.");

        let r = render(MIXED.as_bytes(), false).unwrap();
        assert_eq!(r.attachments.len(), 1, "satır içi görsel ek sayılmaz");
        let a = &r.attachments[0];
        assert_eq!(a.name, "Sozlesme_v3.pdf");
        assert_eq!(a.mime, "application/pdf");
        assert_eq!(a.size, 19);
        let html = r.html.unwrap();
        assert!(
            html.contains("src=\"data:image/png;base64,iVBORw0KGgo=\""),
            "{html}"
        );
        assert!(!r.has_remote_content);
        // Düz metin yoksa HTML'den üretilir.
        assert_eq!(r.text.as_deref(), Some("Son düzeltmeleri işledim."));

        let data = attachment(MIXED.as_bytes(), a.index).unwrap();
        assert_eq!(data.name, "Sozlesme_v3.pdf");
        assert!(data.bytes.starts_with(b"%PDF-1.4"));
        assert!(attachment(MIXED.as_bytes(), 9).is_none());

        // İletirken yalnız görünen ekler kopyalanır.
        let all = visible_attachments(MIXED.as_bytes());
        assert_eq!(all, vec![data]);
        assert!(visible_attachments(ALTERNATIVE.as_bytes()).is_empty());
        assert!(r.bcc.is_empty());
    }

    #[test]
    fn gorunen_adda_satir_sonu_ve_denetim_karakteri_temizlenir() {
        // "Evil\r\nX-Inject: 1" kodlanmış sözcük olarak gelir.
        let raw = "From: =?utf-8?b?RXZpbA0KWC1JbmplY3Q6IDE=?= <evil@kotu.com>\r\n\
To: =?utf-8?b?RXZpbA0KWC1JbmplY3Q6IDE=?= <a@ornek.com>, \"Tab\tAd\" <b@ornek.com>\r\n\
Cc: =?utf-8?b?4oCuZXhlLnBkZg==?= <c@ornek.com>\r\n\
Reply-To: =?utf-8?b?DQo=?= <d@ornek.com>\r\n\
Bcc: =?utf-8?b?RXZpbA0KWC1JbmplY3Q6IDE=?= <e@ornek.com>\r\n\
Subject: x\r\n\r\nMetin";
        let p = parse(raw.as_bytes(), false).unwrap();
        assert_eq!(
            p.from,
            Address::new(Some("Evil X-Inject: 1"), "evil@kotu.com")
        );
        assert_eq!(p.to[0].name.as_deref(), Some("Evil X-Inject: 1"));
        assert_eq!(p.to[1].name.as_deref(), Some("Tab Ad"));
        // U+202E (sağdan sola zorlama) atılır.
        assert_eq!(p.cc[0].name.as_deref(), Some("exe.pdf"));
        // Yalnız satır sonundan oluşan ad hiç ad sayılmaz.
        assert_eq!(p.reply_to, vec![Address::new(None, "d@ornek.com")]);
        let r = render(raw.as_bytes(), false).unwrap();
        assert_eq!(r.bcc[0].name.as_deref(), Some("Evil X-Inject: 1"));
        for a in p.to.iter().chain(&p.cc).chain(&r.bcc).chain([&p.from]) {
            let n = a.name.as_deref().unwrap_or("");
            assert!(!n.chars().any(char::is_control), "{n:?}");
        }
    }

    #[test]
    fn taslakta_gizli_alici() {
        let raw = "From: furkan@ornek.com.tr\r\nBcc: Gizli <gizli@ornek.com.tr>\r\nSubject: Taslak\r\n\r\nMetin";
        let r = render(raw.as_bytes(), false).unwrap();
        assert_eq!(
            r.bcc,
            vec![Address::new(Some("Gizli"), "gizli@ornek.com.tr")]
        );
    }

    #[test]
    fn yalniz_baslik() {
        let head = MIXED.split("\r\n\r\n").next().unwrap().to_owned() + "\r\n\r\n";
        let p = parse(head.as_bytes(), true).unwrap();
        assert!(p.has_attachments);
        assert!(p.snippet.is_empty());
        assert!(p.body_text.is_none());
    }

    #[test]
    fn bulten_ve_bildirim_kategorisi() {
        let bulten = "From: Mimar Haftalik <bulten@mimarhaftalik.com>\r\nList-Unsubscribe: <https://mimarhaftalik.com/cik>\r\nSubject: Sayi 142\r\n\r\nMerhaba";
        assert_eq!(
            parse(bulten.as_bytes(), false).unwrap().category,
            Category::Newsletters
        );
        let kargo = "From: Derin Kargo <bilgi@derinkargo.com.tr>\r\nAuto-Submitted: auto-generated\r\nSubject: Dagitimda\r\n\r\nMerhaba";
        assert_eq!(
            parse(kargo.as_bytes(), false).unwrap().category,
            Category::Notifications
        );
    }
}
