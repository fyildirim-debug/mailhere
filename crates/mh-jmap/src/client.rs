//! JMAP HTTP istemcisi: oturum, API çağrıları, blob indirme/yükleme.
//!
//! Kimlik doğrulama HTTP Basic'tir; parola yalnız `Authorization` başlığında
//! ve yalnız kullanıcının yazdığı oturum adresinin kökenine (şema + sunucu +
//! kapı) gider. Başka kökene yönlendirme izlenmez; oturumun bildirdiği API,
//! indirme, yükleme ve durum akışı adresleri de aynı kökende olmalıdır.
//! Şifresiz `http://` yalnız yerel test sunucularına (localhost) açıktır.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use mh_core::{AppError, AppResult, ErrorCode};
use mh_sync::tls::HttpsTrust;
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
/// Oturum nesnesi ve API yanıtlarının üst sınırı: kötü niyetli ya da bozuk
/// sunucu belleği şişirmesin.
pub(crate) const MAX_JSON: usize = 16 * 1024 * 1024;
/// İndirilen blob'un (ham posta) üst sınırı.
pub(crate) const MAX_BLOB: usize = 64 * 1024 * 1024;
const EVENT_SOURCE: &str = "durum akışı adresi (eventSourceUrl)";

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

/// Yönlendirmeler elle izlenir: kimlik bilgisi yalnız güvenli hedeflere
/// yeniden gönderilir.
fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")))
}

/// Yerel düz http (test sunucuları) için ortak istemci.
fn plain() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| builder().build().unwrap_or_else(|_| reqwest::Client::new()))
        .clone()
}

type Origins = Mutex<HashMap<(String, u16), (reqwest::Client, Arc<HttpsTrust>)>>;

fn origins() -> &'static Origins {
    static ORIGINS: OnceLock<Origins> = OnceLock::new();
    ORIGINS.get_or_init(Origins::default)
}

fn origin_key(url: &Url) -> Option<(String, u16)> {
    Some((
        url.host_str()?.to_ascii_lowercase(),
        url.port_or_known_default()?,
    ))
}

/// Adresin kökenine (sunucu + kapı) ait HTTP istemcisi; bağlantı havuzu
/// kökene özeldir. https'te sertifika webpki ile, olmazsa yalnız kullanıcının
/// o sunucu + kapı için onayladığı parmak iziyle kabul edilir.
pub(crate) fn http_for(url: &Url) -> reqwest::Client {
    if url.scheme() != "https" {
        return plain();
    }
    let Some(key) = origin_key(url) else {
        return plain();
    };
    let mut map = origins().lock().unwrap_or_else(|p| p.into_inner());
    if let Some((client, _)) = map.get(&key) {
        return client.clone();
    }
    let trust = Arc::new(HttpsTrust::new(&key.0, key.1));
    let built = trust
        .client_config()
        .ok()
        .and_then(|cfg| builder().use_preconfigured_tls(cfg).build().ok());
    match built {
        Some(client) => {
            map.insert(key, (client.clone(), trust));
            client
        }
        // Kurulamazsa yalnız gömülü köklerle doğrulayan istemci (onay yok).
        None => plain(),
    }
}

/// İstek, sunucu sertifikası güvenilmediği için düştüyse `Untrusted` hatası.
pub(crate) fn untrusted_for(url: Option<&Url>) -> Option<AppError> {
    let key = origin_key(url?)?;
    let map = origins().lock().unwrap_or_else(|p| p.into_inner());
    map.get(&key)?.1.untrusted()
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

/// İki adres aynı kökende mi (şema + sunucu + kapı)?
pub(crate) fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme() && origin_key(a).is_some() && origin_key(a) == origin_key(b)
}

/// Adres bu bilgisayarı mı gösteriyor (localhost, 127.0.0.0/8, ::1, 0.0.0.0)?
fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6.parse::<std::net::Ipv6Addr>().is_ok_and(|ip| {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_loopback() || v4.is_unspecified())
        });
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return ip.is_loopback() || ip.is_unspecified();
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost")
}

