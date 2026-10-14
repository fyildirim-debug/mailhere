//! İzinli uzak görsellerin indirilmesi: `remote_images`.
//!
//! Bazı sunucular görseli `Cross-Origin-Resource-Policy: same-origin`
//! başlığıyla verir; Chromium tabanlı WebView (Windows WebView2, Android)
//! böyle bir görseli başka kökenli posta çerçevesinde göstermez: bağlantılar
//! çalışır, görseller kırık kalır. Arayüz yüklenemeyen görsellerin adreslerini
//! verir; burada yalnız göndericisi uzak içerik izin listesinde olan postanın
//! temizlenmiş HTML'inde gerçekten geçen http(s) adresleri indirilip `data:`
//! adresi olarak döner.
//!
//! Sınırlar: çerez, Referer ve kimlik bilgisi gönderilmez; sistem vekil
//! sunucusu kullanılmaz; sunucu adı önce çözülür, bağlantı yalnız denetlenen
//! adreslere kurulur (DNS yeniden bağlamaya karşı) ve bu bilgisayarı ya da
//! link-local ağı gösteren hedef reddedilir (özel ağ serbesttir: şirket içi
//! bültenlerin görselleri oradadır, WebView de onları yükleyebilir).
//! Yönlendirme en çok [`MAX_REDIRECTS`] adım ve yalnız http(s)'e; yanıt
//! `image/*` olmalı (SVG dahil; `img` bağlamında betik çalışmaz); görsel
//! başına [`MAX_IMAGE_BYTES`], çağrı başına [`MAX_TOTAL_BYTES`] ve
//! [`MAX_URLS`]; görsel başına [`IMAGE_TIMEOUT`], çağrı başına
//! [`CALL_TIMEOUT`]; aynı anda [`CONCURRENCY`] indirme. Günlüğe adres değil
//! yalnız sunucu adı yazılır (adresler izleme parametresi taşır).

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use mh_core::{AppError, AppResult, ErrorCode, MessageId, RemoteImage};
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use reqwest::{Client, Url};
use tauri::State;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::state::{internal, AppState};

type Res<T> = Result<T, AppError>;

/// Bir çağrıda indirilen en çok adres.
pub const MAX_URLS: usize = 40;
/// Görsel başına en büyük boyut.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Bir çağrıda toplam boyut.
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
/// Görsel başına üst süre (ad çözümleme, bağlantı, yönlendirmeler, indirme).
pub const IMAGE_TIMEOUT: Duration = Duration::from_secs(15);
/// Çağrının tamamı; dolunca o ana kadar inenler döner.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// Aynı anda indirme.
pub const CONCURRENCY: usize = 4;
/// İzlenen en çok yönlendirme.
pub const MAX_REDIRECTS: usize = 3;

/// İndirme kuralları (testler yerel sunucuya ve küçük sınırlara izin verir).
#[derive(Clone)]
pub(crate) struct Policy {
    pub allow: fn(IpAddr) -> bool,
    pub image_bytes: usize,
    pub total_bytes: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            allow: is_fetchable_ip,
            image_bytes: MAX_IMAGE_BYTES,
            total_bytes: MAX_TOTAL_BYTES,
        }
    }
}

/// Adres bu bilgisayarı (loopback, belirtilmemiş) ya da link-local ağı
/// göstermiyor; çoklu yayın ve yayın da değil. IPv4 eşlenmiş IPv6 gömülü
/// adrese göre değerlendirilir.
pub fn is_fetchable_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast())
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_fetchable_ip(IpAddr::V4(v4)),
            None => {
                !(v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    || v6.is_unicast_link_local())
            }
        },
    }
}

fn not_allowed() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Bu göndericinin uzak görsellerine izin verilmemiş. Görselleri görmek için önce “Bu göndericiye izin ver”e tıkla.",
    )
}

