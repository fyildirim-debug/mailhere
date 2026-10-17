//! HTML posta temizleme (ammonia).
//!
//! Betik, stil etiketi, form, çerçeve ve olay öznitelikleri atılır. Bağlantılar
//! yeni pencerede ve `noopener noreferrer` ile açılır; yalnız http, https ve
//! mailto kabul edilir. Uzak görseller izin yoksa `data-mh-src` özniteliğine
//! taşınır; `cid:` görseller postanın kendi parçalarından `data:` adresine
//! çevrilir. İzleme pikselleri izin olsa da tümden atılır ve sayılır;
//! bağlantılardan izleme parametreleri sökülür. Metni bir adres gibi görünüp
//! başka bir siteye giden bağlantıya gerçek hedefin sunucu adı
//! `data-mh-host` olarak yazılır (arayüz tıklamada onay ister).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ammonia::{Builder, UrlRelative};

use crate::prescan::{canonicalize, strip_tags, text_to_html};
use crate::privacy::{clean_url, is_tracker_image, link_text_host, same_site, url_host};
use crate::text::tidy_lines;

/// Temizlenmiş HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    pub html: String,
    /// Postada izleyici olmayan uzak görsel var (izin verilmiş olsa da).
    pub has_remote: bool,
    /// Atılan izleme pikseli sayısı (aynı adres bir kez sayılır).
    pub trackers: u32,
    /// İzleme parametreleri sökülen bağlantı sayısı.
    pub cleaned_links: u32,
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

/// Protokolsüz adres (`//cdn.ornek.com/u.jpg`) https olur; öbür istemciler
/// de böyle yükler. `///` ya da sunucusuz biçim kabul edilmez.
fn absolutize(v: &str) -> Option<String> {
    let v = v.trim();
    let rest = v.strip_prefix("//")?;
    let host_ok = rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    host_ok.then(|| format!("https://{rest}"))
}

/// Göreli adres değerlendirici: yalnız protokolsüz adres kalır (https olarak).
fn relative_url(url: &str) -> Option<Cow<'_, str>> {
    absolutize(url).map(Cow::Owned)
}

/// Uzak görsel adresi: http(s) olduğu gibi, protokolsüz olan https'e çevrilmiş.
fn remote_image(v: &str) -> Option<String> {
    let v = v.trim();
    if is_http(v) {
        Some(v.to_owned())
    } else {
        absolutize(v)
    }
}

/// Satır içi raster görsel (`data:image/png;base64,…`). Ağa çıkmaz; SVG ve
/// başka türler kabul edilmez.
const DATA_IMAGE_TYPES: &[&str] = &["png", "gif", "jpeg", "jpg", "webp", "bmp"];

fn is_data_image(v: &str) -> bool {
    let Some(rest) = strip_prefix_ci(v.trim(), "data:image/") else {
        return false;
    };
    let kind = rest
        .split([';', ','])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    DATA_IMAGE_TYPES.contains(&kind.as_str())
}

/// Tembel yükleme öznitelikleri: `src` yer tutucuyken gerçek adres buradadır.
const LAZY_IMAGE_ATTRS: &[&str] = &[
    "data-src",
    "data-original",
    "data-lazy-src",
    "srcset",
    "data-srcset",
];

