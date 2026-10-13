//! JMAP HTTP istemcisi: oturum, API çağrıları, blob indirme/yükleme.
//!
//! Kimlik doğrulama HTTP Basic'tir; parola yalnız `Authorization` başlığında
//! gider. Şifresiz `http://` yalnız yerel test sunucularına (localhost)
//! açıktır.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use mh_core::{AppError, AppResult, ErrorCode};
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use reqwest::{RequestBuilder, StatusCode, Url};
use serde_json::{json, Value};

use crate::error::{JmapError, JmapResult};
use crate::types::{Session, CORE, MAIL, SUBMISSION};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Oturum ve API çağrılarının üst süresi.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Blob indirme/yükleme üst süresi.
const BLOB_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_REDIRECTS: usize = 5;

/// Sunucu girişi. `Debug` parolayı göstermez.
#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    password: String,
}

impl Credentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    pub(crate) fn password(&self) -> &str {
        &self.password
    }

    fn apply(&self, rb: RequestBuilder) -> RequestBuilder {
        rb.basic_auth(&self.username, Some(&self.password))
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"***")
            .finish()
    }
}

/// Tüm hesapların paylaştığı HTTP istemcisi (bağlantı havuzu). Yönlendirmeler
/// elle izlenir: kimlik bilgisi yalnız güvenli hedeflere yeniden gönderilir.
pub(crate) fn http() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(CONNECT_TIMEOUT)
                .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

fn tls_required() -> AppError {
    AppError::new(
        ErrorCode::Tls,
        "Şifresiz (http) JMAP bağlantısına yalnız yerel test sunucularında izin verilir. Adresi https:// ile yaz.",
    )
}

fn invalid_url() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "JMAP oturum adresi geçersiz. https://posta.alanadi.com/.well-known/jmap biçiminde yaz.",
    )
}

/// Adres güvenli mi: https ya da yerel makinede http.
pub(crate) fn check_url(url: &Url) -> AppResult<()> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if url.host_str().is_some_and(mh_sync::is_local) => Ok(()),
        "http" => Err(tls_required()),
        _ => Err(invalid_url()),
    }
}

/// Kullanıcının yazdığı oturum adresini düzenler: şema yoksa `https://`,
/// yol yoksa `/.well-known/jmap` eklenir.
pub fn normalize_session_url(input: &str) -> AppResult<Url> {
    let input = input.trim();
    if input.is_empty() {
        return Err(invalid_url());
    }
    let with_scheme = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let mut url = Url::parse(&with_scheme).map_err(|_| invalid_url())?;
    if url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid_url());
    }
    check_url(&url)?;
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/.well-known/jmap");
    }
    url.set_fragment(None);
    Ok(url)
}

/// RFC 6570 düzey 1 şablon genişletme: `{ad}` yerine yüzde kodlanmış değer.
pub fn expand_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &tail[..end];
        if let Some((_, v)) = vars.iter().find(|(k, _)| *k == name) {
            out.push_str(&encode(v));
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn encode(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// GET isteği; yönlendirmeleri izler. Kimlik bilgisi yönlendirmede yalnız
/// güvenli (https ya da yerel) hedefe yeniden gönderilir.
pub(crate) async fn get_following(
    url: Url,
    creds: Option<&Credentials>,
    accept: &str,
    limit: Duration,
) -> JmapResult<(reqwest::Response, Url)> {
    let client = http();
    let mut url = url;
    for _ in 0..=MAX_REDIRECTS {
        check_url(&url)?;
        let mut rb = client
            .get(url.clone())
            .header(ACCEPT, accept)
            .timeout(limit);
        if let Some(c) = creds {
            rb = c.apply(rb);
        }
        let resp = rb.send().await?;
        let status = resp.status();
        if status.is_redirection() {
            let next = resp
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| url.join(loc).ok())
                .ok_or_else(|| JmapError::Protocol("yönlendirme adresi yok".into()))?;
            url = next;
            continue;
        }
        return Ok((resp, url));
    }
    Err(JmapError::Protocol("çok fazla yönlendirme".into()))
}

fn status_error(status: StatusCode) -> JmapError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => JmapError::Auth,
        s => JmapError::Status(s.as_u16()),
    }
}

