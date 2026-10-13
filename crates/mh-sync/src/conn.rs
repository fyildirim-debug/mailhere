//! IMAP bağlantısı: TLS, zorunlu STARTTLS ve giriş.

use std::fmt::Debug;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_imap::{Client, Session};
use mh_core::{AppError, AppResult, ErrorCode, Security, ServerConfig};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use crate::error::{auth_error, network_error, tls_error, SyncError, SyncResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// TLS ya da (yalnız yerel testte) düz akış.
pub trait ImapIo: AsyncRead + AsyncWrite + Unpin + Send + Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> ImapIo for T {}

pub type ImapStream = Box<dyn ImapIo>;
pub type ImapSession = Session<ImapStream>;

/// Sistem dışı, gömülü kök sertifikalarla rustls ayarı (ring).
pub fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .expect("ring sağlayıcısı varsayılan TLS sürümlerini destekler")
            .with_root_certificates(roots)
            .with_no_client_auth();
            Arc::new(cfg)
        })
        .clone()
}

/// Şifresiz bağlantıya yalnız yerel makinede izin verilir.
pub fn is_local(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

async fn tcp(host: &str, port: u16) -> AppResult<TcpStream> {
    match timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port))).await {
        Ok(Ok(s)) => {
            let _ = s.set_nodelay(true);
            Ok(s)
        }
        Ok(Err(e)) => {
            tracing::debug!(host, port, error = %e, "TCP bağlantısı kurulamadı");
            Err(network_error(host, port))
        }
        Err(_) => Err(network_error(host, port)),
    }
}

async fn tls<S: AsyncRead + AsyncWrite + Unpin>(
    host: &str,
    stream: S,
) -> AppResult<tokio_rustls::client::TlsStream<S>> {
    let name = ServerName::try_from(host.to_owned())
        .map_err(|_| AppError::new(ErrorCode::Invalid, format!("Sunucu adı geçersiz: {host}")))?;
    let connector = TlsConnector::from(tls_config());
    match timeout(CONNECT_TIMEOUT, connector.connect(name, stream)).await {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => {
            tracing::warn!(host, error = %e, "TLS el sıkışması başarısız");
            Err(tls_error())
        }
        Err(_) => Err(AppError::new(
            ErrorCode::Network,
            "Güvenli bağlantı zaman aşımına uğradı.",
        )),
    }
}

async fn greeting<T: ImapIo>(client: &mut Client<T>) -> SyncResult<()> {
    match timeout(CONNECT_TIMEOUT, client.read_response()).await?? {
        Some(_) => Ok(()),
        None => Err(SyncError::App(AppError::new(
            ErrorCode::Network,
            "Sunucu karşılama iletisi göndermeden bağlantıyı kapattı.",
        ))),
    }
}

/// Bağlanır, gerekirse STARTTLS ile yükseltir ve oturum açar.
pub async fn connect(cfg: &ServerConfig, password: &str) -> SyncResult<ImapSession> {
    let host = cfg.host.trim();
    let stream = tcp(host, cfg.port).await?;
    let client: Client<ImapStream> = match cfg.security {
        Security::Tls => {
            let mut c = Client::new(Box::new(tls(host, stream).await?) as ImapStream);
            greeting(&mut c).await?;
            c
        }
        Security::Starttls => {
            let mut plain = Client::new(stream);
            greeting(&mut plain).await?;
            timeout(CONNECT_TIMEOUT, plain.run_command_and_check_ok("STARTTLS", None))
                .await?
                .map_err(|e| {
                    tracing::warn!(host, error = %e, "STARTTLS reddedildi");
                    SyncError::App(AppError::new(
                        ErrorCode::Tls,
                        "Sunucu STARTTLS ile güvenli bağlantıyı desteklemiyor. Güvenliği TLS olarak dene.",
                    ))
                })?;
            Client::new(Box::new(tls(host, plain.into_inner()).await?) as ImapStream)
        }
        Security::None => {
            if !is_local(host) {
                return Err(AppError::new(
                    ErrorCode::Tls,
                    "Şifresiz bağlantıya yalnız yerel test sunucularında izin verilir. TLS ya da STARTTLS seç.",
                )
                .into());
            }
            let mut c = Client::new(Box::new(stream) as ImapStream);
            greeting(&mut c).await?;
            c
        }
    };
    match timeout(LOGIN_TIMEOUT, client.login(cfg.username.trim(), password)).await? {
        Ok(session) => Ok(session),
        Err((async_imap::error::Error::No(m) | async_imap::error::Error::Bad(m), _)) => {
            tracing::info!(host, reply = %m.trim(), "giriş reddedildi");
            Err(auth_error().into())
        }
        Err((e, _)) => Err(e.into()),
    }
}

/// Hesap eklerken ayarları ve parolayı dener.
pub async fn test_login(cfg: &ServerConfig, password: &str) -> AppResult<()> {
    let mut session = connect(cfg, password).await.map_err(|e| e.to_app())?;
    let _ = timeout(Duration::from_secs(5), session.logout()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sifresiz_uzak_sunucu_reddedilir() {
        // Dinleyen yerel bir kapı aç ki TCP bağlantısı başarılı olsun.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let cfg = ServerConfig {
            host: "127.0.0.2".into(),
            port,
            security: Security::None,
            username: "u".into(),
        };
        // 127.0.0.2 yerel değil sayılır; bağlantı kurulsa da kurulamasa da düz metin reddedilir.
        let err = test_login(&cfg, "p").await.unwrap_err();
        assert!(
            matches!(err.code, ErrorCode::Tls | ErrorCode::Network),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn kapali_kapi_ag_hatasi() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let cfg = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::Tls,
            username: "u".into(),
        };
        let err = test_login(&cfg, "gizli-parola").await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Network);
        assert!(!err.message.contains("gizli-parola"));
    }

    #[tokio::test]
    async fn sahte_sunucuda_giris_reddi() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            w.write_all(b"* OK hazir\r\n").await.unwrap();
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let tag = line.split(' ').next().unwrap_or("*").to_owned();
                w.write_all(
                    format!("{tag} NO [AUTHENTICATIONFAILED] Kimlik dogrulanamadi\r\n").as_bytes(),
                )
                .await
                .unwrap();
            }
        });
        let cfg = ServerConfig {
            host: "127.0.0.1".into(),
            port,
            security: Security::None,
            username: "furkan".into(),
        };
        let err = test_login(&cfg, "yanlis").await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Auth);
    }
}
