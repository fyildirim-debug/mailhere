//! Testler için süreç içi sahte JMAP sunucusu (RFC 8620/8621'in Mailhere'nin
//! kullandığı kısmı). Oturum, API, blob indirme/yükleme ve durum akışı.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

pub const ACCOUNT: &str = "A";

type Body = UnsyncBoxBody<Bytes, Infallible>;

#[derive(Debug, Clone)]
pub struct FEmail {
    pub id: String,
    pub blob_id: String,
    pub thread_id: String,
    pub mailbox_ids: BTreeSet<String>,
    pub keywords: BTreeSet<String>,
    pub received_at: i64,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Submission {
    pub email_id: String,
    pub identity_id: String,
    pub mail_from: String,
    pub rcpt_to: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Created,
    Updated,
    Destroyed,
}

#[derive(Default)]
pub struct State {
    pub user: String,
    pub pass: String,
    pub mailboxes: Vec<(String, String, Option<String>)>,
    /// Kutu kimliği → üst kutu kimliği (`parentId`).
    pub parents: HashMap<String, String>,
    pub emails: BTreeMap<String, FEmail>,
    pub blobs: HashMap<String, Vec<u8>>,
    pub identities: Vec<(String, String)>,
    pub submissions: Vec<Submission>,
    email_state: u64,
    mailbox_state: u64,
    log: Vec<(u64, String, Change)>,
    next: u64,
    subscribers: Vec<UnboundedSender<Bytes>>,
    /// `Email/changes` cannotCalculateChanges dönsün.
    pub fail_changes: bool,
    /// Gönderim reddedilsin (forbiddenFrom).
    pub reject_submission: bool,
    /// Oturumda eventSourceUrl olmasın (yoklama denemesi).
    pub no_eventsource: bool,
    /// Oturum nesnesinde değiştirilecek alanlar (ör. başka kökene giden apiUrl).
    pub session_overrides: Vec<(String, String)>,
    /// Gelen API isteklerinin yöntem adları.
    pub calls: Vec<String>,
}

impl State {
    fn next_id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    fn touch_email(&mut self, id: &str, kind: Change) {
        self.email_state += 1;
        self.log.push((self.email_state, id.to_owned(), kind));
        let data = json!({
            "@type": "StateChange",
            "changed": { ACCOUNT: { "Email": format!("e{}", self.email_state) } }
        });
        let ev = Bytes::from(format!("event: state\ndata: {data}\n\n"));
        self.subscribers.retain(|s| s.send(ev.clone()).is_ok());
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers.iter().filter(|s| !s.is_closed()).count()
    }

    pub fn role_id(&self, role: &str) -> Option<String> {
        self.mailboxes
            .iter()
            .find(|(_, _, r)| r.as_deref() == Some(role))
            .map(|(id, _, _)| id.clone())
    }

    /// Sunucuya posta ekler (başka bir istemciden gelmiş gibi).
    pub fn add_email(
        &mut self,
        mailbox: &str,
        raw: Vec<u8>,
        keywords: &[&str],
        received_at: i64,
    ) -> String {
        let id = self.next_id("M");
        let blob_id = self.next_id("B");
        self.blobs.insert(blob_id.clone(), raw.clone());
        let thread_id = self.next_id("T");
        self.emails.insert(
            id.clone(),
            FEmail {
                id: id.clone(),
                blob_id,
                thread_id,
                mailbox_ids: BTreeSet::from([mailbox.to_owned()]),
                keywords: keywords.iter().map(|k| k.to_string()).collect(),
                received_at,
                raw,
            },
        );
        self.touch_email(&id, Change::Created);
        id
    }

    pub fn update_email(&mut self, id: &str, f: impl FnOnce(&mut FEmail)) {
        if let Some(e) = self.emails.get_mut(id) {
            f(e);
            self.touch_email(id, Change::Updated);
        }
    }

