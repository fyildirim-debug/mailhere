//! Tarayıcıdan dönüşü karşılayan tek seferlik yerel dinleyici
//! (`http://127.0.0.1:<rastgele kapı>/`, RFC 8252 7.3).
//!
//! Yalnız 127.0.0.1'de dinler. `state` değeri tutmayan istekler yok sayılır
//! (başka bir sayfanın sahte dönüşü girişi bozamaz); favicon ve diğer yollar
//! 404 alır. İlk geçerli dönüşte dinleyici kapanır.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::OAuthError;

/// Tek bir bağlantının isteği okuması için süre (tarayıcılar boş ön
/// bağlantılar açabilir; bunlar diğerlerini bekletmez).
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEAD: usize = 16 * 1024;

pub struct Loopback {
    listener: TcpListener,
    port: u16,
}

/// Tek bir isteğin sonucu.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reply {
    pub status: u16,
    pub page: Option<Page>,
    /// `Some` ise giriş sonuçlandı.
    pub outcome: Option<Outcome>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Done,
    Denied,
    Failed,
    Foreign,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Code(String),
    Denied,
    Error(String),
    MissingCode,
}

impl Loopback {
    /// 127.0.0.1 üzerinde boş bir kapıda dinlemeye başlar.
    pub async fn bind() -> Result<Self, OAuthError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Sağlayıcıya verilecek dönüş adresi.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// Geçerli dönüşü bekler ve yetki kodunu döner. Süre sınırı ve iptal
    /// çağıranın işidir (bu gelecek bırakılınca dinleyici kapanır).
    pub async fn wait(self, expected_state: &str) -> Result<String, OAuthError> {
        let state: Arc<str> = Arc::from(expected_state);
        let (tx, mut rx) = mpsc::channel::<Outcome>(4);
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    let (sock, _) = match accepted {
                        Ok(a) => a,
                        Err(e) => {
                            tracing::debug!(error = %e, "yerel dönüş bağlantısı kabul edilemedi");
                            continue;
                        }
                    };
                    let (tx, state) = (tx.clone(), state.clone());
                    tokio::spawn(async move {
                        if let Some(outcome) = serve(sock, &state).await {
                            let _ = tx.send(outcome).await;
                        }
                    });
                }
                Some(outcome) = rx.recv() => {
                    return match outcome {
                        Outcome::Code(code) => Ok(code),
                        Outcome::Denied => Err(OAuthError::Denied),
                        Outcome::Error(code) => Err(OAuthError::Provider { code, description: None }),
                        Outcome::MissingCode => Err(OAuthError::Protocol("dönüşte yetki kodu yok".into())),
                    };
                }
            }
        }
    }
}

async fn read_head(sock: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = sock.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.windows(2).any(|w| w == b"\n\n") {
            return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
        }
        if buf.len() > MAX_HEAD {
            return Ok(None);
        }
    }
}

async fn serve(mut sock: TcpStream, state: &str) -> Option<Outcome> {
    let head = match timeout(READ_TIMEOUT, read_head(&mut sock)).await {
        Ok(Ok(Some(h))) => h,
        _ => return None,
    };
    let mut parts = head.lines().next().unwrap_or("").split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let reply = route(method, target, state);
    let body = reply.page.as_ref().map(html).unwrap_or_default();
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Method Not Allowed",
    };
    let response = format!(
        "HTTP/1.1 {} {reason}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
         Referrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n{body}",
        reply.status,
        body.len()
    );
    let _ = timeout(READ_TIMEOUT, async {
        sock.write_all(response.as_bytes()).await?;
        sock.shutdown().await
    })
    .await;
    reply.outcome
}

/// İsteği yorumlar (ağdan bağımsız, sınanabilir).
pub(crate) fn route(method: &str, target: &str, state: &str) -> Reply {
    let ignore = |status| Reply {
        status,
        page: None,
        outcome: None,
    };
    if method != "GET" {
        return ignore(405);
    }
    if !target.starts_with('/') {
        return ignore(400);
    }
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        return ignore(400);
    };
    if url.path() != "/" {
        // favicon.ico ve diğer yollar.
        return ignore(404);
    }
    let mut q: HashMap<String, String> = HashMap::new();
    for (k, v) in url.query_pairs() {
        q.entry(k.into_owned()).or_insert_with(|| v.into_owned());
    }
    if q.get("state").map(String::as_str) != Some(state) {
        tracing::warn!("OAuth dönüşünde state uyuşmadı; istek yok sayıldı");
        return Reply {
            status: 400,
            page: Some(Page::Foreign),
            outcome: None,
        };
    }
    if let Some(err) = q.get("error") {
        let (page, outcome) = if err == "access_denied" {
            (Page::Denied, Outcome::Denied)
        } else {
            (Page::Failed, Outcome::Error(err.clone()))
        };
        return Reply {
            status: 200,
            page: Some(page),
            outcome: Some(outcome),
        };
    }
    match q.get("code").filter(|c| !c.is_empty()) {
        Some(code) => Reply {
            status: 200,
            page: Some(Page::Done),
            outcome: Some(Outcome::Code(code.clone())),
        },
        None => Reply {
            status: 400,
            page: Some(Page::Failed),
            outcome: Some(Outcome::MissingCode),
        },
    }
}

