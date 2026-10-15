//! Gizlilik: izleme pikseli tanıma, bağlantılardan izleme parametresi sökme
//! ve bağlantı metni ile gerçek hedefin alan adını karşılaştırma.
//!
//! Alan adı, yol ve parametre listeleri bu proje için yazılmıştır (temiz oda);
//! kısa tutulur, yalnız izleme amaçlı olduğu bilinen adlar girer. Yönlendirme
//! sarmalayıcılarına dokunulmaz: hedefi bozabilir, yalnız sorgu parametresi
//! atılır.

/// Yalnız izleme için kullanılan sunucular; bunlardan gelen görsel hiç
/// yüklenmez. Eşleşme etiket sınırında sonektir (`us5.list-manage.com`).
const TRACKER_HOSTS: &[&str] = &[
    "google-analytics.com",
    "doubleclick.net",
    "list-manage.com",
    "ct.sendgrid.net",
    "mandrillapp.com",
    "mailtrack.io",
    "mailfoogae.appspot.com",
    "t.yesware.com",
    "bananatag.com",
    "getnotify.com",
    "exct.net",
    "pi.pardot.com",
    "rs6.net",
    "emltrk.com",
    "pixel.wp.com",
    "bat.bing.com",
    "awstrack.me",
    "api.mixpanel.com",
    "track.hubspot.com",
    "pixel.mathtag.com",
    "mlsend.com",
    "sendibt3.com",
    "cmail19.com",
    "cmail20.com",
    "track.customer.io",
    "pstmrk.it",
];

/// Açılma bildiren görsellerin yaygın yol adları (küçük harf, sorgusuz).
const TRACKER_PATH_ENDS: &[&str] = &[
    "/open.php",
    "/open.gif",
    "/open.aspx",
    "/pixel.gif",
    "/pixel.png",
    "/beacon.gif",
    "/trk.gif",
];
const TRACKER_PATH_PARTS: &[&str] = &["/track/open", "/tracking/open", "/wf/open"];

/// Bağlantıdan atılan izleme parametreleri (küçük harf, tam ad).
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "yclid",
    "igshid",
    "twclid",
    "ttclid",
    "li_fat_id",
    "mc_eid",
    "mc_cid",
    "_hsenc",
    "_hsmi",
    "__hstc",
    "__hssc",
    "__hsfp",
    "hsctatracking",
    "mkt_tok",
    "_kx",
    "ck_subscriber_id",
    "vero_id",
    "vero_conv",
    "_ga",
    "_gl",
    "rb_clickid",
    "irclickid",
];

/// Bu öneklerle başlayan parametreler de atılır (`utm_source`, `oly_enc_id` …).
const TRACKING_PREFIXES: &[&str] = &["utm_", "oly_", "mtm_", "pk_", "hsa_"];

/// Sayısal boyut (`1`, `1px`, `0.5`); yüzde, `auto` ve diğer birimler `None`.
fn dimension(v: &str) -> Option<f32> {
    let v = v.trim().trim_end_matches("!important").trim();
    let n = v.strip_suffix("px").unwrap_or(v).trim();
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    n.parse().ok()
}

/// `style` değerinden bir özelliğin değeri (küçük harf).
fn style_value(style: &str, name: &str) -> Option<String> {
    style.split(';').find_map(|decl| {
        let (k, v) = decl.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_ascii_lowercase())
    })
}

/// Adresin sunucu adı (küçük harf, kullanıcı bilgisi ve kapı atılmış).
/// Yalnız `http`/`https`; başka şema `None`.
pub fn url_host(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = ["https://", "http://"].iter().find_map(|p| {
        url.get(..p.len())
            .filter(|h| h.eq_ignore_ascii_case(p))
            .map(|_| &url[p.len()..])
    })?;
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
    // `https://banka.com@kotu.example/` gerçekte kotu.example'a gider.
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = host.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    let host = host.trim_end_matches('.').to_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Adresin yolu (sorgu ve parça hariç, küçük harf).
fn url_path(url: &str) -> String {
    let after_scheme = url.split_once("//").map_or(url, |(_, r)| r);
    let path = after_scheme
        .find('/')
        .map_or("", |i| &after_scheme[i..])
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    path.to_ascii_lowercase()
}

fn host_matches(host: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|d| host == *d || host.strip_suffix(d).is_some_and(|head| head.ends_with('.')))
}

