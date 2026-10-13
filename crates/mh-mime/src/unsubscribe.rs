//! Abonelikten çıkma başlıkları: `List-Unsubscribe` (RFC 2369) ve tek tıkla
//! çıkma için `List-Unsubscribe-Post` (RFC 8058).
//!
//! Başlıklar gönderenin elindedir: değerler sınırlıdır, bozuk ya da tanınmayan
//! adresler sessizce atlanır. Burada yalnız ayrıştırılır; istek göndermek
//! uygulamanın işidir (ve yalnız kullanıcı isteyince yapılır).

use mh_core::Unsubscribe;

use crate::text::clean_line;

/// Tek bir adresin üst sınırı (karakter).
pub const MAX_URI: usize = 2048;
/// Bir başlık değerinin okunan en büyük boyu (bayt); kalanı yok sayılır.
const MAX_VALUE: usize = 16 * 1024;
/// Başlık bölümünün taranan en büyük boyu (bayt).
const MAX_HEADER_BLOCK: usize = 512 * 1024;
/// Bir başlıkta incelenen en fazla adres.
const MAX_URIS: usize = 16;
/// Çıkma postası konusunun üst sınırı (karakter).
const MAX_SUBJECT: usize = 300;
/// Çıkma postası gövdesinin üst sınırı (karakter).
const MAX_BODY: usize = 2000;

/// `mailto:` adresinden çıkan çıkma postası.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailto {
    /// Tek alıcı (adresteki ilk geçerli adres).
    pub to: String,
    pub subject: Option<String>,
    pub body: Option<String>,
}

/// Ham postanın başlıklarından abonelikten çıkma yollarını çıkarır;
/// başlık yoksa ya da içinde kullanılabilir adres yoksa `None`.
pub fn unsubscribe(raw: &[u8]) -> Option<Unsubscribe> {
    let (list, post) = header_values(raw);
    parse_list_unsubscribe(&list?, post.as_deref())
}

/// `List-Unsubscribe` değerini (ve varsa `List-Unsubscribe-Post` değerini)
/// ayrıştırır. Adresler `<...>` içinde, virgülle ayrılmış gelir; köşeli
/// parantez hiç yoksa (bozuk gönderici) virgülle ayrılmış düz adresler
/// denenir. İlk https (yoksa http) adresi ve ilk mailto adresi alınır.
pub fn parse_list_unsubscribe(value: &str, post: Option<&str>) -> Option<Unsubscribe> {
    let mut https = None;
    let mut http = None;
    let mut mailto = None;
    for uri in uris(value, MAX_URIS) {
        match classify(&uri) {
            Some(Kind::Https(u)) if https.is_none() => https = Some(u),
            Some(Kind::Http(u)) if http.is_none() => http = Some(u),
            Some(Kind::Mailto(u)) if mailto.is_none() => mailto = Some(u),
            _ => {}
        }
    }
    let one_click = https.is_some() && post.is_some_and(is_one_click);
    let url = https.or(http);
    if url.is_none() && mailto.is_none() {
        return None;
    }
    Some(Unsubscribe {
        url,
        mailto,
        one_click,
    })
}

/// `List-Unsubscribe-Post` değeri RFC 8058'deki tek anahtar/değer mi?
fn is_one_click(post: &str) -> bool {
    post.trim()
        .eq_ignore_ascii_case("List-Unsubscribe=One-Click")
}

/// Başlık değerindeki en fazla `limit` aday adres (boşluklar atılmış).
fn uris(value: &str, limit: usize) -> Vec<String> {
    let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    if !value.contains('<') {
        return value
            .split(',')
            .take(limit)
            .map(strip)
            .filter(|s| !s.is_empty())
            .collect();
    }
    let mut out = Vec::new();
    let mut rest = value;
    while out.len() < limit {
        let Some(start) = rest.find('<') else {
            break;
        };
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            break;
        };
        // Katlanmış (uzun) adreslerin içindeki boşluk yok sayılır (RFC 2369).
        out.push(strip(&after[..end]));
        rest = &after[end + 1..];
    }
    out
}

