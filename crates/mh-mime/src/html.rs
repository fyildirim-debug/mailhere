//! HTML posta temizleme (ammonia).
//!
//! Betik, stil etiketi, form, çerçeve ve olay öznitelikleri atılır. Bağlantılar
//! yeni pencerede ve `noopener noreferrer` ile açılır; yalnız http, https ve
//! mailto kabul edilir. Uzak görseller izin yoksa `data-mh-src` özniteliğine
//! taşınır; `cid:` görseller postanın kendi parçalarından `data:` adresine
//! çevrilir.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ammonia::{Builder, UrlRelative};

use crate::prescan::{canonicalize, strip_tags, text_to_html};
use crate::text::tidy_lines;

/// Temizlenmiş HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    pub html: String,
    /// Postada uzak görsel var (izin verilmiş olsa da).
    pub has_remote: bool,
}

const EXTRA_TAGS: &[&str] = &["font", "center"];
const DROP_WITH_CONTENT: &[&str] = &[
    "script", "style", "title", "noscript", "template", "iframe", "object", "embed", "svg", "math",
    "select", "textarea",
];
const GENERIC_ATTRS: &[&str] = &[
    "style",
    "align",
    "valign",
    "width",
    "height",
    "bgcolor",
    "color",
    "dir",
    "border",
    "cellpadding",
    "cellspacing",
    "colspan",
    "rowspan",
    "face",
    "size",
    "lang",
    "title",
];

fn is_http(v: &str) -> bool {
    let l = v.trim_start().get(..8).unwrap_or("").to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

fn strip_prefix_ci<'a>(v: &'a str, prefix: &str) -> Option<&'a str> {
    let head = v.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &v[prefix.len()..])
}

/// `cid:` adresini Content-ID anahtarına çevirir.
pub fn normalize_cid(cid: &str) -> String {
    cid.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_ascii_lowercase()
}

/// Satır içi stilde izin verilen özellikler: postada yaygın, kaynak
/// yükleyemeyen ve konum/katman oyunlarına izin vermeyenler. Listede olmayan
/// her bildirim (ör. `background-image`, `content`, `cursor`, `behavior`,
/// `position`) atılır.
const CSS_PROPERTIES: &[&str] = &[
    "color",
    "background",
    "background-color",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "letter-spacing",
    "line-height",
    "word-spacing",
    "word-break",
    "word-wrap",
    "overflow-wrap",
    "white-space",
    "text-align",
    "text-decoration",
    "text-decoration-color",
    "text-decoration-line",
    "text-decoration-style",
    "text-indent",
    "text-transform",
    "text-overflow",
    "text-shadow",
    "vertical-align",
    "direction",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-width",
    "border-style",
    "border-color",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-collapse",
    "border-spacing",
    "outline",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "display",
    "float",
    "clear",
    "overflow",
    "overflow-x",
    "overflow-y",
    "visibility",
    "opacity",
    "box-sizing",
    "box-shadow",
    "table-layout",
    "caption-side",
    "empty-cells",
    "list-style",
    "list-style-type",
    "list-style-position",
];

/// Değerde izin verilen CSS işlevleri; başka her işlev (`url`, `src`,
/// `image`, `image-set`, `var`, `expression` …) bildirimi düşürür.
const CSS_FUNCTIONS: &[&str] = &["rgb", "rgba", "hsl", "hsla", "calc"];