    pub fn destroy_email(&mut self, id: &str) {
        if self.emails.remove(id).is_some() {
            self.touch_email(id, Change::Destroyed);
        }
    }

    fn email_json(&self, e: &FEmail, props: Option<&Vec<Value>>) -> Value {
        let p = mh_mime::parse(&e.raw, false).expect("ayrıştırılabilir posta");
        let addr = |a: &mh_core::Address| json!({ "name": a.name, "email": a.email });
        let list = |l: &[mh_core::Address]| Value::Array(l.iter().map(addr).collect());
        let date = |ms: i64| {
            chrono::DateTime::from_timestamp_millis(ms)
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        };
        let mut full = json!({
            "id": e.id,
            "blobId": e.blob_id,
            "threadId": e.thread_id,
            "mailboxIds": e.mailbox_ids.iter().map(|m| (m.clone(), Value::Bool(true))).collect::<Map<_, _>>(),
            "keywords": e.keywords.iter().map(|k| (k.clone(), Value::Bool(true))).collect::<Map<_, _>>(),
            "size": e.raw.len(),
            "receivedAt": date(e.received_at),
            "messageId": p.message_id.as_ref().map(|m| vec![m.clone()]),
            "inReplyTo": p.in_reply_to.as_ref().map(|m| vec![m.clone()]),
            "references": if p.references.is_empty() { Value::Null } else { json!(p.references) },
            "sender": null,
            "from": [addr(&p.from)],
            "to": list(&p.to),
            "cc": list(&p.cc),
            "replyTo": list(&p.reply_to),
            "subject": p.subject,
            "sentAt": p.date.map(date),
            "hasAttachment": p.has_attachments,
            "preview": p.snippet,
        });
        let Some(props) = props else {
            return full;
        };
        let mut out = Map::new();
        out.insert("id".into(), json!(e.id));
        for prop in props.iter().filter_map(Value::as_str) {
            if let Some(name) = prop.strip_prefix("header:") {
                out.insert(
                    prop.into(),
                    raw_header(&e.raw, name).map_or(Value::Null, Value::String),
                );
            } else if let Some(v) = full.get_mut(prop) {
                out.insert(prop.into(), v.take());
            }
        }
        Value::Object(out)
    }
}

/// Ham postadan başlığın ham değeri (ilk eşleşme).
fn raw_header(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    head.split("\r\n").find_map(|line| {
        let (n, v) = line.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.to_owned())
    })
}

/// Basit RFC 5322 posta.
pub fn raw_mail(
    from: &str,
    to: &str,
    subject: &str,
    msgid: &str,
    extra: &[(&str, &str)],
    body: &str,
) -> Vec<u8> {
    let mut s =
        format!("From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nMessage-ID: <{msgid}>\r\n");
    for (k, v) in extra {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    s.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n");
    s.push_str(body);
    s.push_str("\r\n");
    s.into_bytes()
}

pub struct FakeServer {
    pub state: Arc<Mutex<State>>,
    pub base: String,
}

impl FakeServer {
    pub async fn start(user: &str, pass: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let mut st = State {
            user: user.into(),
            pass: pass.into(),
            email_state: 1,
            mailbox_state: 1,
            ..State::default()
        };
        for (id, name, role) in [
            ("mb-in", "Inbox", Some("inbox")),
            ("mb-dr", "Drafts", Some("drafts")),
            ("mb-se", "Sent Items", Some("sent")),
            ("mb-tr", "Deleted Items", Some("trash")),
            ("mb-ju", "Junk Mail", Some("junk")),
            ("mb-pr", "Projeler", None),
        ] {
            st.mailboxes
                .push((id.into(), name.into(), role.map(str::to_owned)));
        }
        st.identities
            .push(("I1".into(), format!("{user}@ornek.com")));
        let state = Arc::new(Mutex::new(st));
        let s2 = state.clone();
        let b2 = base.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let state = s2.clone();
                let base = b2.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| handle(state.clone(), base.clone(), req));
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), svc)
                        .await;
                });
            }
        });
        Self { state, base }
    }

    pub fn session_url(&self) -> String {
        format!("{}/.well-known/jmap", self.base)
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().unwrap())
    }
}