/// Uzak görsel bir izleyici mi? `width`, `height` ve `style` temizlenmiş
/// öznitelik değerleridir.
///
/// İzleyici sayılan: 1×1 ya da daha küçük (bir boyutu verilmemişse öteki
/// yeter), sıfır boyutlu, gizli (`display:none`, `visibility:hidden`,
/// `opacity:0`, en büyük boyutu 0) ya da bilinen bir izleme sunucusundan ya da
/// açılma yolundan gelen görsel.
pub fn is_tracker_image(
    url: &str,
    width: Option<&str>,
    height: Option<&str>,
    style: Option<&str>,
) -> bool {
    let style = style.unwrap_or("");
    let sv = |name: &str| style_value(style, name);
    let dim = |name: &str, attr: Option<&str>| {
        sv(name)
            .as_deref()
            .and_then(dimension)
            .or_else(|| attr.and_then(dimension))
    };
    let (w, h) = (dim("width", width), dim("height", height));
    let zero = |v: Option<String>| v.as_deref().and_then(dimension) == Some(0.0);
    let hidden = sv("display").is_some_and(|v| v.starts_with("none"))
        || sv("visibility").is_some_and(|v| v.starts_with("hidden"))
        || zero(sv("opacity"))
        || zero(sv("max-width"))
        || zero(sv("max-height"))
        || w == Some(0.0)
        || h == Some(0.0);
    let tiny = |d: Option<f32>| d.is_some_and(|v| v <= 1.0);
    let pixel = (tiny(w) && (tiny(h) || h.is_none())) || (tiny(h) && w.is_none());
    if hidden || pixel {
        return true;
    }
    if url_host(url).is_some_and(|h| host_matches(&h, TRACKER_HOSTS)) {
        return true;
    }
    let path = url_path(url);
    TRACKER_PATH_ENDS.iter().any(|e| path.ends_with(e))
        || TRACKER_PATH_PARTS.iter().any(|p| path.contains(p))
}

fn is_tracking_param(pair: &str) -> bool {
    let key = pair.split('=').next().unwrap_or("").trim();
    if key.is_empty() {
        return false;
    }
    let key = key.to_ascii_lowercase();
    TRACKING_PARAMS.contains(&key.as_str()) || TRACKING_PREFIXES.iter().any(|p| key.starts_with(p))
}

/// `http`/`https` adresinden izleme parametrelerini söker. Değişiklik yoksa
/// `None`. Parametrelerin sırası ve yazımı korunur; parça (`#…`) dokunulmadan
/// kalır, adres yeniden kodlanmaz.
pub fn clean_url(url: &str) -> Option<String> {
    url_host(url)?;
    let (base, fragment) = match url.find('#') {
        Some(i) => url.split_at(i),
        None => (url, ""),
    };
    let (path, query) = base.split_once('?')?;
    let pairs: Vec<&str> = query.split('&').collect();
    let kept: Vec<&str> = pairs
        .iter()
        .copied()
        .filter(|p| !is_tracking_param(p))
        .collect();
    if kept.len() == pairs.len() {
        return None;
    }
    let kept: Vec<&str> = kept.into_iter().filter(|p| !p.is_empty()).collect();
    let mut out = String::with_capacity(url.len());
    out.push_str(path);
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out.push_str(fragment);
    Some(out)
}

/// Bağlantı metni bir adres ya da alan adı gibi görünüyorsa sunucu adı
/// ("www.banka.com", "https://banka.com/giris", "banka.com.tr").
pub fn link_text_host(text: &str) -> Option<String> {
    let t = text.trim().trim_end_matches(['.', ',', ';', ':', ')', '!']);
    if t.is_empty() || t.len() > 300 || t.chars().any(char::is_whitespace) || t.contains('@') {
        return None;
    }
    let lower = t.to_lowercase();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("");
    let host = host.trim_end_matches('.');
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty() || l.len() > 63) {
        return None;
    }
    let tld = labels[labels.len() - 1];
    if tld.chars().count() < 2 || !tld.chars().all(char::is_alphabetic) {
        return None;
    }
    let ok = labels
        .iter()
        .all(|l| l.chars().all(|c| c.is_alphanumeric() || c == '-'));
    ok.then(|| host.to_owned())
}

/// Ülke alan adlarının altında yaygın ikinci düzey adlar (`com.tr`, `co.uk`).
const SECOND_LEVEL: &[&str] = &[
    "com", "net", "org", "gov", "edu", "ac", "co", "gen", "web", "bel", "k12", "av", "dr", "pol",
    "tsk", "biz", "info", "name", "nom", "tv", "ltd", "plc", "sch", "mil",
];

/// Yaklaşık kayıtlı alan adı: son iki etiket; ülke alan adında ikinci
/// düzey yaygın bir adsa son üç etiket.
fn site(host: &str) -> &str {
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return host;
    }
    let take = if labels[n - 1].len() == 2 && SECOND_LEVEL.contains(&labels[n - 2]) {
        3
    } else {
        2
    };
    let skip: usize = labels[..n - take].iter().map(|l| l.len() + 1).sum();
    &host[skip..]
}

