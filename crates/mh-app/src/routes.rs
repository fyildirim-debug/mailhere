//! Arayüz rotaları ve adres denetimi; iki kabuk da aynı sayfaları kullanır
//! (`index.html#/<rota>`).

use mh_core::{ComposeMode, ComposePrefill};

/// Pencere yalnız uygulamanın kendi sayfalarında kalabilir: posta içindeki
/// ya da bir betiğin açtığı dış adres (oltalama sayfası, `file://`) uygulama
/// penceresinde açılmaz. Dış bağlantılar varsayılan tarayıcıya gider.
///
/// `dev_host` yapılandırmadaki geliştirme sunucusunun ana makinesidir
/// (`devUrl`); yalnız geliştirme derlemesinde ve yalnız o makine için izin
/// verilir. Sürüm derlemesinde değer ne olursa olsun yok sayılır.
pub fn is_app_url(url: &tauri::Url, dev_host: Option<&str>) -> bool {
    match url.scheme() {
        "tauri" | "about" | "data" | "blob" => true,
        "http" | "https" => match url.host_str() {
            Some("tauri.localhost") => true,
            // Geliştirme sunucusu (`pnpm tauri dev`, `pnpm tauri android dev`).
            Some(host) => cfg!(debug_assertions) && dev_host == Some(host),
            None => false,
        },
        _ => false,
    }
}

pub fn mode_route(mode: ComposeMode) -> &'static str {
    match mode {
        ComposeMode::New => "new",
        ComposeMode::Reply => "reply",
        ComposeMode::ReplyAll => "replyAll",
        ComposeMode::Forward => "forward",
        ComposeMode::Draft => "draft",
    }
}

/// Önceden doldurulan en çok alıcı.
pub const MAX_PREFILL_TO: usize = 20;

/// Yazma penceresinin rotası: `compose/<kip>/<posta>`; alıcılar varsa
/// `?to=Ad <adres>`, hesap verilmişse `from=<hesap>` sorgu parametreleri
/// (arayüz çözer).
pub fn compose_route(prefill: &ComposePrefill) -> String {
    let mut route = format!(
        "compose/{}/{}",
        mode_route(prefill.mode),
        prefill.message_id.unwrap_or(0)
    );
    let to: Vec<&mh_core::Address> = prefill
        .to
        .iter()
        .filter(|a| a.email.contains('@') && !a.email.chars().any(char::is_whitespace))
        .take(MAX_PREFILL_TO)
        .collect();
    // Hesap kimliği yalnız güvenli karakterlerden oluşuyorsa taşınır.
    let from = prefill.account_id.as_deref().filter(|id| {
        !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    });
    if to.is_empty() && from.is_none() {
        return route;
    }
    let mut url = tauri::Url::parse("mailhere://compose/").expect("sabit adres");
    {
        let mut q = url.query_pairs_mut();
        if let Some(id) = from {
            q.append_pair("from", id);
        }
        for a in to {
            let value = match a
                .name
                .as_deref()
                .map(mh_send::clean_name)
                .filter(|n| !n.is_empty())
            {
                Some(name) => format!("{name} <{}>", a.email.trim()),
                None => a.email.trim().to_owned(),
            };
            q.append_pair("to", &value);
        }
    }
    if let Some(q) = url.query() {
        route.push('?');
        route.push_str(q);
    }
    route
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yalniz_uygulama_adresleri() {
        let ok = |u: &str| is_app_url(&tauri::Url::parse(u).unwrap(), Some("localhost"));
        assert!(ok("tauri://localhost/index.html#/main"));
        assert!(ok("http://tauri.localhost/index.html#/quick/3"));
        assert!(ok("about:srcdoc"));
        assert!(!ok("https://banka-giris.example/"));
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("http://tauri.localhost.evil.example/"));
    }

    #[test]
    fn gelistirme_sunucusu_yalniz_yapilandirmadaki_makine() {
        let dev = |u: &str, host: Option<&str>| is_app_url(&tauri::Url::parse(u).unwrap(), host);
        // Geliştirme derlemesinde yapılandırmadaki makineye izin var; başkasına yok.
        assert_eq!(
            dev("http://localhost:5173/", Some("localhost")),
            cfg!(debug_assertions)
        );
        assert_eq!(
            dev("http://192.168.1.20:5173/", Some("192.168.1.20")),
            cfg!(debug_assertions)
        );
        assert!(!dev("http://localhost:5173/", Some("192.168.1.20")));
        assert!(!dev("http://localhost:5173/", None));
        assert!(!dev("http://localhost.evil.example/", Some("localhost")));
    }

    #[test]
    fn yazma_rotasi() {
        assert_eq!(mode_route(ComposeMode::ReplyAll), "replyAll");
        assert_eq!(mode_route(ComposeMode::New), "new");
        let plain = ComposePrefill {
            mode: ComposeMode::Reply,
            message_id: Some(7),
            to: Vec::new(),
            account_id: None,
        };
        assert_eq!(compose_route(&plain), "compose/reply/7");
        let with_to = ComposePrefill {
            mode: ComposeMode::New,
            message_id: None,
            to: vec![
                mh_core::Address::new(Some("Ayşe Demir"), "ayse@ornek.com"),
                mh_core::Address::new(None, "bozuk adres"),
                mh_core::Address::new(Some("Evil\r\nX"), "e@ornek.com"),
            ],
            account_id: None,
        };
        let route = compose_route(&with_to);
        assert!(route.starts_with("compose/new/0?to="), "{route}");
        assert!(
            route.contains("Ay%C5%9Fe+Demir+%3Cayse%40ornek.com%3E"),
            "{route}"
        );
        assert!(!route.contains("bozuk"), "boşluklu adres atılır");
        assert!(route.contains("to=Evil+X+%3Ce%40ornek.com%3E"), "{route}");
        assert!(!route.contains('\n'));
        // Hesap kimliği taşınır; güvensiz kimlik atılır.
        let from = ComposePrefill {
            mode: ComposeMode::New,
            message_id: None,
            to: Vec::new(),
            account_id: Some("demo-is".into()),
        };
        assert_eq!(compose_route(&from), "compose/new/0?from=demo-is");
        let bad = ComposePrefill {
            account_id: Some("a&to=x@y.z".into()),
            ..from
        };
        assert_eq!(compose_route(&bad), "compose/new/0");
    }
}
