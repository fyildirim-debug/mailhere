//! Testler için süreç içi sahte OAuth sağlayıcısı: yetkilendirme, jeton,
//! userinfo ve (Dovecot `oauth2` passdb için) introspection uç noktaları.
//!
//! Yalnız `testing` özelliğiyle ya da bu crate'in kendi testlerinde derlenir.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use mh_core::AuthKind;
use reqwest::Url;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::identity::unsigned_id_token;
use crate::pkce::challenge_for;
use crate::Provider;

/// Sahte sunucuya gelen istek.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    /// Küçük harfli başlık adları.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// `application/x-www-form-urlencoded` gövde.
    pub fn form(&self) -> HashMap<String, String> {
        Url::parse(&format!(
            "http://x/?{}",
            String::from_utf8_lossy(&self.body)
        ))
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default()
    }

    pub fn bearer(&self) -> Option<&str> {
        self.headers
            .get("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn json(status: u16, v: serde_json::Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: v.to_string().into_bytes(),
        }
    }

    pub fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            headers: vec![("Location".into(), location.into())],
            body: Vec::new(),
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

type Handler = Arc<dyn Fn(HttpRequest) -> HttpResponse + Send + Sync>;

/// Her isteği bir işleyiciye veren küçük HTTP/1.1 sunucusu.
pub struct FakeHttp {
    pub port: u16,
    task: JoinHandle<()>,
}

