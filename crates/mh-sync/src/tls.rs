//! TLS: gömülü kök sertifikalarla (webpki) doğrulama ve kullanıcının açıkça
//! onayladığı (sabitlenmiş) sertifikalar.
//!
//! Doğrulama başarısız olursa (tanınmayan kuruluş, kendinden imzalı, süresi
//! dolmuş, ad uyuşmuyor) bağlantı yalnız sunucunun yaprak sertifikasının
//! SHA-256 özeti o sunucu + kapı için onaylanmışsa kabul edilir; yoksa
//! `ErrorCode::Untrusted` döner. Sessizce kabul yoktur. Onaylı sertifikada da
//! el sıkışma imzası sertifikanın anahtarıyla doğrulanır.
//!
//! IMAP (bu crate) ve SMTP (`mh-send`) aynı doğrulayıcıyı kullanır.

use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use mh_core::{AppError, AppResult, CertInfo, ErrorCode, Security, TrustedCert};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::{verify_server_cert_signed_by_trust_anchor, verify_server_name};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::ParsedCertificate;
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::error::tls_error;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Düz metin STARTTLS görüşmesinde satır ve satır sayısı sınırı.
const MAX_LINE: usize = 4096;
const MAX_LINES: usize = 64;

// ---------------------------------------------------------------- ortak parçalar

fn provider() -> Arc<CryptoProvider> {
    static P: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    P.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider()))
        .clone()
}

fn roots() -> Arc<RootCertStore> {
    static R: OnceLock<Arc<RootCertStore>> = OnceLock::new();
    R.get_or_init(|| {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Arc::new(roots)
    })
    .clone()
}

fn webpki() -> Arc<rustls::client::WebPkiServerVerifier> {
    static V: OnceLock<Arc<rustls::client::WebPkiServerVerifier>> = OnceLock::new();
    V.get_or_init(|| {
        rustls::client::WebPkiServerVerifier::builder_with_provider(roots(), provider())
            .build()
            .expect("gömülü kök sertifikalarla doğrulayıcı kurulur")
    })
    .clone()
}

fn client_config(verifier: Arc<dyn ServerCertVerifier>) -> AppResult<Arc<ClientConfig>> {
    let cfg = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| {
            tracing::error!(error = %e, "TLS ayarı kurulamadı");
            tls_error()
        })?
        // Özel doğrulayıcı: önce webpki, olmazsa yalnız onaylı sertifika.
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// Karşılaştırma için sunucu adı: boşluksuz, küçük harf, sondaki nokta ve
/// IPv6 köşeli ayraçları atılmış.
fn host_key(host: &str) -> String {
    let h = host.trim().trim_end_matches('.');
    let h = h
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or(h);
    h.to_ascii_lowercase()
}

fn server_name(host: &str) -> AppResult<ServerName<'static>> {
    ServerName::try_from(host_key(host))
        .map_err(|_| AppError::new(ErrorCode::Invalid, format!("Sunucu adı geçersiz: {host}")))
}

/// Sertifikanın DER kodlu halinin SHA-256 özeti: büyük harf onaltılık, `:` ayraçlı.
pub fn fingerprint(der: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, der);
    digest
        .as_ref()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Parmak izini olağan biçime çevirir (ayraçlı ya da ayraçsız, her harf
/// büyüklüğünde kabul edilir); 32 baytlık onaltılık değilse `None`.
pub fn normalize_fingerprint(s: &str) -> Option<String> {
    let hex: String = s
        .chars()
        .filter(|c| !matches!(c, ':' | ' ' | '-'))
        .collect::<String>()
        .to_ascii_uppercase();
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let pairs: Vec<&str> = (0..32).map(|i| &hex[i * 2..i * 2 + 2]).collect();
    Some(pairs.join(":"))
}

// ------------------------------------------------------------ onaylı sertifikalar

static TRUSTED: RwLock<Vec<TrustedCert>> = RwLock::new(Vec::new());

/// Ayarlardaki onaylı sertifikaları yükler (açılışta ve ayar değişince).
pub fn set_trusted(list: &[TrustedCert]) {
    let mut t = TRUSTED.write().unwrap_or_else(|e| e.into_inner());
    *t = list.to_vec();
}

/// Tek bir onayı ekler ya da aynı sunucu + kapının onayını değiştirir.
pub fn trust(cert: TrustedCert) {
    let mut t = TRUSTED.write().unwrap_or_else(|e| e.into_inner());
    t.retain(|c| !(host_key(&c.host) == host_key(&cert.host) && c.port == cert.port));
    t.push(cert);
}

/// Bu sunucu + kapı için bu parmak izi onaylanmış mı?
pub fn is_trusted(host: &str, port: u16, sha256: &str) -> bool {
    let Some(fp) = normalize_fingerprint(sha256) else {
        return false;
    };
    let host = host_key(host);
    TRUSTED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|c| {
            c.port == port
                && host_key(&c.host) == host
                && normalize_fingerprint(&c.sha256).as_deref() == Some(fp.as_str())
        })
}