/// Hata metninde gösterilecek adres: kullanıcı bilgisi ve parça atılır,
/// çok uzunsa kısaltılır.
fn shown(url: &Url) -> String {
    let mut u = url.clone();
    let _ = u.set_username("");
    let _ = u.set_password(None);
    u.set_fragment(None);
    let s = u.to_string();
    match s.char_indices().nth(200) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

/// Yönlendirme izlenebilir mi? https'ten http'ye inilmez, uzak sunucudan bu
/// bilgisayara dönülmez; kimlik bilgisiyle yapılan istekte başka kökene
/// gidilmez (parola yalnız kullanıcının yazdığı sunucuya gider).
fn check_redirect(from: &Url, to: &Url, with_credentials: bool) -> AppResult<()> {
    if from.scheme() == "https" && to.scheme() != "https" {
        return Err(AppError::new(
            ErrorCode::Tls,
            "JMAP sunucusu güvenli (https) bağlantıyı şifresiz (http) bir adrese yönlendirdi; güvenlik için izlenmedi. Sunucunun https ile başlayan oturum adresini yaz.",
        ));
    }
    if is_loopback(to) && !is_loopback(from) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!(
                "JMAP sunucusu bağlantıyı bu bilgisayardaki bir adrese ({}) yönlendirdi; güvenlik için izlenmedi. Oturum adresini denetle.",
                shown(to)
            ),
        ));
    }
    if with_credentials && !same_origin(from, to) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!(
                "JMAP sunucusu oturum adresini başka bir sunucuya yönlendiriyor: {}. Parolan oraya gönderilmedi. Bu adres doğruysa oturum adresi olarak doğrudan onu yaz.",
                shown(to)
            ),
        ));
    }
    Ok(())
}

/// Oturumun bildirdiği adres oturumla aynı kökende mi? Değilse kimlik
/// bilgisi oraya gönderilmez.
fn check_same_origin(session_url: &Url, url: &Url, what: &str) -> AppResult<()> {
    if same_origin(session_url, url) {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::Invalid,
        format!(
            "JMAP sunucusu {what} için oturum adresinden farklı bir sunucu bildirdi ({}). Güvenlik için parola yalnız oturum adresinin sunucusuna ({}) gönderilir. Oturum adresini denetle ya da sunucu yöneticisine danış.",
            shown(url),
            session_url.origin().ascii_serialization()
        ),
    ))
}