/// İstenen adreslerden indirilecekleri seçer: gönderici izinli değilse hata;
/// yalnız postanın temizlenmiş HTML'inde geçen http(s) adresler, yinelenenler
/// atılarak ve [`MAX_URLS`] ile sınırlı. Postada geçmeyen adres sessizce
/// atlanır; geçerli adres kalmazsa hata.
pub fn select_urls(
    remote_allowed: bool,
    body_html: Option<&str>,
    requested: &[String],
) -> AppResult<Vec<String>> {
    if !remote_allowed {
        return Err(not_allowed());
    }
    let Some(html) = body_html else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "Postanın içeriği yerelde yok. Eşitlemenin bitmesini bekleyip postayı yeniden aç.",
        ));
    };
    let known: HashSet<String> = mh_mime::remote_image_urls(html).into_iter().collect();
    let mut out: Vec<String> = Vec::new();
    for url in requested {
        let url = url.trim();
        if known.contains(url) && !out.iter().any(|u| u == url) {
            out.push(url.to_owned());
            if out.len() == MAX_URLS {
                break;
            }
        }
    }
    if out.is_empty() {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "İstenen görsel adresleri bu postada geçmiyor. Postayı kapatıp yeniden aç.",
        ));
    }
    Ok(out)
}

/// `Content-Type` değerinden görsel türü (`image/png`): parametreler atılır,
/// küçük harfe çevrilir; `image/` ile başlamayan ya da bozuk tür `None`.
pub fn image_mime(header: &str) -> Option<String> {
    let t = header.split(';').next()?.trim().to_ascii_lowercase();
    let sub = t.strip_prefix("image/")?;
    let ok = !sub.is_empty()
        && sub.len() <= 64
        && sub
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'));
    ok.then_some(t)
}

/// Bir görselin inmeme nedeni (günlük ve testler için; adres içermez).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Skip {
    /// Geçersiz adres, http(s) dışı şema ya da kullanıcı bilgisi.
    BadUrl,
    /// Hedef bu bilgisayar ya da link-local ağ.
    Blocked,
    /// Sunucu adı çözülemedi.
    Unresolved,
    /// Bağlantı ya da okuma hatası.
    Request,
    /// 2xx dışı yanıt.
    Status(u16),
    /// Yanıt `image/*` değil.
    NotImage,
    /// Görsel başına boyut sınırı.
    TooLarge,
    /// Çağrının toplam bütçesi doldu.
    Budget,
    /// Çok fazla ya da hedefsiz yönlendirme.
    Redirects,
}

/// Bir çağrının indiricisi: kural, kalan toplam bütçe ve sunucu başına
/// (yalnız denetlenen adreslere bağlanan) istemci önbelleği.
pub(crate) struct Fetcher {
    policy: Policy,
    remaining: AtomicUsize,
    clients: Mutex<HashMap<String, Client>>,
}

impl Fetcher {
    pub(crate) fn new(policy: Policy) -> Self {
        Self {
            remaining: AtomicUsize::new(policy.total_bytes),
            policy,
            clients: Mutex::default(),
        }
    }

