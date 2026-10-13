//! Kutu rolleri: önce SPECIAL-USE öznitelikleri (RFC 6154), yoksa ad tahmini.

use async_imap::types::NameAttribute;
use mh_core::MailboxRole;
use mh_mime::fold_tr;

/// Özniteliklerden rol. `\All` ayrı döner (Gmail'de arşiv yerine geçer).
pub fn from_attributes(attrs: &[NameAttribute<'_>]) -> Option<MailboxRole> {
    attrs.iter().find_map(|a| match a {
        NameAttribute::Sent => Some(MailboxRole::Sent),
        NameAttribute::Drafts => Some(MailboxRole::Drafts),
        NameAttribute::Archive => Some(MailboxRole::Archive),
        NameAttribute::Trash => Some(MailboxRole::Trash),
        NameAttribute::Junk => Some(MailboxRole::Junk),
        _ => None,
    })
}

pub fn is_all(attrs: &[NameAttribute<'_>]) -> bool {
    attrs.iter().any(|a| matches!(a, NameAttribute::All))
}

pub fn is_noselect(attrs: &[NameAttribute<'_>]) -> bool {
    attrs.iter().any(|a| {
        matches!(a, NameAttribute::NoSelect)
            || matches!(a, NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\NonExistent"))
    })
}

/// Yolun son parçası (görünen ad için, kodu çözülmüş).
pub fn leaf(path: &str, delimiter: Option<&str>) -> String {
    let decoded = crate::mutf7::decode(path);
    match delimiter.filter(|d| !d.is_empty()) {
        Some(d) => decoded.rsplit(d).next().unwrap_or(&decoded).to_owned(),
        None => decoded,
    }
}

/// Ad tahmini (Türkçe ve İngilizce yaygın adlar).
pub fn from_name(path: &str, delimiter: Option<&str>) -> Option<MailboxRole> {
    if path.eq_ignore_ascii_case("INBOX") {
        return Some(MailboxRole::Inbox);
    }
    let name = fold_tr(&leaf(path, delimiter));
    let name = name.trim();
    const SENT: &[&str] = &[
        "sent",
        "sent items",
        "sent mail",
        "sent messages",
        "gönderilmiş",
        "gönderilmiş öğeler",
        "gönderilmiş postalar",
        "gönderilenler",
        "gönderilen",
        "gönderilen öğeler",
        "giden",
    ];
    const DRAFTS: &[&str] = &["drafts", "draft", "taslaklar", "taslak"];
    const TRASH: &[&str] = &[
        "trash",
        "deleted",
        "deleted items",
        "deleted messages",
        "bin",
        "çöp",
        "çöp kutusu",
        "silinmiş öğeler",
        "silinmiş",
        "silinenler",
        "geri dönüşüm kutusu",
    ];
    const JUNK: &[&str] = &[
        "junk",
        "spam",
        "junk e-mail",
        "junk email",
        "junk mail",
        "bulk mail",
        "önemsiz",
        "önemsiz posta",
        "önemsiz e-posta",
        "istenmeyen",
        "istenmeyen posta",
        "gereksiz",
    ];
    const ARCHIVE: &[&str] = &["archive", "archives", "arşiv", "arşivlenenler"];
    let is = |list: &[&str]| list.contains(&name);
    if is(SENT) {
        Some(MailboxRole::Sent)
    } else if is(DRAFTS) {
        Some(MailboxRole::Drafts)
    } else if is(TRASH) {
        Some(MailboxRole::Trash)
    } else if is(JUNK) {
        Some(MailboxRole::Junk)
    } else if is(ARCHIVE) {
        Some(MailboxRole::Archive)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn ozniteliklerden() {
        assert_eq!(
            from_attributes(&[NameAttribute::Marked, NameAttribute::Sent]),
            Some(MailboxRole::Sent)
        );
        assert_eq!(
            from_attributes(&[NameAttribute::Junk]),
            Some(MailboxRole::Junk)
        );
        assert_eq!(from_attributes(&[NameAttribute::All]), None);
        assert!(is_all(&[NameAttribute::All]));
        assert!(is_noselect(&[NameAttribute::NoSelect]));
        assert!(is_noselect(&[NameAttribute::Extension(Cow::Borrowed(
            "\\NonExistent"
        ))]));
    }

    #[test]
    fn adlardan() {
        assert_eq!(from_name("INBOX", Some("/")), Some(MailboxRole::Inbox));
        assert_eq!(from_name("inbox", None), Some(MailboxRole::Inbox));
        assert_eq!(
            from_name("[Gmail]/Sent Mail", Some("/")),
            Some(MailboxRole::Sent)
        );
        assert_eq!(
            from_name("INBOX.Sent Items", Some(".")),
            Some(MailboxRole::Sent)
        );
        assert_eq!(
            from_name(&crate::mutf7::encode("Gönderilmiş Öğeler"), Some("/")),
            Some(MailboxRole::Sent)
        );
        assert_eq!(
            from_name(&crate::mutf7::encode("ÇÖP KUTUSU"), Some("/")),
            Some(MailboxRole::Trash)
        );
        assert_eq!(from_name("Taslaklar", Some("/")), Some(MailboxRole::Drafts));
        assert_eq!(from_name("Spam", Some("/")), Some(MailboxRole::Junk));
        assert_eq!(
            from_name(&crate::mutf7::encode("Arşiv"), Some("/")),
            Some(MailboxRole::Archive)
        );
        assert_eq!(from_name("Projeler", Some("/")), None);
        assert_eq!(
            leaf(&crate::mutf7::encode("İş/Müşteriler"), Some("/")),
            "Müşteriler"
        );
    }
}
