//! SMTP bağlantısı: TCP, doğrudan TLS ya da zorunlu STARTTLS ve EHLO.
//!
//! TLS el sıkışması `mh_sync::tls` doğrulayıcısıyla yapılır (gömülü kökler,
//! olmazsa kullanıcının onayladığı sertifika; ikisi de değilse
//! `ErrorCode::Untrusted`). lettre'ye hazır, şifreli akış verilir; böylece
//! IMAP ve SMTP aynı sertifika kuralına uyar.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use lettre::transport::smtp::client::{AsyncSmtpConnection, AsyncTokioStream};
use lettre::transport::smtp::extension::ClientId;
use mh_core::{AppError, AppResult, ErrorCode, Security, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const GREETING_TIMEOUT: Duration = Duration::from_secs(30);

/// STARTTLS sonrası sunucu yeni karşılama göndermez, lettre ise önce
/// karşılama okur; yükseltilmiş akışın başına bu yerel satır konur.
const LOCAL_GREETING: &[u8] = b"220 mailhere starttls\r\n";

/// lettre'nin istediği akış: iç akış + karşı adres (+ isteğe bağlı yerel
/// karşılama satırı).
struct Stream<S> {
    inner: S,
    peer: SocketAddr,
    prefix: &'static [u8],
}

impl<S> fmt::Debug for Stream<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream").field("peer", &self.peer).finish()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Stream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.prefix.is_empty() {
            let n = self.prefix.len().min(buf.remaining());
            buf.put_slice(&self.prefix[..n]);
            self.prefix = &self.prefix[n..];
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Stream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl<S: AsyncRead + AsyncWrite + Send + Sync + Unpin> AsyncTokioStream for Stream<S> {
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.peer)
    }
}

fn unreachable_server(cfg: &ServerConfig) -> AppError {
    AppError::new(
        ErrorCode::Network,
        format!(
            "Giden posta sunucusuna ({}:{}) ulaşılamadı.",
            cfg.host.trim(),
            cfg.port
        ),
    )
}

async fn tcp(cfg: &ServerConfig) -> AppResult<(TcpStream, SocketAddr)> {
    let host = cfg.host.trim();
    let stream = match timeout(CONNECT_TIMEOUT, TcpStream::connect((host, cfg.port))).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            tracing::debug!(host, port = cfg.port, error = %e, "SMTP TCP bağlantısı kurulamadı");
            return Err(unreachable_server(cfg));
        }
        Err(_) => return Err(unreachable_server(cfg)),
    };
    let _ = stream.set_nodelay(true);
    let peer = stream.peer_addr().map_err(|_| unreachable_server(cfg))?;
    Ok((stream, peer))
}

/// Bağlanır, güvenli kanalı kurar ve EHLO ile sunucu bilgisini alır. Kimlik
/// doğrulama ve gönderim çağırana kalır.
pub async fn open(cfg: &ServerConfig) -> AppResult<AsyncSmtpConnection> {
    let host = cfg.host.trim();
    if cfg.security == Security::None && !mh_sync::is_local(host) {
        return Err(AppError::new(
            ErrorCode::Tls,
            "Şifresiz gönderime yalnız yerel test sunucularında izin verilir.",
        ));
    }
    let hello = ClientId::default();
    let (mut tcp, peer) = tcp(cfg).await?;
    let stream: Box<dyn AsyncTokioStream> = match cfg.security {
        Security::Tls => Box::new(Stream {
            inner: mh_sync::tls::handshake(host, cfg.port, tcp).await?,
            peer,
            prefix: b"",
        }),
        Security::Starttls => {
            mh_sync::tls::smtp_starttls(&mut tcp, &hello.to_string()).await?;
            Box::new(Stream {
                inner: mh_sync::tls::handshake(host, cfg.port, tcp).await?,
                peer,
                prefix: LOCAL_GREETING,
            })
        }
        Security::None => Box::new(Stream {
            inner: tcp,
            peer,
            prefix: b"",
        }),
    };
    match timeout(
        GREETING_TIMEOUT,
        AsyncSmtpConnection::connect_with_transport(stream, &hello),
    )
    .await
    {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(e)) => {
            tracing::warn!(host, error = %e, "SMTP oturumu açılamadı");
            Err(crate::transport::map_err(&e, cfg))
        }
        Err(_) => Err(unreachable_server(cfg)),
    }
}

/// Gönderim süre sınırı: büyük ekte yavaş bağlantıya pay bırakılır.
pub fn send_timeout(size: usize) -> Duration {
    Duration::from_secs(60 + size as u64 / 20_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn yerel_karsilama_once_okunur() {
        let (a, mut b) = tokio::io::duplex(64);
        let mut s = Stream {
            inner: a,
            peer: "127.0.0.1:25".parse().unwrap(),
            prefix: LOCAL_GREETING,
        };
        b.write_all(b"250 tamam\r\n").await.unwrap();
        let mut got = vec![0u8; LOCAL_GREETING.len() + 11];
        s.read_exact(&mut got).await.unwrap();
        assert_eq!(&got[..LOCAL_GREETING.len()], LOCAL_GREETING);
        assert_eq!(&got[LOCAL_GREETING.len()..], b"250 tamam\r\n");
        assert_eq!(send_timeout(0), Duration::from_secs(60));
    }
}
