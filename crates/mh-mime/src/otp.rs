//! Doğrulama kodu (tek kullanımlık şifre) algılama.
//!
//! Konu ve düz metinde bir anahtar sözcüğün ("doğrulama kodu", "verification
//! code", "OTP" …) yakınındaki 4–8 haneli sayı ya da 6–8 karakterli büyük harf
//! ve rakam karışımı aranır. Tarih, saat, telefon, tutar, yıl, adres içindeki
//! sayılar ve "posta kodu", "takip kodu", "indirim kodu" gibi bağlamlar elenir.
//! Birden çok aday varsa anahtar sözcüğe en yakın olan seçilir.

/// İncelenen en uzun metin (karakter): kodlar postanın başında olur.
const SCAN_LIMIT: usize = 6000;
/// Anahtar sözcükten önce ve sonra kabul edilen en uzak aday (karakter).
const BEFORE: usize = 160;
const AFTER: usize = 60;
/// Harf ve rakam karışık kodlar için daha sıkı uzaklık.
const MIXED_RANGE: usize = 50;

/// Önek olarak eşleşen anahtar sözcükler (küçük harf): "kod" → "kodunuz".
const STEMS: &[&str] = &[
    "kod",
    "doğrula",
    "dogrula",
    "şifre",
    "sifre",
    "parola",
    "code",
    "verif",
    "passcode",
    "authenticat",
];
/// Tam eşleşen anahtar sözcükler.
const WORDS: &[&str] = &["otp", "pin", "2fa", "mfa", "tan"];
/// İki sözcüklü anahtarlar.
const PHRASES: &[(&str, &str)] = &[
    ("one", "time"),
    ("tek", "kullanımlık"),
    ("tek", "kullanimlik"),
    ("tek", "seferlik"),
    ("two", "factor"),
    ("iki", "adımlı"),
    ("iki", "faktörlü"),
];
/// Anahtar sözcüğü ya da adayı geçersiz kılan yakın bağlam (önek eşleşir).
const NEGATIVE: &[&str] = &[
    "posta",
    "zip",
    "postal",
    "takip",
    "track",
    "sipariş",
    "siparis",
    "order",
    "indirim",
    "discount",
    "kupon",
    "coupon",
    "promo",
    "hediye",
    "gift",
    "referans",
    "reference",
    "ref",
    "referral",
    "davet",
    "invite",
    "rezervasyon",
    "reservation",
    "booking",
    "pnr",
    "kaynak",
    "source",
    "ürün",
    "product",
    "vergi",
    "tax",
    "iban",
    "swift",
    "ülke",
    "country",
    "alan",
    "area",
    "fatura",
    "invoice",
    "müşteri",
    "customer",
    "kargo",
    "shipment",
    "voucher",
    "barkod",
    "barcode",
    "qr",
    "kart",
    "card",
    "ending",
    "hane",
];
/// Adaydan hemen sonra gelince onu tutar ya da ölçü yapan sözcükler.
const UNITS: &[&str] = &[
    "tl", "try", "usd", "eur", "gbp", "lira", "dolar", "euro", "gb", "mb", "kb", "km", "kg",
    "adet", "puan", "points", "kişi", "people",
];

#[derive(Clone, Copy)]
struct Word {
    start: usize,
    end: usize,
}

/// Konu ve metinden doğrulama kodu; bulunamazsa `None`. Kod boşluksuz döner
/// ("482 913" → "482913").
pub fn find_otp(subject: &str, text: &str) -> Option<String> {
    let mut all: Vec<char> = subject.chars().take(500).collect();
    all.push('\n');
    all.extend(text.chars().take(SCAN_LIMIT));
    let lower: Vec<char> = all
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let words = words(&lower);
    let urls = url_spans(&lower);
    let keywords: Vec<Word> = keyword_hits(&lower, &words)
        .into_iter()
        .filter(|k| !negated_before(&lower, &words, k.start, 2))
        .collect();
    if keywords.is_empty() {
        return None;
    }
    let mut best: Option<(usize, usize, String)> = None;
    for cand in candidates(&all) {
        if urls.iter().any(|&(s, e)| cand.start >= s && cand.start < e) {
            continue;
        }
        if negated_before(&lower, &words, cand.start, 3) {
            continue;
        }
        let range_before = if cand.mixed { MIXED_RANGE } else { BEFORE };
        let range_after = if cand.mixed { MIXED_RANGE } else { AFTER };
        let dist = keywords
            .iter()
            .filter_map(|k| {
                if k.end <= cand.start {
                    let d = cand.start - k.end;
                    (d <= range_before).then_some(d)
                } else if k.start >= cand.end {
                    let d = k.start - cand.end;
                    (d <= range_after).then_some(d)
                } else {
                    None
                }
            })
            .min();
        let Some(dist) = dist else {
            continue;
        };
        // Eşit uzaklıkta rakamlı kod, sonra önce gelen seçilir.
        let rank = dist * 2 + usize::from(cand.mixed);
        if best
            .as_ref()
            .is_none_or(|(r, s, _)| rank < *r || (rank == *r && cand.start < *s))
        {
            best = Some((rank, cand.start, cand.code));
        }
    }
    best.map(|(_, _, code)| code)
}