/// Tarayıcıda gösterilen sade sayfa. Sağlayıcıdan gelen metin yansıtılmaz.
fn html(page: &Page) -> String {
    let (title, detail) = match page {
        Page::Done => (
            "Giriş tamamlandı, Mailhere'ye dönebilirsin.",
            "Bu sekmeyi kapatabilirsin.",
        ),
        Page::Denied => (
            "Giriş onaylanmadı.",
            "Mailhere'ye dönüp yeniden deneyebilirsin.",
        ),
        Page::Failed => ("Giriş tamamlanamadı.", "Mailhere'ye dönüp yeniden dene."),
        Page::Foreign => (
            "Bu bağlantı geçersiz.",
            "Girişi Mailhere'den yeniden başlat.",
        ),
    };
    format!(
        "<!doctype html><html lang=\"tr\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Mailhere</title>\
<style>body{{margin:0;min-height:100vh;display:grid;place-items:center;font:15px/1.5 system-ui,sans-serif;background:#f7f7f5;color:#1c1c1a}}\
main{{max-width:420px;padding:24px;text-align:center}}h1{{font-size:19px;font-weight:600;margin:0 0 8px}}p{{margin:0;color:#55554f}}\
@media (prefers-color-scheme:dark){{body{{background:#161615;color:#ececea}}p{{color:#a6a6a0}}}}</style></head>\
<body><main><h1>{title}</h1><p>{detail}</p></main></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yonlendirme_kurallari() {
        let r = route("GET", "/?code=abc&state=s1", "s1");
        assert_eq!(r.outcome, Some(Outcome::Code("abc".into())));
        assert_eq!((r.status, r.page), (200, Some(Page::Done)));

        // state uyuşmazsa sonuç yok, dinleme sürer.
        let r = route("GET", "/?code=abc&state=baska", "s1");
        assert_eq!((r.status, r.outcome), (400, None));
        let r = route("GET", "/?code=abc", "s1");
        assert_eq!(r.outcome, None);

        let r = route("GET", "/?error=access_denied&state=s1", "s1");
        assert_eq!(r.outcome, Some(Outcome::Denied));
        let r = route("GET", "/?error=server_error&state=s1", "s1");
        assert_eq!(r.outcome, Some(Outcome::Error("server_error".into())));
        let r = route("GET", "/?state=s1", "s1");
        assert_eq!(r.outcome, Some(Outcome::MissingCode));

        assert_eq!(route("GET", "/favicon.ico", "s1").status, 404);
        assert_eq!(route("GET", "/favicon.ico", "s1").outcome, None);
        assert_eq!(route("POST", "/?code=a&state=s1", "s1").outcome, None);
        assert_eq!(
            route("GET", "http://evil/?code=a&state=s1", "s1").outcome,
            None
        );
    }

    #[test]
    fn sayfa_turkce() {
        assert!(html(&Page::Done).contains("Giriş tamamlandı, Mailhere'ye dönebilirsin."));
        assert!(html(&Page::Done).contains("lang=\"tr\""));
    }

    async fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn yerel_dinleyici_kodu_alir() {
        let lb = Loopback::bind().await.unwrap();
        let port = lb.port();
        assert_eq!(lb.redirect_uri(), format!("http://127.0.0.1:{port}/"));
        let waiter = tokio::spawn(async move { lb.wait("durum-1").await });

        // İstek göndermeyen boş bağlantı diğerlerini bekletmez.
        let _idle = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let fav = get(port, "/favicon.ico").await;
        assert!(fav.starts_with("HTTP/1.1 404"), "{fav}");
        let foreign = get(port, "/?code=x&state=yanlis").await;
        assert!(foreign.starts_with("HTTP/1.1 400"), "{foreign}");
        assert!(!waiter.is_finished());

        let ok = get(port, "/?code=kod-42&state=durum-1&scope=x").await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.contains("Giriş tamamlandı"), "{ok}");
        assert!(ok.contains("Referrer-Policy: no-referrer"));
        assert_eq!(waiter.await.unwrap().unwrap(), "kod-42");

        // Dinleyici kapandı.
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }

    #[tokio::test]
    async fn yerel_dinleyici_red() {
        let lb = Loopback::bind().await.unwrap();
        let port = lb.port();
        let waiter = tokio::spawn(async move { lb.wait("s").await });
        let page = get(port, "/?error=access_denied&state=s").await;
        assert!(page.contains("Giriş onaylanmadı"), "{page}");
        assert!(matches!(waiter.await.unwrap(), Err(OAuthError::Denied)));
    }
}
