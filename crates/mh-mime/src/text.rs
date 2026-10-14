//! Düz metin yardımcıları: HTML → metin, önizleme, konu sadeleştirme.

use std::collections::HashSet;

use crate::prescan::{canonicalize, strip_tags};

/// Önizleme uzunluğu (karakter).
pub const SNIPPET_LEN: usize = 200;

/// Blok öğelerinin önüne satır sonu koyar; böylece etiketler silinince
/// paragraflar birbirine yapışmaz.
fn insert_breaks(html: &str) -> String {
    const BLOCKS: &[&str] = &[
        "br",
        "p",
        "/p",
        "div",
        "/div",
        "tr",
        "/tr",
        "li",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "/h1",
        "/h2",
        "/h3",
        "/h4",
        "/h5",
        "/h6",
        "table",
        "/table",
        "blockquote",
        "/blockquote",
        "hr",
        "ul",
        "/ul",
        "ol",
        "/ol",
        "pre",
        "/pre",
    ];
    let mut out = String::with_capacity(html.len() + html.len() / 16);
    let bytes = html.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &html[i + 1..];
            let name_len = rest
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'/')
                .count();
            let name = &rest[..name_len];
            if BLOCKS.iter().any(|b| b.eq_ignore_ascii_case(name)) {
                out.push_str(&html[last..i]);
                out.push('\n');
                last = i;
            }
        }
        i += 1;
    }
    out.push_str(&html[last..]);
    out
}

/// html5ever'ın metin kaçışlarını geri çevirir.
fn unescape_serialized(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&amp;", "&")
}

/// html5ever'la düz metne çevrilecek en büyük HTML (bayt). Önizleme ve arama
/// dizini için ilk ~64 bin karakter yeter; daha büyüğü doğrusal yedek yoldan
/// geçer.
pub const TEXT_HTML_LIMIT: usize = 256 * 1024;

/// HTML'i okunabilir düz metne çevirir (betik ve stil içeriği atılır).
///
/// Çok büyük ya da çok derin iç içe HTML html5ever'da karesel süre alır; o
/// durumda etiketler doğrusal olarak atılır.
pub fn html_to_text(html: &str) -> String {
    let canon = (html.len() <= TEXT_HTML_LIMIT)
        .then(|| canonicalize(&insert_breaks(html)))
        .flatten();
    let Some(canon) = canon else {
        return tidy_lines(&strip_tags(html));
    };
    let drop: HashSet<&str> = ["script", "style", "title", "noscript", "template", "head"]
        .into_iter()
        .collect();
    let cleaned = ammonia::Builder::empty()
        .clean_content_tags(drop)
        .clean(&canon)
        .to_string();
    tidy_lines(&unescape_serialized(&cleaned))
}

/// Satır sonlarını korur, satır içi boşlukları sadeleştirir, ardışık boş
/// satırları teke indirir.
pub fn tidy_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = collapse_ws(line);
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(&line);
    }
    out
}