/// `srcset` adaylarından en yüksek yoğunluklu/genişlikli olanın adresi;
/// tek adresli öznitelikte (data-src) o adres.
fn best_candidate(value: &str) -> Option<String> {
    let mut best: Option<(f32, String)> = None;
    for part in value.split(',') {
        let mut it = part.split_whitespace();
        let Some(url) = it.next().and_then(remote_image) else {
            continue;
        };
        let weight = it
            .next()
            .and_then(|d| d.trim_end_matches(['x', 'w', 'X', 'W']).parse::<f32>().ok())
            .unwrap_or(1.0);
        if best.as_ref().is_none_or(|(w, _)| weight > *w) {
            best = Some((weight, url));
        }
    }
    best.map(|(_, url)| url)
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
            trackers: 0,
            cleaned_links: 0,
        };
    };
    let marker = format!("mh-blocked:{}:", nonce());
    let blocked: Arc<Mutex<Vec<String>>> = Arc::default();
    let cleaned = Arc::new(AtomicU32::new(0));

    let mut b = Builder::default();
    b.add_tags(EXTRA_TAGS)
        .clean_content_tags(DROP_WITH_CONTENT.iter().copied().collect::<HashSet<_>>())
        .add_generic_attributes(GENERIC_ATTRS)
        .add_tag_attributes("img", &["src", "alt", "width", "height", "border"])
        .add_tag_attributes("img", LAZY_IMAGE_ATTRS)
        .add_tag_attributes("a", &["href", "title", "name"])
        // `data` yalnız img src'de raster görsel olarak kalır (filter_attr).
        .url_schemes(
            ["http", "https", "mailto", "cid", "data"]
                .into_iter()
                .collect(),
        )
        // Göreli adreslerden yalnız protokolsüz olan (`//sunucu/…`) https'e
        // çevrilip kalır; gerisi atılır.
        .url_relative(UrlRelative::Custom(Box::new(relative_url)))
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank")
        .strip_comments(true);
    {
        let marker = marker.clone();
        let blocked = blocked.clone();
        let cleaned = cleaned.clone();
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
                    cleaned: &cleaned,
                },
            )
        });
    }
    let out = b.clean(&canon).to_string();

    let urls = std::mem::take(&mut *blocked.lock().unwrap_or_else(|e| e.into_inner()));
    let images = finish_images(&out, &marker, &urls, remote_allowed);
    Sanitized {
        html: mark_deceptive_links(&images.html),
        has_remote: images.has_remote,
        trackers: images.trackers,
        cleaned_links: cleaned.load(Ordering::Relaxed),
    }
}

/// Görsel geçişinin sonucu.
struct Images {
    html: String,
    has_remote: bool,
    trackers: u32,
}

/// Temizlenmiş çıktıdaki `<img …>` etiketlerinde tek geçişte: izleme
/// pikselleri atılır (sayılır), engellenen görsellerin `src="<işaret><sıra>"`
/// öznitelikleri `data-mh-src="<adres>"` olur (adres başına ayrı `replace`
/// karesel olurdu).
///
/// Çıktı html5ever'dan gelir: etiketler küçük harfli, öznitelik değerleri çift
/// tırnaklı, `"`, `<` ve `>` kaçışlıdır; metindeki `<` de kaçışlı olduğundan
/// her `<img` gerçek bir etikettir.
fn finish_images(html: &str, marker: &str, urls: &[String], remote_allowed: bool) -> Images {
    let extra: usize = urls.iter().map(|u| u.len() + 8).sum();
    let mut out = String::with_capacity(html.len() + extra);
    let mut seen_trackers: HashSet<String> = HashSet::new();
    let mut has_remote = false;
    let mut rest = html;
    while let Some(i) = rest.find("<img") {
        let after = &rest[i + 4..];
        let Some(end) = after.find('>') else {
            break;
        };
        let tag = &after[..end];
        out.push_str(&rest[..i]);
        rest = &after[end + 1..];
        let src = attr(tag, "src");
        let blocked = src
            .and_then(|v| v.strip_prefix(marker))
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| urls.get(n));
        // Gerçek görsel yoksa (src yok ya da satır içi yer tutucu) tembel
        // yükleme adayı kullanılır; adaylar her durumda çıktıdan atılır.
        let placeholder = src.is_none_or(|v| v.starts_with("data:"));
        let lazy = if blocked.is_none() && placeholder {
            LAZY_IMAGE_ATTRS
                .iter()
                .find_map(|a| attr(tag, a))
                .map(decode_entities)
                .filter(|v| is_http(v))
        } else {
            None
        };
        let remote = match (blocked, &lazy) {
            (Some(url), _) => Some(url.clone()),
            (None, Some(url)) => Some(url.clone()),
            (None, None) => src.map(decode_entities).filter(|v| is_http(v)),
        };
        if let Some(url) = &remote {
            let style = attr(tag, "style").map(decode_entities);
            if is_tracker_image(
                url,
                attr(tag, "width"),
                attr(tag, "height"),
                style.as_deref(),
            ) {
                seen_trackers.insert(url.clone());
                continue;
            }
            has_remote = true;
        }
        let mut tag = tag.to_owned();
        for a in LAZY_IMAGE_ATTRS {
            tag = remove_attr(&tag, a);
        }
        match (blocked, src, &lazy) {
            (Some(url), Some(v), _) => {
                // Yalnız `src` değeri yerinde değişir; diğer öznitelikler kalır.
                let old = format!(" src=\"{v}\"");
                let new = format!(" data-mh-src=\"{}\"", escape_attr(url));
                tag = tag.replacen(&old, &new, 1);
            }
            (None, _, Some(url)) => {
                tag = remove_attr(&tag, "src");
                let name = if remote_allowed { "src" } else { "data-mh-src" };
                tag.push_str(&format!(" {name}=\"{}\"", escape_attr(url)));
            }
            _ => {}
        }
        out.push_str("<img");
        out.push_str(&tag);
        out.push('>');
    }
    out.push_str(rest);
    Images {
        html: out,
        has_remote,
        trackers: seen_trackers.len() as u32,
    }
}

