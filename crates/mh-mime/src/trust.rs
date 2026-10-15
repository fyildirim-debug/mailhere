//! Gönderen güveni: görünen adda başka adres, benzetilmiş alan adı ve
//! alıcı sunucunun kimlik doğrulama sonucu (`Authentication-Results`).

use mail_parser::MessageParser;
use mh_core::{Address, SenderWarning};

use crate::privacy::same_site;

/// Başlıkta incelenen en uzun `Authentication-Results` değeri (bayt).
const MAX_AUTH_VALUE: usize = 4096;

/// Gönderen için uyarı; birden çok işaret varsa en güçlüsü: kimlik
/// doğrulama başarısızlığı, görünen adda başka adres, benzetilmiş alan adı.
///
/// `raw` verilmişse en üstteki `Authentication-Results` başlığı (alıcı
/// sunucunun eklediği) okunur; yalnız başlık indirilmiş posta da yeter.
pub fn sender_warning(raw: Option<&[u8]>, from: &Address) -> Option<SenderWarning> {
    let auth = raw.and_then(first_auth_results);
    if auth.as_deref().is_some_and(auth_failed) {
        return Some(SenderWarning::AuthFailed);
    }
    if name_has_other_address(from) {
        return Some(SenderWarning::NameHasOtherAddress);
    }
    let domain = from.email.rsplit_once('@').map(|(_, d)| d)?;
    lookalike_domain(domain).then_some(SenderWarning::LookalikeDomain)
}