/// Tüm boşluk dizilerini tek boşluğa indirir ve kırpar.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split(|c: char| c.is_whitespace()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Yön değiştiren biçim karakterleri (U+202A–U+202E, U+2066–U+2069): adın
/// görünüşünü ters çevirip yanıltabilir.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Görünen adı (ör. `"Ayşe Demir" <ayse@...>`) temizler: CR, LF, sekme ve
/// diğer denetim karakterleri ile yön değiştiren karakterler boşluğa çevrilir,
/// boşluklar sadeleşir. Kodlanmış sözcükten (`=?utf-8?b?...?=`) çözülen ad
/// satır sonu içerebilir; yanıtlarken başlığa olduğu gibi yazılamaz.
pub fn clean_display_name(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| {
            if c.is_control() || is_bidi_control(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    collapse_ws(&spaced)
}

/// Metin aslında HTML kaynağı mı? Düz metin bölümüne HTML koyan göndericiler
/// için: baştaki boşluk ve BOM atıldıktan sonra `<` ile başlayan ve ilk
/// kilobaytında belge ya da yaygın blok etiketi geçen metin HTML sayılır.
/// `<ayse@ornek.com> yazdı:` gibi düz metinler HTML sayılmaz.
pub fn looks_like_html(text: &str) -> bool {
    let head = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if !head.starts_with('<') {
        return false;
    }
    let end = head.char_indices().nth(1024).map_or(head.len(), |(i, _)| i);
    let lower = head[..end].to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "<!doctype",
        "<html",
        "<head",
        "<body",
        "<meta ",
        "<table",
        "</div>",
        "</p>",
        "</td>",
        "<br>",
        "<br/>",
        "<br />",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// Liste satırında gösterilen önizleme: alıntı satırları atlanır, boşluklar
/// sadeleşir, ~200 karakterde kelime sınırından kesilir. HTML kaynağı verilirse
/// (ör. sunucunun hazırladığı önizleme) önce metne çevrilir.
pub fn snippet(text: &str) -> String {
    if looks_like_html(text) {
        return snippet(&html_to_text(text));
    }
    let body: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('>'))
        .collect();
    let flat = clean_preview(&body.join(" "));
    truncate_words(&flat, SNIPPET_LEN)
}

/// Önizlemede okunmayan parçaları atar: bültenlerin görünmez dolgu
/// karakterleri, doldurulmamış şablon değişkenleri (`{PREHEADER}`,
/// `{{ad}}`, `*|MC_PREVIEW_TEXT|*`, `%%preheader%%`), bağlantılar ve
/// bağlantıdan geriye kalan boş ayraçlar. Boşluklar sadeleşir.
fn clean_preview(s: &str) -> String {
    // Önizleme en çok ~200 karakter; gövdenin başı yeter. Büyük gövdede
    // temizlik doğrusal kalsın.
    let visible: String = s
        .chars()
        .filter(|&c| !is_invisible(c))
        .take(PREVIEW_SCAN)
        .collect();
    let mut t = strip_between(&visible, "{{", "}}", |_| true);
    t = strip_between(&t, "{%", "%}", |_| true);
    // Birleştirme etiketleri tek sözcüktür: "CPU %%95, bellek %%80" gibi
    // metnin arası silinmesin.
    t = strip_between(&t, "*|", "|*", is_merge_tag);
    t = strip_between(&t, "%%", "%%", is_merge_tag);
    let words: Vec<&str> = t
        .split_whitespace()
        .filter(|w| !is_link(w) && !is_template_token(w))
        .collect();
    // Bağlantı atılınca `( )`, `< >`, `[ ]` kalır; yalnız boş ayraç çiftleri gider.
    let mut kept: Vec<&str> = Vec::with_capacity(words.len());
    for w in words {
        if is_empty_brackets(w) {
            continue;
        }
        if let Some(&prev) = kept.last() {
            if closes(prev, w) {
                kept.pop();
                continue;
            }
        }
        kept.push(w);
    }
    // Atılan değişkenden sonra yalnız kalan noktalama öncekine yapışır ("Merhaba ,").
    let mut out = String::with_capacity(t.len());
    for w in kept {
        let punct = w
            .chars()
            .all(|c| matches!(c, ',' | '.' | ';' | ':' | '!' | '?'));
        if !out.is_empty() && !punct {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// Görünmez biçim karakterleri: bülten önizleme dolgusu (U+034F, U+200C),
/// sıfır genişlikli boşluklar, yumuşak tire, BOM.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}' | '\u{034F}' | '\u{180E}' | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}'
    )
}

/// Önizleme temizliğinde gövdenin en çok bu kadar karakterine bakılır.
const PREVIEW_SCAN: usize = 4096;
/// Şablon değişkeninin en uzun iç metni (karakter).
const TAG_MAX: usize = 60;

/// `*|MC_PREVIEW_TEXT|*`, `%%preheader%%`: boşluksuz tek sözcük.
fn is_merge_tag(inner: &str) -> bool {
    !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
}

/// `open … close` arasını (en çok `TAG_MAX` karakter, satır sonu yok, `valid`
/// kabul ederse) atar; kapanmayan açılış olduğu gibi kalır. Kapanış yalnız
/// kısa bir pencerede aranır: uzun metinde karesel tarama olmasın.
fn strip_between(s: &str, open: &str, close: &str, valid: fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len()..];
        let window_end = after
            .char_indices()
            .nth(TAG_MAX + close.chars().count())
            .map_or(after.len(), |(k, _)| k);
        match after[..window_end].find(close) {
            Some(j)
                if after[..j].chars().count() <= TAG_MAX
                    && !after[..j].contains('\n')
                    && valid(after[..j].trim()) =>
            {
                out.push_str(&rest[..i]);
                out.push(' ');
                rest = &after[j + close.len()..];
            }
            _ => {
                out.push_str(&rest[..i + open.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Bağlantı sözcüğü mü (`https://…`, `www.…`, `mailto:…`; önündeki ayraçla)?
fn is_link(word: &str) -> bool {
    let w = word.trim_start_matches(['(', '<', '[', '"', '\'']);
    let head: String = w.chars().take(8).collect::<String>().to_ascii_lowercase();
    head.starts_with("http://")
        || head.starts_with("https://")
        || head.starts_with("www.")
        || head.starts_with("mailto:")
}

/// Doldurulmamış büyük harfli şablon değişkeni: `{PREHEADER}`, `{FIRST_NAME},`.
/// Küçük harfli `{ad}` gibi metinler sayılmaz.
fn is_template_token(word: &str) -> bool {
    let w = word.trim_end_matches(['.', ',', ':', ';', '!', '?']);
    let Some(inner) = w.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else {
        return false;
    };
    (2..=40).contains(&inner.len())
        && inner.chars().any(|c| c.is_ascii_uppercase())
        && inner
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn is_empty_brackets(word: &str) -> bool {
    matches!(word, "()" | "[]" | "<>" | "{}" | "( )" | "[ ]" | "< >")
}

/// `prev` açılış, `w` eşi olan kapanış ayracı mı (arası boş kalmış)?
fn closes(prev: &str, w: &str) -> bool {
    let w = w.trim_end_matches(['.', ',', ':', ';']);
    matches!((prev, w), ("(", ")") | ("[", "]") | ("<", ">"))
}

fn truncate_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max).collect();
    match cut.rfind(' ') {
        Some(pos) if pos > max / 2 => cut[..pos].to_owned(),
        _ => cut,
    }
}

/// Türkçe büyük/küçük harf farkını gözeterek küçültür (İ/I/ı → i).
pub fn fold_tr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // ASCII'de `I` da `i` olur.
            c if c.is_ascii() => out.push(c.to_ascii_lowercase()),
            'İ' | 'ı' => out.push('i'),
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Konuşma kimliğinde kullanılan sadeleştirilmiş konunun üst sınırı (karakter).
const THREAD_SUBJECT_LIMIT: usize = 1000;

/// Konu satırından yanıt/iletme öneklerini atar ("Re:", "Fwd:", "YNT:",
/// "İLT:", "Cevap:" …, art arda olsa da). Sonuç Türkçe katlanmış küçük
/// harflidir (ı → i); konuşma kimliği için kullanılır.
///
/// Önekler dilimlenerek atılır (kopyalanmaz): binlerce `re:` içeren konu da
/// doğrusal sürede işlenir. Sonuç en çok 1000 karakterdir.
pub fn normalize_subject(subject: &str) -> String {
    const PREFIXES: &[&str] = &["re", "fwd", "fw", "ynt", "ilt", "cevap", "yanit", "ilet"];
    let folded = fold_tr(subject.trim());
    let mut s = folded.as_str();
    loop {
        let trimmed = s.trim_start();
        let mut stripped = None;
        for p in PREFIXES {
            if let Some(rest) = trimmed.strip_prefix(p) {
                // "Re[2]:" ya da "Re :" biçimlerini de kabul et.
                let mut rest = rest.trim_start();
                if let Some(r) = rest.strip_prefix('[') {
                    let digits = r.bytes().take_while(u8::is_ascii_digit).count();
                    if let Some(r) = r[digits..].strip_prefix(']') {
                        rest = r.trim_start();
                    }
                }
                if let Some(r) = rest.strip_prefix(':') {
                    stripped = Some(r);
                    break;
                }
            }
        }
        match stripped {
            Some(r) => s = r,
            None => break,
        }
    }
    let out = collapse_ws(s);
    match out.char_indices().nth(THREAD_SUBJECT_LIMIT) {
        Some((pos, _)) => out[..pos].trim_end().to_owned(),
        None => out,
    }
}

/// Tek satırlık görünen metni (konu, ek adı) temizler: yön değiştiren biçim
/// karakterleri (U+202A–U+202E, U+2066–U+2069) ve yön işaretleri (U+200E,
/// U+200F, U+061C) atılır; satır sonu ve sekme gibi boşluk denetimleri boşluğa,
/// diğer denetim karakterleri (C0, DEL, C1) hiçe çevrilir; baş ve son boşluk
/// kırpılır. `fatura\u{202E}fdp.exe` gibi adlar uzantıyı ters gösteremez.
pub fn clean_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_bidi_control(c) || matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}') {
            continue;
        }
        if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
            if c.is_whitespace() {
                out.push(' ');
            }
            continue;
        }
        out.push(c);
    }
    let trimmed = out.trim();
    if trimmed.len() == out.len() {
        out
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_metne_paragraflari_ayirir() {
        let t = html_to_text(
            "<html><head><style>p{color:red}</style><title>Başlık</title></head>\
             <body><p>Merhaba&nbsp;dünya</p><p>İkinci &amp; son</p><script>alert(1)</script></body></html>",
        );
        assert_eq!(t, "Merhaba dünya\n\nİkinci & son");
    }

    #[test]
    fn onizleme_alintiyi_atar_ve_keser() {
        let s = snippet("Selam,\n\n> eski satır\nYeni   satır");
        assert_eq!(s, "Selam, Yeni satır");
        let long = "kelime ".repeat(100);
        let s = snippet(&long);
        assert!(s.chars().count() <= SNIPPET_LEN);
        assert!(s.ends_with("kelime"));
    }

    #[test]
    fn onizleme_sablon_artigini_ve_baglantilari_atar() {
        assert_eq!(
            snippet("{PREHEADER} E-Posta adresini doğrula"),
            "E-Posta adresini doğrula"
        );
        assert_eq!(
            snippet("( https://www-new.yemeksepeti.com/ )\nSiparişin yolda."),
            "Siparişin yolda."
        );
        assert_eq!(
            snippet("Merhaba {{ first_name }}, *|MC_PREVIEW_TEXT|* kampanya %%preheader%% başladı"),
            "Merhaba, kampanya başladı"
        );
        assert_eq!(
            snippet("LinkedIn <https://linkedin.com/x> Profilin 6 kez görüntülendi"),
            "LinkedIn Profilin 6 kez görüntülendi"
        );
        assert_eq!(
            snippet("Fatura hazır\u{034f}\u{200c} \u{034f}\u{200c} \u{00ad}Ödeme bağlantısı: www.ornek.com."),
            "Fatura hazır Ödeme bağlantısı:"
        );
        // Anlamlı metin korunur: küçük harfli süslü ayraç, parantez içi metin,
        // kapanmayan şablon açılışı.
        assert_eq!(
            snippet("Biçim {ad} soyad (yarın) {{ eksik"),
            "Biçim {ad} soyad (yarın) {{ eksik"
        );
        assert_eq!(snippet("Hata kodu {X}"), "Hata kodu {X}");
        // Yüzde işaretli gerçek metin silinmez; yalnız tek sözcüklü etiket gider.
        assert_eq!(snippet("CPU %%95, bellek %%80"), "CPU %%95, bellek %%80");
        assert_eq!(snippet("Kampanya *|FNAME|* başladı"), "Kampanya başladı");
    }

    #[test]
    fn onizleme_temizligi_buyuk_govdede_dogrusal() {
        // Kapanmayan açılışlarla dolu yüzlerce KB'lık gövde (önceden karesel).
        for open in ["{{ ", "*| ", "%% "] {
            let body = open.repeat(200_000);
            let t = std::time::Instant::now();
            let _ = snippet(&body);
            assert!(
                t.elapsed() < std::time::Duration::from_millis(500),
                "{open:?}: {:?}",
                t.elapsed()
            );
        }
    }

    #[test]
    fn html_kaynagi_onizlemede_metne_cevrilir() {
        let html = "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Doğrulama</title>\
                    <style>p{color:red}</style></head><body><p>Kurtarma e-postanı doğrula:</p>\
                    <p>Kod: 123456</p></body></html>";
        assert!(looks_like_html(html));
        assert_eq!(snippet(html), "Kurtarma e-postanı doğrula: Kod: 123456");
        // Düz metin HTML sayılmaz.
        assert!(!looks_like_html("<ayse@ornek.com> yazdı:\n> merhaba"));
        assert!(!looks_like_html("Merhaba <b> etiketi"));
        assert!(!looks_like_html(""));
        assert!(looks_like_html("\u{feff}  <div>Selam</div>"));
    }

    #[test]
    fn konu_onekleri_atilir() {
        assert_eq!(
            normalize_subject("Re: RE: Fwd: Sunucu taşıma planı"),
            "sunucu taşima plani"
        );
        assert_eq!(normalize_subject("YNT: İLT: Cevap: Bütçe"), "bütçe");
        assert_eq!(normalize_subject("Re[2]: Fw : Toplantı"), "toplanti");
        assert_eq!(normalize_subject("Rehber"), "rehber");
        assert_eq!(normalize_subject("  ISPARTA  gezisi "), "isparta gezisi");
        assert_eq!(normalize_subject("Re[]: Re[x]: a"), "re[x]: a");
    }

    #[test]
    fn uzun_onek_zinciri_dogrusal_islenir() {
        // 300 KB'lık `re:re:…` konusu (önceden önek başına kopya: karesel).
        let subject = format!("{}Toplantı", "re:".repeat(100_000));
        let t = std::time::Instant::now();
        assert_eq!(normalize_subject(&subject), "toplanti");
        // Karesel işlem saniyeler sürer; sınır yüklü makinede hata ayıklama
        // derlemesine de pay bırakır.
        assert!(
            t.elapsed() < std::time::Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
        let subject = format!("{}x", "Re[1]: ".repeat(50_000));
        assert_eq!(normalize_subject(&subject), "x");
        // Sonuç sınırlı.
        let long = "a".repeat(5_000);
        assert_eq!(normalize_subject(&long).chars().count(), 1000);
    }

    #[test]
    fn derin_ic_ice_html_hizla_metne_doner() {
        // 50 bin: boyut sınırının altında, derinlik denetimine takılır;
        // 100 bin (500 KB): boyut sınırının üstünde.
        for n in [50_000, 100_000] {
            let html = format!("{}Merhaba{}", "<div>".repeat(n), "</div>".repeat(10));
            let t = std::time::Instant::now();
            assert_eq!(html_to_text(&html), "Merhaba");
            assert!(
                t.elapsed() < std::time::Duration::from_secs(1),
                "{n}: {:?}",
                t.elapsed()
            );
        }
        // Sınırın üstündeki büyük HTML doğrusal yoldan geçer.
        let big = format!(
            "<style>p{{}}</style>{}<p>son &amp; bitti</p>",
            "<p>satır</p>".repeat(30_000)
        );
        assert!(big.len() > TEXT_HTML_LIMIT);
        let text = html_to_text(&big);
        assert!(text.starts_with("satır\n\nsatır"), "{}", &text[..40]);
        assert!(text.ends_with("satır\n\nson & bitti"));
    }

    #[test]
    fn tek_satir_metin_temizlenir() {
        assert_eq!(clean_line("s\u{202E}fdp.exe"), "sfdp.exe");
        assert_eq!(clean_line("fatura\u{202E}fdp.exe"), "faturafdp.exe");
        assert_eq!(
            clean_line(" a\u{200E}b\u{200F}c\u{061C}\u{2066}d\u{2069} "),
            "abcd"
        );
        assert_eq!(
            clean_line("Satır\r\nİki\tÜç\u{0}\u{7f}\u{9b}"),
            "Satır  İki Üç"
        );
        assert_eq!(clean_line("Furkan Yıldırım"), "Furkan Yıldırım");
    }

    #[test]
    fn turkce_katlama() {
        assert_eq!(fold_tr("IŞIK İzmir"), "işik izmir");
    }

    #[test]
    fn gorunen_ad_temizlenir() {
        assert_eq!(
            clean_display_name("Evil\r\nX-Inject: 1"),
            "Evil X-Inject: 1"
        );
        assert_eq!(clean_display_name(" Ayşe\tDemir\u{0}\u{7f} "), "Ayşe Demir");
        assert_eq!(
            clean_display_name("fatura\u{202E}fdp.exe\u{2066}x\u{2069}"),
            "fatura fdp.exe x"
        );
        assert_eq!(clean_display_name("\u{85}\u{2028}"), "");
        assert_eq!(clean_display_name("Furkan Yıldırım"), "Furkan Yıldırım");
    }
}
