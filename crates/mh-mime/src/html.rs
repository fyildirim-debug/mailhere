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

/// Satır içi `style` değerinden tehlikeli bildirimleri atar.
pub fn sanitize_style(value: &str) -> Option<String> {
    const BANNED: &[&str] = &[
        "url(",
        "expression(",
        "@import",
        "javascript:",
        "behavior:",
        "-moz-binding",
        "image-set(",
    ];
    let kept: Vec<&str> = value
        .split(';')
        .map(str::trim)
        .filter(|decl| !decl.is_empty())
        .filter(|decl| {
            let norm = normalize_css(decl);
            !BANNED.iter().any(|b| norm.contains(b))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

/// Yorumları, ters bölüleri ve boşlukları atıp küçültür; `u\rl(` ya da
/// `ur/**/l(` gibi kaçamakları yakalamak için.
fn normalize_css(decl: &str) -> String {
    let mut out = String::with_capacity(decl.len());
    let mut rest = decl;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 2..].find("*/") {
            Some(end) => &rest[start + 2 + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out.chars()
        .filter(|c| !c.is_whitespace() && *c != '\\')
        .flat_map(char::to_lowercase)
        .collect()
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

/// HTML postayı temizler.
///
/// `inline`: küçük harfli Content-ID → `data:` adresi.
pub fn sanitize_html(
    html: &str,
    remote_allowed: bool,
    inline: HashMap<String, String>,
) -> Sanitized {
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
    let mut out = b.clean(html).to_string();

    let urls = std::mem::take(&mut *blocked.lock().unwrap_or_else(|e| e.into_inner()));
    for (i, url) in urls.iter().enumerate() {
        let needle = format!("src=\"{marker}{i}\"");
        out = out.replace(&needle, &format!("data-mh-src=\"{}\"", escape_attr(url)));
    }
    Sanitized {
        html: out,
        has_remote: has_remote.load(Ordering::Relaxed),
    }
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
        let s = clean(
            r##"<td style="color:#333;background-image:url('https://iz.example/t.gif')">x</td>"##,
            false,
        );
        assert!(!s.html.contains("url("), "{}", s.html);
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