impl Drop for FakeHttp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeHttp {
    /// `port` 0 ise boş bir kapı seçilir.
    pub async fn start(port: u16, handler: Handler) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .expect("sahte HTTP sunucusu dinleyemedi");
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let handler = handler.clone();
                tokio::spawn(async move {
                    let Some(req) = read_request(&mut sock).await else {
                        return;
                    };
                    let resp = handler(req);
                    let reason = match resp.status {
                        200 => "OK",
                        302 => "Found",
                        400 => "Bad Request",
                        401 => "Unauthorized",
                        _ => "Other",
                    };
                    let mut out = format!("HTTP/1.1 {} {reason}\r\n", resp.status);
                    for (k, v) in &resp.headers {
                        out.push_str(&format!("{k}: {v}\r\n"));
                    }
                    out.push_str(&format!(
                        "Content-Length: {}\r\nConnection: close\r\n\r\n",
                        resp.body.len()
                    ));
                    let mut bytes = out.into_bytes();
                    bytes.extend_from_slice(&resp.body);
                    let _ = sock.write_all(&bytes).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { port, task }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Option<HttpRequest> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let url = Url::parse(&format!("http://x{target}")).ok()?;
    Some(HttpRequest {
        method,
        path: url.path().to_owned(),
        query: url.query_pairs().into_owned().collect(),
        headers,
        body,
    })
}

/// Sahte sağlayıcının durumu; testler doğrudan okuyup değiştirebilir.
#[derive(Debug)]
pub struct FakeState {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub email: String,
    pub name: Option<String>,
    /// Introspection yanıtındaki kullanıcı adı (Dovecot için).
    pub username: String,
    /// Yetkilendirme ekranında "reddet".
    pub deny: bool,
    /// Kod değişiminde `id_token` verilsin mi (yoksa userinfo kullanılır).
    pub id_token: bool,
    /// Kod değişiminde yenileme jetonu verilsin mi.
    pub issue_refresh: bool,
    /// Yenilemede yeni yenileme jetonu verilsin mi (Microsoft gibi).
    pub rotate_refresh: bool,
    pub expires_in: u64,
    /// Son yetkilendirme isteğinin parametreleri.
    pub last_authorize: Option<HashMap<String, String>>,
    /// kod → (code_challenge, redirect_uri)
    pub codes: HashMap<String, (String, String)>,
    pub refresh_tokens: HashSet<String>,
    pub access_tokens: HashSet<String>,
    pub token_requests: Vec<HashMap<String, String>>,
    pub counter: u32,
}

/// Süreç içi sahte OAuth sağlayıcısı.
pub struct FakeProvider {
    pub http: FakeHttp,
    pub state: Arc<Mutex<FakeState>>,
}

impl FakeProvider {
    pub async fn start(email: &str) -> Self {
        Self::start_on(0, email).await
    }

    /// Sabit kapıda başlatır (ör. Dovecot ayarında yazılı introspection adresi).
    pub async fn start_on(port: u16, email: &str) -> Self {
        let state = Arc::new(Mutex::new(FakeState {
            client_id: "sahte-istemci".into(),
            client_secret: None,
            email: email.into(),
            name: Some("Furkan Yıldırım".into()),
            username: email.into(),
            deny: false,
            id_token: true,
            issue_refresh: true,
            rotate_refresh: false,
            expires_in: 3600,
            last_authorize: None,
            codes: HashMap::new(),
            refresh_tokens: HashSet::new(),
            access_tokens: HashSet::new(),
            token_requests: Vec::new(),
            counter: 0,
        }));
        let s = state.clone();
        let http = FakeHttp::start(port, Arc::new(move |req| handle(&s, req))).await;
        Self { http, state }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bu sunucuya yönelmiş sağlayıcı ayarı.
    pub fn provider(&self, kind: AuthKind) -> Provider {
        let (client_id, client_secret) = {
            let st = self.lock();
            (st.client_id.clone(), st.client_secret.clone())
        };
        let mut p = match kind {
            AuthKind::Microsoft => Provider::microsoft(client_id),
            _ => Provider::google(client_id, client_secret),
        };
        p.authorize_url = self.http.url("/authorize");
        p.token_url = self.http.url("/token");
        if p.userinfo_url.is_some() {
            p.userinfo_url = Some(self.http.url("/userinfo"));
        }
        p
    }

    /// Girişi atlayıp doğrudan geçerli bir yenileme jetonu verir.
    pub fn issue_refresh_token(&self) -> String {
        let mut st = self.lock();
        st.counter += 1;
        let t = format!("yenileme-{}", st.counter);
        st.refresh_tokens.insert(t.clone());
        t
    }

    /// Doğrudan geçerli bir erişim jetonu verir.
    pub fn issue_access_token(&self) -> String {
        let mut st = self.lock();
        st.counter += 1;
        let t = format!("erisim-{}", st.counter);
        st.access_tokens.insert(t.clone());
        t
    }

    /// Kullanıcı erişimi kaldırdı: yenileme jetonu artık `invalid_grant`.
    pub fn revoke(&self, refresh_token: &str) {
        self.lock().refresh_tokens.remove(refresh_token);
    }

    /// Verilen tüm erişim jetonlarını geçersiz kılar.
    pub fn expire_access_tokens(&self) {
        self.lock().access_tokens.clear();
    }

    /// Yenileme isteği sayısı.
    pub fn refresh_count(&self) -> usize {
        self.lock()
            .token_requests
            .iter()
            .filter(|f| f.get("grant_type").map(String::as_str) == Some("refresh_token"))
            .count()
    }
}

fn oauth_error(code: &str) -> HttpResponse {
    HttpResponse::json(
        400,
        json!({"error": code, "error_description": "sahte sağlayıcı"}),
    )
}

fn handle(state: &Mutex<FakeState>, req: HttpRequest) -> HttpResponse {
    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/authorize") => authorize(&mut st, &req),
        ("POST", "/token") => token(&mut st, &req),
        ("GET", "/userinfo") => match req.bearer() {
            Some(t) if st.access_tokens.contains(t) => HttpResponse::json(
                200,
                json!({"email": st.email, "name": st.name, "email_verified": true}),
            ),
            _ => HttpResponse::status(401),
        },
        (_, "/introspect") => {
            // Dovecot: `introspection_mode = post` gövdede `token=`, `auth`
            // kipinde Authorization başlığı gönderir.
            let form = req.form();
            let token = form
                .get("token")
                .cloned()
                .or_else(|| req.bearer().map(str::to_owned))
                .unwrap_or_default();
            if st.access_tokens.contains(&token) {
                HttpResponse::json(
                    200,
                    json!({"active": true, "username": st.username, "email": st.email}),
                )
            } else {
                HttpResponse::json(200, json!({"active": false}))
            }
        }
        _ => HttpResponse::status(404),
    }
}

fn authorize(st: &mut FakeState, req: &HttpRequest) -> HttpResponse {
    let q = &req.query;
    st.last_authorize = Some(q.clone());
    let redirect = q.get("redirect_uri").cloned().unwrap_or_default();
    let state = q.get("state").cloned().unwrap_or_default();
    let valid = q.get("response_type").map(String::as_str) == Some("code")
        && q.get("client_id") == Some(&st.client_id)
        && q.get("code_challenge_method").map(String::as_str) == Some("S256")
        && q.get("code_challenge").is_some_and(|c| c.len() >= 43)
        && redirect.starts_with("http://127.0.0.1:");
    if !valid {
        return HttpResponse::status(400);
    }
    let mut back = Url::parse(&redirect).expect("dönüş adresi");
    if st.deny {
        back.query_pairs_mut()
            .append_pair("error", "access_denied")
            .append_pair("state", &state);
    } else {
        st.counter += 1;
        let code = format!("kod-{}", st.counter);
        st.codes.insert(
            code.clone(),
            (q["code_challenge"].clone(), redirect.clone()),
        );
        back.query_pairs_mut()
            .append_pair("code", &code)
            .append_pair("state", &state);
    }
    HttpResponse::redirect(back.as_str())
}

fn token(st: &mut FakeState, req: &HttpRequest) -> HttpResponse {
    let f = req.form();
    st.token_requests.push(f.clone());
    if f.get("client_id") != Some(&st.client_id) {
        return oauth_error("invalid_client");
    }
    if st.client_secret.is_some() && f.get("client_secret") != st.client_secret.as_ref() {
        return oauth_error("invalid_client");
    }
    match f.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let code = f.get("code").cloned().unwrap_or_default();
            // Kod tek kullanımlıktır.
            let Some((challenge, redirect)) = st.codes.remove(&code) else {
                return oauth_error("invalid_grant");
            };
            let verifier = f.get("code_verifier").cloned().unwrap_or_default();
            if challenge_for(&verifier) != challenge || f.get("redirect_uri") != Some(&redirect) {
                return oauth_error("invalid_grant");
            }
            st.counter += 1;
            let n = st.counter;
            let access = format!("erisim-{n}");
            st.access_tokens.insert(access.clone());
            let mut body = json!({
                "access_token": access,
                "token_type": "Bearer",
                "expires_in": st.expires_in,
                "scope": "https://mail.google.com/ openid email profile",
            });
            if st.issue_refresh {
                let refresh = format!("yenileme-{n}");
                st.refresh_tokens.insert(refresh.clone());
                body["refresh_token"] = json!(refresh);
            }
            if st.id_token {
                body["id_token"] = json!(unsigned_id_token(
                    &json!({"email": st.email, "name": st.name})
                ));
            }
            HttpResponse::json(200, body)
        }
        Some("refresh_token") => {
            let rt = f.get("refresh_token").cloned().unwrap_or_default();
            if !st.refresh_tokens.contains(&rt) {
                return oauth_error("invalid_grant");
            }
            st.counter += 1;
            let n = st.counter;
            let access = format!("erisim-{n}");
            st.access_tokens.insert(access.clone());
            let mut body = json!({
                "access_token": access,
                "token_type": "Bearer",
                // Metin olarak (bazı sağlayıcılar böyle döner).
                "expires_in": st.expires_in.to_string(),
            });
            if st.rotate_refresh {
                st.refresh_tokens.remove(&rt);
                let refresh = format!("yenileme-{n}");
                st.refresh_tokens.insert(refresh.clone());
                body["refresh_token"] = json!(refresh);
            }
            HttpResponse::json(200, body)
        }
        _ => oauth_error("unsupported_grant_type"),
    }
}