/// Her isteğin başlığını kaydeden ve `reply` ile sabit yanıt veren düz HTTP
/// sunucusu (yönlendirme ve kimlik sızıntısı testleri).
pub struct RawServer {
    pub base: String,
    pub port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl RawServer {
    pub async fn start(reply: impl Fn(&str) -> String + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let reply = Arc::new(reply);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let seen = s2.clone();
                let reply = reply.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&buf[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head).into_owned();
                    let resp = reply(&head);
                    seen.lock().unwrap().push(head);
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            port,
            seen,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Gelen isteklerin başlık bölümleri.
    pub fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

/// `location` adresine 302 yönlendirmesi.
pub fn redirect_to(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    )
}

fn full(status: StatusCode, ct: &str, body: impl Into<Bytes>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", ct)
        .body(Full::new(body.into()).boxed_unsync())
        .unwrap()
}

fn json_resp(v: Value) -> Response<Body> {
    full(
        StatusCode::OK,
        "application/json",
        serde_json::to_vec(&v).unwrap(),
    )
}

fn authorized(state: &State, req: &Request<Incoming>) -> bool {
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", state.user, state.pass))
    );
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == expected)
}

async fn handle(
    state: Arc<Mutex<State>>,
    base: String,
    req: Request<Incoming>,
) -> Result<Response<Body>, Infallible> {
    let path = req.uri().path().to_owned();
    if path == "/.well-known/jmap" {
        return Ok(Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header("location", "/jmap/session")
            .body(Full::new(Bytes::new()).boxed_unsync())
            .unwrap());
    }
    if !path.starts_with("/jmap/") {
        return Ok(full(StatusCode::NOT_FOUND, "text/plain", "yok"));
    }
    if !authorized(&state.lock().unwrap(), &req) {
        let mut r = full(StatusCode::UNAUTHORIZED, "text/plain", "giriş gerekli");
        r.headers_mut()
            .insert("www-authenticate", "Basic realm=\"jmap\"".parse().unwrap());
        return Ok(r);
    }
    if path == "/jmap/session" {
        let st = state.lock().unwrap();
        // Küçük sınırlar: istemcinin parçalı istekleri denensin.
        let caps = json!({
            "urn:ietf:params:jmap:core": { "maxObjectsInGet": 2, "maxCallsInRequest": 4, "maxSizeUpload": 50_000_000 },
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:submission": {},
        });
        let mut s = json!({
            "capabilities": caps,
            "accounts": { ACCOUNT: { "name": st.user, "isPersonal": true, "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:mail": {}, "urn:ietf:params:jmap:submission": {} } } },
            "primaryAccounts": { "urn:ietf:params:jmap:mail": ACCOUNT, "urn:ietf:params:jmap:submission": ACCOUNT },
            "username": st.user,
            "apiUrl": format!("{base}/jmap/api"),
            "downloadUrl": format!("{base}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
            "uploadUrl": format!("{base}/jmap/upload/{{accountId}}/"),
            "eventSourceUrl": format!("{base}/jmap/eventsource/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
            "state": "s1",
        });
        if st.no_eventsource {
            s.as_object_mut().unwrap().remove("eventSourceUrl");
        }
        for (k, v) in &st.session_overrides {
            s[k] = json!(v);
        }
        return Ok(json_resp(s));
    }
    if let Some(rest) = path.strip_prefix("/jmap/download/") {
        let blob = rest.split('/').nth(1).unwrap_or("").to_owned();
        let st = state.lock().unwrap();
        return Ok(match st.blobs.get(&blob) {
            Some(b) => full(StatusCode::OK, "message/rfc822", b.clone()),
            None => full(StatusCode::NOT_FOUND, "text/plain", "yok"),
        });
    }
    if path.starts_with("/jmap/upload/") {
        let ct = req
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let body = req.into_body().collect().await.unwrap().to_bytes().to_vec();
        let mut st = state.lock().unwrap();
        let id = st.next_id("B");
        let size = body.len();
        st.blobs.insert(id.clone(), body);
        return Ok(json_resp(
            json!({ "accountId": ACCOUNT, "blobId": id, "type": ct, "size": size }),
        ));
    }
    if path.starts_with("/jmap/eventsource") {
        let (tx, mut rx) = unbounded_channel::<Bytes>();
        let _ = tx.send(Bytes::from_static(
            b": merhaba\n\nevent: ping\ndata: {\"interval\": 60}\n\n",
        ));
        state.lock().unwrap().subscribers.push(tx);
        let stream = futures::stream::poll_fn(move |cx| {
            rx.poll_recv(cx)
                .map(|o| o.map(|b| Ok::<_, Infallible>(Frame::data(b))))
        });
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(BodyExt::boxed_unsync(StreamBody::new(stream)))
            .unwrap());
    }
    if path == "/jmap/api" {
        let body = req.into_body().collect().await.unwrap().to_bytes();
        let v: Value = serde_json::from_slice(&body).unwrap();
        let mut st = state.lock().unwrap();
        let mut responses = Vec::new();
        for call in v["methodCalls"].as_array().unwrap() {
            let name = call[0].as_str().unwrap();
            let args = &call[1];
            let id = call[2].as_str().unwrap();
            st.calls.push(name.to_owned());
            for (n, a) in api(&mut st, name, args) {
                responses.push(json!([n, a, id]));
            }
        }
        return Ok(json_resp(
            json!({ "methodResponses": responses, "sessionState": "s1" }),
        ));
    }
    Ok(full(StatusCode::NOT_FOUND, "text/plain", "yok"))
}