/// Harf ya da rakam dizileri (küçük harfli metinde).
fn words(lower: &[char]) -> Vec<Word> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lower.len() {
        if lower[i].is_alphanumeric() {
            let start = i;
            while i < lower.len() && lower[i].is_alphanumeric() {
                i += 1;
            }
            out.push(Word { start, end: i });
        } else {
            i += 1;
        }
    }
    out
}

fn text_of(lower: &[char], w: Word) -> String {
    lower[w.start..w.end].iter().collect()
}

fn keyword_hits(lower: &[char], words: &[Word]) -> Vec<Word> {
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let t = text_of(lower, *w);
        if STEMS.iter().any(|s| t.starts_with(s)) || WORDS.contains(&t.as_str()) {
            out.push(*w);
            continue;
        }
        if let Some(next) = words.get(i + 1) {
            let n = text_of(lower, *next);
            // İki sözcük arasında yalnız boşluk ya da tire.
            let gap_ok = lower[w.end..next.start]
                .iter()
                .all(|c| c.is_whitespace() || *c == '-');
            if gap_ok && PHRASES.iter().any(|(a, b)| t == *a && n.starts_with(b)) {
                out.push(Word {
                    start: w.start,
                    end: next.end,
                });
            }
        }
    }
    out
}

/// `at` konumundan önceki `n` sözcükten biri (aynı satırda) olumsuz bağlam mı?
fn negated_before(lower: &[char], words: &[Word], at: usize, n: usize) -> bool {
    let line_start = lower[..at]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |p| p + 1);
    words
        .iter()
        .rev()
        .filter(|w| w.end <= at && w.start >= line_start)
        .take(n)
        .any(|w| {
            let t = text_of(lower, *w);
            NEGATIVE.iter().any(|neg| t.starts_with(neg))
        })
}

/// `http(s)://` ve `www.` ile başlayan adreslerin kapladığı aralıklar.
fn url_spans(lower: &[char]) -> Vec<(usize, usize)> {
    let s: String = lower.iter().collect();
    let mut out = Vec::new();
    // Karakter konumu için bayt → karakter eşlemesi.
    let idx: Vec<usize> = s.char_indices().map(|(b, _)| b).collect();
    let char_at = |b: usize| idx.partition_point(|x| *x < b);
    for pat in ["http://", "https://", "www."] {
        let mut from = 0;
        while let Some(p) = s[from..].find(pat) {
            let start = from + p;
            let end = s[start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | ')'))
                .map_or(s.len(), |e| start + e);
            out.push((char_at(start), char_at(end)));
            from = end.max(start + pat.len());
        }
    }
    out
}