/// Satır içi `style` değerini izin listesiyle süzer.
///
/// Yalnız [`CSS_PROPERTIES`] içindeki özellikler kalır. Değerde ters bölü
/// (CSS kaçışı `u\72 l(`), yorum (`/*`, `*/`), `@`, süslü/açılı parantez,
/// denetim karakteri ya da izinli olmayan bir işlev varsa bildirim atılır.
/// Tırnak yalnız yazı tipi adlarında (`font`, `font-family`) ve dengeliyse
/// kabul edilir. Yorumları ayıklamaya çalışmak yerine reddetmek, tırnak içinde
/// `/*` gibi ayrıştırıcı farklarından doğan kaçamakları kapatır.
pub fn sanitize_style(value: &str) -> Option<String> {
    let kept: Vec<&str> = value
        .split(';')
        .map(str::trim)
        .filter(|decl| css_declaration_ok(decl))
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

fn css_declaration_ok(decl: &str) -> bool {
    let Some((name, value)) = decl.split_once(':') else {
        return false;
    };
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim();
    if value.is_empty() || !CSS_PROPERTIES.contains(&name.as_str()) {
        return false;
    }
    let quotes_ok = matches!(name.as_str(), "font" | "font-family");
    if value.contains("/*") || value.contains("*/") {
        return false;
    }
    let (mut dq, mut sq) = (0usize, 0usize);
    for c in value.chars() {
        match c {
            '"' => dq += 1,
            '\'' => sq += 1,
            '\\' | '{' | '}' | '<' | '>' | '@' | '`' => return false,
            c if c.is_control() => return false,
            _ => {}
        }
    }
    if (dq + sq > 0 && !quotes_ok) || dq % 2 != 0 || sq % 2 != 0 {
        return false;
    }
    // İşlev adı: `(` öncesindeki son ad (aradaki boşluk da sayılır; `url (`
    // CSS'te işlev olmasa da reddedilir).
    let lower = value.to_lowercase();
    let (mut start, mut end) = (0, 0);
    for (i, c) in lower.char_indices() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            if end != i {
                start = i;
            }
            end = i + c.len_utf8();
            continue;
        }
        if c.is_whitespace() {
            continue;
        }
        if c == '(' {
            let f = &lower[start..end];
            if !f.is_empty() && !CSS_FUNCTIONS.contains(&f) {
                return false;
            }
        }
        start = i + c.len_utf8();
        end = start;
    }
    true
}

fn escape_attr(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn nonce() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{t:x}{n:x}")
}

/// Okuma alanında html5ever'la işlenecek en büyük HTML (bayt); daha büyüğü
/// düz metin olarak gösterilir.
pub const RENDER_HTML_LIMIT: usize = 2 * 1024 * 1024;

/// HTML postayı temizler.
///
/// `inline`: küçük harfli Content-ID → `data:` adresi.
///
/// HTML [`RENDER_HTML_LIMIT`]'ten büyükse ya da iç içelik html5ever'ı karesel
/// süreye sokacak kadar derinse etiketler doğrusal olarak atılır ve metin
/// kaçışlı paragraflar olarak döner.
pub fn sanitize_html(
    html: &str,
    remote_allowed: bool,
    inline: HashMap<String, String>,
) -> Sanitized {
    let canon = (html.len() <= RENDER_HTML_LIMIT)
        .then(|| canonicalize(html))
        .flatten();
    let Some(canon) = canon else {
        return Sanitized {
            html: text_to_html(&tidy_lines(&strip_tags(html))),
            has_remote: false,
        };
    };
    let marker = format!("mh-blocked:{}:", nonce());
    let blocked: Arc<Mutex<Vec<String>>> = Arc::default();
    let has_remote = Arc::new(AtomicBool::new(false));

    let mut b = Builder::default();
    b.add_tags(EXTRA_TAGS)
        .clean_content_tags(DROP_WITH_CONTENT.iter().copied().collect::<HashSet<_>>())
        .add_generic_attributes(GENERIC_ATTRS)
        .add_tag_attributes("img", &["src", "alt", "width", "height", "border"])
        .add_tag_attributes("a", &["href", "title", "name"])
        .url_schemes(["http", "https", "mailto", "cid"].into_iter().collect())
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank")
        .strip_comments(true);
    {
        let marker = marker.clone();
        let blocked = blocked.clone();
        let has_remote = has_remote.clone();
        b.attribute_filter(move |element, attribute, value| {
            filter_attr(
                element,
                attribute,
                value,
                &FilterCtx {
                    remote_allowed,
                    inline: &inline,
                    marker: &marker,
                    blocked: &blocked,
                    has_remote: &has_remote,
                },
            )
        });
    }
    let out = b.clean(&canon).to_string();

    let urls = std::mem::take(&mut *blocked.lock().unwrap_or_else(|e| e.into_inner()));
    Sanitized {
        html: restore_blocked(out, &marker, &urls),
        has_remote: has_remote.load(Ordering::Relaxed),
    }
}

