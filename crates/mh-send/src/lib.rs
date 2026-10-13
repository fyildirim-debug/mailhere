//! Mailhere gönderim: posta kurma, SMTP, giden kutusu ve geri alma.

mod message;
mod mime;
mod outbox;
mod smtp;
mod transport;

pub use message::{build, validate, ReplyRef, MAX_ATTACHMENTS};
pub use mime::for_path as mime_for_path;
pub use outbox::Outbox;
pub use transport::{send, server_saves_sent};
