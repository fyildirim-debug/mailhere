//! HTML ön işleme: postadaki HTML, html5ever'a (ammonia) gitmeden önce
//! doğrusal sürede kanonik biçime çevrilir.
//!
//! html5ever'ın ağaç kurucusu bazı girdilerde karesel çalışır:
//!
//! - açık öğe yığınında kapsam aramaları: binlerce iç içe etiket (100 bin
//!   `<div>` ≈ 1 dk);
//! - biçim öğelerinin yeniden kurulması (`<p><b id=1></p>…`): yığın ve ağaç
//!   karesel büyür;
//! - tablo içindeki başıboş içerik ("foster parenting"): her ekleme, tablonun
//!   üst öğesinin çocukları arasında doğrusal arama yapar;
//! - tek etikette binlerce öznitelik: her öznitelik öncekilerle karşılaştırılır.
//!
//! Kendi belirteçleyicimiz HTML belirteçleme kurallarına göre okur ve
//! html5ever'a yalnız belirsizliği olmayan bir metin verir: yorumlar boş yoruma,
//! ham metin öğelerinin (`<style>`, `<script>`, `<title>` …) içeriği kaçışlı
//! metne çevrilir, metindeki `<` `&lt;` olur, svg/math alt ağaçları ile
//! `html`/`head`/`body`/`col`/`colgroup` etiketleri atılır, etiket başına
//! öznitelik sayısı sınırlanır. Böylece html5ever tam olarak burada görülen
//! etiketleri görür.
//!
//! Etiketler bir yığın modelinden geçer. Model, html5ever'ın açık öğe
//! yığınından yalnız onun da kesin olarak kapattığı öğeleri düşürür; bu yüzden
//! model derinliği gerçek derinliğin (sabit çarpanla) üst sınırıdır. Derinlik
//! [`MAX_DEPTH`]'i aşarsa [`canonicalize`] `None` döner ve çağıran düz metne
//! düşer. Tablo yapısı ayrıca izlenir; tablo bağlamında başıboş içerik
//! gelirse önce bir hücre açılır, böylece "foster parenting" hiç yaşanmaz.

/// İzin verilen en büyük iç içelik derinliği (model yığını).
pub(crate) const MAX_DEPTH: usize = 512;
/// Etiket başına aktarılan en çok öznitelik.
const MAX_ATTRS: usize = 32;
/// Belirteçleyicinin etiket başına sakladığı en çok öznitelik.
const MAX_PARSED_ATTRS: usize = 256;
/// Metin düğümü parça boyu (bayt), bkz. [`push_text_chunked`].
const TEXT_CHUNK: usize = 512;
/// Betik ve stil dışındaki ham metin öğelerinden (`<textarea>`, `<xmp>` …)
/// aktarılan en çok içerik (bayt); bu düğümler bölünemez.
const RAW_KEEP: usize = 1024;
/// Bundan çok `&`/U+0080–U+00BF içeren öznitelik değeri atılır.
const MAX_ATTR_SPECIALS: usize = 256;
/// Atılan belirteçlerin yerine yazılan boş yorum: metin parçalarının sınırı
/// korunur (`&am<!-- -->p;` gibi parçalar birleşip varlığa dönüşmez).
const COMMENT: &str = "<!---->";