/// En üstteki `Authentication-Results` değeri (katlama boşlukları tek boşluk).
fn first_auth_results(raw: &[u8]) -> Option<String> {
    let msg = MessageParser::default().parse_headers(raw)?;
    let value = msg
        .headers_raw()
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("Authentication-Results"))
        .map(|(_, v)| v)?;
    let value = &value[..value.len().min(MAX_AUTH_VALUE)];
    Some(value.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Parantez içindeki yorumları atar (`dkim=pass (imza geçti; dmarc=fail değil)`).
fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// `Authentication-Results` değerinde DMARC başarısız mı, ya da DKIM ve SPF
/// birlikte mi başarısız? (RFC 8601: `yöntem=sonuç` çiftleri `;` ile ayrılır.)
/// Birden çok DKIM imzasında biri geçtiyse DKIM başarısız sayılmaz.
pub fn auth_failed(value: &str) -> bool {
    let value = strip_comments(value).to_ascii_lowercase();
    let mut dmarc_fail = false;
    let (mut dkim_fail, mut dkim_pass, mut spf_fail) = (false, false, false);
    for part in value.split(';') {
        let Some((method, rest)) = part.trim().split_once('=') else {
            continue;
        };
        let result = rest.split_whitespace().next().unwrap_or("");
        match (method.trim(), result) {
            ("dmarc", "fail") => dmarc_fail = true,
            ("dkim", "fail") => dkim_fail = true,
            ("dkim", "pass") => dkim_pass = true,
            ("spf", "fail") => spf_fail = true,
            _ => {}
        }
    }
    dmarc_fail || (dkim_fail && !dkim_pass && spf_fail)
}

/// Görünen adın içinde, gönderen adresinden başka bir siteye ait e-posta
/// adresi var mı? ("Banka Destek destek@banka.com" <x@kotu.example>)
pub fn name_has_other_address(from: &Address) -> bool {
    let Some(name) = from.name.as_deref() else {
        return false;
    };
    let own = from.email.trim().to_ascii_lowercase();
    let own_domain = own.rsplit_once('@').map_or("", |(_, d)| d);
    addresses_in(name).iter().any(|a| {
        let a = a.to_ascii_lowercase();
        let domain = a.rsplit_once('@').map_or("", |(_, d)| d);
        a != own && !same_site(domain, own_domain)
    })
}

/// Metindeki e-posta adresi biçimli parçalar.
fn addresses_in(text: &str) -> Vec<String> {
    let is_part = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let mut out = Vec::new();
    for token in text.split(|c: char| !(is_part(c) || c == '@')) {
        let Some((local, domain)) = token.split_once('@') else {
            continue;
        };
        let domain = domain.trim_end_matches('.');
        let local = local.trim_start_matches('.');
        let tld = domain.rsplit_once('.').map_or("", |(_, t)| t);
        if !local.is_empty()
            && !domain.contains('@')
            && tld.chars().count() >= 2
            && tld.chars().all(char::is_alphabetic)
        {
            out.push(format!("{local}@{domain}"));
        }
    }
    out
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Script {
    Latin,
    Cyrillic,
    Greek,
}

fn script_of(c: char) -> Option<Script> {
    match c as u32 {
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x1E00..=0x1EFF => Some(Script::Latin),
        0x370..=0x3FF | 0x1F00..=0x1FFF => Some(Script::Greek),
        0x400..=0x52F => Some(Script::Cyrillic),
        _ => None,
    }
}

/// Alan adı punycode (`xn--`) etiketi taşıyor ya da bir etikette Latin ile
/// Kiril ya da Yunan harfleri karışık mı?
pub fn lookalike_domain(domain: &str) -> bool {
    domain.trim_end_matches('.').split('.').any(|label| {
        if label.len() >= 4 && label[..4].eq_ignore_ascii_case("xn--") {
            return true;
        }
        let mut seen: Option<Script> = None;
        for s in label.chars().filter_map(script_of) {
            match seen {
                Some(prev) if prev != s => return true,
                _ => seen = Some(s),
            }
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: Option<&str>, email: &str) -> Address {
        Address::new(name, email)
    }

    #[test]
    fn gorunen_adda_baska_adres() {
        assert!(name_has_other_address(&addr(
            Some("Ödeme Merkezi destek@odememerkezi.com.tr"),
            "guvenlik@odeme-destek.example"
        )));
        assert!(name_has_other_address(&addr(
            Some("service@paypal.example"),
            "x@kotu.example"
        )));
        // Aynı adres ya da aynı site uyarı değildir.
        assert!(!name_has_other_address(&addr(
            Some("Kerem kerem@ornek.com.tr"),
            "kerem@ornek.com.tr"
        )));
        assert!(!name_has_other_address(&addr(
            Some("Destek destek@banka.com.tr"),
            "bildirim@posta.banka.com.tr"
        )));
        assert!(!name_has_other_address(&addr(
            Some("Kerem Aydın"),
            "kerem@ornek.com.tr"
        )));
        assert!(!name_has_other_address(&addr(
            Some("Kampanya @ yaz"),
            "a@b.com"
        )));
        assert!(!name_has_other_address(&addr(None, "a@b.com")));
    }

    #[test]
    fn benzetilmis_alan_adi() {
        assert!(lookalike_domain("xn--pypal-4ve.com"));
        assert!(lookalike_domain("posta.XN--80ak6aa92e.com"));
        // Latin "p", "y", "l" + Kiril "а".
        assert!(lookalike_domain("pаypal.com"));
        // Latin + Yunan omikron.
        assert!(lookalike_domain("gοogle.com"));
        assert!(!lookalike_domain("ornek.com.tr"));
        assert!(!lookalike_domain("güzel.com"));
        assert!(!lookalike_domain("пример.рф"));
        assert!(!lookalike_domain("my-xn.com"));
    }

    #[test]
    fn kimlik_dogrulama_sonucu() {
        assert!(auth_failed(
            "mx.ornek.com; dkim=pass header.d=kotu.example; spf=pass; dmarc=fail (p=REJECT) header.from=banka.com"
        ));
        assert!(auth_failed(
            "mx.ornek.com; dkim=fail; spf=fail smtp.mailfrom=x"
        ));
        assert!(!auth_failed(
            "mx.ornek.com; dkim=fail; spf=pass; dmarc=pass"
        ));
        assert!(!auth_failed("mx.ornek.com; dkim=fail; dkim=pass; spf=fail"));
        assert!(!auth_failed("mx.ornek.com; spf=softfail; dkim=none"));
        assert!(!auth_failed(
            "mx.ornek.com; dkim=pass (dmarc=fail değil); dmarc=pass"
        ));
        assert!(!auth_failed("mx.ornek.com; none"));
    }

    #[test]
    fn ham_postadan_uyari() {
        let from = addr(Some("Banka"), "bildirim@banka.com");
        let raw = b"Authentication-Results: mx.ornek.com;\r\n dkim=fail header.d=banka.com;\r\n spf=fail smtp.mailfrom=banka.com\r\nAuthentication-Results: sahte; dmarc=pass\r\nFrom: Banka <bildirim@banka.com>\r\nSubject: x\r\n\r\nmetin";
        assert_eq!(
            sender_warning(Some(raw), &from),
            Some(SenderWarning::AuthFailed)
        );
        // Yalnız en üstteki (alıcı sunucunun) başlık sayılır.
        let raw = b"Authentication-Results: mx.ornek.com; dmarc=pass\r\nAuthentication-Results: sahte; dmarc=fail\r\nFrom: a@b.com\r\n\r\nx";
        assert_eq!(sender_warning(Some(raw), &from), None);
        assert_eq!(
            sender_warning(
                None,
                &addr(Some("Banka destek@banka.com"), "a@kotu.example")
            ),
            Some(SenderWarning::NameHasOtherAddress)
        );
        assert_eq!(
            sender_warning(None, &addr(None, "a@xn--bnka-xxa.com")),
            Some(SenderWarning::LookalikeDomain)
        );
        assert_eq!(
            sender_warning(None, &addr(Some("Ayşe"), "ayse@eposta.com")),
            None
        );
    }
}
