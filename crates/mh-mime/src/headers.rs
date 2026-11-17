//! Ham başlık okuma çekirdeği: gönderen güveni (`trust`) ve istenmeyen puanı
//! (`spam`) başlıkları aynı yoldan okur.
//!
//! Başlıklar `(ad, ham değer)` çiftleridir, postadaki sırayla (en üstteki
//! önce): ham postadan [`with_raw`] ile, yalnız seçili başlıklarını getiren
//! kaynaklardan (JMAP) doğrudan verilir. Ad büyük/küçük harf duyarsız
//! karşılaştırılır; değerin katlaması (satır sonu + boşluk) tek boşluğa iner.

use mail_parser::MessageParser;

/// Başlıkta incelenen en uzun değer (bayt).
const MAX_VALUE: usize = 4096;

/// Ham değerin incelenen biçimi: en çok [`MAX_VALUE`] bayt (karakter
/// sınırında kesilir), katlama ve ardışık boşluklar tek boşluk.
pub(crate) fn unfold(value: &str) -> String {
    let mut end = value.len().min(MAX_VALUE);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// En üstteki `name` başlığının değeri (katlaması açılmış); yoksa `None`.
/// Alıcı sunucu kendi başlığını en üste ekler; aşağıdakiler gönderenin
/// yazdığı (sahte olabilecek) başlıklardır.
pub(crate) fn first(headers: &[(&str, &str)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(n, _)| n.trim().eq_ignore_ascii_case(name))
        .map(|(_, v)| unfold(v))
}

/// Ham postanın (yalnız başlığı indirilmiş posta da olur) üst düzey
/// başlıklarıyla `f`i çağırır; ayrıştırılamazsa `None`.
pub(crate) fn with_raw<T>(raw: &[u8], f: impl FnOnce(&[(&str, &str)]) -> T) -> Option<T> {
    let msg = MessageParser::default().parse_headers(raw)?;
    let headers: Vec<(&str, &str)> = msg.headers_raw().collect();
    Some(f(&headers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_ustteki_baslik_ve_katlama() {
        let raw = b"X-Spam-Flag: YES\r\nx-spam-flag: NO\r\nAuthentication-Results: mx.ornek.com;\r\n\tdkim=fail;\r\n  spf=fail\r\nSubject: x\r\n\r\ngovde\r\nX-Spam-Level: *****\r\n";
        let got = with_raw(raw, |h| {
            (
                first(h, "x-spam-flag"),
                first(h, "AUTHENTICATION-RESULTS"),
                first(h, "X-Spam-Level"),
            )
        })
        .unwrap();
        assert_eq!(got.0.as_deref(), Some("YES"), "en üstteki, harf duyarsız");
        assert_eq!(
            got.1.as_deref(),
            Some("mx.ornek.com; dkim=fail; spf=fail"),
            "katlama tek boşluk"
        );
        assert_eq!(got.2, None, "gövdedeki satır başlık değildir");
    }

    #[test]
    fn uzun_deger_karakter_sinirinda_kesilir() {
        // 4095 ASCII + iki baytlık "ş": kesme karakterin ortasına düşmez.
        let value = format!("{}ş son", "a".repeat(MAX_VALUE - 1));
        let out = unfold(&value);
        assert_eq!(out.len(), MAX_VALUE - 1);
        assert!(unfold(&"b ".repeat(5000)).len() <= MAX_VALUE);
        assert_eq!(unfold("  a \r\n\t b  "), "a b");
    }
}