/// Oturum nesnesini getirir; (oturum, yönlendirmelerden sonraki adres).
pub async fn fetch_session(url: &Url, creds: Option<&Credentials>) -> JmapResult<(Session, Url)> {
    let (resp, final_url) =
        get_following(url.clone(), creds, "application/json", REQUEST_TIMEOUT).await?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status()));
    }
    let bytes = resp.bytes().await?;
    let session: Session = serde_json::from_slice(&bytes)
        .map_err(|e| JmapError::Protocol(format!("oturum çözülemedi: {e}")))?;
    Ok((session, final_url))
}

/// Bir JMAP hesabına bağlı istemci.
pub struct Client {
    creds: Credentials,
    session_url: Url,
    session: Session,
    api_url: Url,
    account_id: String,
    /// Sunucunun oturumu değişti; bir sonraki kullanımda yeniden alınmalı.
    stale: AtomicBool,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("session_url", &self.session_url.as_str())
            .field("account_id", &self.account_id)
            .finish_non_exhaustive()
    }
}

/// Tek bir yöntem çağrısı: (ad, bağımsız değişkenler).
pub type Call = (String, Value);

impl Client {
    /// Oturumu kimlik bilgisiyle alır ve posta hesabını seçer.
    pub async fn connect(session_url: &str, creds: Credentials) -> JmapResult<Self> {
        let url = normalize_session_url(session_url)?;
        let (session, final_url) = fetch_session(&url, Some(&creds)).await?;
        Self::from_session(final_url, session, creds)
    }

