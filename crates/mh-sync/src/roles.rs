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

/// Başka kutulardaki postaların kopyasını gösteren sanal kutu: `\Flagged`
/// (RFC 6154; Gmail "Yıldızlı") ya da `\Important` (RFC 8457; Gmail "Önemli").
/// Eşitlenirse aynı posta iki kez görünür; bu yüzden ilk eklemede gizlenir.
pub fn is_copy(attrs: &[NameAttribute<'_>]) -> bool {
    attrs.iter().any(|a| {
        matches!(a, NameAttribute::Flagged)
            || matches!(a, NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\Important"))
    })
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

/// Klasörün ağaçtaki yeri: (seçilebilir en yakın atanın yolu, görünen ad).
/// Seçilemeyen ya da listede olmayan ara düğümler atlanır; atlanan
/// parçalar görünen ada " / " ile eklenir ("Projeler / 2024"). Hiç
/// seçilebilir ata yoksa kökte durur. `prefix` kişisel ad alanıdır
/// ("INBOX."): altındaki klasörler köke bağlanır, önek addan atılır.
pub fn place(
    path: &str,
    delimiter: Option<&str>,
    prefix: &str,
    selectable: impl Fn(&str) -> bool,
) -> (Option<String>, String) {
    let Some(d) = delimiter.filter(|d| !d.is_empty()) else {
        return (None, crate::mutf7::decode(path));
    };
    let base = if !prefix.is_empty()
        && path.len() > prefix.len()
        && path
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    {
        prefix.len()
    } else {
        0
    };
    let parts: Vec<&str> = path[base..].split(d).collect();
    let name = |from: usize| {
        parts[from..]
            .iter()
            .map(|p| crate::mutf7::decode(p))
            .collect::<Vec<_>>()
            .join(" / ")
    };
    for i in (1..parts.len()).rev() {
        let candidate = format!("{}{}", &path[..base], parts[..i].join(d));
        if selectable(&candidate) {
            return (Some(candidate), name(i));
        }
    }
    (None, name(0))
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
        assert!(is_copy(&[NameAttribute::Flagged]));
        assert!(is_copy(&[NameAttribute::Extension(Cow::Borrowed(
            "\\important"
        ))]));
        assert!(!is_copy(&[NameAttribute::All, NameAttribute::Sent]));
    }

    #[test]
    fn agactaki_yer() {
        let sel = |p: &str| ["A", "A/B"].contains(&p);
        let at = |path: &str, d: Option<&str>, prefix: &str| place(path, d, prefix, sel);
        assert_eq!(at("A/B/C", Some("/"), ""), (Some("A/B".into()), "C".into()));
        assert_eq!(
            at("A/X/C", Some("/"), ""),
            (Some("A".into()), "X / C".into()),
            "seçilemeyen ara düğüm addan atlanmaz"
        );
        assert_eq!(at("Q/R", Some("/"), ""), (None, "Q / R".into()));
        assert_eq!(
            at("A/B", None, ""),
            (None, "A/B".into()),
            "ayraçsız ağaç yok"
        );
        assert_eq!(
            place("INBOX.A", Some("."), "INBOX.", |_| true),
            (None, "A".into()),
            "ad alanı öneki INBOX'a bağlamaz"
        );
        assert_eq!(
            place("inbox.A.B", Some("."), "INBOX.", |p| p == "inbox.A"),
            (Some("inbox.A".into()), "B".into())
        );
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