    /// Sunucu adını çözer, adresleri kurala vurur ve yalnız onlara bağlanan
    /// istemciyi döner (aynı çağrıda sunucu başına bir kez kurulur).
    async fn client_for(&self, url: &Url) -> Result<Client, Skip> {
        let host = url
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or(Skip::BadUrl)?;
        let port = url.port_or_known_default().ok_or(Skip::BadUrl)?;
        let key = format!("{host}:{port}");
        if let Some(c) = self
            .clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return Ok(c.clone());
        }
        let literal = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .ok();
        let addrs: Vec<SocketAddr> = match literal {
            Some(ip) => vec![SocketAddr::new(ip, port)],
            None => tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| Skip::Unresolved)?
                .collect(),
        };
        if addrs.is_empty() {
            return Err(Skip::Unresolved);
        }
        if !addrs.iter().all(|a| (self.policy.allow)(a.ip())) {
            return Err(Skip::Blocked);
        }
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .no_proxy()
            .connect_timeout(IMAGE_TIMEOUT)
            .timeout(IMAGE_TIMEOUT)
            .user_agent(concat!("Mailhere/", env!("CARGO_PKG_VERSION")));
        if literal.is_none() {
            // Bağlantı yalnız denetlenen adreslere kurulur.
            builder = builder.resolve_to_addrs(host, &addrs);
        }
        let client = builder.build().map_err(|_| Skip::Request)?;
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, client.clone());
        Ok(client)
    }

    /// Görseli indirir ve `data:` adresi döner; yönlendirmeleri kendisi,
    /// her adımı yeniden denetleyerek izler.
    pub(crate) async fn fetch(&self, url: &str) -> Result<String, Skip> {
        let mut current = Url::parse(url).map_err(|_| Skip::BadUrl)?;
        for _ in 0..=MAX_REDIRECTS {
            if !matches!(current.scheme(), "http" | "https")
                || !current.username().is_empty()
                || current.password().is_some()
            {
                return Err(Skip::BadUrl);
            }
            let client = self.client_for(&current).await?;
            let resp = client
                .get(current.clone())
                .header(ACCEPT, "image/*")
                .send()
                .await
                .map_err(|_| Skip::Request)?;
            let status = resp.status();
            if status.is_redirection() {
                current = resp
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|l| current.join(l).ok())
                    .ok_or(Skip::Redirects)?;
                continue;
            }
            if !status.is_success() {
                return Err(Skip::Status(status.as_u16()));
            }
            let mime = resp
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .and_then(image_mime)
                .ok_or(Skip::NotImage)?;
            let limit = self.policy.image_bytes;
            if let Some(n) = resp.content_length() {
                if n > limit as u64 {
                    return Err(Skip::TooLarge);
                }
                if n > self.remaining.load(Ordering::SeqCst) as u64 {
                    return Err(Skip::Budget);
                }
            }
            let bytes = read_limited(resp, limit).await?;
            self.reserve(bytes.len())?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Ok(format!("data:{mime};base64,{b64}"));
        }
        Err(Skip::Redirects)
    }

    /// Toplam bütçeden düşer; yetmiyorsa görsel atlanır.
    fn reserve(&self, n: usize) -> Result<(), Skip> {
        let mut cur = self.remaining.load(Ordering::SeqCst);
        loop {
            let next = cur.checked_sub(n).ok_or(Skip::Budget)?;
            match self
                .remaining
                .compare_exchange(cur, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Ok(()),
                Err(seen) => cur = seen,
            }
        }
    }
}

