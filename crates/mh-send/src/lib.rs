//! Mailhere gönderim: posta kurma, SMTP, giden kutusu, geri alma ve taslaklar.

mod drafts;
mod message;
mod mime;
mod outbox;
mod parts;
mod transport;

pub use message::{
    build, build_with, validate, validate_with, BuildOptions, ReplyRef, MAX_ATTACHMENTS,
};
pub use mime::for_path as mime_for_path;
pub use outbox::Outbox;
pub use transport::{send, send_with, server_saves_sent};