    pub(crate) fn from_session(
        session_url: Url,
        session: Session,
        creds: Credentials,
    ) -> JmapResult<Self> {
        if !session.capabilities.contains_key(CORE) {
            return Err(JmapError::Protocol("çekirdek yeteneği yok".into()));
        }
        if !session.capabilities.contains_key(MAIL) {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "Bu JMAP sunucusu e-posta hizmeti sunmuyor.",
            )
            .into());
        }
        let account_id = session
            .primary_accounts
            .get(MAIL)
            .cloned()
            .or_else(|| {
                session
                    .accounts
                    .iter()
                    .find(|(_, a)| a.account_capabilities.contains_key(MAIL))
                    .map(|(id, _)| id.clone())
            })
            .ok_or_else(|| {
                JmapError::App(AppError::new(
                    ErrorCode::Invalid,
                    "JMAP sunucusunda bu kullanıcıya ait posta hesabı bulunamadı.",
                ))
            })?;
        let api_url = session_url
            .join(&session.api_url)
            .map_err(|_| JmapError::Protocol("apiUrl geçersiz".into()))?;
        check_url(&api_url)?;
        Ok(Self {
            creds,
            session_url,
            session,
            api_url,
            account_id,
            stale: AtomicBool::new(false),
        })
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_url(&self) -> &Url {
        &self.session_url
    }

    pub(crate) fn credentials(&self) -> &Credentials {
        &self.creds
    }

    /// Sunucu oturumu değişti mi (yeniden bağlanmak gerekir)?
    pub fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Relaxed)
    }

    /// Gönderim yeteneği var mı?
    pub fn can_submit(&self) -> bool {
        self.session.capabilities.contains_key(SUBMISSION)
            && self.session.accounts.get(&self.account_id).is_none_or(|a| {
                a.account_capabilities.is_empty() || a.account_capabilities.contains_key(SUBMISSION)
            })
    }

    /// Tek `*/get` çağrısında istenebilecek en fazla nesne.
    pub fn max_get(&self) -> usize {
        self.session.core_limit("maxObjectsInGet", 500).min(500) as usize
    }

    /// Tek istekteki en fazla yöntem çağrısı.
    pub fn max_calls(&self) -> usize {
        self.session.core_limit("maxCallsInRequest", 16).min(64) as usize
    }

    fn resolve(&self, template_url: &str) -> JmapResult<Url> {
        let url = self
            .session_url
            .join(template_url)
            .map_err(|_| JmapError::Protocol("adres şablonu geçersiz".into()))?;
        check_url(&url)?;
        Ok(url)
    }

    /// Yöntem çağrılarını tek istekte gönderir. Dönen liste sunucunun sırasıyla
    /// tüm yanıtlardır: (ad, bağımsız değişkenler, çağrı kimliği). Örtük
    /// yanıtlar (ör. `onSuccessUpdateEmail`) aynı çağrı kimliğiyle gelir.
    pub async fn request(&self, calls: &[Call]) -> JmapResult<Vec<(String, Value, String)>> {
        let mut using = vec![CORE, MAIL];
        if calls
            .iter()
            .any(|(m, _)| m.starts_with("EmailSubmission/") || m.starts_with("Identity/"))
        {
            using.push(SUBMISSION);
        }
        let method_calls: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(i, (m, args))| json!([m, args, i.to_string()]))
            .collect();
        let body = json!({ "using": using, "methodCalls": method_calls });
        let rb = http()
            .post(self.api_url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .body(serde_json::to_vec(&body).unwrap_or_default());
        let resp = self.creds.apply(rb).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            // İstek düzeyi hata (RFC 7807 sorun ayrıntısı).
            if let Ok(problem) = serde_json::from_slice::<Value>(&bytes) {
                if let Some(kind) = problem.get("type").and_then(Value::as_str) {
                    if status != StatusCode::UNAUTHORIZED {
                        return Err(JmapError::Method {
                            kind: kind.rsplit(':').next().unwrap_or(kind).to_owned(),
                            description: problem
                                .get("detail")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        });
                    }
                }
            }
            return Err(status_error(status));
        }
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|e| JmapError::Protocol(format!("yanıt çözülemedi: {e}")))?;
        if let (Some(now), Some(known)) = (
            v.get("sessionState").and_then(Value::as_str),
            self.session.state.as_deref(),
        ) {
            if now != known {
                self.stale.store(true, Ordering::Relaxed);
            }
        }
        let list = v
            .get("methodResponses")
            .and_then(Value::as_array)
            .ok_or_else(|| JmapError::Protocol("methodResponses yok".into()))?;
        let mut out = Vec::with_capacity(list.len());
        for r in list {
            let (Some(name), Some(args), Some(id)) = (
                r.get(0).and_then(Value::as_str),
                r.get(1),
                r.get(2).and_then(Value::as_str),
            ) else {
                return Err(JmapError::Protocol("yöntem yanıtı bozuk".into()));
            };
            out.push((name.to_owned(), args.clone(), id.to_owned()));
        }
        Ok(out)
    }

    /// Çağrıları gönderir; her çağrının birincil yanıtını sırayla döner.
    /// Herhangi bir yöntem hata verirse ilk hata döner.
    pub async fn calls(&self, calls: &[Call]) -> JmapResult<Vec<Value>> {
        let responses = self.request(calls).await?;
        let mut out = Vec::with_capacity(calls.len());
        for i in 0..calls.len() {
            let id = i.to_string();
            let (name, args, _) = responses
                .iter()
                .find(|(_, _, cid)| *cid == id)
                .ok_or_else(|| JmapError::Protocol(format!("{id} numaralı yanıt yok")))?;
            if name == "error" {
                return Err(method_error(args));
            }
            out.push(args.clone());
        }
        Ok(out)
    }

    /// Tek yöntem çağrısı.
    pub async fn call(&self, method: &str, args: Value) -> JmapResult<Value> {
        self.calls(&[(method.to_owned(), args)])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| JmapError::Protocol("yanıt yok".into()))
    }

    /// Blob'u indirir (ör. ham RFC 5322 posta).
    pub async fn download(&self, blob_id: &str, name: &str, ty: &str) -> JmapResult<Vec<u8>> {
        let url = self.resolve(&expand_template(
            &self.session.download_url,
            &[
                ("accountId", &self.account_id),
                ("blobId", blob_id),
                ("name", name),
                ("type", ty),
            ],
        ))?;
        let (resp, _) = get_following(url, Some(&self.creds), "*/*", BLOB_TIMEOUT).await?;
        match resp.status() {
            s if s.is_success() => Ok(resp.bytes().await?.to_vec()),
            StatusCode::NOT_FOUND => Err(AppError::new(
                ErrorCode::NotFound,
                "Posta sunucuda bulunamadı; başka bir yerden silinmiş olabilir.",
            )
            .into()),
            s => Err(status_error(s)),
        }
    }

    /// Blob yükler; blob kimliğini döner.
    pub async fn upload(&self, bytes: Vec<u8>, ty: &str) -> JmapResult<String> {
        let url = self.resolve(&expand_template(
            &self.session.upload_url,
            &[("accountId", &self.account_id)],
        ))?;
        let rb = http()
            .post(url)
            .header(CONTENT_TYPE, ty)
            .header(ACCEPT, "application/json")
            .timeout(BLOB_TIMEOUT)
            .body(bytes);
        let resp = self.creds.apply(rb).send().await?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status()));
        }
        let v: Value = serde_json::from_slice(&resp.bytes().await?)
            .map_err(|e| JmapError::Protocol(format!("yükleme yanıtı çözülemedi: {e}")))?;
        v.get("blobId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| JmapError::Protocol("blobId yok".into()))
    }

    /// Durum değişikliği akışının adresi (yoksa `None`).
    pub fn event_source_url(&self, types: &str, ping_secs: u32) -> Option<Url> {
        let template = self.session.event_source_url.as_deref()?;
        let ping = ping_secs.to_string();
        self.resolve(&expand_template(
            template,
            &[("types", types), ("closeafter", "no"), ("ping", &ping)],
        ))
        .ok()
    }
}