fn is_ws(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

/// Belirteç.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Text(&'a str),
    /// Ham metin öğesinin içeriği.
    Raw(&'a str),
    /// Yorum, DOCTYPE, işleme talimatı ya da bozuk yorum.
    Comment,
    Start {
        name: &'a str,
        self_closing: bool,
    },
    End {
        name: &'a str,
    },
}

/// HTML belirteçleme kurallarına uyan doğrusal belirteçleyici.
struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    /// Bitiş etiketi beklenen ham metin öğesi.
    raw: Option<&'a str>,
    /// `<plaintext>` sonrası: kalan her şey metindir.
    plaintext: bool,
    /// svg/math içi (atlanıyor): CDATA bölümü tanınır.
    foreign: bool,
    attrs: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            raw: None,
            plaintext: false,
            foreign: false,
            attrs: Vec::new(),
        }
    }

    /// Son başlangıç etiketi ham metin öğesiydi; içeriği bitiş etiketine dek
    /// tek parça okunur.
    fn enter_raw(&mut self, name: &'a str, plaintext: bool) {
        if plaintext {
            self.plaintext = true;
        } else {
            self.raw = Some(name);
        }
    }

    fn rest_from(&mut self, from: usize) -> Tok<'a> {
        let t = &self.src[from..];
        self.pos = self.src.len();
        Tok::Text(t)
    }

    /// `from`dan sonraki ilk `>`'ya dek atlar (bozuk yorum, DOCTYPE).
    fn bogus(&mut self, from: usize) -> Tok<'a> {
        self.pos = match self.src[from..].find('>') {
            Some(i) => from + i + 1,
            None => self.src.len(),
        };
        Tok::Comment
    }

    /// `<!--` sonrası yorumun sonu: `-->`, `--!>`, ya da hemen `>`/`->`.
    fn comment(&mut self, body: usize) -> Tok<'a> {
        let s = &self.src[body..];
        if s.starts_with('>') {
            self.pos = body + 1;
            return Tok::Comment;
        }
        if s.starts_with("->") {
            self.pos = body + 2;
            return Tok::Comment;
        }
        let mut from = 0;
        self.pos = loop {
            let Some(i) = s[from..].find("--") else {
                break self.src.len();
            };
            let at = from + i;
            let tail = &s[at + 2..];
            if tail.starts_with('>') {
                break body + at + 3;
            }
            if tail.starts_with("!>") {
                break body + at + 4;
            }
            from = at + 1;
        };
        Tok::Comment
    }

    /// Ham metnin bittiği yer: `</ad` ardından boşluk, `/` ya da `>`.
    fn raw_end(&self, name: &str) -> usize {
        let b = self.src.as_bytes();
        let mut from = self.pos;
        loop {
            let Some(i) = self.src[from..].find("</") else {
                return self.src.len();
            };
            let at = from + i;
            let n0 = at + 2;
            let n1 = n0 + name.len();
            if n1 <= b.len()
                && b[n0..n1].eq_ignore_ascii_case(name.as_bytes())
                && b.get(n1)
                    .is_some_and(|&c| is_ws(c) || c == b'/' || c == b'>')
            {
                return at;
            }
            from = n0;
        }
    }

    /// Etiketi (ad ve öznitelikler) okur. Etiket bitmeden girdi biterse
    /// etiket yok sayılır (HTML kuralı).
    fn tag(&mut self, name_start: usize, end: bool) -> Option<Tok<'a>> {
        let src = self.src;
        let b = src.as_bytes();
        let len = b.len();
        let mut i = name_start;
        while i < len && !is_ws(b[i]) && b[i] != b'/' && b[i] != b'>' {
            i += 1;
        }
        let name = &src[name_start..i];
        self.attrs.clear();
        let mut self_closing = false;
        loop {
            while i < len && is_ws(b[i]) {
                i += 1;
            }
            if i >= len {
                self.pos = len;
                return None;
            }
            match b[i] {
                b'>' => {
                    i += 1;
                    break;
                }
                b'/' => {
                    i += 1;
                    if b.get(i) == Some(&b'>') {
                        self_closing = true;
                        i += 1;
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            // Öznitelik adı; ilk karakter `=` bile olabilir.
            let a0 = i;
            i += 1;
            while i < len && !is_ws(b[i]) && !matches!(b[i], b'/' | b'>' | b'=') {
                i += 1;
            }
            let aname = &src[a0..i];
            while i < len && is_ws(b[i]) {
                i += 1;
            }
            let mut value = None;
            if b.get(i) == Some(&b'=') {
                i += 1;
                while i < len && is_ws(b[i]) {
                    i += 1;
                }
                match b.get(i).copied() {
                    None => {
                        self.pos = len;
                        return None;
                    }
                    Some(q @ (b'"' | b'\'')) => {
                        let v0 = i + 1;
                        let Some(k) = b[v0..].iter().position(|&c| c == q) else {
                            self.pos = len;
                            return None;
                        };
                        value = Some(&src[v0..v0 + k]);
                        i = v0 + k + 1;
                    }
                    Some(b'>') => value = Some(""),
                    Some(_) => {
                        let v0 = i;
                        while i < len && !is_ws(b[i]) && b[i] != b'>' {
                            i += 1;
                        }
                        value = Some(&src[v0..i]);
                    }
                }
            }
            if !end && self.attrs.len() < MAX_PARSED_ATTRS {
                self.attrs.push((aname, value));
            }
        }
        self.pos = i;
        Some(if end {
            Tok::End { name }
        } else {
            Tok::Start { name, self_closing }
        })
    }

    fn next(&mut self) -> Option<Tok<'a>> {
        loop {
            if self.plaintext {
                self.plaintext = false;
                if self.pos < self.src.len() {
                    let t = &self.src[self.pos..];
                    self.pos = self.src.len();
                    return Some(Tok::Raw(t));
                }
                return None;
            }
            if let Some(name) = self.raw.take() {
                let end = self.raw_end(name);
                if end > self.pos {
                    let t = &self.src[self.pos..end];
                    self.pos = end;
                    return Some(Tok::Raw(t));
                }
                continue;
            }
            let src = self.src;
            let b = src.as_bytes();
            let pos = self.pos;
            if pos >= b.len() {
                return None;
            }
            if b[pos] != b'<' {
                let end = src[pos..].find('<').map_or(b.len(), |i| pos + i);
                self.pos = end;
                return Some(Tok::Text(&src[pos..end]));
            }
            return match b.get(pos + 1) {
                Some(b'!') => {
                    let s = &src[pos..];
                    Some(if s.starts_with("<!--") {
                        self.comment(pos + 4)
                    } else if self.foreign && s.starts_with("<![CDATA[") {
                        self.pos = s.find("]]>").map_or(b.len(), |i| pos + i + 3);
                        Tok::Comment
                    } else {
                        self.bogus(pos + 2)
                    })
                }
                Some(b'?') => Some(self.bogus(pos + 2)),
                Some(b'/') => match b.get(pos + 2) {
                    Some(c) if c.is_ascii_alphabetic() => self.tag(pos + 2, true),
                    Some(b'>') => {
                        self.pos = pos + 3;
                        Some(Tok::Comment)
                    }
                    None => Some(self.rest_from(pos)),
                    Some(_) => Some(self.bogus(pos + 2)),
                },
                Some(c) if c.is_ascii_alphabetic() => self.tag(pos + 1, false),
                _ => {
                    self.pos = pos + 1;
                    Some(Tok::Text(&src[pos..pos + 1]))
                }
            };
        }
    }

    /// svg/math alt ağacını eşleşen bitiş etiketine dek atlar.
    fn skip_subtree(&mut self, root: &str) {
        self.foreign = true;
        let mut depth = 1usize;
        while let Some(t) = self.next() {
            match t {
                Tok::Start {
                    name,
                    self_closing: false,
                } if name.eq_ignore_ascii_case(root) => depth += 1,
                Tok::End { name } if name.eq_ignore_ascii_case(root) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        self.foreign = false;
    }
}

/// html5ever'ın ağaç kurucusu açısından etiket sınıfı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Hemen kapanan (yığına girmeyen) öğe.
    Void,
    /// Kanonik metne hiç yazılmayan etiket.
    Dropped,
    /// svg/math: alt ağaç atılır.
    Foreign,
    Table,
    Caption,
    Section,
    Row,
    Cell,
    Template,
    /// Biçim öğesi (`b`, `a`, `font` …): kapanınca yeniden kurulabilir.
    Formatting,
    /// `applet`, `marquee`, `object`: kapsam sınırı ve biçim listesi işareti.
    Marker,
    Select,
    Button,
    List,
    P,
    Li,
    DdDt,
    Heading,
    /// Bitiş etiketi kapsamdaysa üstündekileri de kapatan özel öğeler.
    Closable,
    Form,
    /// İçeriği ham metin olan öğeler.
    RawText,
    Plaintext,
    Other,
}

fn classify(name: &str) -> Class {
    let mut buf = [0u8; 16];
    let b = name.as_bytes();
    if b.len() > buf.len() {
        return Class::Other;
    }
    for (d, s) in buf.iter_mut().zip(b) {
        *d = s.to_ascii_lowercase();
    }
    match &buf[..b.len()] {
        b"area" | b"base" | b"basefont" | b"bgsound" | b"br" | b"embed" | b"hr" | b"image"
        | b"img" | b"input" | b"keygen" | b"link" | b"meta" | b"param" | b"source" | b"track"
        | b"wbr" => Class::Void,
        b"html" | b"head" | b"body" | b"frameset" | b"frame" | b"col" | b"colgroup"
        | b"selectedcontent" => Class::Dropped,
        b"svg" | b"math" => Class::Foreign,
        b"table" => Class::Table,
        b"caption" => Class::Caption,
        b"tbody" | b"thead" | b"tfoot" => Class::Section,
        b"tr" => Class::Row,
        b"td" | b"th" => Class::Cell,
        b"template" => Class::Template,
        b"a" | b"b" | b"big" | b"code" | b"em" | b"font" | b"i" | b"nobr" | b"s" | b"small"
        | b"strike" | b"strong" | b"tt" | b"u" => Class::Formatting,
        b"applet" | b"marquee" | b"object" => Class::Marker,
        b"select" => Class::Select,
        b"button" => Class::Button,
        b"ol" | b"ul" => Class::List,
        b"p" => Class::P,
        b"li" => Class::Li,
        b"dd" | b"dt" => Class::DdDt,
        b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => Class::Heading,
        b"address" | b"article" | b"aside" | b"blockquote" | b"center" | b"details" | b"dialog"
        | b"dir" | b"div" | b"dl" | b"fieldset" | b"figcaption" | b"figure" | b"footer"
        | b"header" | b"hgroup" | b"listing" | b"main" | b"menu" | b"nav" | b"pre" | b"search"
        | b"section" | b"summary" => Class::Closable,
        b"form" => Class::Form,
        b"title" | b"textarea" | b"style" | b"xmp" | b"iframe" | b"noembed" | b"noframes"
        | b"noscript" | b"script" => Class::RawText,
        b"plaintext" => Class::Plaintext,
        _ => Class::Other,
    }
}

/// Tablo yapısı öğesi mi (yalnız tablo izleyicisi kapatır)?
fn is_table_part(c: Class) -> bool {
    matches!(
        c,
        Class::Table | Class::Caption | Class::Section | Class::Row | Class::Cell | Class::Template
    )
}

/// Başlangıç etiketi açık `<p>`'yi kapatır mı?
fn closes_p(name: &str, class: Class) -> bool {
    match class {
        Class::Closable
        | Class::List
        | Class::P
        | Class::Li
        | Class::DdDt
        | Class::Heading
        | Class::Plaintext => true,
        Class::RawText => name.eq_ignore_ascii_case("xmp"),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy)]
struct El<'a> {
    name: &'a str,
    class: Class,
}