// ------------------------------------------------------------------ doğrulayıcı

type Chain = Vec<CertificateDer<'static>>;

fn owned_chain(end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>]) -> Chain {
    std::iter::once(end_entity)
        .chain(intermediates)
        .map(|c| c.clone().into_owned())
        .collect()
}

/// webpki; olmazsa yalnız onaylı parmak izi. Reddedilen zinciri saklar ki
/// hata iletisi nedeni söyleyebilsin.
#[derive(Debug)]
struct PinVerifier {
    host: String,
    port: u16,
    rejected: Mutex<Option<Chain>>,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match webpki().verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            Ok(ok) => Ok(ok),
            Err(e @ rustls::Error::InvalidCertificate(_)) => {
                if is_trusted(&self.host, self.port, &fingerprint(end_entity)) {
                    tracing::info!(host = %self.host, port = self.port, "onaylanmış sertifika kabul edildi");
                    return Ok(ServerCertVerified::assertion());
                }
                *self.rejected.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(owned_chain(end_entity, intermediates));
                Err(e)
            }
            Err(e) => Err(e),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        webpki().verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        webpki().verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        webpki().supported_verify_schemes()
    }
}

/// Yalnız inceleme: zinciri alır ve el sıkışmayı keser (bağlantı hiçbir
/// zaman kurulmaz, veri gönderilmez).
#[derive(Debug, Default)]
struct CaptureVerifier {
    chain: Mutex<Option<Chain>>,
}

impl ServerCertVerifier for CaptureVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        *self.chain.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(owned_chain(end_entity, intermediates));
        Err(rustls::Error::General("yalnız sertifika incelemesi".into()))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        webpki().verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        webpki().verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        webpki().supported_verify_schemes()
    }
}

// ------------------------------------------------------------------- sorunlar

/// Zincirin neden güvenilmediği (Türkçe, kısa). Geçerli sertifikada boş.
fn problems(chain: &[CertificateDer<'_>], name: &ServerName<'_>, now: UnixTime) -> Vec<&'static str> {
    let mut out = Vec::new();
    let Some(leaf) = chain.first() else {
        return vec!["sertifika yok"];
    };
    let mut self_signed = false;
    match x509_parser::parse_x509_certificate(leaf) {
        Ok((_, cert)) => {
            self_signed = cert.subject().as_raw() == cert.issuer().as_raw();
            if self_signed {
                out.push("kendinden imzalı");
            }
            let secs = now.as_secs() as i64;
            if secs > cert.validity().not_after.timestamp() {
                out.push("süresi dolmuş");
            } else if secs < cert.validity().not_before.timestamp() {
                out.push("henüz geçerli değil");
            }
        }
        Err(_) => out.push("okunamayan sertifika"),
    }
    match ParsedCertificate::try_from(leaf) {
        Ok(parsed) => {
            if verify_server_name(&parsed, name).is_err() {
                out.push("ad uyuşmuyor");
            }
            if !self_signed {
                let algs = provider().signature_verification_algorithms.all;
                if let Err(rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer)) =
                    verify_server_cert_signed_by_trust_anchor(&parsed, &roots(), &chain[1..], now, algs)
                {
                    out.push("tanınmayan kuruluş");
                }
            }
        }
        Err(_) if out.is_empty() => out.push("okunamayan sertifika"),
        Err(_) => {}
    }
    out
}