/// Etiket metninden ` ad="değer"` özniteliğini (ilk geçişini) siler.
fn remove_attr(tag: &str, name: &str) -> String {
    let needle = format!(" {name}=\"");
    let Some(i) = tag.find(&needle) else {
        return tag.to_owned();
    };
    let after = &tag[i + needle.len()..];
    match after.find('"') {
        Some(end) => format!("{}{}", &tag[..i], &after[end + 1..]),
        None => tag.to_owned(),
    }
}

/// Etiket metnindeki ` ad="değer"` özniteliğinin (çözülmemiş) değeri.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let mut rest = tag;
    while let Some(i) = rest.find(&needle) {
        // `data-mh-src="…"` gibi başka bir özniteliğin sonu olmasın.
        let boundary = rest[..i]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        let after = &rest[i + needle.len()..];
        let end = after.find('"')?;
        if boundary {
            return Some(&after[..end]);
        }
        rest = &after[end + 1..];
    }
    None
}

/// Bağlantı metninin incelenen en uzun kısmı (bayt).
const LINK_TEXT_LIMIT: usize = 300;

/// Metni bir adres ya da alan adı gibi görünen ama başka siteye giden
/// bağlantılara gerçek hedefin sunucu adını `data-mh-host` olarak ekler.
///
/// Her `<a …>` için metin, sonraki `<a ` ya da `</a>` etiketine kadar okunur
/// (tablo içindeki iç içe bağlantılar da ayrı ayrı denetlenir); okuma
/// [`LINK_TEXT_LIMIT`] ile sınırlı olduğundan iş doğrusal kalır.
fn mark_deceptive_links(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("<a ") {
        let after = &rest[i + 3..];
        let Some(end) = after.find('>') else {
            break;
        };
        let tag = &after[..end];
        let body = &after[end + 1..];
        out.push_str(&rest[..i]);
        out.push_str("<a ");
        let real = attr(tag, "href")
            .map(decode_entities)
            .and_then(|href| url_host(&href));
        if let Some(real) = real {
            if let Some(shown) = link_text_host(&link_text(body)) {
                if !same_site(&shown, &real) {
                    out.push_str("data-mh-host=\"");
                    out.push_str(&escape_attr(&real));
                    out.push_str("\" ");
                }
            }
        }
        out.push_str(tag);
        out.push('>');
        rest = body;
    }
    out.push_str(rest);
    out
}