enum Kind {
    Https(String),
    Http(String),
    Mailto(String),
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Adresin türü; geçersizse `None`. Şema küçük harfe çevrilir.
fn classify(uri: &str) -> Option<Kind> {
    if uri.is_empty() || uri.len() > MAX_URI || !uri.is_ascii() {
        return None;
    }
    if uri.chars().any(|c| {
        c.is_ascii_control() || matches!(c, '"' | '<' | '>' | '\\' | '`' | '{' | '}' | '|' | '^')
    }) {
        return None;
    }
    if let Some(rest) = strip_prefix_ci(uri, "https://") {
        return valid_authority(rest).then(|| Kind::Https(format!("https://{rest}")));
    }
    if let Some(rest) = strip_prefix_ci(uri, "http://") {
        return valid_authority(rest).then(|| Kind::Http(format!("http://{rest}")));
    }
    if let Some(rest) = strip_prefix_ci(uri, "mailto:") {
        let normalized = format!("mailto:{rest}");
        return parse_mailto(&normalized).map(|_| Kind::Mailto(normalized));
    }
    None
}

/// `//` sonrası sunucu bölümü: boş olamaz, kullanıcı bilgisi (`a@b`) taşıyamaz
/// (`https://banka.com@kotu.com` gibi yanıltıcı adresler), yalnız sunucu
/// adı, IP ve kapı karakterleri.
fn valid_authority(rest: &str) -> bool {
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    !authority.is_empty()
        && authority
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '[' | ']'))
        && authority
            .trim_start_matches('[')
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
}

/// `mailto:` adresini (RFC 6068) çözer: ilk geçerli alıcı ile `subject` ve
/// `body` alanları. `cc`, `bcc` ve diğer alanlar yok sayılır; çıkma postası
/// yalnız tek adrese gider.
pub fn parse_mailto(uri: &str) -> Option<Mailto> {
    if uri.len() > MAX_URI {
        return None;
    }
    let rest = strip_prefix_ci(uri.trim(), "mailto:")?;
    let rest = rest.split('#').next().unwrap_or(rest);
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let first_address = |s: &str| {
        percent_decode(s)
            .split(',')
            .map(str::trim)
            .find(|a| !a.is_empty())
            .map(str::to_owned)
    };
    let mut to = first_address(path);
    let mut subject = None;
    let mut body = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match percent_decode(k).to_ascii_lowercase().as_str() {
            "to" if to.is_none() => to = first_address(v),
            "subject" if subject.is_none() => {
                subject = Some(truncate_chars(&clean_line(&percent_decode(v)), MAX_SUBJECT));
            }
            "body" if body.is_none() => {
                let text = percent_decode(v).replace("\r\n", "\n");
                body = Some(truncate_chars(text.trim(), MAX_BODY));
            }
            _ => {}
        }
    }
    let to = to.filter(|a| is_plain_address(a))?;
    Some(Mailto {
        to,
        subject: subject.filter(|s| !s.is_empty()),
        body: body.filter(|b| !b.is_empty()),
    })
}

/// Düz (ASCII) e-posta adresi mi? Alıntılı yerel bölüm, yorum ve görünen ad
/// kabul edilmez: çıkma postası yalnız açık bir adrese gider.
fn is_plain_address(a: &str) -> bool {
    if a.len() > 254 {
        return false;
    }
    let Some((local, domain)) = a.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c));
    let domain_ok = domain.contains('.')
        && !domain.starts_with(['.', '-'])
        && !domain.ends_with(['.', '-'])
        && !domain.contains("..")
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    local_ok && domain_ok
}

/// `%XX` kaçışlarını çözer; bozuk kaçış olduğu gibi kalır. `+` boşluk
/// değildir (RFC 6068).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((pos, _)) => s[..pos].to_owned(),
        None => s.to_owned(),
    }
}

#[derive(Clone, Copy)]
enum Which {
    List,
    Post,
}