/// Güvenilmeyen sertifika hatası (arayüz `cert_inspect` ile inceler).
pub fn untrusted_error(host: &str, port: u16, problem: &str) -> AppError {
    let host = host.trim();
    let why = if problem.is_empty() {
        "doğrulanamadı".to_owned()
    } else {
        problem.to_owned()
    };
    AppError::new(
        ErrorCode::Untrusted,
        format!(
            "{host}:{port} sunucusunun sertifikası güvenilir değil ({why}). Sertifikayı inceleyip bu sunucuya güveniyorsan onaylayabilirsin."
        ),
    )
}

/// Akışı TLS'e yükseltir. Sertifika webpki ile doğrulanamaz ve onaylı da
/// değilse `ErrorCode::Untrusted` döner.
pub async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
    host: &str,
    port: u16,
    stream: S,
) -> AppResult<TlsStream<S>> {
    let name = server_name(host)?;
    let verifier = Arc::new(PinVerifier {
        host: host_key(host),
        port,
        rejected: Mutex::new(None),
    });
    let connector = TlsConnector::from(client_config(verifier.clone())?);
    match timeout(HANDSHAKE_TIMEOUT, connector.connect(name.clone(), stream)).await {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => {
            let rejected = verifier
                .rejected
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            if let Some(chain) = rejected {
                let problem = problems(&chain, &name, UnixTime::now()).join(", ");
                tracing::warn!(host, port, problem, "sunucu sertifikası güvenilir değil");
                return Err(untrusted_error(host, port, &problem));
            }
            tracing::warn!(host, error = %e, "TLS el sıkışması başarısız");
            Err(tls_error())
        }
        Err(_) => Err(AppError::new(
            ErrorCode::Network,
            "Güvenli bağlantı zaman aşımına uğradı.",
        )),
    }
}

// ------------------------------------------------------- düz metin STARTTLS

fn starttls_refused() -> AppError {
    AppError::new(
        ErrorCode::Tls,
        "Sunucu STARTTLS ile güvenli bağlantıyı desteklemiyor. Güvenliği TLS olarak dene.",
    )
}

fn protocol_error() -> AppError {
    AppError::new(
        ErrorCode::Network,
        "Sunucunun yanıtı anlaşılamadı; bağlantı kapatıldı.",
    )
}