pub(crate) fn method_error(args: &Value) -> JmapError {
    JmapError::Method {
        kind: args
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("serverFail")
            .to_owned(),
        description: args
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

/// Kimlik bilgisinin doğruluğunu oturum alarak sınar (hesap eklerken).
pub async fn test_login(session_url: &str, username: &str, password: &str) -> AppResult<Url> {
    let client = Client::connect(session_url, Credentials::new(username, password))
        .await
        .map_err(|e| e.to_app())?;
    Ok(client.session_url().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oturum_adresi_duzenlenir() {
        assert_eq!(
            normalize_session_url("posta.alanadi.com").unwrap().as_str(),
            "https://posta.alanadi.com/.well-known/jmap"
        );
        assert_eq!(
            normalize_session_url(" https://posta.alanadi.com/jmap/session ")
                .unwrap()
                .as_str(),
            "https://posta.alanadi.com/jmap/session"
        );
        assert_eq!(
            normalize_session_url("http://127.0.0.1:8080")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:8080/.well-known/jmap"
        );
        assert_eq!(
            normalize_session_url("http://posta.alanadi.com")
                .unwrap_err()
                .code,
            ErrorCode::Tls
        );
        for bad in ["", "ftp://a.com", "https://k:p@a.com/x", "https://"] {
            assert!(normalize_session_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sablon_genisletme() {
        let t = "https://a.com/jmap/download/{accountId}/{blobId}/{name}?accept={type}";
        assert_eq!(
            expand_template(
                t,
                &[
                    ("accountId", "u1"),
                    ("blobId", "B/9"),
                    ("name", "posta.eml"),
                    ("type", "message/rfc822")
                ]
            ),
            "https://a.com/jmap/download/u1/B%2F9/posta.eml?accept=message%2Frfc822"
        );
        assert_eq!(
            expand_template(
                "/es?types={types}&closeafter={closeafter}&ping={ping}",
                &[
                    ("types", "Email,Mailbox"),
                    ("closeafter", "no"),
                    ("ping", "60")
                ]
            ),
            "/es?types=Email%2CMailbox&closeafter=no&ping=60"
        );
        assert_eq!(expand_template("/x/{yok}/y", &[]), "/x//y");
        assert_eq!(expand_template("/x/{acik", &[]), "/x/{acik");
    }

    #[test]
    fn parola_gorunmez() {
        let c = Credentials::new("furkan", "cok-gizli");
        assert!(!format!("{c:?}").contains("cok-gizli"));
    }
}
