//! Mailhere posta biçimi: ayrıştırma, HTML temizleme, önizleme ve akıllı
//! kategori. Ağ ya da depo bilmez; saf işlevlerdir.

pub mod category;
pub mod html;
pub mod parse;
mod prescan;
pub mod text;
pub mod unsubscribe;

pub use category::classify;
pub use html::{sanitize_html, Sanitized};
pub use parse::{
    attachment, parse, render, visible_attachments, AttachmentData, ParsedMessage, Rendered,
};
pub use text::{
    clean_display_name, clean_line, collapse_ws, fold_tr, html_to_text, normalize_subject, snippet,
};
pub use unsubscribe::{parse_list_unsubscribe, parse_mailto, unsubscribe, Mailto};