/// Tablo izleyicisinin kaydı. `base`: öğe açılmadan önceki model yığını
/// uzunluğu; öğe kapanınca model buraya dek kısalır (html5ever de üstündeki
/// her şeyi kapatır).
#[derive(Debug, Clone, Copy)]
struct TEntry<'a> {
    kind: Class,
    name: &'a str,
    base: usize,
}

/// Derinlik aşıldı.
struct TooDeep;

struct Canon<'a> {
    out: String,
    stack: Vec<El<'a>>,
    tables: Vec<TEntry<'a>>,
}

fn valid_attr_name(n: &str) -> bool {
    !n.is_empty()
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
}

fn push_escaped_text(out: &mut String, t: &str) {
    let mut rest = t;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        out.push_str("&lt;");
        rest = &rest[i + 1..];
    }
    out.push_str(rest);
}

/// Metni kaçışlı yazar ve ~[`TEXT_CHUNK`] baytta bir boş yorumla böler.
///
/// html5ever'ın yazdırıcısı metin düğümünde her `&` ve U+0080–U+00BF
/// karakteri için düğümün geri kalanında `<`/`>` arar; uzun bir düğümde bu
/// karesel sürer. Bölünen parçalar ayrı metin düğümü olur. Bölme yalnız harf,
/// rakam, `;` ve `#` olmayan bir karakterden önce yapılır; varlık
/// (`&amp;`, `&#38;`) ortasından bölünmez. `amp` doğruysa `&` de kaçışlanır
/// (varlık çözülmeyen `<plaintext>` içeriği).
fn push_text_chunked(out: &mut String, t: &str, amp: bool) {
    let b = t.as_bytes();
    let mut last = 0;
    let mut since = 0usize;
    for (i, &c) in b.iter().enumerate() {
        // UTF-8 devam baytı (0x80–0xBF) karakter başı değildir.
        let char_start = !(0x80..0xC0).contains(&c);
        if since >= TEXT_CHUNK
            && char_start
            && !(c.is_ascii_alphanumeric() || c == b';' || c == b'#')
        {
            out.push_str(&t[last..i]);
            out.push_str(COMMENT);
            last = i;
            since = 0;
        }
        since += 1;
        let rep = match c {
            b'<' => "&lt;",
            b'&' if amp => "&amp;",
            _ => continue,
        };
        out.push_str(&t[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&t[last..]);
}

/// Öznitelik değerinde yazdırıcıyı karesel çalıştıran karakter sayısı
/// (`&` ve U+0080–U+00BF'nin ilk baytı; ham değerde üst sınır).
fn attr_specials(v: &str) -> usize {
    v.bytes().filter(|&b| b == b'&' || b == 0xC2).count()
}

impl<'a> Canon<'a> {
    fn new(cap: usize) -> Self {
        Self {
            out: String::with_capacity(cap + cap / 8),
            stack: Vec::new(),
            tables: Vec::new(),
        }
    }

    fn top(&self) -> Option<El<'a>> {
        self.stack.last().copied()
    }

    fn push(&mut self, name: &'a str, class: Class) -> Result<(), TooDeep> {
        self.stack.push(El { name, class });
        if self.stack.len() > MAX_DEPTH {
            return Err(TooDeep);
        }
        Ok(())
    }

    /// En üstteki tablo ya da şablon kaydı.
    fn anchor(&self) -> Option<usize> {
        self.tables
            .iter()
            .rposition(|e| matches!(e.kind, Class::Table | Class::Template))
    }

    /// html5ever tablo kipinde mi (başıboş içerik tablonun dışına taşınır)?
    fn table_context(&self) -> Option<Class> {
        self.tables
            .last()
            .map(|e| e.kind)
            .filter(|k| matches!(k, Class::Table | Class::Section | Class::Row))
    }

    /// `i`. kaydı ve üstündekileri kapatır.
    fn close_from(&mut self, i: usize) {
        if let Some(e) = self.tables.get(i) {
            let base = e.base.min(self.stack.len());
            self.tables.truncate(i);
            self.stack.truncate(base);
        }
    }

    fn find_in_region(&self, from: usize, kind: Class) -> Option<usize> {
        (from..self.tables.len())
            .rev()
            .find(|&i| self.tables[i].kind == kind)
    }

    fn push_table(&mut self, kind: Class, name: &'a str) -> Result<(), TooDeep> {
        self.tables.push(TEntry {
            kind,
            name,
            base: self.stack.len(),
        });
        self.push(name, kind)
    }

    fn push_implied(&mut self, kind: Class, name: &'a str) {
        self.tables.push(TEntry {
            kind,
            name,
            base: self.stack.len(),
        });
    }

    /// Tablo yapısı başlangıç etiketi. html5ever etiketi yok sayacaksa
    /// (tablo dışında `<td>`) `false`.
    fn table_start(&mut self, name: &'a str, class: Class) -> Result<bool, TooDeep> {
        match class {
            Class::Table => {
                // Tablo kipinde yeni tablo öncekini kapatır.
                if self.table_context().is_some() {
                    if let Some(a) = self
                        .anchor()
                        .filter(|&a| self.tables[a].kind == Class::Table)
                    {
                        self.close_from(a);
                    }
                }
            }
            Class::Template => {}
            _ => {
                let Some(a) = self.anchor() else {
                    return Ok(false);
                };
                let region = a + 1;
                match class {
                    Class::Caption | Class::Section => self.close_from(region),
                    Class::Row => match self.find_in_region(region, Class::Section) {
                        Some(i) => self.close_from(i + 1),
                        None => {
                            self.close_from(region);
                            self.push_implied(Class::Section, "tbody");
                        }
                    },
                    _ => {
                        if let Some(i) = self.find_in_region(region, Class::Row) {
                            self.close_from(i + 1);
                        } else {
                            match self.find_in_region(region, Class::Section) {
                                Some(i) => self.close_from(i + 1),
                                None => {
                                    self.close_from(region);
                                    self.push_implied(Class::Section, "tbody");
                                }
                            }
                            self.push_implied(Class::Row, "tr");
                        }
                    }
                }
            }
        }
        self.push_table(class, name)?;
        Ok(true)
    }

    fn table_end(&mut self, name: &str, class: Class) {
        match class {
            Class::Table => {
                if let Some(a) = self
                    .anchor()
                    .filter(|&a| self.tables[a].kind == Class::Table)
                {
                    self.close_from(a);
                }
            }
            Class::Template => {
                if let Some(i) = self.tables.iter().rposition(|e| e.kind == Class::Template) {
                    self.close_from(i);
                }
            }
            _ => {
                let Some(a) = self.anchor() else {
                    return;
                };
                let found = (a + 1..self.tables.len()).rev().find(|&i| {
                    let e = self.tables[i];
                    e.kind == class
                        && (!matches!(class, Class::Section | Class::Cell)
                            || e.name.eq_ignore_ascii_case(name))
                });
                // Hücre yalnız en üstteyse kapanır.
                if let Some(i) = found {
                    if class != Class::Cell || i + 1 == self.tables.len() {
                        self.close_from(i);
                    }
                }
            }
        }
    }

    /// Tablo bağlamında başıboş içerikten önce hücre açar.
    fn inject_cell(&mut self) -> Result<(), TooDeep> {
        match self.table_context() {
            Some(Class::Row) => {}
            Some(_) => {
                self.table_start("tr", Class::Row)?;
                self.out.push_str("<tr>");
            }
            None => return Ok(()),
        }
        self.table_start("td", Class::Cell)?;
        self.out.push_str("<td>");
        Ok(())
    }

    /// Başlangıç etiketinin html5ever'da kesin olarak kapattığı öğeler.
    fn auto_close(&mut self, name: &str, class: Class) {
        if closes_p(name, class) && self.top().is_some_and(|t| t.class == Class::P) {
            self.stack.pop();
        }
        let Some(top) = self.top() else {
            return;
        };
        let close = match class {
            Class::Li | Class::DdDt | Class::Button => {
                top.class == class && {
                    // `li` yalnız `li`'yi, `dd`/`dt` birbirini kapatır.
                    class != Class::Li || top.name.eq_ignore_ascii_case("li")
                }
            }
            Class::Formatting => {
                (name.eq_ignore_ascii_case("a") || name.eq_ignore_ascii_case("nobr"))
                    && top.name.eq_ignore_ascii_case(name)
            }
            _ => false,
        };
        if close {
            self.stack.pop();
        }
    }

    fn emit_start(&mut self, name: &str, attrs: &[(&str, Option<&str>)]) {
        let out = &mut self.out;
        out.push('<');
        out.push_str(name);
        for (k, v) in attrs
            .iter()
            .filter(|(k, v)| {
                valid_attr_name(k) && v.is_none_or(|v| attr_specials(v) <= MAX_ATTR_SPECIALS)
            })
            .take(MAX_ATTRS)
        {
            out.push(' ');
            out.push_str(k);
            if let Some(v) = v {
                out.push_str("=\"");
                let mut rest = *v;
                while let Some(i) = rest.find('"') {
                    out.push_str(&rest[..i]);
                    out.push_str("&quot;");
                    rest = &rest[i + 1..];
                }
                out.push_str(rest);
                out.push('"');
            }
        }
        out.push('>');
    }

    fn start(
        &mut self,
        name: &'a str,
        class: Class,
        attrs: &[(&str, Option<&str>)],
    ) -> Result<(), TooDeep> {
        let keeps_table = is_table_part(class)
            || class == Class::Form
            || (class == Class::RawText
                && (name.eq_ignore_ascii_case("style") || name.eq_ignore_ascii_case("script")));
        if !keeps_table && self.table_context().is_some() {
            self.inject_cell()?;
        }
        match class {
            c if is_table_part(c) => {
                if !self.table_start(name, class)? {
                    self.out.push_str(COMMENT);
                    return Ok(());
                }
            }
            Class::Void => {}
            // Tabloda form eklenip hemen kapanır (ya da yok sayılır).
            Class::Form if self.table_context().is_some() => {}
            _ => {
                self.auto_close(name, class);
                self.push(name, class)?;
            }
        }
        self.emit_start(name, attrs);
        Ok(())
    }

    /// Bitiş etiketi; yığından yalnız html5ever'ın da kesin kapattıkları düşer.
    fn end(&mut self, name: &'a str, class: Class) {
        match class {
            Class::Dropped | Class::Foreign => {
                self.out.push_str(COMMENT);
                return;
            }
            // Tablo bağlamında `</p>` ve `</br>` tablonun dışına öğe ekler.
            Class::P | Class::Void if self.table_context().is_some() => {
                self.out.push_str(COMMENT);
                return;
            }
            Class::Void => {}
            c if is_table_part(c) => self.table_end(name, class),
            Class::Closable
            | Class::List
            | Class::Select
            | Class::Button
            | Class::P
            | Class::Li
            | Class::DdDt
            | Class::Heading => self.close_walk(name, class),
            _ => {
                if self
                    .top()
                    .is_some_and(|t| !is_table_part(t.class) && t.name.eq_ignore_ascii_case(name))
                {
                    self.stack.pop();
                }
            }
        }
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
    }

    /// Kapsam içindeki öğeye dek kapatır. Biçim öğeleri (yeniden kurulabilir)
    /// ve kapsam sınırları aşılmaz; o durumda hiçbir şey düşmez.
    fn close_walk(&mut self, name: &str, class: Class) {
        for i in (0..self.stack.len()).rev() {
            let e = self.stack[i];
            if e.name.eq_ignore_ascii_case(name) {
                self.stack.truncate(i);
                return;
            }
            let blocks = match e.class {
                Class::Formatting | Class::Marker | Class::Select => true,
                c if is_table_part(c) => true,
                Class::Button => class == Class::P,
                Class::List => class == Class::Li,
                Class::Heading => class == Class::Heading,
                _ => false,
            };
            if blocks {
                return;
            }
        }
    }

    fn text(&mut self, t: &str) -> Result<(), TooDeep> {
        if self.table_context().is_some() && t.bytes().any(|b| !is_ws(b)) {
            self.inject_cell()?;
        }
        push_text_chunked(&mut self.out, t, false);
        Ok(())
    }

    /// Varlıkları çözülmeyecek düz metin (`<plaintext>` içeriği).
    fn text_literal(&mut self, t: &str) -> Result<(), TooDeep> {
        if self.table_context().is_some() && t.bytes().any(|b| !is_ws(b)) {
            self.inject_cell()?;
        }
        push_text_chunked(&mut self.out, t, true);
        Ok(())
    }
}

/// HTML'i html5ever için kanonik biçime çevirir. Ağaç [`MAX_DEPTH`]'ten derin
/// olacaksa `None` (çağıran düz metne düşer).
pub(crate) fn canonicalize(html: &str) -> Option<String> {
    let mut lx = Lexer::new(html);
    let mut c = Canon::new(html.len());
    // Son ham metin öğesi: betik/stil içeriği (zaten atılır) tam, diğerleri
    // kısaltılarak aktarılır; `<plaintext>` sonrası düz metne çevrilir.
    let mut raw_mode = Class::RawText;
    let mut raw_full = false;
    while let Some(tok) = lx.next() {
        let step = match tok {
            Tok::Text(t) => c.text(t),
            Tok::Raw(t) if raw_mode == Class::Plaintext => c.text_literal(t),
            Tok::Raw(t) => {
                let keep = if raw_full || t.len() <= RAW_KEEP {
                    t
                } else {
                    let mut end = RAW_KEEP;
                    while !t.is_char_boundary(end) {
                        end -= 1;
                    }
                    &t[..end]
                };
                push_escaped_text(&mut c.out, keep);
                Ok(())
            }
            Tok::Comment => {
                c.out.push_str(COMMENT);
                Ok(())
            }
            Tok::Start { name, self_closing } => {
                let class = classify(name);
                match class {
                    Class::Foreign => {
                        if !self_closing {
                            lx.skip_subtree(name);
                        }
                        c.out.push_str(COMMENT);
                        Ok(())
                    }
                    Class::Dropped => {
                        c.out.push_str(COMMENT);
                        Ok(())
                    }
                    // Kalan her şey düz metindir; etiket yazılmaz, içerik
                    // bölünebilir metin olarak aktarılır.
                    Class::Plaintext => {
                        c.out.push_str(COMMENT);
                        raw_mode = Class::Plaintext;
                        lx.enter_raw(name, true);
                        Ok(())
                    }
                    _ => {
                        let attrs = std::mem::take(&mut lx.attrs);
                        let r = c.start(name, class, &attrs);
                        lx.attrs = attrs;
                        if class == Class::RawText {
                            raw_mode = Class::RawText;
                            raw_full = name.eq_ignore_ascii_case("script")
                                || name.eq_ignore_ascii_case("style");
                            lx.enter_raw(name, false);
                        }
                        r
                    }
                }
            }
            Tok::End { name } => {
                c.end(name, classify(name));
                Ok(())
            }
        };
        if step.is_err() {
            return None;
        }
    }
    Some(c.out)
}

/// Düz metinde satır sonu gerektiren etiket mi?
fn is_block(name: &str) -> bool {
    match classify(name) {
        Class::Closable
        | Class::List
        | Class::P
        | Class::Li
        | Class::DdDt
        | Class::Heading
        | Class::Table
        | Class::Row
        | Class::Caption => true,
        Class::Void => name.eq_ignore_ascii_case("br") || name.eq_ignore_ascii_case("hr"),
        _ => false,
    }
}

/// Düz metne alınmayan ham metin öğesi (betik, stil, başlık).
fn drops_content(name: &str) -> bool {
    [
        "script", "style", "title", "noscript", "iframe", "noembed", "noframes",
    ]
    .iter()
    .any(|n| n.eq_ignore_ascii_case(name))
}

/// HTML'i doğrusal sürede düz metne çevirir (html5ever'sız yedek yol):
/// etiketler atılır, blok etiketleri satır sonu olur, sık kullanılan
/// karakter varlıkları çözülür.
pub(crate) fn strip_tags(html: &str) -> String {
    let mut lx = Lexer::new(html);
    let mut out = String::with_capacity(html.len() / 2);
    let mut drop_raw = false;
    while let Some(tok) = lx.next() {
        match tok {
            Tok::Text(t) => decode_entities_into(&mut out, t),
            Tok::Raw(t) => {
                if !drop_raw {
                    decode_entities_into(&mut out, t);
                }
            }
            Tok::Comment => {}
            Tok::Start { name, self_closing } => {
                let class = classify(name);
                if class == Class::Foreign && !self_closing {
                    lx.skip_subtree(name);
                } else if is_block(name) {
                    out.push('\n');
                } else if matches!(class, Class::Cell) {
                    out.push(' ');
                }
                if matches!(class, Class::RawText | Class::Plaintext) {
                    drop_raw = drops_content(name);
                    lx.enter_raw(name, class == Class::Plaintext);
                }
            }
            Tok::End { name } => {
                if is_block(name) {
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// Düz metni güvenli HTML'e çevirir: boş satırla ayrılan paragraflar `<p>`,
/// tek satır sonları `<br>` olur.
pub(crate) fn text_to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    for para in text.split("\n\n").filter(|p| !p.trim().is_empty()) {
        out.push_str("<p>");
        for (i, line) in para.lines().enumerate() {
            if i > 0 {
                out.push_str("<br>");
            }
            for ch in line.chars() {
                match ch {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '"' => out.push_str("&quot;"),
                    c => out.push(c),
                }
            }
        }
        out.push_str("</p>\n");
    }
    out
}

/// Sık kullanılan adlı karakter varlıkları.
const ENTITIES: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("copy", '©'),
    ("reg", '®'),
    ("trade", '™'),
    ("hellip", '…'),
    ("mdash", '—'),
    ("ndash", '–'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("sbquo", '‚'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("bdquo", '„'),
    ("laquo", '«'),
    ("raquo", '»'),
    ("bull", '•'),
    ("middot", '·'),
    ("euro", '€'),
    ("pound", '£'),
    ("yen", '¥'),
    ("cent", '¢'),
    ("deg", '°'),
    ("plusmn", '±'),
    ("times", '×'),
    ("divide", '÷'),
    ("sect", '§'),
    ("para", '¶'),
    ("shy", '\u{ad}'),
    ("zwnj", '\u{200c}'),
    ("zwj", '\u{200d}'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("thinsp", '\u{2009}'),
    ("iexcl", '¡'),
    ("iquest", '¿'),
    ("ccedil", 'ç'),
    ("Ccedil", 'Ç'),
    ("ouml", 'ö'),
    ("Ouml", 'Ö'),
    ("uuml", 'ü'),
    ("Uuml", 'Ü'),
    ("auml", 'ä'),
    ("Auml", 'Ä'),
    ("szlig", 'ß'),
    ("eacute", 'é'),
    ("Eacute", 'É'),
    ("egrave", 'è'),
    ("agrave", 'à'),
    ("aacute", 'á'),
    ("iacute", 'í'),
    ("oacute", 'ó'),
    ("uacute", 'ú'),
    ("ntilde", 'ñ'),
    ("acirc", 'â'),
    ("ecirc", 'ê'),
    ("icirc", 'î'),
    ("ocirc", 'ô'),
    ("ucirc", 'û'),
];

/// Noktalı virgülsüz de çözülen eski varlıklar.
const LEGACY: &[&str] = &["amp", "lt", "gt", "quot", "nbsp", "copy", "reg"];

/// `&` sonrası varlığı çözer: (karakter, tüketilen bayt).
fn decode_entity(s: &str) -> Option<(char, usize)> {
    let b = s.as_bytes();
    if b.first() == Some(&b'#') {
        let (hex, start) = match b.get(1) {
            Some(b'x' | b'X') => (true, 2),
            _ => (false, 1),
        };
        let digits = b[start..]
            .iter()
            .take_while(|c| {
                if hex {
                    c.is_ascii_hexdigit()
                } else {
                    c.is_ascii_digit()
                }
            })
            .count();
        if digits == 0 || digits > 8 {
            return None;
        }
        let n = u32::from_str_radix(&s[start..start + digits], if hex { 16 } else { 10 }).ok()?;
        let c = match char::from_u32(n) {
            Some(c) if n != 0 => c,
            _ => '\u{fffd}',
        };
        let mut used = start + digits;
        if b.get(used) == Some(&b';') {
            used += 1;
        }
        return Some((c, used));
    }
    let len = b
        .iter()
        .take(32)
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    let name = &s[..len];
    let &(_, c) = ENTITIES.iter().find(|(n, _)| *n == name)?;
    if b.get(len) == Some(&b';') {
        return Some((c, len + 1));
    }
    LEGACY.contains(&name).then_some((c, len))
}

fn decode_entities_into(out: &mut String, t: &str) {
    let mut rest = t;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match decode_entity(after) {
            Some((c, used)) => {
                out.push(c);
                rest = &after[used..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn depth_ok(html: &str) -> bool {
        canonicalize(html).is_some()
    }

    /// ammonia'nın (html5ever) özgün HTML'den ve kanonik biçimden ürettiği
    /// çıktı aynı olmalı.
    fn same_clean(html: &str) {
        let canon = canonicalize(html).expect("kanonik biçim");
        let direct = ammonia::Builder::default().clean(html).to_string();
        let via = ammonia::Builder::default().clean(&canon).to_string();
        assert_eq!(direct, via, "girdi: {html}\nkanonik: {canon}");
    }

    #[test]
    fn kanonik_bicim_ayni_sonucu_verir() {
        for html in [
            r#"<p class="x" title='a "b" c' data-x=1>Merhaba &amp; selam &lt;3 a<b</p>"#,
            "<div><!-- yorum --><b>kalın<i>eğik</b>düz</i></div>",
            "<!DOCTYPE html><html><head><title>Başlık</title><style>p{color:red}</style></head><body><p>a<p>b</body></html>",
            "<table><tr><td>a<td>b<tr><td>c</table><ul><li>1<li>2</ul>",
            "<a href=\"https://ornek.com/?a=1&amp;b=2\">bağlantı</a><img src=x alt=\"&quot;\">",
            "<p>Satır<br/>iki</p><hr/><pre>\n  kod</pre>",
            "<!--[if mso]><table><tr><td>mso</td></tr></table><![endif]--><p>son",
            "metin <3 & devam; &copy &nbsp; &#x41;&#66;",
            "<div title=\"<b>\">x</div><div =bad a\"b=1>y</div>",
            "<p>&am<!-- -->p; ve &amp</p>",
            "<font face=Arial color=red>eski</font><center>orta</center>",
            "<textarea>a<b>c</textarea><xmp>x y</xmp>",
            // Outlook
            "<html xmlns:o=\"urn:schemas-microsoft-com:office:office\"><head><meta http-equiv=Content-Type \
             content=\"text/html; charset=utf-8\"><style><!-- p.MsoNormal {margin:0cm;} --></style><!--[if gte mso 9]>\
             <xml><o:shapedefaults v:ext=\"edit\" spidmax=\"1026\" /></xml><![endif]--></head><body lang=TR \
             link=\"#0563C1\"><div class=WordSection1><p class=MsoNormal>Merhaba,<o:p></o:p></p><p class=MsoNormal>\
             <o:p>&nbsp;</o:p></p><table class=MsoNormalTable border=0 cellspacing=0 cellpadding=0><tr><td width=200 \
             style='padding:0cm'><p class=MsoNormal><b><span style='font-size:9.0pt'>Ad</span></b></p></td></tr>\
             </table></div></body></html>",
            // Gmail
            "<div dir=\"ltr\">Selam<div><br></div><div class=\"gmail_quote\"><div dir=\"ltr\" class=\"gmail_attr\">\
             12 Eki 2026 Pzt yazdı:<br></div><blockquote class=\"gmail_quote\" style=\"margin:0px 0px 0px 0.8ex;\
             border-left:1px solid rgb(204,204,204);padding-left:1ex\">alıntı</blockquote></div></div>",
            // Bülten
            "<center><table width=\"100%\"><tbody><tr><td align=center><!--[if mso]><table><tr><td><![endif]-->\
             <table width=600><tr><td><a href=\"https://ornek.com\"><img src=\"https://ornek.com/l.png\" width=100>\
             </a></td><td><font face=Arial size=2>Kampanya <b>yüzde 20</b></font></td></tr></table>\
             <!--[if mso]></td></tr></table><![endif]--></td></tr></tbody></table></center>",
            "<p>a<table><tr><td>b</td></tr></table>c</p><ol><li><p>x</p><li>y</ol>",
            "a<plaintext>x &amp; <b>y</b> &#65;",
        ] {
            same_clean(html);
        }
        // Uzun metin parçalara bölünür; çıktı değişmez.
        let long = format!(
            "<p>{}</p><div>{}&amp;son</div>",
            "Merhaba&nbsp;dünya &amp; «selam» &#38;x ".repeat(200),
            "a".repeat(3000)
        );
        same_clean(&long);
        assert!(canonicalize(&long).unwrap().matches(COMMENT).count() > 10);
    }

    #[test]
    fn yazdirici_kareselligi_sinirlanir() {
        // html5ever'ın yazdırıcısı uzun metin düğümünde her `&` için düğümün
        // sonuna dek arar.
        let html = format!("<p>{}</p>", "x&amp;".repeat(100_000));
        let t = Instant::now();
        let s = crate::sanitize_html(&html, false, Default::default());
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        assert_eq!(s.html.len(), html.len());
        // Çok `&` içeren öznitelik değeri atılır.
        let c = canonicalize(&format!(
            "<a title=\"{}\" href=x>y</a>",
            "&amp;".repeat(300)
        ))
        .unwrap();
        assert_eq!(c, "<a href=\"x\">y</a>");
        // Ham metin öğesinin (betik/stil dışı) içeriği kısaltılır.
        let c = canonicalize(&format!("<textarea>{}</textarea>", "ç".repeat(1000))).unwrap();
        assert_eq!(c.len(), "<textarea></textarea>".len() + RAW_KEEP);
        let style = format!("<style>{}</style>", "p{}".repeat(1000));
        assert_eq!(canonicalize(&style).unwrap(), style);
    }

    #[test]
    fn yuz_bin_ic_ice_div_hizla_reddedilir() {
        let html = "<div>".repeat(100_000);
        let t = Instant::now();
        assert!(!depth_ok(&html));
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
        assert!(depth_ok(&"<div>".repeat(MAX_DEPTH)));
        assert!(!depth_ok(&"<div>".repeat(MAX_DEPTH + 1)));
    }

    #[test]
    fn derinlik_kacamaklari_yakalanir() {
        let n = 5_000;
        let mut reconstruct = String::new();
        let mut nested_b = String::new();
        for k in 0..n {
            reconstruct.push_str(&format!("<p><b id={k}></p>"));
            nested_b.push_str(&format!("<b id={k}>"));
        }
        for html in [
            // `</x>`, `<p>` üstteyken html5ever'da yok sayılır.
            "<x><p></x></p>".repeat(n),
            // Yorumdaki ya da ham metindeki bitiş etiketi sayılmaz.
            "<div><!--</div>-->".repeat(n),
            "<div><style></div></style>".repeat(n),
            "<span><title></span></title>".repeat(n),
            // Kendinden kapanan yazım html öğelerinde yok sayılır.
            "<div/>".repeat(n),
            // Tablo dışındaki hücre html5ever'da yok sayılır.
            "<td><optgroup></td>".repeat(n),
            reconstruct,
            nested_b,
            // `</div>` biçim öğesinin üstünden kapatmaz.
            "<div><b></div>".repeat(n),
            "<table><tr><td>".repeat(n),
        ] {
            assert!(!depth_ok(&html), "{}", &html[..60.min(html.len())]);
        }
    }

    #[test]
    fn kapanmayan_ama_duzgun_yapilar_sizmaz() {
        let n = 2_000;
        for html in [
            "<ul><li>a<li>b</ul>".repeat(n),
            "<p>a<p>b<div>c</div>".repeat(n),
            "<table><tr><td>a<td>b<tr><td>c</table>".repeat(n),
            "<table><tr><td><p>x</td></tr></table>".repeat(n),
            "<dl><dt>a<dd>b</dl>".repeat(n),
            "<a href=1>x<a href=2>y</a>".repeat(n),
            "<div><span>x</div>".repeat(n),
            "<table><tr><td><b>x</td></tr></table>".repeat(n),
            "<select><option>a<option>b</select>".repeat(n),
        ] {
            assert!(depth_ok(&html), "{}", &html[..60]);
        }
    }

    #[test]
    fn tablodaki_basibos_icerik_hucreye_alinir() {
        let c = canonicalize("<table>x<tr>y<td>z</td>w</tr></table>").unwrap();
        assert_eq!(c, "<table><tr><td>x<tr><td>y<td>z</td><td>w</tr></table>");
        // Boşluk ve yorum tabloda kalır; `</p>` tablo dışına öğe eklerdi.
        let c = canonicalize("<table> <!-- y --></p><tr><td>a</td></tr></table>").unwrap();
        assert_eq!(c, "<table> <!----><!----><tr><td>a</td></tr></table>");
        // Tablo dışında hücre etiketi yok sayılır.
        assert_eq!(canonicalize("<td>a</td>").unwrap(), "<!---->a</td>");
    }

    #[test]
    fn svg_ve_kok_etiketler_atilir() {
        let c = canonicalize("<html><body><svg><style><!--</style><div>--></svg>son</body></html>")
            .unwrap();
        assert_eq!(c, "<!----><!----><!---->son<!----><!---->");
    }

    #[test]
    fn oznitelik_sayisi_sinirli() {
        let mut html = String::from("<a ");
        for k in 0..10_000 {
            html.push_str(&format!("x{k} "));
        }
        html.push_str(">y</a>");
        let c = canonicalize(&html).unwrap();
        assert_eq!(c.matches(" x").count(), MAX_ATTRS);
    }

    #[test]
    fn duz_metin_yedegi() {
        let t = strip_tags(
            "<style>p{}</style><p>Merhaba&nbsp;d&uuml;nya &amp; &#x41;&#66;</p><div>İkinci<br>satır</div><svg><text>x</text></svg>son &bilinmez;",
        );
        assert_eq!(
            crate::text::tidy_lines(&t),
            "Merhaba dünya & AB\n\nİkinci\nsatır\nson &bilinmez;"
        );
        assert_eq!(
            text_to_html("a <b> & \"c\"\nd\n\ne"),
            "<p>a &lt;b&gt; &amp; &quot;c&quot;<br>d</p>\n<p>e</p>\n"
        );
    }
}
