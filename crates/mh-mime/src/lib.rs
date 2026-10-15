//! Mailhere posta biçimi: ayrıştırma, HTML temizleme, önizleme ve akıllı
//! kategori. Ağ ya da depo bilmez; saf işlevlerdir.

pub mod category;
pub mod html;
pub mod ics;
pub mod otp;
pub mod parse;
mod prescan;
pub mod privacy;
pub mod text;
pub mod trust;
pub mod unsubscribe;

pub use category::classify;
pub use html::{decode_entities, remote_image_urls, sanitize_html, Sanitized};
pub use ics::{build_reply, calendar_text, invite, parse_calendar, to_invite, Calendar};
pub use otp::find_otp;
pub use parse::{
    attachment, otp_of, parse, render, visible_attachments, AttachmentData, ParsedMessage, Rendered,
};
pub use text::{
    clean_display_name, clean_line, collapse_ws, fold_tr, html_to_text, looks_like_html,
    normalize_subject, snippet,
};
pub use trust::sender_warning;
pub use unsubscribe::{parse_list_unsubscribe, parse_mailto, unsubscribe, Mailto};