/// Tek satır okur (CRLF hariç). Arabelleğe fazladan bayt almaz: TLS'ten önce
/// gelen düz metin hiçbir zaman şifreli oturuma taşınmaz.
async fn read_line(s: &mut TcpStream) -> AppResult<String> {
    let mut line = Vec::new();
    loop {
        let b = s.read_u8().await.map_err(|_| protocol_error())?;
        if b == b'\n' {
            break;
        }
        line.push(b);
        if line.len() > MAX_LINE {
            return Err(protocol_error());
        }
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

async fn write_line(s: &mut TcpStream, line: &str) -> AppResult<()> {
    s.write_all(format!("{line}\r\n").as_bytes())
        .await
        .map_err(|_| protocol_error())
}

/// SMTP yanıtı: (kod, satırların metni). "250-..." satırları sürer.
async fn smtp_reply(s: &mut TcpStream) -> AppResult<(u16, Vec<String>)> {
    let mut lines = Vec::new();
    for _ in 0..MAX_LINES {
        let line = read_line(s).await?;
        let code: u16 = line
            .get(..3)
            .and_then(|c| c.parse().ok())
            .ok_or_else(protocol_error)?;
        let more = line.as_bytes().get(3) == Some(&b'-');
        lines.push(line.get(4..).unwrap_or("").trim().to_owned());
        if !more {
            return Ok((code, lines));
        }
    }
    Err(protocol_error())
}

/// SMTP: karşılama, EHLO ve STARTTLS. Dönüşte akış TLS el sıkışmasına hazırdır
/// (TLS'ten sonra EHLO yeniden gönderilmelidir).
pub async fn smtp_starttls(s: &mut TcpStream, hello: &str) -> AppResult<()> {
    let work = async {
        let (code, _) = smtp_reply(s).await?;
        if code != 220 {
            return Err(protocol_error());
        }
        write_line(s, &format!("EHLO {hello}")).await?;
        let (code, lines) = smtp_reply(s).await?;
        if code != 250 {
            return Err(protocol_error());
        }
        if !lines
            .iter()
            .any(|l| l.split_whitespace().next().is_some_and(|k| k.eq_ignore_ascii_case("STARTTLS")))
        {
            return Err(starttls_refused());
        }
        write_line(s, "STARTTLS").await?;
        match smtp_reply(s).await? {
            (220, _) => Ok(()),
            _ => Err(starttls_refused()),
        }
    };
    timeout(HANDSHAKE_TIMEOUT, work)
        .await
        .unwrap_or_else(|_| Err(protocol_error()))
}

/// IMAP: karşılama ve STARTTLS (yalnız incelemede; oturum açan bağlantı
/// `async-imap` ile yükseltir).
async fn imap_starttls(s: &mut TcpStream) -> AppResult<()> {
    let work = async {
        let greeting = read_line(s).await?;
        if !greeting.starts_with("* OK") && !greeting.starts_with("* PREAUTH") {
            return Err(protocol_error());
        }
        write_line(s, "m1 STARTTLS").await?;
        for _ in 0..MAX_LINES {
            let line = read_line(s).await?;
            if let Some(rest) = line.strip_prefix("m1 ") {
                return if rest.to_ascii_uppercase().starts_with("OK") {
                    Ok(())
                } else {
                    Err(starttls_refused())
                };
            }
        }
        Err(protocol_error())
    };
    timeout(HANDSHAKE_TIMEOUT, work)
        .await
        .unwrap_or_else(|_| Err(protocol_error()))
}

// ------------------------------------------------------------------- inceleme

/// Kapıdan protokol: 465/587/25 SMTP, diğerleri (993/143 dahil) IMAP.
fn is_smtp_port(port: u16) -> bool {
    matches!(port, 465 | 587 | 25 | 2525)
}

fn cert_info(
    host: &str,
    port: u16,
    chain: &[CertificateDer<'_>],
    name: &ServerName<'_>,
) -> AppResult<CertInfo> {
    let leaf = chain.first().ok_or_else(no_certificate)?;
    let (_, cert) = x509_parser::parse_x509_certificate(leaf).map_err(|_| {
        AppError::new(
            ErrorCode::Tls,
            "Sunucunun sertifikası okunamadı; güvenli bağlantı kurulamaz.",
        )
    })?;
    let mut names: Vec<String> = Vec::new();
    if let Ok(Some(san)) = cert.subject_alternative_name() {
        for n in &san.value.general_names {
            match n {
                x509_parser::extensions::GeneralName::DNSName(d) => names.push((*d).to_owned()),
                x509_parser::extensions::GeneralName::IPAddress(ip) => match ip.len() {
                    4 => names.push(std::net::Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]).to_string()),
                    16 => {
                        let mut b = [0u8; 16];
                        b.copy_from_slice(ip);
                        names.push(std::net::Ipv6Addr::from(b).to_string());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    if names.is_empty() {
        names.extend(
            cert.subject()
                .iter_common_name()
                .filter_map(|cn| cn.as_str().ok().map(str::to_owned)),
        );
    }
    Ok(CertInfo {
        host: host.trim().to_owned(),
        port,
        sha256: fingerprint(leaf),
        subject: cert.subject().to_string(),
        issuer: cert.issuer().to_string(),
        not_before: Some(cert.validity().not_before.timestamp() * 1000),
        not_after: Some(cert.validity().not_after.timestamp() * 1000),
        names,
        problem: problems(chain, name, UnixTime::now()).join(", "),
    })
}

fn no_certificate() -> AppError {
    AppError::new(
        ErrorCode::Tls,
        "Sunucu sertifika göndermedi; güvenli bağlantı kurulamadı.",
    )
}

/// Sunucunun sertifikasını doğrulamadan alır ve inceler (onay ekranı için).
/// Bağlantı kurulmaz: el sıkışma sertifika alınınca kesilir.
pub async fn inspect(host: &str, port: u16, security: Security) -> AppResult<CertInfo> {
    let host = host.trim();
    if security == Security::None {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Şifresiz bağlantıda sertifika yok. Güvenliği TLS ya da STARTTLS seç.",
        ));
    }
    let name = server_name(host)?;
    let mut tcp = crate::conn::tcp(host, port).await?;
    if security == Security::Starttls {
        if is_smtp_port(port) {
            smtp_starttls(&mut tcp, "localhost").await?;
        } else {
            imap_starttls(&mut tcp).await?;
        }
    }
    let capture = Arc::new(CaptureVerifier::default());
    let connector = TlsConnector::from(client_config(capture.clone())?);
    let res = timeout(HANDSHAKE_TIMEOUT, connector.connect(name.clone(), tcp)).await;
    let chain = capture
        .chain
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    match chain {
        Some(chain) => cert_info(host, port, &chain, &name),
        None => {
            if let Ok(Err(e)) = res {
                tracing::warn!(host, port, error = %e, "sertifika alınamadı");
            }
            Err(no_certificate())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    /// Test sertifikası: (DER, özel anahtar).
    pub struct TestCert {
        pub der: CertificateDer<'static>,
        pub key: Vec<u8>,
    }

    impl TestCert {
        pub fn sha256(&self) -> String {
            fingerprint(&self.der)
        }

        pub fn acceptor(&self) -> TlsAcceptor {
            let cfg = rustls::ServerConfig::builder_with_provider(provider())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![self.der.clone()],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())),
                )
                .unwrap();
            TlsAcceptor::from(Arc::new(cfg))
        }
    }

    /// Kendinden imzalı sertifika; `days` negatifse süresi dolmuş olur.
    pub fn self_signed(names: &[&str], expired: bool) -> TestCert {
        let mut params =
            rcgen::CertificateParams::new(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                .unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, names[0]);
        if expired {
            params.not_before = rcgen::date_time_ymd(2020, 1, 1);
            params.not_after = rcgen::date_time_ymd(2021, 1, 1);
        }
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        TestCert {
            der: cert.der().clone(),
            key: key.serialize_der(),
        }
    }

    /// Tek bağlantı kabul eden TLS sunucusu: el sıkışırsa "* OK" yazar.
    async fn tls_server(cert: &TestCert) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = cert.acceptor();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(mut tls) = acceptor.accept(sock).await {
                        let _ = tls.write_all(b"* OK hazir\r\n").await;
                        let _ = tls.flush().await;
                    }
                });
            }
        });
        port
    }

    async fn connect_tls(host: &str, port: u16) -> AppResult<()> {
        let tcp = crate::conn::tcp("127.0.0.1", port).await?;
        let mut s = handshake(host, port, tcp).await?;
        let mut buf = [0u8; 4];
        s.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"* OK");
        Ok(())
    }

    #[test]
    fn parmak_izi_bicimi() {
        let fp = fingerprint(b"mailhere");
        assert_eq!(fp.len(), 32 * 3 - 1);
        assert!(fp.chars().all(|c| c == ':' || c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
        let plain = fp.replace(':', "").to_ascii_lowercase();
        assert_eq!(normalize_fingerprint(&plain).as_deref(), Some(fp.as_str()));
        assert_eq!(normalize_fingerprint("AB:CD"), None);
        assert_eq!(normalize_fingerprint(&"zz".repeat(32)), None);
        assert_eq!(host_key(" IMAP.Ornek.com. "), "imap.ornek.com");
        assert_eq!(host_key("[::1]"), "::1");
    }

    #[tokio::test]
    async fn kendinden_imzali_reddedilir_onaylaninca_kabul() {
        let cert = self_signed(&["localhost"], false);
        let port = tls_server(&cert).await;
        let err = connect_tls("localhost", port).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Untrusted, "{err:?}");
        assert!(err.message.contains("kendinden imzalı"), "{}", err.message);
        assert!(err.message.contains(&format!("localhost:{port}")));

        // Başka kapı için onay işe yaramaz; yanlış parmak izi de.
        trust(TrustedCert {
            host: "localhost".into(),
            port: 1,
            sha256: cert.sha256(),
        });
        trust(TrustedCert {
            host: "localhost".into(),
            port,
            sha256: fingerprint(b"baska"),
        });
        assert_eq!(
            connect_tls("localhost", port).await.unwrap_err().code,
            ErrorCode::Untrusted
        );

        // Doğru onay (küçük harf, ayraçsız yazılsa da) kabul edilir.
        trust(TrustedCert {
            host: "LOCALHOST".into(),
            port,
            sha256: cert.sha256().replace(':', "").to_ascii_lowercase(),
        });
        connect_tls("localhost", port).await.unwrap();
    }

    #[tokio::test]
    async fn inceleme_sorunlari_soyler() {
        let cert = self_signed(&["posta.ornek.test"], true);
        let port = tls_server(&cert).await;
        // 127.0.0.1 adı sertifikada yok: ad da uyuşmuyor.
        let info = inspect("127.0.0.1", port, Security::Tls).await.unwrap();
        assert_eq!(info.sha256, cert.sha256());
        assert_eq!(info.names, vec!["posta.ornek.test".to_string()]);
        assert!(info.subject.contains("posta.ornek.test"), "{}", info.subject);
        assert_eq!(info.subject, info.issuer);
        assert!(info.not_after.unwrap() < info.not_before.unwrap() + 400 * 86_400_000);
        for p in ["kendinden imzalı", "süresi dolmuş", "ad uyuşmuyor"] {
            assert!(info.problem.contains(p), "{}", info.problem);
        }
        assert!(!info.problem.contains("tanınmayan"));

        let err = connect_tls("127.0.0.1", port).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Untrusted);
        assert!(err.message.contains("süresi dolmuş"), "{}", err.message);

        assert_eq!(
            inspect("127.0.0.1", port, Security::None)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Invalid
        );
    }

    #[tokio::test]
    async fn imap_starttls_incelemesi() {
        let cert = self_signed(&["localhost"], false);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = cert.acceptor();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            sock.write_all(b"* OK [CAPABILITY IMAP4rev1 STARTTLS] hazir\r\n")
                .await
                .unwrap();
            let mut buf = [0u8; 64];
            let n = sock.read(&mut buf).await.unwrap();
            assert!(buf[..n].ends_with(b"STARTTLS\r\n"));
            sock.write_all(b"m1 OK baslat\r\n").await.unwrap();
            let _ = acceptor.accept(sock).await;
        });
        let info = inspect("localhost", port, Security::Starttls).await.unwrap();
        assert_eq!(info.sha256, cert.sha256());
        assert_eq!(info.problem, "kendinden imzalı");
    }

    #[test]
    fn bilinmeyen_kokle_imzali_zincir() {
        // Kökler arasında olmayan bir CA ile imzalanmış sertifika: tanınmayan kuruluş.
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Ornek Kok");
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::new(ca_params, ca_key);
        let leaf_key = rcgen::KeyPair::generate().unwrap();
        let mut leaf_params =
            rcgen::CertificateParams::new(vec!["posta.ornek.test".to_string()]).unwrap();
        leaf_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "posta.ornek.test");
        let leaf = leaf_params.signed_by(&leaf_key, &issuer).unwrap();
        let chain = vec![leaf.der().clone(), ca.der().clone()];
        let name = server_name("posta.ornek.test").unwrap();
        assert_eq!(problems(&chain, &name, UnixTime::now()), vec!["tanınmayan kuruluş"]);
    }
}