fn err(kind: &str) -> Vec<(String, Value)> {
    vec![("error".into(), json!({ "type": kind }))]
}

fn state_num(s: &Value, prefix: char) -> Option<u64> {
    s.as_str()?.strip_prefix(prefix)?.parse().ok()
}

/// `mailboxIds/x` ya da `keywords/x` yamasını uygular.
fn apply_patch(e: &mut FEmail, patch: &Map<String, Value>) {
    for (k, v) in patch {
        if k == "mailboxIds" {
            e.mailbox_ids = v.as_object().unwrap().keys().cloned().collect();
        } else if k == "keywords" {
            e.keywords = v.as_object().unwrap().keys().cloned().collect();
        } else if let Some(m) = k.strip_prefix("mailboxIds/") {
            if v.as_bool() == Some(true) {
                e.mailbox_ids.insert(m.into());
            } else {
                e.mailbox_ids.remove(m);
            }
        } else if let Some(kw) = k.strip_prefix("keywords/") {
            if v.as_bool() == Some(true) {
                e.keywords.insert(kw.into());
            } else {
                e.keywords.remove(kw);
            }
        }
    }
}

fn api(st: &mut State, name: &str, a: &Value) -> Vec<(String, Value)> {
    let one = |v: Value| vec![(name.to_owned(), v)];
    match name {
        "Mailbox/get" => {
            let list: Vec<Value> = st
                .mailboxes
                .iter()
                .enumerate()
                .map(|(i, (id, n, r))| json!({ "id": id, "name": n, "parentId": st.parents.get(id), "role": r, "sortOrder": i }))
                .collect();
            one(
                json!({ "accountId": ACCOUNT, "state": format!("m{}", st.mailbox_state), "list": list, "notFound": [] }),
            )
        }
        "Mailbox/changes" => {
            let Some(since) = state_num(&a["sinceState"], 'm') else {
                return err("cannotCalculateChanges");
            };
            let updated: Vec<&String> = if since == st.mailbox_state {
                vec![]
            } else {
                st.mailboxes.iter().map(|(id, _, _)| id).collect()
            };
            one(
                json!({ "accountId": ACCOUNT, "oldState": a["sinceState"], "newState": format!("m{}", st.mailbox_state),
                "hasMoreChanges": false, "created": [], "updated": updated, "destroyed": [], "updatedProperties": null }),
            )
        }
        "Mailbox/set" => {
            let mut created = Map::new();
            if let Some(c) = a["create"].as_object() {
                for (k, v) in c {
                    let id = st.next_id("mb-");
                    st.mailboxes.push((
                        id.clone(),
                        v["name"].as_str().unwrap().into(),
                        v["role"].as_str().map(str::to_owned),
                    ));
                    created.insert(k.clone(), json!({ "id": id }));
                }
            }
            let mut updated = Map::new();
            if let Some(u) = a["update"].as_object() {
                for (k, v) in u {
                    if let Some(mb) = st.mailboxes.iter_mut().find(|(id, _, _)| id == k) {
                        if let Some(r) = v.get("role") {
                            mb.2 = r.as_str().map(str::to_owned);
                        }
                        updated.insert(k.clone(), Value::Null);
                    }
                }
            }
            st.mailbox_state += 1;
            one(
                json!({ "accountId": ACCOUNT, "newState": format!("m{}", st.mailbox_state), "created": created, "updated": updated }),
            )
        }
        "Email/get" => {
            let props = a["properties"].as_array();
            let ids: Vec<String> = a["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
            if ids.len() > 2 {
                return err("requestTooLarge");
            }
            let mut list = Vec::new();
            let mut nf = Vec::new();
            for id in ids {
                match st.emails.get(&id) {
                    Some(e) => list.push(st.email_json(e, props)),
                    None => nf.push(id),
                }
            }
            one(
                json!({ "accountId": ACCOUNT, "state": format!("e{}", st.email_state), "list": list, "notFound": nf }),
            )
        }
        "Email/query" => {
            let mb = a["filter"]["inMailbox"].as_str().unwrap();
            let limit = a["limit"].as_u64().unwrap_or(100) as usize;
            let mut list: Vec<&FEmail> = st
                .emails
                .values()
                .filter(|e| e.mailbox_ids.contains(mb))
                .collect();
            list.sort_by_key(|e| std::cmp::Reverse(e.received_at));
            let ids: Vec<&String> = list.iter().take(limit).map(|e| &e.id).collect();
            one(
                json!({ "accountId": ACCOUNT, "queryState": "q", "canCalculateChanges": false, "position": 0, "ids": ids }),
            )
        }
        "Email/changes" => {
            let Some(since) = state_num(&a["sinceState"], 'e') else {
                return err("cannotCalculateChanges");
            };
            if st.fail_changes {
                st.fail_changes = false;
                return err("cannotCalculateChanges");
            }
            let mut kinds: BTreeMap<&String, Vec<Change>> = BTreeMap::new();
            for (s, id, k) in &st.log {
                if *s > since {
                    kinds.entry(id).or_default().push(*k);
                }
            }
            let (mut created, mut updated, mut destroyed) = (vec![], vec![], vec![]);
            for (id, ks) in kinds {
                let was_created = ks.contains(&Change::Created);
                if ks.contains(&Change::Destroyed) {
                    if !was_created {
                        destroyed.push(id);
                    }
                } else if was_created {
                    created.push(id);
                } else {
                    updated.push(id);
                }
            }
            one(
                json!({ "accountId": ACCOUNT, "oldState": a["sinceState"], "newState": format!("e{}", st.email_state),
                "hasMoreChanges": false, "created": created, "updated": updated, "destroyed": destroyed }),
            )
        }
        "Email/set" => {
            let mut updated = Map::new();
            let mut not_updated = Map::new();
            if let Some(u) = a["update"].as_object() {
                for (id, patch) in u {
                    if st.emails.contains_key(id) {
                        st.update_email(id, |e| apply_patch(e, patch.as_object().unwrap()));
                        updated.insert(id.clone(), Value::Null);
                    } else {
                        not_updated.insert(id.clone(), json!({ "type": "notFound" }));
                    }
                }
            }
            let mut destroyed = Vec::new();
            let mut not_destroyed = Map::new();
            if let Some(d) = a["destroy"].as_array() {
                for id in d.iter().filter_map(Value::as_str) {
                    if st.emails.contains_key(id) {
                        st.destroy_email(id);
                        destroyed.push(id.to_owned());
                    } else {
                        not_destroyed.insert(id.into(), json!({ "type": "notFound" }));
                    }
                }
            }
            one(
                json!({ "accountId": ACCOUNT, "newState": format!("e{}", st.email_state), "updated": updated,
                "notUpdated": not_updated, "destroyed": destroyed, "notDestroyed": not_destroyed }),
            )
        }
        "Email/import" => {
            let mut created = Map::new();
            let mut not_created = Map::new();
            for (k, v) in a["emails"].as_object().unwrap() {
                let Some(raw) = st.blobs.get(v["blobId"].as_str().unwrap()).cloned() else {
                    not_created.insert(k.clone(), json!({ "type": "blobNotFound" }));
                    continue;
                };
                let mailbox = v["mailboxIds"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .next()
                    .unwrap()
                    .clone();
                let kws: Vec<String> = v["keywords"]
                    .as_object()
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                let kw_refs: Vec<&str> = kws.iter().map(String::as_str).collect();
                let id = st.add_email(&mailbox, raw, &kw_refs, 1_791_800_000_000);
                let e = &st.emails[&id];
                created.insert(k.clone(), json!({ "id": id, "blobId": e.blob_id, "threadId": e.thread_id, "size": e.raw.len() }));
            }
            one(json!({ "accountId": ACCOUNT, "created": created, "notCreated": not_created }))
        }
        "Identity/get" => {
            let list: Vec<Value> = st
                .identities
                .iter()
                .map(|(id, e)| json!({ "id": id, "name": "", "email": e }))
                .collect();
            one(json!({ "accountId": ACCOUNT, "state": "i1", "list": list, "notFound": [] }))
        }
        "Identity/set" => {
            let mut created = Map::new();
            for (k, v) in a["create"].as_object().unwrap() {
                let id = st.next_id("I");
                st.identities
                    .push((id.clone(), v["email"].as_str().unwrap().into()));
                created.insert(k.clone(), json!({ "id": id }));
            }
            one(json!({ "accountId": ACCOUNT, "created": created }))
        }
        "EmailSubmission/set" => {
            let (k, sub) = a["create"].as_object().unwrap().iter().next().unwrap();
            if st.reject_submission {
                return one(
                    json!({ "accountId": ACCOUNT, "notCreated": { (k.clone()): { "type": "forbiddenFrom" } } }),
                );
            }
            let email_id = sub["emailId"].as_str().unwrap().to_owned();
            let env = &sub["envelope"];
            st.submissions.push(Submission {
                email_id: email_id.clone(),
                identity_id: sub["identityId"].as_str().unwrap().into(),
                mail_from: env["mailFrom"]["email"].as_str().unwrap().into(),
                rcpt_to: env["rcptTo"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r["email"].as_str().unwrap().to_owned())
                    .collect(),
            });
            let sub_id = st.next_id("S");
            let mut out = vec![(
                name.to_owned(),
                json!({ "accountId": ACCOUNT, "created": { (k.clone()): { "id": sub_id } } }),
            )];
            if let Some(patch) = a["onSuccessUpdateEmail"]
                .get(format!("#{k}"))
                .and_then(Value::as_object)
            {
                st.update_email(&email_id, |e| apply_patch(e, patch));
                out.push((
                    "Email/set".into(),
                    json!({ "accountId": ACCOUNT, "updated": { (email_id.clone()): null } }),
                ));
            }
            out
        }
        _ => err("unknownMethod"),
    }
}