/// Engellenen görsellerin `src="<işaret><sıra>"` özniteliklerini tek geçişte
/// `data-mh-src="<adres>"` yapar (adres başına ayrı `replace` karesel olurdu).
fn restore_blocked(html: String, marker: &str, urls: &[String]) -> String {
    if urls.is_empty() {
        return html;
    }
    let needle = format!("src=\"{marker}");
    let extra: usize = urls.iter().map(|u| u.len() + 8).sum();
    let mut out = String::with_capacity(html.len() + extra);
    let mut rest = html.as_str();
    while let Some(i) = rest.find(&needle) {
        let after = &rest[i + needle.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let url = after[..digits]
            .parse::<usize>()
            .ok()
            .and_then(|n| urls.get(n));
        match url {
            Some(url) if after[digits..].starts_with('"') => {
                out.push_str(&rest[..i]);
                out.push_str("data-mh-src=\"");
                out.push_str(&escape_attr(url));
                out.push('"');
                rest = &after[digits + 1..];
            }
            _ => {
                out.push_str(&rest[..i + needle.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Temizlenmiş HTML'deki uzak (`http`/`https`) görsel adresleri, belge
/// sırasıyla ve yinelenenler atılarak.
///
/// Yalnız [`sanitize_html`] çıktısı içindir: etiketler küçük harfli,
/// öznitelik değerleri çift tırnaklı ve kaçışlıdır (`&amp;`, `&quot;`).
/// Adresler çözülmüş (`&amp;` → `&`) döner; böylece arayüzün
/// `img.getAttribute('src')` ile verdiği değerle birebir karşılaştırılabilir.
/// Engellenmiş görseller (`data-mh-src`) ve `data:`/`cid:` adresler girmez.
pub fn remote_image_urls(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut rest = html;
    while let Some(i) = rest.find("<img") {
        let after = &rest[i + 4..];
        // Etiketin sonu; kaçışlı değerlerde `>` bulunmaz.
        let Some(end) = after.find('>') else {
            break;
        };
        let tag = &after[..end];
        if let Some(url) = img_src(tag).map(decode_entities) {
            if is_http(&url) && seen.insert(url.clone()) {
                out.push(url);
            }
        }
        rest = &after[end + 1..];
    }
    out
}

/// `<img …>` etiketinin içindeki ` src="…"` değeri (çözülmemiş).
fn img_src(tag: &str) -> Option<&str> {
    let mut rest = tag;
    while let Some(i) = rest.find("src=\"") {
        // `data-mh-src="…"` gibi başka bir özniteliğin sonu olmasın.
        let boundary = rest[..i]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        let after = &rest[i + 5..];
        let end = after.find('"')?;
        if boundary {
            return Some(&after[..end]);
        }
        rest = &after[end + 1..];
    }
    None
}

/// HTML öznitelik değerindeki karakter göndermelerini çözer (`&amp;`,
/// `&quot;`, `&lt;`, `&gt;`, `&apos;`, `&nbsp;`, `&#NN;`, `&#xHH;`). Tanınmayan
/// gönderme olduğu gibi kalır.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(semi) = after[..after.len().min(12)].find(';') else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let name = &after[1..semi];
        let decoded = match name {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => name.strip_prefix('#').and_then(|n| {
                let code = match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                }?;
                char::from_u32(code)
            }),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

struct FilterCtx<'a> {
    remote_allowed: bool,
    inline: &'a HashMap<String, String>,
    marker: &'a str,
    blocked: &'a Mutex<Vec<String>>,
    has_remote: &'a AtomicBool,
}

fn filter_attr<'u>(
    element: &str,
    attribute: &str,
    value: &'u str,
    ctx: &FilterCtx,
) -> Option<Cow<'u, str>> {
    match (element, attribute) {
        ("img", "src") => {
            let v = value.trim();
            if let Some(cid) = strip_prefix_ci(v, "cid:") {
                return ctx.inline.get(&normalize_cid(cid)).cloned().map(Cow::Owned);
            }
            if !is_http(v) {
                return None;
            }
            ctx.has_remote.store(true, Ordering::Relaxed);
            if ctx.remote_allowed {
                return Some(Cow::Borrowed(value));
            }
            let mut list = ctx.blocked.lock().unwrap_or_else(|e| e.into_inner());
            list.push(v.to_owned());
            Some(Cow::Owned(format!("{}{}", ctx.marker, list.len() - 1)))
        }
        (_, "href") if strip_prefix_ci(value.trim(), "cid:").is_some() => None,
        (_, "style") => sanitize_style(value).map(Cow::Owned),
        _ => Some(Cow::Borrowed(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str, remote: bool) -> Sanitized {
        sanitize_html(html, remote, HashMap::new())
    }

    #[test]
    fn betik_form_cerceve_ve_olaylar_atilir() {
        let s = clean(
            r#"<div onclick="x()"><script>alert(1)</script><style>p{}</style>
               <form action="https://kotu.example"><input name="p"><button>Gönder</button></form>
               <iframe src="https://kotu.example"></iframe><p onmouseover="y()">Merhaba</p></div>"#,
            false,
        );
        let h = s.html.to_lowercase();
        for bad in [
            "script",
            "<style",
            "<form",
            "<input",
            "iframe",
            "onclick",
            "onmouseover",
            "alert",
        ] {
            assert!(!h.contains(bad), "{bad} kaldı: {h}");
        }
        assert!(h.contains("<p>merhaba</p>"));
        assert!(!s.has_remote);
    }

    #[test]
    fn baglantilar_yeni_pencerede_ve_guvenli_semalarla() {
        let s = clean(
            r#"<a href="https://ornek.com">a</a><a href="javascript:alert(1)">b</a><a href="mailto:x@y.com">c</a><a href="/goreli">d</a>"#,
            false,
        );
        assert!(s.html.contains(r#"href="https://ornek.com""#));
        assert!(s.html.contains(r#"target="_blank""#));
        assert!(s.html.contains(r#"rel="noopener noreferrer""#));
        assert!(s.html.contains("mailto:x@y.com"));
        assert!(!s.html.contains("javascript"));
        assert!(!s.html.contains("/goreli"));
    }

    #[test]
    fn uzak_gorsel_engellenir() {
        let s = clean(
            r#"<p>x</p><img src="https://iz.example/a.png?u=1&amp;v=2" alt="kapak">"#,
            false,
        );
        assert!(s.has_remote);
        assert!(!s.html.contains(" src="), "{}", s.html);
        assert!(
            s.html
                .contains(r#"data-mh-src="https://iz.example/a.png?u=1&amp;v=2""#),
            "{}",
            s.html
        );
        assert!(s.html.contains(r#"alt="kapak""#));
    }

    #[test]
    fn uzak_gorsel_izinle_kalir() {
        let s = clean(r#"<img src="https://iz.example/a.png">"#, true);
        assert!(s.has_remote);
        assert!(s.html.contains(r#"src="https://iz.example/a.png""#));
        assert!(!s.html.contains("data-mh-src"));
    }

    #[test]
    fn cid_gorsel_veri_adresine_doner() {
        let mut inline = HashMap::new();
        inline.insert(
            "logo@ornek".to_owned(),
            "data:image/png;base64,AAAA".to_owned(),
        );
        let s = sanitize_html(
            r#"<img src="cid:LOGO@ornek"><img src="cid:yok@ornek">"#,
            false,
            inline,
        );
        assert!(
            s.html.contains(r#"src="data:image/png;base64,AAAA""#),
            "{}",
            s.html
        );
        assert_eq!(s.html.matches("src=").count(), 1);
        assert!(!s.has_remote);
    }

    #[test]
    fn stil_temizlenir() {
        assert_eq!(
            sanitize_style(
                "color: red; background: URL(https://iz.example/x.png); font-weight:bold"
            )
            .as_deref(),
            Some("color: red; font-weight:bold")
        );
        assert_eq!(sanitize_style("width: expression(alert(1))"), None);
        assert_eq!(sanitize_style("background:u\\rl(x)"), None);
        assert_eq!(sanitize_style("background:ur/**/l(x)"), None);
        // CSS onaltılık kaçışı: `u\72 l(` = `url(`.
        assert_eq!(
            sanitize_style("color:red; background:u\\72 l(https://iz.example/t.gif)").as_deref(),
            Some("color:red")
        );
        assert_eq!(sanitize_style("background:\\75 rl(x)"), None);
        assert_eq!(sanitize_style("font-family:\"A\\\"B\""), None);
        let s = clean(
            r##"<p style="color:#333;background-image:u\72 l('https://iz.example/t.gif')">x</p>"##,
            false,
        );
        assert!(!s.html.contains("72"), "{}", s.html);
        assert!(s.html.contains("color:#333"), "{}", s.html);
        let s = clean(
            r##"<td style="color:#333;background-image:url('https://iz.example/t.gif')">x</td>"##,
            false,
        );
        assert!(!s.html.contains("url("), "{}", s.html);
    }

    #[test]
    fn stil_kacamaklari_atilir() {
        // Tırnak içindeki `/*` eski ayıklayıcıyı yoruma inandırıyordu.
        for bad in [
            r#"list-style:"/*" url(http://evil/t.gif?*/)"#,
            r#"list-style:'/*' url(http://evil/t.gif?*/)"#,
            r#"content:"/*" url(http://evil/t.gif?*/)"#,
            r#"background:src("http://evil/t.gif")"#,
            r#"background-image:src("http://evil/t.gif")"#,
            "background:image(http://evil/t.gif)",
            "background:image-set(\"http://evil/t.gif\" 1x)",
            "background:-webkit-image-set(url(http://evil/t.gif) 1x)",
            "background:cross-fade(url(http://evil/a.gif), red)",
            "list-style-image:url(http://evil/t.gif)",
            "list-style:URL (http://evil/t.gif)",
            "font-family:a, url(http://evil/t.gif)",
            "color:red/**/",
            "color:var(--x)",
            "width:expression(alert(1))",
            "@import 'http://evil/x.css'",
            "color:@import",
            "behavior:url(x.htc)",
            "cursor:url(http://evil/c.cur), auto",
            "position:fixed",
            "color:red\u{0}",
            "font-family:\"Arial",
            "-moz-binding:url(http://evil/x.xml#x)",
        ] {
            assert_eq!(sanitize_style(bad), None, "{bad}");
        }
        for bad in [
            r#"<li style='list-style:"/*" url(http://evil/t.gif?*/)'>x</li>"#,
            r#"<p style='content:"/*" url(http://evil/t.gif?*/)'>x</p>"#,
            r#"<p style='background:src("http://evil/t.gif")'>x</p>"#,
            r#"<p style="background:u&#114;l(http://evil/t.gif)">x</p>"#,
        ] {
            let s = clean(bad, false);
            assert!(!s.html.contains("evil"), "{}", s.html);
            assert!(!s.html.contains("style="), "{}", s.html);
        }
    }

    #[test]
    fn olagan_stiller_korunur() {
        let s = "color: rgb(1, 2, 3); background:#fff; font: 12px/1.5 \"Segoe UI\", 'Helvetica Neue', Arial; \
                 width: calc(100% - 20px); border:1px solid RGBA(0,0,0,.5) !important; margin:0 auto; \
                 mso-line-height-rule:exactly; display:none";
        assert_eq!(
            sanitize_style(s).as_deref(),
            Some(
                "color: rgb(1, 2, 3); background:#fff; font: 12px/1.5 \"Segoe UI\", 'Helvetica Neue', Arial; \
                 width: calc(100% - 20px); border:1px solid RGBA(0,0,0,.5) !important; margin:0 auto; display:none"
            )
        );
    }

    #[test]
    fn cok_sayida_engelli_gorsel_tek_geciste() {
        let n = 20_000;
        let html: String = (0..n)
            .map(|i| format!(r#"<img src="https://iz.example/{i}.png?a=1&amp;b=&quot;">"#))
            .collect();
        let t = std::time::Instant::now();
        let s = clean(&html, false);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(3),
            "{:?}",
            t.elapsed()
        );
        assert_eq!(s.html.matches("data-mh-src=").count(), n);
        assert!(!s.html.contains(" src="));
        assert!(s
            .html
            .contains(r#"data-mh-src="https://iz.example/19999.png?a=1&amp;b=&quot;""#));
    }

    #[test]
    fn derin_ic_ice_html_duz_metne_duser() {
        let html = format!(
            "{}<p>Merhaba <b>dünya</b></p><img src=\"https://iz.example/a.png\">",
            "<div>".repeat(100_000)
        );
        let t = std::time::Instant::now();
        let s = clean(&html, false);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            t.elapsed()
        );
        assert_eq!(s.html, "<p>Merhaba dünya</p>\n");
        assert!(!s.has_remote);
        // Boyut sınırının üstü de düz metne düşer; metin kaçışlıdır.
        let big = format!("<p>a &lt;b&gt; {}</p>", "x ".repeat(RENDER_HTML_LIMIT / 2));
        let s = clean(&big, false);
        assert!(
            s.html.starts_with("<p>a &lt;b&gt; x x"),
            "{}",
            &s.html[..30]
        );
    }

    #[test]
    fn tablodaki_basibos_icerik_hizli() {
        // "Foster parenting": her başıboş parça tablonun kardeşleri arasında
        // aranırdı (karesel).
        let html = format!(
            "{}<table>{}",
            "<br>".repeat(20_000),
            "x&amp;".repeat(20_000)
        );
        let t = std::time::Instant::now();
        let s = clean(&html, false);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(3),
            "{:?}",
            t.elapsed()
        );
        assert!(s.html.contains("x&amp;x&amp;"));
        let s = clean(&"<table>x</table>".repeat(20_000), false);
        assert_eq!(s.html.matches("<td>x</td>").count(), 20_000);
    }

    #[test]
    fn uzak_gorsel_adresleri_cozulmus_ve_tekil_cikar() {
        let mut inline = HashMap::new();
        inline.insert(
            "logo@ornek".to_owned(),
            "data:image/png;base64,AAAA".to_owned(),
        );
        let s = sanitize_html(
            r#"<p>x</p><img src="https://iz.example/a.png?u=1&amp;v=&quot;2&quot;" alt="kapak">
               <img src="https://iz.example/a.png?u=1&amp;v=&quot;2&quot;"><img src="cid:logo@ornek">
               <img src="HTTP://buyuk.example/b.gif"><img alt="kaynaksız">
               <img src="javascript:alert(1)"><img src="/goreli.png">"#,
            true,
            inline,
        );
        assert_eq!(
            remote_image_urls(&s.html),
            vec![
                "https://iz.example/a.png?u=1&v=\"2\"".to_owned(),
                "HTTP://buyuk.example/b.gif".to_owned(),
            ]
        );
        // İzin yokken görseller engellenmiştir; liste boş döner.
        let blocked = clean(r#"<img src="https://iz.example/a.png">"#, false);
        assert!(remote_image_urls(&blocked.html).is_empty());
        // Elle yazılmış işaretleme: `data-mh-src` değeri `src` sanılmaz.
        assert!(remote_image_urls(r#"<img data-mh-src="https://iz.example/a.png">"#).is_empty());
        assert_eq!(
            remote_image_urls(
                r#"<img data-mh-src="https://a.example/x" src="https://b.example/y">"#
            ),
            vec!["https://b.example/y".to_owned()]
        );
        assert!(remote_image_urls("<img src=\"https://acik.example/kapanmadi").is_empty());
    }

    #[test]
    fn gondermeler_cozulur() {
        assert_eq!(
            decode_entities("a&amp;b&quot;c&lt;d&gt;e&apos;f"),
            "a&b\"c<d>e'f"
        );
        assert_eq!(
            decode_entities("x&#65;&#x42;&#X43;y&nbsp;z"),
            "xABCy\u{a0}z"
        );
        assert_eq!(decode_entities("&bilinmeyen;&amp"), "&bilinmeyen;&amp");
        assert_eq!(decode_entities("&#xZZ;&#;"), "&#xZZ;&#;");
        assert_eq!(decode_entities("yok"), "yok");
    }

    #[test]
    fn duzen_etiketleri_korunur() {
        let s = clean(
            r##"<table width="600" cellpadding="0" style="margin:0 auto"><tr><td align="center" bgcolor="#ffffff"><font color="red">Ek</font></td></tr></table>"##,
            false,
        );
        assert!(s.html.contains("<table"));
        assert!(s.html.contains(r#"width="600""#));
        assert!(s.html.contains(r##"bgcolor="#ffffff""##));
        assert!(s.html.contains("<font"));
        assert!(s.html.contains("margin:0 auto"));
    }
}