/// Yanıt gövdesini en fazla `limit` bayt okur; sınırı aşan yanıtın kalanı
/// okunmaz.
pub(crate) async fn read_limited(mut resp: reqwest::Response, limit: usize) -> JmapResult<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > limit as u64) {
        return Err(JmapError::TooLarge(limit));
    }
    let mut out = Vec::with_capacity(
        resp.content_length()
            .map_or(0, |n| n as usize)
            .min(1024 * 1024),
    );
    while let Some(chunk) = resp.chunk().await? {
        if out.len() + chunk.len() > limit {
            return Err(JmapError::TooLarge(limit));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
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

/// GET isteği; yönlendirmeleri izler (bkz. `check_redirect`). Kimlik bilgisi
/// yalnız ilk adresin kökenindeki adreslere gönderilir: başka kökene
/// yönlendirmede istek durur ve kullanıcıdan son adresi doğrudan yazması
/// istenir. Kimliksiz istek (keşif) başka kökene geçebilir.
pub(crate) async fn get_following(
    url: Url,
    creds: Option<&Credentials>,
    accept: &str,
    limit: Duration,
) -> JmapResult<(reqwest::Response, Url)> {
    let mut url = url;
    for _ in 0..=MAX_REDIRECTS {
        check_url(&url)?;
        let mut rb = http_for(&url)
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
            // Kimlikli istekte her adım bir öncekiyle, dolayısıyla ilk adresle
            // aynı kökende kalır.
            check_redirect(&url, &next, creds.is_some())?;
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
    let bytes = read_limited(resp, MAX_JSON).await?;
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
        check_same_origin(&session_url, &api_url, "API adresi (apiUrl)")?;
        let client = Self {
            creds,
            session_url,
            session,
            api_url,
            account_id,
            stale: AtomicBool::new(false),
        };
        // İndirme ve yükleme şablonları da baştan denetlenir: başka kökene
        // giden hesap hiç açılmaz. (Her kullanımda `resolve` yeniden denetler.)
        client.download_url("x", "x", "x")?;
        client.upload_url()?;
        if client.session.event_source_url.is_some()
            && client.event_source_url("Email,Mailbox", 60).is_none()
        {
            // Akış isteğe bağlıdır; kullanılamazsa yoklamayla eşitlenir.
            tracing::warn!(
                "durum akışı adresi geçersiz ya da oturumla aynı kökende değil; kullanılmayacak"
            );
        }
        Ok(client)
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

    /// Oturumun bildirdiği (genişletilmiş) adres: geçerli, güvenli ve oturumla
    /// aynı kökende olmalı.
    fn resolve(&self, template_url: &str, what: &str) -> AppResult<Url> {
        let url = self.session_url.join(template_url).map_err(|_| {
            AppError::new(
                ErrorCode::Invalid,
                format!("JMAP sunucusunun bildirdiği {what} geçersiz. Oturum adresini denetle."),
            )
        })?;
        check_url(&url)?;
        check_same_origin(&self.session_url, &url, what)?;
        Ok(url)
    }

    fn download_url(&self, blob_id: &str, name: &str, ty: &str) -> AppResult<Url> {
        self.resolve(
            &expand_template(
                &self.session.download_url,
                &[
                    ("accountId", &self.account_id),
                    ("blobId", blob_id),
                    ("name", name),
                    ("type", ty),
                ],
            ),
            "indirme adresi (downloadUrl)",
        )
    }

    fn upload_url(&self) -> AppResult<Url> {
        self.resolve(
            &expand_template(&self.session.upload_url, &[("accountId", &self.account_id)]),
            "yükleme adresi (uploadUrl)",
        )
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
        if calls
            .iter()
            .any(|(m, _)| m.starts_with("VacationResponse/"))
        {
            using.push(crate::vacation::VACATION);
        }
        let method_calls: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(i, (m, args))| json!([m, args, i.to_string()]))
            .collect();
        let body = json!({ "using": using, "methodCalls": method_calls });
        let rb = http_for(&self.api_url)
            .post(self.api_url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .body(serde_json::to_vec(&body).unwrap_or_default());
        let resp = self.creds.apply(rb).send().await?;
        let status = resp.status();
        let bytes = read_limited(resp, MAX_JSON).await?;
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
        let url = self.download_url(blob_id, name, ty)?;
        let (resp, _) = get_following(url, Some(&self.creds), "*/*", BLOB_TIMEOUT).await?;
        match resp.status() {
            s if s.is_success() => read_limited(resp, MAX_BLOB).await,
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
        let url = self.upload_url()?;
        let rb = http_for(&url)
            .post(url)
            .header(CONTENT_TYPE, ty)
            .header(ACCEPT, "application/json")
            .timeout(BLOB_TIMEOUT)
            .body(bytes);
        let resp = self.creds.apply(rb).send().await?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status()));
        }
        let v: Value = serde_json::from_slice(&read_limited(resp, MAX_JSON).await?)
            .map_err(|e| JmapError::Protocol(format!("yükleme yanıtı çözülemedi: {e}")))?;
        v.get("blobId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| JmapError::Protocol("blobId yok".into()))
    }

    /// Durum değişikliği akışının adresi (yoksa ya da oturumla aynı kökende
    /// değilse `None`).
    pub fn event_source_url(&self, types: &str, ping_secs: u32) -> Option<Url> {
        let template = self.session.event_source_url.as_deref()?;
        let ping = ping_secs.to_string();
        self.resolve(
            &expand_template(
                template,
                &[("types", types), ("closeafter", "no"), ("ping", &ping)],
            ),
            EVENT_SOURCE,
        )
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
pub(crate) mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";

    /// Her isteğe `reply` yazan düz HTTP sunucusu; (kök adres, gelen
    /// isteklerin başlıkları).
    pub(crate) async fn raw_server(reply: String) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (seen, reply) = (s2.clone(), reply.clone());
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    seen.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                    let _ = sock.write_all(reply.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (base, seen)
    }

    /// Kendinden imzalı sertifikalı, her isteğe `reply` yazan HTTPS sunucusu;
    /// (kapı, sertifika parmak izi).
    async fn https_server(reply: String) -> (u16, String) {
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

        let mut params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()]).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "127.0.0.1");
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        let sha = mh_sync::tls::fingerprint(cert.der());
        let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let (acceptor, reply) = (acceptor.clone(), reply.clone());
                tokio::spawn(async move {
                    if let Ok(mut tls) = acceptor.accept(sock).await {
                        let mut buf = [0u8; 4096];
                        let _ = tls.read(&mut buf).await;
                        let _ = tls.write_all(reply.as_bytes()).await;
                        let _ = tls.shutdown().await;
                    }
                });
            }
        });
        (port, sha)
    }

    #[test]
    fn kokenler_ve_yerel_adresler() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(same_origin(
            &u("https://Posta.Ornek.com/a"),
            &u("https://posta.ornek.com:443/b?c")
        ));
        for other in [
            "http://posta.ornek.com/a",
            "https://posta.ornek.com:8443/a",
            "https://ornek.com/a",
            "https://posta.ornek.com.saldirgan.net/a",
        ] {
            assert!(
                !same_origin(&u("https://posta.ornek.com/"), &u(other)),
                "{other}"
            );
        }
        for local in [
            "http://localhost:1/",
            "http://LOCALHOST./",
            "https://a.localhost/",
            "http://127.0.0.1/",
            "http://127.9.9.9/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::]/",
        ] {
            assert!(is_loopback(&u(local)), "{local}");
        }
        for remote in [
            "https://ornek.com/",
            "http://10.0.0.1/",
            "http://[2001:db8::1]/",
        ] {
            assert!(!is_loopback(&u(remote)), "{remote}");
        }
    }

    #[test]
    fn yonlendirme_kurallari() {
        let u = |s: &str| Url::parse(s).unwrap();
        let from = u("https://posta.ornek.com/.well-known/jmap");
        // Aynı köken: kimlikle de izlenir.
        check_redirect(&from, &u("https://posta.ornek.com/jmap/session"), true).unwrap();
        // Başka köken: kimlikle durur, adres kullanıcıya gösterilir.
        let e = check_redirect(&from, &u("https://api.baska.com/jmap/session"), true).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("https://api.baska.com/jmap/session"));
        // Kimliksiz (keşif) başka kökene gidebilir.
        check_redirect(&from, &u("https://api.baska.com/jmap/session"), false).unwrap();
        // https'ten http'ye hiç inilmez.
        for creds in [true, false] {
            let e = check_redirect(&from, &u("http://posta.ornek.com/jmap"), creds).unwrap_err();
            assert_eq!(e.code, ErrorCode::Tls);
        }
        // Uzak sunucudan bu bilgisayara dönülmez (https bile olsa).
        for to in ["https://127.0.0.1:8443/jmap", "https://localhost/jmap"] {
            let e = check_redirect(&from, &u(to), false).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid, "{to}");
        }
        // Yerel test sunucusunda yerel yönlendirme serbest.
        check_redirect(&u("http://127.0.0.1:1/a"), &u("http://127.0.0.1:1/b"), true).unwrap();
        // Hata metninde kullanıcı bilgisi gösterilmez.
        let e = check_redirect(&from, &u("https://k:p@api.baska.com/s#x"), true).unwrap_err();
        assert!(!e.message.contains("k:p"), "{}", e.message);
    }

    #[tokio::test]
    async fn https_ten_http_ye_yonlendirmede_kimlik_gitmez() {
        let (target, seen) = raw_server(OK.into()).await;
        let (port, sha) = https_server(format!(
            "HTTP/1.1 302 Found\r\nlocation: {target}/jmap/session\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        ))
        .await;
        mh_sync::tls::trust(mh_core::TrustedCert {
            host: "127.0.0.1".into(),
            port,
            sha256: sha,
        });
        let url = Url::parse(&format!("https://127.0.0.1:{port}/.well-known/jmap")).unwrap();
        let creds = Credentials::new("furkan", "gizli");
        for c in [Some(&creds), None] {
            let err = get_following(url.clone(), c, "application/json", REQUEST_TIMEOUT)
                .await
                .unwrap_err()
                .to_app();
            assert_eq!(err.code, ErrorCode::Tls, "{}", err.message);
        }
        assert!(
            seen.lock().unwrap().is_empty(),
            "http adresine istek gitmez"
        );
    }

    #[tokio::test]
    async fn yanit_govdesi_akista_sinirlanir() {
        // Uzunluk bildirmeyen, bitmeyen gövde.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!(
            "http://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        ))
        .unwrap();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock
                        .write_all(b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n")
                        .await;
                    let chunk = vec![b'x'; 64 * 1024];
                    while sock.write_all(&chunk).await.is_ok() {}
                });
            }
        });
        let resp = http_for(&url).get(url.clone()).send().await.unwrap();
        let err = tokio::time::timeout(Duration::from_secs(5), read_limited(resp, 256 * 1024))
            .await
            .expect("sınır aşılınca hemen döner")
            .unwrap_err();
        assert!(matches!(err, JmapError::TooLarge(n) if n == 256 * 1024));

        let (base, _) = raw_server(
            "HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nmerha".into(),
        )
        .await;
        let url = Url::parse(&base).unwrap();
        let resp = http_for(&url).get(url.clone()).send().await.unwrap();
        assert_eq!(read_limited(resp, 5).await.unwrap(), b"merha");
        let resp = http_for(&url).get(url.clone()).send().await.unwrap();
        assert!(matches!(
            read_limited(resp, 4).await,
            Err(JmapError::TooLarge(4))
        ));
    }

    fn session(api: &str, download: &str, upload: &str, es: Option<&str>) -> Session {
        serde_json::from_value(json!({
            "capabilities": { CORE: {}, MAIL: {} },
            "accounts": { "A": { "name": "a", "accountCapabilities": { MAIL: {} } } },
            "primaryAccounts": { MAIL: "A" },
            "apiUrl": api,
            "downloadUrl": download,
            "uploadUrl": upload,
            "eventSourceUrl": es,
        }))
        .unwrap()
    }

    #[test]
    fn oturum_adresleri_ayni_kokende_olmali() {
        let base = Url::parse("https://posta.ornek.com/.well-known/jmap").unwrap();
        let creds = || Credentials::new("furkan", "gizli");
        let ok = session(
            "/jmap/api",
            "https://posta.ornek.com/d/{accountId}/{blobId}/{name}?t={type}",
            "/u/{accountId}/",
            Some("/es?types={types}&closeafter={closeafter}&ping={ping}"),
        );
        let c = Client::from_session(base.clone(), ok, creds()).unwrap();
        assert_eq!(
            c.event_source_url("Email", 5).unwrap().as_str(),
            "https://posta.ornek.com/es?types=Email&closeafter=no&ping=5"
        );
        let bad = [
            ("https://baska.com/api", "/d/{blobId}", "/u/"),
            ("/api", "https://baska.com/d/{blobId}", "/u/"),
            ("/api", "/d/{blobId}", "https://posta.ornek.com:8443/u/"),
            // https oturumdan yerel makineye (http ya da https) gidilmez.
            ("http://127.0.0.1:8080/api", "/d/{blobId}", "/u/"),
            ("/api", "https://localhost/d/{blobId}", "/u/"),
        ];
        for (api, download, upload) in bad {
            let err =
                Client::from_session(base.clone(), session(api, download, upload, None), creds())
                    .unwrap_err()
                    .to_app();
            assert_eq!(err.code, ErrorCode::Invalid, "{api} {download} {upload}");
            assert!(
                err.message.contains("https://posta.ornek.com"),
                "{}",
                err.message
            );
        }
        // Başka kökendeki durum akışı kullanılmaz; hesap yine açılır.
        let c = Client::from_session(
            base,
            session("/api", "/d/{blobId}", "/u/", Some("https://baska.com/es")),
            creds(),
        )
        .unwrap();
        assert!(c.event_source_url("Email", 5).is_none());
    }

    #[tokio::test]
    async fn https_kendinden_imzali_onaysiz_reddedilir_onayla_gecer() {
        let (port, sha) = https_server(OK.into()).await;
        let url = Url::parse(&format!("https://127.0.0.1:{port}/.well-known/jmap")).unwrap();

        let err = http_for(&url).get(url.clone()).send().await.unwrap_err();
        let app = JmapError::Http(err).to_app();
        assert_eq!(app.code, ErrorCode::Untrusted, "{}", app.message);
        assert!(app.message.contains("kendinden imzalı"), "{}", app.message);

        // Kullanıcı bu sunucu + kapı için parmak izini onaylar.
        mh_sync::tls::trust(mh_core::TrustedCert {
            host: "127.0.0.1".into(),
            port,
            sha256: sha,
        });
        let resp = http_for(&url).get(url).send().await.unwrap();
        assert_eq!(resp.text().await.unwrap(), "ok");
    }

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