/// İki sunucu adı aynı siteye mi ait? (`www.` ve alt alan adları aynı sayılır.)
pub fn same_site(a: &str, b: &str) -> bool {
    let norm = |h: &str| {
        let h = h.trim_end_matches('.').to_lowercase();
        h.strip_prefix("www.").map(str::to_owned).unwrap_or(h)
    };
    let (a, b) = (norm(a), norm(b));
    a == b || site(&a) == site(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piksel_tanima() {
        let px = "https://iz.example/a.gif";
        assert!(is_tracker_image(px, Some("1"), Some("1"), None));
        assert!(is_tracker_image(px, Some("1px"), None, None));
        assert!(is_tracker_image(px, None, Some("0"), None));
        assert!(is_tracker_image(
            px,
            None,
            None,
            Some("width:1px;height:1px")
        ));
        assert!(is_tracker_image(px, None, None, Some("display: none")));
        assert!(is_tracker_image(px, None, None, Some("visibility:hidden")));
        assert!(is_tracker_image(px, None, None, Some("opacity:0")));
        assert!(is_tracker_image(
            px,
            None,
            None,
            Some("max-height:0px; border:0")
        ));
        assert!(is_tracker_image(
            px,
            Some("600"),
            Some("1"),
            Some("width:1px")
        ));
        // Olağan görseller.
        assert!(!is_tracker_image(px, None, None, None));
        assert!(!is_tracker_image(px, Some("600"), Some("200"), None));
        assert!(!is_tracker_image(
            px,
            Some("560"),
            Some("220"),
            Some("display:block;width:100%;height:auto")
        ));
        assert!(!is_tracker_image(px, Some("1"), Some("40"), None));
        assert!(!is_tracker_image(px, None, None, Some("opacity:0.8")));
    }

    #[test]
    fn bilinen_izleme_sunuculari_ve_yollari() {
        for url in [
            "https://us5.list-manage.com/track/open.php?u=1",
            "https://u123.ct.sendgrid.net/wf/open?upn=x",
            "https://www.google-analytics.com/collect?v=1",
            "HTTPS://Mailtrack.IO/trace/mail/abc.png",
            "https://haber.ornek.com/open.gif?id=5",
            "https://posta.ornek.com/e/track/open/1",
        ] {
            assert!(
                is_tracker_image(url, Some("600"), Some("200"), None),
                "{url}"
            );
        }
        for url in [
            "https://notlist-manage.com/kapak.png",
            "https://ornek.com/kapak.png",
            "https://ornek.com/images/opengraph.gif",
        ] {
            assert!(
                !is_tracker_image(url, Some("600"), Some("200"), None),
                "{url}"
            );
        }
    }

    #[test]
    fn izleme_parametreleri_sokulur() {
        assert_eq!(
            clean_url("https://ornek.com/yazi?utm_source=bulten&id=7&UTM_Medium=e&fbclid=x#bolum")
                .as_deref(),
            Some("https://ornek.com/yazi?id=7#bolum")
        );
        assert_eq!(
            clean_url("https://ornek.com/?gclid=1&mc_eid=2&_hsenc=3&mkt_tok=4").as_deref(),
            Some("https://ornek.com/")
        );
        assert_eq!(
            clean_url("https://ornek.com/a?oly_enc_id=1&q=kitap%20ara").as_deref(),
            Some("https://ornek.com/a?q=kitap%20ara")
        );
        // Değişiklik yoksa dokunulmaz; parça içindeki parametre kalır.
        assert_eq!(clean_url("https://ornek.com/a?id=1&ref=2"), None);
        assert_eq!(clean_url("https://ornek.com/a#utm_source=x"), None);
        assert_eq!(clean_url("https://ornek.com/a"), None);
        assert_eq!(clean_url("mailto:a@b.com?utm_source=x"), None);
        // Yönlendirme sarmalayıcısının içindeki adres çözülmez.
        assert_eq!(
            clean_url("https://tik.esp.example/r?url=https%3A%2F%2Fornek.com%3Futm_source%3Dx")
                .as_deref(),
            None
        );
    }

    #[test]
    fn sunucu_adi() {
        assert_eq!(
            url_host("https://Banka.com/giris").as_deref(),
            Some("banka.com")
        );
        assert_eq!(
            url_host("https://banka.com@kotu.example:8443/x").as_deref(),
            Some("kotu.example")
        );
        assert_eq!(url_host("http://[::1]:80/").as_deref(), Some("::1"));
        assert_eq!(url_host("mailto:a@b.com"), None);
        assert_eq!(url_host("https://"), None);
    }

    #[test]
    fn baglanti_metni_adres_gibi() {
        assert_eq!(
            link_text_host("www.banka.com").as_deref(),
            Some("www.banka.com")
        );
        assert_eq!(
            link_text_host(" https://Banka.com.tr/giris?x=1 ").as_deref(),
            Some("banka.com.tr")
        );
        assert_eq!(link_text_host("banka.com.").as_deref(), Some("banka.com"));
        for no in [
            "Hemen giriş yap",
            "Sayı 142",
            "1.2.3",
            "v2.0",
            "a@b.com",
            "",
            "ornek",
        ] {
            assert_eq!(link_text_host(no), None, "{no}");
        }
    }

    #[test]
    fn ayni_site() {
        assert!(same_site("banka.com", "www.banka.com"));
        assert!(same_site("banka.com", "tik.posta.banka.com"));
        assert!(same_site("www.banka.com.tr", "kampanya.banka.com.tr"));
        assert!(!same_site("banka.com", "banka.com.kotu.example"));
        assert!(!same_site("banka.com.tr", "kotu.com.tr"));
        assert!(!same_site("banka.com", "banka-giris.com"));
    }
}