/// Bağlantının görünen metni: sonraki `<a `/`</a>` etiketine kadar,
/// etiketler atılmış ve göndermeler çözülmüş; en çok [`LINK_TEXT_LIMIT`]
/// bayt incelenir.
fn link_text(body: &str) -> String {
    let window = &body[..floor_char(body, LINK_TEXT_LIMIT)];
    let mut text = String::new();
    let mut rest = window;
    while let Some(i) = rest.find('<') {
        text.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("</a>") || tail.starts_with("<a ") || tail.starts_with("<a>") {
            return decode_entities(&text);
        }
        rest = tail.find('>').map_or("", |e| &tail[e + 1..]);
    }
    text.push_str(rest);
    decode_entities(&text)
}

/// `max` bayttan küçük ya da eşit, karakter sınırına denk gelen konum.
fn floor_char(s: &str, max: usize) -> usize {
    if s.len() <= max {
        return s.len();
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
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
        if let Some(url) = attr(tag, "src").map(decode_entities) {
            if is_http(&url) && seen.insert(url.clone()) {
                out.push(url);
            }
        }
        rest = &after[end + 1..];
    }
    out
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
    cleaned: &'a AtomicU32,
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
            if is_data_image(v) {
                return Some(Cow::Borrowed(value));
            }
            let url = remote_image(v)?;
            if ctx.remote_allowed {
                return Some(Cow::Owned(url));
            }
            let mut list = ctx.blocked.lock().unwrap_or_else(|e| e.into_inner());
            list.push(url);
            Some(Cow::Owned(format!("{}{}", ctx.marker, list.len() - 1)))
        }
        // Tembel yükleme adayı: yalnız seçilen adres kalır, görsel geçişi
        // (finish_images) gerekirse src yapar ve özniteliği siler.
        ("img", a) if LAZY_IMAGE_ATTRS.contains(&a) => best_candidate(value).map(Cow::Owned),
        // `data:` yalnız img src'de raster görsel olarak kabul edilir.
        (_, _) if strip_prefix_ci(value.trim(), "data:").is_some() => None,
        (_, "href") if strip_prefix_ci(value.trim(), "cid:").is_some() => None,
        ("a", "href") => match clean_url(value) {
            Some(clean) => {
                ctx.cleaned.fetch_add(1, Ordering::Relaxed);
                Some(Cow::Owned(clean))
            }
            None => Some(Cow::Borrowed(value)),
        },
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
    fn izleme_pikselleri_atilir_ve_sayilir() {
        let html = r#"<p>x</p><img src="https://bulten.example/kapak.jpg" width="600" alt="kapak">
            <img src="https://iz.example/o.gif?u=1&amp;m=2" width="1" height="1" alt="">
            <img src="https://iz.example/o.gif?u=1&amp;m=2" width="1" height="1">
            <img src="https://iz.example/g.png" style="display:none">
            <img src="https://u1.ct.sendgrid.net/wf/open?upn=abc" width="600" height="80">
            <img src="cid:yok"><img src="data:image/gif;base64,R0lGOD" width="1" height="1">"#;
        for remote in [false, true] {
            let s = clean(html, remote);
            assert_eq!(s.trackers, 3, "{}", s.html);
            assert!(s.has_remote);
            assert!(
                !s.html.contains("iz.example") && !s.html.contains("sendgrid"),
                "{}",
                s.html
            );
            assert!(s.html.contains("kapak.jpg"));
        }
        // Yalnız izleyici varsa gösterilecek uzak içerik yoktur.
        let s = clean(
            r#"<p>x</p><img src="https://iz.example/p.gif" width="1" height="1">"#,
            false,
        );
        assert_eq!((s.trackers, s.has_remote), (1, false));
        assert!(!s.html.contains("<img"), "{}", s.html);
    }

    #[test]
    fn engelli_gorselin_diger_oznitelikleri_kalir() {
        let s = clean(
            r#"<img alt="a &quot;b&quot;" src="https://iz.example/k.png?x=1&amp;y=2" width="320">"#,
            false,
        );
        assert!(
            s.html
                .contains(r#"data-mh-src="https://iz.example/k.png?x=1&amp;y=2""#),
            "{}",
            s.html
        );
        assert!(s.html.contains(r#"alt="a &quot;b&quot;""#), "{}", s.html);
        assert!(s.html.contains(r#"width="320""#));
        assert!(!s.html.contains(" src="));
    }

    #[test]
    fn baglantilardan_izleme_parametresi_sokulur() {
        let s = clean(
            r#"<a href="https://ornek.com/a?utm_source=e&amp;id=1">a</a>
               <a href="https://ornek.com/b?fbclid=x">b</a>
               <a href="https://ornek.com/c?id=2">c</a><a href="mailto:a@b.com?utm_source=x">d</a>"#,
            false,
        );
        assert_eq!(s.cleaned_links, 2);
        assert!(
            s.html.contains(r#"href="https://ornek.com/a?id=1""#),
            "{}",
            s.html
        );
        assert!(
            s.html.contains(r#"href="https://ornek.com/b""#),
            "{}",
            s.html
        );
        assert!(s.html.contains(r#"href="https://ornek.com/c?id=2""#));
        assert!(s.html.contains("mailto:a@b.com?utm_source=x"));
    }

    #[test]
    fn aldatici_baglanti_isaretlenir() {
        let s = clean(
            r#"<a href="https://kotu.example/giris">www.banka.com.tr</a>
               <a href="https://banka.com.tr@kotu.example/">https://banka.com.tr/giris</a>
               <a href="https://kampanya.banka.com.tr/x"><b>banka.com.tr</b></a>
               <a href="https://kotu.example/">Hemen giriş yap</a>
               <a href="mailto:a@kotu.example">banka.com.tr</a>"#,
            false,
        );
        assert_eq!(
            s.html.matches(r#"data-mh-host="kotu.example""#).count(),
            2,
            "{}",
            s.html
        );
        assert_eq!(s.html.matches("data-mh-host").count(), 2, "{}", s.html);
        // Postanın kendi yazdığı öznitelik geçmez.
        let s = clean(
            r#"<a data-mh-host="banka.com" href="https://banka.com">Giriş</a>"#,
            false,
        );
        assert!(!s.html.contains("data-mh-host"), "{}", s.html);
        // Tablo içinde iç içe bağlantı ayrı denetlenir.
        let s = clean(
            r#"<a href="https://banka.com"><table><tr><td><a href="https://kotu.example">banka.com</a></td></tr></table></a>"#,
            false,
        );
        assert_eq!(
            s.html.matches(r#"data-mh-host="kotu.example""#).count(),
            1,
            "{}",
            s.html
        );
    }

    #[test]
    fn cok_sayida_izleyici_ve_baglanti_dogrusal() {
        // ~190 bayt × 8000 ≈ 1,5 MB: HTML sınırının altında.
        let n = 8_000;
        let html: String = (0..n)
            .map(|i| {
                format!(
                    r#"<a href="https://kotu.example/{i}?utm_source=x">banka{i}.com</a><img src="https://iz.example/{i}.gif" width="1" height="1"><a href="https://ornek.com/{i}">devam</a>"#
                )
            })
            .collect();
        let t = std::time::Instant::now();
        let s = clean(&html, false);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(3),
            "{:?}",
            t.elapsed()
        );
        assert_eq!(s.trackers, n);
        assert_eq!(s.cleaned_links, n);
        assert_eq!(s.html.matches("data-mh-host=").count(), n as usize);
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

    #[test]
    fn protokolsuz_gorsel_adresi_https_olur() {
        // Mağaza şablonları `//cdn…` yazar; öbür istemciler https ile yükler.
        let s = clean(
            r#"<img src="//cdn.ornek.com/u/120.jpg" width="120" alt="ürün">"#,
            true,
        );
        assert!(s.has_remote);
        assert!(
            s.html.contains(r#"src="https://cdn.ornek.com/u/120.jpg""#),
            "{}",
            s.html
        );
        let s = clean(r#"<img src="//cdn.ornek.com/u/120.jpg">"#, false);
        assert!(s.has_remote);
        assert!(
            s.html
                .contains(r#"data-mh-src="https://cdn.ornek.com/u/120.jpg""#),
            "{}",
            s.html
        );
        assert!(!s.html.contains(" src="), "{}", s.html);
        // Bağlantıda da https olur; başka göreli adres yine atılır.
        let s = clean(
            r#"<a href="//ornek.com/x">a</a><a href="/goreli">b</a>"#,
            true,
        );
        assert!(
            s.html.contains(r#"href="https://ornek.com/x""#),
            "{}",
            s.html
        );
        assert!(!s.html.contains("goreli"), "{}", s.html);
    }

    #[test]
    fn tembel_yuklenen_gorselin_gercek_adresi_kullanilir() {
        let lazy = r#"<img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" data-src="https://cdn.ornek.com/u.jpg" width="120" height="120">"#;
        let s = clean(lazy, true);
        assert!(s.has_remote);
        assert!(
            s.html.contains(r#"src="https://cdn.ornek.com/u.jpg""#),
            "{}",
            s.html
        );
        assert!(!s.html.contains("data-src"), "{}", s.html);
        assert!(!s.html.contains("R0lGOD"), "{}", s.html);
        let s = clean(lazy, false);
        assert!(
            s.html
                .contains(r#"data-mh-src="https://cdn.ornek.com/u.jpg""#),
            "{}",
            s.html
        );
        assert!(!s.html.contains(" src="), "{}", s.html);
        // Yalnız srcset: en yüksek yoğunluklu aday seçilir; srcset çıktıya geçmez.
        let s = clean(
            r#"<img srcset="https://cdn.ornek.com/a.jpg 1x, //cdn.ornek.com/b.jpg 2x" alt="x">"#,
            true,
        );
        assert!(
            s.html.contains(r#"src="https://cdn.ornek.com/b.jpg""#),
            "{}",
            s.html
        );
        assert!(!s.html.contains("srcset"), "{}", s.html);
        // Gerçek src varken yedek adaylar yok sayılır.
        let s = clean(
            r#"<img src="https://cdn.ornek.com/asil.jpg" data-src="https://cdn.ornek.com/diger.jpg">"#,
            true,
        );
        assert!(
            s.html.contains("asil.jpg") && !s.html.contains("diger.jpg"),
            "{}",
            s.html
        );
        // Tembel aday da izleyici denetiminden geçer.
        let s = clean(
            r#"<img data-src="https://iz.example/open.gif" width="1" height="1">"#,
            true,
        );
        assert_eq!(s.trackers, 1);
        assert!(!s.html.contains("<img"), "{}", s.html);
    }

    #[test]
    fn satir_ici_raster_gorsel_kalir_baska_data_adresi_atilir() {
        let s = clean(
            r#"<img src="data:image/png;base64,iVBORw0KGgo=" alt="logo">"#,
            false,
        );
        assert!(
            s.html
                .contains(r#"src="data:image/png;base64,iVBORw0KGgo=""#),
            "{}",
            s.html
        );
        assert!(!s.has_remote);
        for bad in [
            r#"<img src="data:image/svg+xml;base64,PHN2Zz4=">"#,
            r#"<img src="data:text/html;base64,PHNjcmlwdD4=">"#,
            r#"<a href="data:text/html;base64,PHNjcmlwdD4=">a</a>"#,
        ] {
            let s = clean(bad, true);
            assert!(!s.html.contains("data:"), "{bad} → {}", s.html);
        }
    }
}
