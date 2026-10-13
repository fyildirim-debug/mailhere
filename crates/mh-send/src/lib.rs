//! Mailhere gönderim: posta kurma, SMTP, giden kutusu, geri alma ve taslaklar.

mod drafts;
mod message;
mod mime;
mod outbox;
mod parts;
mod smtp;
mod transport;
mod unsubscribe;

pub use message::{
    build, build_with, clean_name, validate, validate_with, BuildOptions, ReplyRef, MAX_ATTACHMENTS,
};
pub use mime::for_path as mime_for_path;
pub use outbox::Outbox;
pub use transport::{send, send_with, server_saves_sent, test_login};
pub use unsubscribe::{is_public_ip, mailto_message, one_click, OneClickError};