struct Candidate {
    start: usize,
    end: usize,
    code: String,
    mixed: bool,
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

fn run_len(chars: &[char], from: usize, f: impl Fn(char) -> bool) -> usize {
    chars[from..].iter().take_while(|c| f(**c)).count()
}

/// Sayıyı bir sayı dizisinin (tarih, saat, tutar, telefon) parçası yapan
/// ayraçlar.
fn number_joiner(c: char) -> bool {
    matches!(c, '.' | ',' | '/' | ':')
}

fn digit_candidate(chars: &[char], start: usize) -> Option<Candidate> {
    let prev = start.checked_sub(1).map(|i| chars[i]);
    let prev2 = start.checked_sub(2).map(|i| chars[i]);
    if prev.is_some_and(|c| {
        c.is_alphanumeric()
            || matches!(
                c,
                '+' | '#' | '$' | '€' | '£' | '₺' | '¥' | '@' | '_' | '=' | '&' | '?' | '%'
            )
    }) {
        return None;
    }
    // "12.10", "1,5", "14:30", "2026/10": öncesi rakam + ayraç.
    if prev.is_some_and(|c| number_joiner(c) || c == '-' || c == ' ') && prev2.is_some_and(is_digit)
    {
        return None;
    }
    let first = run_len(chars, start, is_digit);
    let mut end = start + first;
    let mut code: String = chars[start..end].iter().collect();
    // "482 913" ya da "482-913": iki öbek, toplam 6–8 hane.
    let grouped = (3..=4).contains(&first)
        && chars
            .get(end)
            .is_some_and(|c| matches!(c, ' ' | '-' | '\u{a0}'));
    if grouped {
        let second = run_len(chars, end + 1, is_digit);
        let next = chars.get(end + 1 + second).copied();
        if (3..=4).contains(&second) && !next.is_some_and(|c| c.is_alphanumeric()) {
            code.extend(&chars[end + 1..end + 1 + second]);
            end += 1 + second;
        }
    }
    let n = code.len();
    if !(4..=8).contains(&n) {
        return None;
    }
    let next = chars.get(end).copied();
    let next2 = chars.get(end + 1).copied();
    if next.is_some_and(|c| {
        c.is_alphanumeric() || matches!(c, '@' | '_' | '%' | '/' | '€' | '$' | '₺')
    }) {
        return None;
    }
    if next.is_some_and(|c| number_joiner(c) || c == '-') && next2.is_some_and(is_digit) {
        return None;
    }
    // Ardından boşlukla başka bir rakam öbeği: telefon ya da gönderi numarası.
    if next.is_some_and(|c| c == ' ' || c == '\u{a0}') && next2.is_some_and(is_digit) {
        return None;
    }
    // Ardından birim ya da para birimi ("250000 TL", "1024 MB").
    let after: String = chars[end..]
        .iter()
        .skip_while(|c| **c == ' ' || **c == '\u{a0}')
        .take_while(|c| c.is_alphabetic() || matches!(c, '₺' | '$' | '€'))
        .collect::<String>()
        .to_lowercase();
    if UNITS.contains(&after.as_str()) || after.starts_with(['₺', '$', '€']) {
        return None;
    }
    // Tek öbek dört hane yıl gibi duruyorsa (1900–2099) atlanır.
    if n == 4 && first == 4 && matches!(code.parse::<u32>(), Ok(1900..=2099)) {
        return None;
    }
    Some(Candidate {
        start,
        end,
        code,
        mixed: false,
    })
}

fn mixed_candidate(chars: &[char], start: usize) -> Option<Candidate> {
    if start > 0 && chars[start - 1].is_alphanumeric() {
        return None;
    }
    let len = run_len(chars, start, |c| {
        c.is_ascii_uppercase() || c.is_ascii_digit()
    });
    let end = start + len;
    if !(6..=8).contains(&len) || chars.get(end).is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    let code: String = chars[start..end].iter().collect();
    let digits = code.bytes().filter(u8::is_ascii_digit).count();
    let letters = len - digits;
    (digits >= 2 && letters >= 2).then_some(Candidate {
        start,
        end,
        code,
        mixed: true,
    })
}

fn candidates(chars: &[char]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at_word_start = i == 0 || !chars[i - 1].is_alphanumeric();
        if at_word_start && (c.is_ascii_digit() || c.is_ascii_uppercase()) {
            let found = mixed_candidate(chars, i).or_else(|| {
                // Rakamla başlayan ama harf de içeren sözcük sayı değildir.
                (c.is_ascii_digit())
                    .then(|| digit_candidate(chars, i))
                    .flatten()
            });
            if let Some(cand) = found {
                i = cand.end;
                out.push(cand);
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn otp(subject: &str, text: &str) -> Option<String> {
        find_otp(subject, text)
    }

    #[test]
    fn bulunanlar() {
        let cases: &[(&str, &str, &str)] = &[
            ("", "Doğrulama kodunuz: 482913. Bu kodu kimseyle paylaşmayın.", "482913"),
            ("", "Your verification code is 739201", "739201"),
            ("G-482913 is your verification code", "", "482913"),
            ("Giriş", "Tek kullanımlık şifreniz 5821'dir.", "5821"),
            ("", "Your code: 482 913", "482913"),
            ("", "Security code: 123-456", "123456"),
            ("", "Your one-time code is K7P2QX", "K7P2QX"),
            ("", "GİRİŞ KODU: 12345678", "12345678"),
            (
                "Oturum açma",
                "Oturum açma kodunuz\n\n   604 218   \n\nBu kod 10 dakika geçerlidir.",
                "604218",
            ),
            ("", "Your PIN is 3391", "3391"),
            ("", "OTP: 778899", "778899"),
            ("", "Enter 550123 to verify your email address.", "550123"),
            (
                "",
                "Siparişiniz 12.10.2026 tarihinde alındı. Onay kodunuz: 902341",
                "902341",
            ),
            (
                "",
                "Your security code is 246810. It expires in 15 minutes. Ref: 99887766",
                "246810",
            ),
            ("Kurtarma e-postası", "Kurtarma adresini dogrulamak icin kodu gir:\n482913", "482913"),
            ("Your login code", "<b>Hi Furkan</b>\n\n119 284\n\nDidn't request this?", "119284"),
            ("", "Use passcode 0042 1977 to sign in.", "00421977"),
            ("2FA", "Two-factor code\n\n731 004", "731004"),
            (
                "",
                "Değerli müşterimiz, giriş için tek kullanımlık şifreniz: 739104. Kimseyle paylaşmayınız.",
                "739104",
            ),
        ];
        for (subject, text, want) in cases {
            assert_eq!(
                otp(subject, text).as_deref(),
                Some(*want),
                "{subject} | {text}"
            );
        }
    }

    #[test]
    fn bulunmayanlar() {
        let cases: &[(&str, &str)] = &[
            (
                "Siparişiniz onaylandı",
                "Sipariş numarası 12345678. Teşekkürler.",
            ),
            ("", "Doğrulama 12.10.2026 tarihinde yapıldı."),
            ("", "Kod incelemesi saat 14:30'da başlıyor."),
            (
                "",
                "Doğrulama için bizi 0212 555 12 34 numarasından arayın.",
            ),
            ("", "Doğrulama için +90 532 123 45 67 numarasını arayın."),
            ("", "Security code updated; your balance is $123456."),
            ("", "Kodunuzla 250000 TL harcama yaptınız."),
            ("Verification", "Copyright 2026 Örnek A.Ş."),
            ("", "Posta kodu: 34710 Kadıköy"),
            ("", "Kargo takip kodu: 723998"),
            ("", "İndirim kodu: YAZ2026 ile yüzde 20 indirim."),
            (
                "Verify your email",
                "Click https://ornek.com/v/482913 to verify.",
            ),
            ("", "Toplantı 482913 numaralı odada."),
            ("", "Your code is ready: pizza1"),
            ("", "Sürüm 1.2.3 kodu yayında."),
            ("", "Hesap kodu 12345678901234"),
            ("", "Rezervasyon kodunuz: X7K2PQ"),
            ("", "Use promo code SAVE1020 for 10% off."),
            ("", "Kaynak kodu 2048 satır."),
            (
                "Sözleşme",
                "Fesih bildirimi 30 güne indi; hizmet süresi 24 ay.",
            ),
            (
                "",
                "Kargo 4021 8833 17 numaralı gönderiniz dağıtımda; kod gerekmez.",
            ),
            (
                "",
                "Order #482913 shipped. Use your account code to log in.",
            ),
            ("", "Kartınızın son dört hanesi 1234 ile doğrulama yapıldı."),
            (
                "",
                "Giriş: 15.10.2026 14:32, IP 85.105.12.34. Siz değilseniz şifrenizi değiştirin.",
            ),
            ("", "Verify order 112-3456789-1234567 within 30 days."),
        ];
        for (subject, text) in cases {
            assert_eq!(otp(subject, text), None, "{subject} | {text}");
        }
    }

    #[test]
    fn uzun_metin_hizli() {
        let text = format!("{}doğrulama kodu 123456", "kod 12 ab ".repeat(100_000));
        let t = std::time::Instant::now();
        let _ = otp("", &text);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            t.elapsed()
        );
    }
}
