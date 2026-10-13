//! Sözleşme tiplerini veritabanı metnine çevirme.

use mh_core::{AccountKind, Address, Category, MailboxRole};
use rusqlite::types::Type;

pub fn role_str(r: MailboxRole) -> &'static str {
    match r {
        MailboxRole::Inbox => "inbox",
        MailboxRole::Sent => "sent",
        MailboxRole::Drafts => "drafts",
        MailboxRole::Archive => "archive",
        MailboxRole::Trash => "trash",
        MailboxRole::Junk => "junk",
        MailboxRole::Other => "other",
    }
}

pub fn role_from(s: &str) -> MailboxRole {
    match s {
        "inbox" => MailboxRole::Inbox,
        "sent" => MailboxRole::Sent,
        "drafts" => MailboxRole::Drafts,
        "archive" => MailboxRole::Archive,
        "trash" => MailboxRole::Trash,
        "junk" => MailboxRole::Junk,
        _ => MailboxRole::Other,
    }
}

pub fn category_str(c: Category) -> &'static str {
    match c {
        Category::People => "people",
        Category::Notifications => "notifications",
        Category::Newsletters => "newsletters",
    }
}

pub fn category_from(s: &str) -> Category {
    match s {
        "notifications" => Category::Notifications,
        "newsletters" => Category::Newsletters,
        _ => Category::People,
    }
}

pub fn kind_str(k: AccountKind) -> &'static str {
    match k {
        AccountKind::Imap => "imap",
        AccountKind::Jmap => "jmap",
    }
}

pub fn kind_from(s: &str) -> AccountKind {
    match s {
        "jmap" => AccountKind::Jmap,
        _ => AccountKind::Imap,
    }
}

pub fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// JSON sütununu çözer; bozuk veri satır hatası olarak döner.
pub fn from_json<T: serde::de::DeserializeOwned>(idx: usize, s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e)))
}

pub fn addr_list(idx: usize, s: &str) -> rusqlite::Result<Vec<Address>> {
    from_json(idx, s)
}

/// Gönderen alanı FTS için: ad + adres.
pub fn sender_text(a: &Address) -> String {
    match a.name.as_deref() {
        Some(n) => format!("{n} {}", a.email),
        None => a.email.clone(),
    }
}