/// Başlık bölümünden (ilk boş satıra kadar) `List-Unsubscribe` ve
/// `List-Unsubscribe-Post` değerleri; katlanmış satırlar birleştirilir.
/// Aynı başlık birden çok kez varsa ilki alınır.
fn header_values(raw: &[u8]) -> (Option<String>, Option<String>) {
    let block = &raw[..raw.len().min(MAX_HEADER_BLOCK)];
    let mut list = None;
    let mut post = None;
    let mut current: Option<(Which, Vec<u8>)> = None;
    let mut finish = |cur: &mut Option<(Which, Vec<u8>)>| {
        if let Some((which, value)) = cur.take() {
            let value = String::from_utf8_lossy(&value).trim().to_owned();
            let slot = match which {
                Which::List => &mut list,
                Which::Post => &mut post,
            };
            if slot.is_none() {
                *slot = Some(value);
            }
        }
    };
    for line in block.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.first().is_some_and(|b| *b == b' ' || *b == b'\t') {
            if let Some((_, value)) = current.as_mut() {
                let room = MAX_VALUE.saturating_sub(value.len());
                value.extend_from_slice(&line[..line.len().min(room)]);
            }
            continue;
        }
        finish(&mut current);
        if line.is_empty() {
            break;
        }
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        let name = line[..colon].trim_ascii();
        let which = if name.eq_ignore_ascii_case(b"List-Unsubscribe") {
            Which::List
        } else if name.eq_ignore_ascii_case(b"List-Unsubscribe-Post") {
            Which::Post
        } else {
            continue;
        };
        let value = &line[colon + 1..];
        current = Some((which, value[..value.len().min(MAX_VALUE)].to_vec()));
    }
    finish(&mut current);
    (list, post)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(url: Option<&str>, mailto: Option<&str>, one_click: bool) -> Option<Unsubscribe> {
        Some(Unsubscribe {
            url: url.map(str::to_owned),
            mailto: mailto.map(str::to_owned),
            one_click,
        })
    }

    #[test]
    fn baslik_adresleri_ayrisir() {
        let v = "<mailto:cik@bulten.com?subject=unsubscribe>, <https://bulten.com/cik?u=42>";
        assert_eq!(
            parse_list_unsubscribe(v, Some("List-Unsubscribe=One-Click")),
            u(
                Some("https://bulten.com/cik?u=42"),
                Some("mailto:cik@bulten.com?subject=unsubscribe"),
                true
            )
        );
        // Post başlığı yoksa ya da başka değerse tek tık yok.
        assert!(!parse_list_unsubscribe(v, None).unwrap().one_click);
        assert!(
            !parse_list_unsubscribe(v, Some("List-Unsubscribe=Hayir"))
                .unwrap()
                .one_click
        );
        // https, http'den önce gelir; tek tık yalnız https ile.
        let v = "<http://a.com/x>, <https://b.com/y>";
        assert_eq!(
            parse_list_unsubscribe(v, Some(" list-unsubscribe=one-click ")),
            u(Some("https://b.com/y"), None, true)
        );
        assert_eq!(
            parse_list_unsubscribe("<http://a.com/x>", Some("List-Unsubscribe=One-Click")),
            u(Some("http://a.com/x"), None, false)
        );
        // Yalnız mailto.
        assert_eq!(
            parse_list_unsubscribe("<MAILTO:liste@ornek.com>", None),
            u(None, Some("mailto:liste@ornek.com"), false)
        );
        // Köşeli parantezsiz bozuk gönderici.
        assert_eq!(
            parse_list_unsubscribe("https://c.com/z, mailto:d@c.com", None),
            u(Some("https://c.com/z"), Some("mailto:d@c.com"), false)
        );
    }

    #[test]
    fn bozuk_ve_tehlikeli_degerler_atlanir() {
        for v in [
            "",
            "<>",
            "<javascript:alert(1)>",
            "<file:///etc/passwd>",
            "<ftp://a.com/x>",
            "<https://>",
            "<https://banka.com@kotu.com/x>",
            "<https://a.com/\"onmouseover>",
            "<https://çık.com/x>",
            "<mailto:>",
            "<mailto:adres-yok>",
            "<mailto:a@b>",
            "<https://a.com/x",
            "<<<<",
        ] {
            assert_eq!(parse_list_unsubscribe(v, None), None, "{v}");
        }
        // Çok uzun adres atlanır, sonraki kullanılır.
        let long = format!("<https://a.com/{}>, <https://b.com/k>", "x".repeat(MAX_URI));
        assert_eq!(
            parse_list_unsubscribe(&long, None).unwrap().url.as_deref(),
            Some("https://b.com/k")
        );
        // Çok sayıda çöp adres: sınırdan sonrası incelenmez, çökmez.
        let many = "<x:y>,".repeat(10_000) + "<https://son.com/>";
        assert_eq!(parse_list_unsubscribe(&many, None), None);
    }

    #[test]
    fn katlanmis_baslik_hamdan_okunur() {
        // Katlanan satır boşlukla başlar (`\x20`, `\t`).
        let raw = b"From: Bulten <haber@bulten.com>\r\n\
List-Unsubscribe: <mailto:cik@bulten.com?subject=cik>,\r\n\
\x20<https://bulten.com/cik?u=1&\r\n\tt=2>\r\n\
list-unsubscribe-post: List-Unsubscribe=One-Click\r\n\
Subject: Haftalik\r\n\
\r\n\
List-Unsubscribe: <https://govde.com/sayilmaz>\r\n";
        assert_eq!(
            unsubscribe(raw),
            u(
                Some("https://bulten.com/cik?u=1&t=2"),
                Some("mailto:cik@bulten.com?subject=cik"),
                true
            )
        );
        // Gövdedeki "başlık" sayılmaz; başlık yoksa None.
        let raw = b"From: a@b.com\r\nSubject: x\r\n\r\nList-Unsubscribe: <https://govde.com/x>\r\n";
        assert_eq!(unsubscribe(raw), None);
        // Yalnız \n satır sonu, çift başlıkta ilki.
        let raw =
            b"List-Unsubscribe: <https://ilk.com/>\nList-Unsubscribe: <https://ikinci.com/>\n\n";
        assert_eq!(
            unsubscribe(raw).unwrap().url.as_deref(),
            Some("https://ilk.com/")
        );
        // Bozuk UTF-8 ve çok uzun değer çökertmez.
        let mut raw = b"List-Unsubscribe: <https://a.com/\xff\xfe>, <mailto:c@a.com>".to_vec();
        raw.extend(std::iter::repeat_n(b' ', 100_000));
        raw.extend_from_slice(b"\r\n\r\n");
        assert_eq!(unsubscribe(&raw), u(None, Some("mailto:c@a.com"), false));
        assert_eq!(unsubscribe(b""), None);
        assert_eq!(unsubscribe(b"\xff\xff\xff"), None);
    }

    #[test]
    fn mailto_cozulur() {
        assert_eq!(
            parse_mailto(
                "mailto:cik@bulten.com?subject=Abonelik%20iptali&body=L%C3%BCtfen+%C3%A7%C4%B1kar"
            ),
            Some(Mailto {
                to: "cik@bulten.com".into(),
                subject: Some("Abonelik iptali".into()),
                body: Some("Lütfen+çıkar".into()),
            })
        );
        // Alıcı alanı boşsa `to=` kullanılır; birden çok adreste yalnız ilki.
        assert_eq!(
            parse_mailto("mailto:?to=a%40b.com,c@d.com&cc=x@y.com")
                .unwrap()
                .to,
            "a@b.com"
        );
        assert_eq!(
            parse_mailto("mailto:a@b.com,c@d.com").unwrap().to,
            "a@b.com"
        );
        // Konuya satır sonu eklenemez (başlık ekleme).
        let m = parse_mailto("mailto:a@b.com?subject=x%0D%0ABcc:%20kurban@c.com").unwrap();
        assert!(!m.subject.as_deref().unwrap().contains('\n'));
        assert_eq!(m.body, None);
        // Geçersizler.
        for bad in [
            "mailto:",
            "mailto:%22a%20b%22@c.com",
            "mailto:Ad <a@b.com>",
            "mailto:a@b.com\r\nBcc: x@y.com",
            "http://a.com",
            "mailto:a@-b.com",
            "mailto:a@b..com",
        ] {
            assert_eq!(parse_mailto(bad), None, "{bad}");
        }
        // Bozuk kaçış olduğu gibi kalır.
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%41%"), "A%");
        // Uzun gövde kısaltılır.
        let long = format!("mailto:a@b.com?body={}", "x".repeat(MAX_URI - 30));
        assert!(parse_mailto(&long).unwrap().body.unwrap().chars().count() <= MAX_BODY);
    }
}