/// Gövdeyi en çok `limit` bayt okur; aşarsa yarıda keser.
async fn read_limited(mut resp: reqwest::Response, limit: usize) -> Result<Vec<u8>, Skip> {
    let hint = resp
        .content_length()
        .map_or(0, |n| n.min(limit as u64) as usize);
    let mut out = Vec::with_capacity(hint);
    while let Some(chunk) = resp.chunk().await.map_err(|_| Skip::Request)? {
        if out.len() + chunk.len() > limit {
            return Err(Skip::TooLarge);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Günlük için yalnız sunucu adı (adres izleme parametresi taşır).
fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| "?".into())
}

/// Adresleri aynı anda en çok [`CONCURRENCY`] bağlantıyla indirir; inenler
/// döner, inmeyenler günlüğe yalnız sunucu adıyla yazılır. [`CALL_TIMEOUT`]
/// dolunca kalanlar bırakılır.
pub(crate) async fn download(urls: Vec<String>, policy: Policy) -> Vec<RemoteImage> {
    let fetcher = Arc::new(Fetcher::new(policy));
    let gate = Arc::new(Semaphore::new(CONCURRENCY));
    let mut tasks = JoinSet::new();
    for url in urls {
        let (fetcher, gate) = (fetcher.clone(), gate.clone());
        tasks.spawn(async move {
            let _permit = gate.acquire_owned().await.ok()?;
            match tokio::time::timeout(IMAGE_TIMEOUT, fetcher.fetch(&url)).await {
                Ok(Ok(data)) => Some(RemoteImage { url, data }),
                Ok(Err(why)) => {
                    tracing::info!(host = %host_of(&url), reason = ?why, "uzak görsel indirilmedi");
                    None
                }
                Err(_) => {
                    tracing::info!(host = %host_of(&url), "uzak görsel zamanında inmedi");
                    None
                }
            }
        });
    }
    let deadline = tokio::time::Instant::now() + CALL_TIMEOUT;
    let mut out = Vec::new();
    while let Ok(Some(joined)) = tokio::time::timeout_at(deadline, tasks.join_next()).await {
        if let Ok(Some(img)) = joined {
            out.push(img);
        }
    }
    tasks.abort_all();
    out
}

/// İzinli göndericinin postasında geçen, çerçevede yüklenemeyen uzak
/// görselleri indirir ve `data:` adresi olarak döner. İnmeyen görsel listede
/// yer almaz; arayüz onu olduğu gibi (alt metniyle) bırakır.
#[tauri::command]
pub async fn remote_images(
    state: State<'_, AppState>,
    id: MessageId,
    urls: Vec<String>,
) -> Res<Vec<RemoteImage>> {
    let allow = state.store.settings()?.remote_allow;
    // HTML temizleme ağır olabilir; çalışma iş parçacıklarını tutmasın.
    let store = state.store.clone();
    let detail = tokio::task::spawn_blocking(move || store.detail(id, &allow))
        .await
        .map_err(internal)??;
    let urls = select_urls(detail.remote_allowed, detail.body_html.as_deref(), &urls)?;
    Ok(download(urls, Policy::default()).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    type Seen = Arc<Mutex<Vec<String>>>;

    /// Tam HTTP yanıtı: durum, ek başlıklar, gövde (`content-length` ile).
    fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nconnection: close\r\ncontent-length: {}\r\n",
            body.len()
        );
        for (k, v) in headers {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    /// Uzunluğu bildirilmeyen yanıt (bağlantı kapanınca biter).
    fn unsized_response(mime: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes =
            format!("HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-type: {mime}\r\n\r\n")
                .into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    /// Yola göre hazır yanıt veren küçük HTTP sunucusu: (kapı, görülen
    /// istekler). Bilinmeyen yol 404.
    async fn http_server(routes: Vec<(&'static str, Vec<u8>)>) -> (u16, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Seen = Arc::default();
        let (s2, routes) = (seen.clone(), Arc::new(routes));
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (seen, routes) = (s2.clone(), routes.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        if buf.len() > 64 * 1024 {
                            return;
                        }
                    }
                    let req = String::from_utf8_lossy(&buf).into_owned();
                    // Yol sorgusuz eşlenir (`/a.png?iz=42` → `/a.png`).
                    let path = req
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .split('?')
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    seen.lock().unwrap().push(req);
                    let reply = routes
                        .iter()
                        .find(|(p, _)| *p == path)
                        .map(|(_, r)| r.clone())
                        .unwrap_or_else(|| response("404 Not Found", &[], b""));
                    let _ = sock.write_all(&reply).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (port, seen)
    }

    fn local(image_bytes: usize, total_bytes: usize) -> Policy {
        Policy {
            allow: |ip| ip.is_loopback(),
            image_bytes,
            total_bytes,
        }
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];

    fn png_route(path: &'static str) -> (&'static str, Vec<u8>) {
        (
            path,
            response(
                "200 OK",
                &[
                    ("content-type", "image/PNG; charset=binary"),
                    ("set-cookie", "iz=1; Path=/"),
                ],
                PNG,
            ),
        )
    }

    fn redirect(path: &'static str, to: &str) -> (&'static str, Vec<u8>) {
        (path, response("302 Found", &[("location", to)], b""))
    }

    #[test]
    fn adres_secimi_izin_ve_postaya_bagli() {
        let html = r#"<p>x</p><img src="https://cdn.example/a.png?u=1&amp;v=2"><img src="https://cdn.example/b.png">"#;
        let a = "https://cdn.example/a.png?u=1&v=2".to_owned();
        let b = "https://cdn.example/b.png".to_owned();
        let other = "https://baska.example/c.png".to_owned();

        // İzin yok: hiçbir adres indirilmez, ne yapılacağı söylenir.
        let err = select_urls(false, Some(html), std::slice::from_ref(&a)).unwrap_err();
        assert_eq!(err.code, ErrorCode::Invalid);
        assert!(
            err.message.contains("Bu göndericiye izin ver"),
            "{}",
            err.message
        );
        // Gövde yerelde yok.
        assert_eq!(
            select_urls(true, None, std::slice::from_ref(&a))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        // Postada geçmeyen adres atlanır, yinelenen ve boşluklu adres tekilleşir.
        let got = select_urls(
            true,
            Some(html),
            &[
                other.clone(),
                a.clone(),
                format!(" {a} "),
                b.clone(),
                a.clone(),
            ],
        )
        .unwrap();
        assert_eq!(got, vec![a.clone(), b]);
        // Geçerli adres kalmazsa hata.
        assert_eq!(
            select_urls(true, Some(html), &[other]).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            select_urls(true, Some(html), &[]).unwrap_err().code,
            ErrorCode::Invalid
        );
        // Engellenmiş (izin verilmemiş gibi temizlenmiş) HTML'de adres yoktur.
        let blocked = r#"<img data-mh-src="https://cdn.example/a.png?u=1&amp;v=2">"#;
        assert_eq!(
            select_urls(true, Some(blocked), std::slice::from_ref(&a))
                .unwrap_err()
                .code,
            ErrorCode::Invalid
        );
        // Çağrı başına en çok MAX_URLS adres.
        let many: String = (0..MAX_URLS + 10)
            .map(|i| format!(r#"<img src="https://cdn.example/{i}.png">"#))
            .collect();
        let req: Vec<String> = (0..MAX_URLS + 10)
            .map(|i| format!("https://cdn.example/{i}.png"))
            .collect();
        let got = select_urls(true, Some(&many), &req).unwrap();
        assert_eq!(got.len(), MAX_URLS);
        assert_eq!(got[0], req[0]);
    }

    #[test]
    fn gorsel_turu_denetimi() {
        assert_eq!(
            image_mime("image/PNG; charset=binary").as_deref(),
            Some("image/png")
        );
        assert_eq!(
            image_mime(" image/svg+xml ").as_deref(),
            Some("image/svg+xml")
        );
        assert_eq!(
            image_mime("image/vnd.microsoft.icon").as_deref(),
            Some("image/vnd.microsoft.icon")
        );
        for bad in [
            "text/html",
            "image/",
            "image",
            "application/octet-stream",
            "image/png\r\nx: y",
            "image/p ng",
            "",
        ] {
            assert_eq!(image_mime(bad), None, "{bad}");
        }
    }

    #[test]
    fn bu_bilgisayar_ve_link_local_disi_adresler_indirilebilir() {
        for bad in [
            "127.0.0.1",
            "127.9.9.9",
            "0.0.0.0",
            "169.254.169.254",
            "224.0.0.251",
            "255.255.255.255",
            "::1",
            "::",
            "::ffff:127.0.0.1",
            "::ffff:169.254.1.1",
            "fe80::1",
            "ff02::1",
        ] {
            assert!(!is_fetchable_ip(bad.parse().unwrap()), "{bad}");
        }
        // Genel ve özel ağ (şirket içi bülten) serbest.
        for good in [
            "1.1.1.1",
            "93.184.216.34",
            "10.0.0.5",
            "192.168.1.20",
            "172.16.0.9",
            "100.64.0.1",
            "2606:4700:4700::1111",
            "fd12:3456::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_fetchable_ip(good.parse().unwrap()), "{good}");
        }
    }

    #[tokio::test]
    async fn yerel_hedefler_ve_bozuk_adresler_istek_gonderilmeden_reddedilir() {
        let (port, seen) = http_server(vec![png_route("/a.png")]).await;
        let f = Fetcher::new(Policy::default());
        for url in [
            format!("http://127.0.0.1:{port}/a.png"),
            format!("http://[::1]:{port}/a.png"),
            format!("http://[::ffff:127.0.0.1]:{port}/a.png"),
            format!("http://localhost:{port}/a.png"),
            "http://169.254.169.254/latest/meta-data".to_owned(),
            "http://0.0.0.0/a.png".to_owned(),
        ] {
            assert_eq!(f.fetch(&url).await, Err(Skip::Blocked), "{url}");
        }
        for url in [
            "ftp://ornek.example/a.png",
            "https://kullanici:parola@ornek.example/a.png",
            "https://kullanici@ornek.example/a.png",
            "data:image/png;base64,AAAA",
            "bozuk",
            "https://",
        ] {
            assert_eq!(f.fetch(url).await, Err(Skip::BadUrl), "{url}");
        }
        assert!(seen.lock().unwrap().is_empty(), "hiç istek gitmedi");
    }

    #[tokio::test]
    async fn gorsel_cerezsiz_iner_ve_veri_adresine_doner() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, seen) = http_server(vec![png_route("/a.png"), png_route("/b.png")]).await;
        let f = Fetcher::new(local(MAX_IMAGE_BYTES, MAX_TOTAL_BYTES));
        let expected = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(PNG)
        );
        assert_eq!(
            f.fetch(&format!("http://localhost:{port}/a.png?iz=42"))
                .await,
            Ok(expected.clone())
        );
        // Aynı sunucu: istemci yeniden kullanılır, ilk yanıtın çerezi gitmez.
        assert_eq!(
            f.fetch(&format!("http://localhost:{port}/b.png")).await,
            Ok(expected)
        );
        assert_eq!(f.clients.lock().unwrap().len(), 1);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[0].starts_with("GET /a.png?iz=42 HTTP/1.1\r\n"),
            "{}",
            seen[0]
        );
        assert!(
            seen[1].starts_with("GET /b.png HTTP/1.1\r\n"),
            "{}",
            seen[1]
        );
        for req in &seen {
            let lower = req.to_ascii_lowercase();
            assert!(lower.contains("\r\naccept: image/*\r\n"), "{req}");
            assert!(lower.contains("\r\nuser-agent: mailhere/"), "{req}");
            assert!(!lower.contains("\r\ncookie:"), "{req}");
            assert!(!lower.contains("\r\nreferer:"), "{req}");
            assert!(!lower.contains("\r\nauthorization:"), "{req}");
        }
        // Toplam bütçeden düşülmüştür.
        assert_eq!(
            f.remaining.load(Ordering::SeqCst),
            MAX_TOTAL_BYTES - 2 * PNG.len()
        );
    }

    #[tokio::test]
    async fn gorsel_olmayan_yanit_ve_hata_durumu_atlanir() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, _) = http_server(vec![
            (
                "/sayfa",
                response(
                    "200 OK",
                    &[("content-type", "text/html; charset=utf-8")],
                    b"<svg onload=alert(1)>",
                ),
            ),
            ("/tipsiz", response("200 OK", &[], PNG)),
            (
                "/ikili",
                response(
                    "200 OK",
                    &[("content-type", "application/octet-stream")],
                    PNG,
                ),
            ),
        ])
        .await;
        let f = Fetcher::new(local(MAX_IMAGE_BYTES, MAX_TOTAL_BYTES));
        let base = format!("http://localhost:{port}");
        assert_eq!(f.fetch(&format!("{base}/sayfa")).await, Err(Skip::NotImage));
        assert_eq!(
            f.fetch(&format!("{base}/tipsiz")).await,
            Err(Skip::NotImage)
        );
        assert_eq!(f.fetch(&format!("{base}/ikili")).await, Err(Skip::NotImage));
        assert_eq!(
            f.fetch(&format!("{base}/yok")).await,
            Err(Skip::Status(404))
        );
        // Yanıt vermeyen kapı: bağlantı hatası.
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead = closed.local_addr().unwrap().port();
        drop(closed);
        assert_eq!(
            f.fetch(&format!("http://127.0.0.1:{dead}/a.png")).await,
            Err(Skip::Request)
        );
    }

    #[tokio::test]
    async fn boyut_sinirlari_gorsel_ve_toplam() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let big = vec![7u8; 20];
        let (port, seen) = http_server(vec![
            png_route("/a.png"),
            png_route("/b.png"),
            (
                "/buyuk",
                response("200 OK", &[("content-type", "image/gif")], &big),
            ),
            ("/akis", unsized_response("image/gif", &big)),
        ])
        .await;
        let base = format!("http://localhost:{port}");
        // Görsel başına 12 bayt: bildirilen uzunluk da, akan gövde de kesilir.
        let f = Fetcher::new(local(12, MAX_TOTAL_BYTES));
        assert_eq!(f.fetch(&format!("{base}/buyuk")).await, Err(Skip::TooLarge));
        assert_eq!(f.fetch(&format!("{base}/akis")).await, Err(Skip::TooLarge));
        assert!(f.fetch(&format!("{base}/a.png")).await.is_ok());
        // Çağrı başına toplam: ikinci görsel bütçeye sığmaz.
        let f = Fetcher::new(local(MAX_IMAGE_BYTES, PNG.len() + 3));
        assert!(f.fetch(&format!("{base}/a.png")).await.is_ok());
        assert_eq!(f.fetch(&format!("{base}/b.png")).await, Err(Skip::Budget));
        // Uzunluğu bildirilmeyen gövde de bütçeyi aşamaz.
        let f = Fetcher::new(local(MAX_IMAGE_BYTES, 5));
        assert_eq!(f.fetch(&format!("{base}/akis")).await, Err(Skip::Budget));
        assert!(seen.lock().unwrap().len() >= 5);
    }

    #[tokio::test]
    async fn yonlendirme_sinirli_ve_yalniz_http() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, seen) = http_server(vec![
            png_route("/a.png"),
            redirect("/r1", "/r2"),
            redirect("/r2", "/r3"),
            redirect("/r3", "/a.png"),
            redirect("/z1", "/z2"),
            redirect("/z2", "/z3"),
            redirect("/z3", "/z4"),
            redirect("/z4", "/a.png"),
            redirect("/ftp", "ftp://ornek.example/a.png"),
            redirect("/veri", "data:image/png;base64,AAAA"),
            redirect("/yerel", "http://169.254.169.254/x.png"),
            ("/hedefsiz", response("302 Found", &[], b"")),
        ])
        .await;
        let f = Fetcher::new(local(MAX_IMAGE_BYTES, MAX_TOTAL_BYTES));
        let base = format!("http://localhost:{port}");
        // Üç adım izlenir.
        assert!(f.fetch(&format!("{base}/r1")).await.is_ok());
        assert_eq!(seen.lock().unwrap().len(), 4);
        // Dördüncü adım izlenmez.
        assert_eq!(f.fetch(&format!("{base}/z1")).await, Err(Skip::Redirects));
        assert_eq!(
            seen.lock().unwrap().len(),
            8,
            "dördüncü yönlendirme izlenmedi"
        );
        assert_eq!(f.fetch(&format!("{base}/ftp")).await, Err(Skip::BadUrl));
        assert_eq!(f.fetch(&format!("{base}/veri")).await, Err(Skip::BadUrl));
        assert_eq!(
            f.fetch(&format!("{base}/hedefsiz")).await,
            Err(Skip::Redirects)
        );
        // Yönlendirilen hedef de kurala vurulur.
        assert_eq!(f.fetch(&format!("{base}/yerel")).await, Err(Skip::Blocked));
    }

    #[tokio::test]
    async fn toplu_indirmede_yalniz_inenler_doner() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (port, _) = http_server(vec![
            png_route("/a.png"),
            (
                "/sayfa",
                response("200 OK", &[("content-type", "text/html")], b"x"),
            ),
        ])
        .await;
        let ok = format!("http://localhost:{port}/a.png");
        let urls = vec![
            ok.clone(),
            format!("http://localhost:{port}/sayfa"),
            format!("http://localhost:{port}/yok"),
            "bozuk".to_owned(),
        ];
        let got = download(urls, local(MAX_IMAGE_BYTES, MAX_TOTAL_BYTES)).await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].url, ok);
        assert!(got[0].data.starts_with("data:image/png;base64,"));
        assert_eq!(host_of(&ok), "localhost");
        assert_eq!(host_of("bozuk"), "?");
    }
}
