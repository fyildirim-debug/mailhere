//! İstenmeyen puanı: postanın başlıklarından, göndereninden, yanıt
//! adresinden ve ek adlarından 0–100 arası bir puan ve nedenleri çıkarır.
//!
//! Saf işlevdir (ağ, depo, ayar bilmez). Gönderen güveni işaretleri
//! (`crate::trust`) ve başlık okuma (`crate::headers`) aynı çekirdekten
//! gelir. Listeler (başlık adları, uzantılar) bu depoda yazılmıştır.
//!
//! Ağırlıklar toplanır ve 100'e kırpılır; [`mh_core::SPAM_THRESHOLD`] ve
//! üstü "şüpheli"dir. Tek başına eşiği geçenler: alıcı sunucunun işareti ve
//! kimlik doğrulama başarısızlığı. Ötekiler ancak birlikte geçer (ör.
//! benzetilmiş alan adı + başka alan adına yanıt adresi).

use mh_core::{Address, SpamReason, SpamVerdict};

use crate::headers;
use crate::privacy::same_site;
use crate::trust::{auth_failed, lookalike_sender, name_has_other_address, AUTH_RESULTS};

/// Alıcı sunucu postayı istenmeyen diye işaretlemiş.
pub const WEIGHT_SERVER_FLAGGED: u8 = 60;
/// DMARC başarısız ya da DKIM ve SPF birlikte başarısız.
pub const WEIGHT_AUTH_FAILED: u8 = 50;
/// Punycode ya da karışık alfabe alan adı.
pub const WEIGHT_LOOKALIKE_DOMAIN: u8 = 40;
/// Görünen adda başka alan adına ait adres.
pub const WEIGHT_NAME_HAS_OTHER_ADDRESS: u8 = 40;
/// Çalıştırılabilir ya da çift uzantılı ek.
pub const WEIGHT_RISKY_ATTACHMENT: u8 = 40;
/// Yalnız HTML eki (çalıştırılabilir değil ama oltalama sayfası olabilir).
pub const WEIGHT_HTML_ATTACHMENT: u8 = 20;
/// Yanıt adresi gönderenin alan adı dışında.
pub const WEIGHT_REPLY_TO_MISMATCH: u8 = 15;

/// Puanın baktığı başlıklar. Postanın tamamını değil yalnız seçili
/// başlıklarını getiren kaynaklar (JMAP) bunları ister.
pub const HEADERS: &[&str] = &[
    AUTH_RESULTS,
    "X-Spam-Flag",
    "X-Spam-Status",
    "X-Spam",
    "X-Spam-Level",
    "X-Rspamd-Action",
    "X-Forefront-Antispam-Report",
];

/// `X-Spam-Level` bu kadar yıldız ve üstüyse posta işaretli sayılır.
const SPAM_LEVEL_STARS: usize = 5;

/// Çift tıklanınca çalışan ya da kurulum başlatan ek uzantıları.
const RISKY_EXTENSIONS: &[&str] = &[
    "exe", "scr", "com", "pif", "bat", "cmd", "js", "jse", "vbs", "vbe", "wsf", "wsh", "hta",
    "lnk", "msi", "jar", "ps1", "iso", "img", "apk",
];

/// Tarayıcıda açılan sayfa ekleri.
const HTML_EXTENSIONS: &[&str] = &["html", "htm", "xhtml", "shtml"];

/// Zararsız görünen belge ve ortam uzantıları: bunlardan birinin ardına
/// başka bir uzantı eklenmişse (`fatura.pdf.exe`, `dekont.pdf.html`) ad,
/// gerçek türünü gizlemeye çalışıyordur.
const DISGUISE_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "txt", "csv",
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "mp3", "mp4", "avi", "mov", "wav",
];

/// Puanın girdisi.
#[derive(Debug, Clone, Copy)]
pub struct SpamInput<'a> {
    /// Üst düzey başlıklar `(ad, ham değer)`, postadaki sırayla. Aynı adlı
    /// başlıklardan en üstteki (alıcı sunucunun eklediği) okunur.
    pub headers: &'a [(&'a str, &'a str)],
    pub from: &'a Address,
    pub reply_to: &'a [Address],
    /// Ek adları (yalnız başlık indirildiyse boş).
    pub attachments: &'a [&'a str],
    /// Gönderen engellenenler listesinde (`mh_core::is_blocked`).
    pub blocked: bool,
}

/// Değer "yes" sözcüğüyle mi başlıyor ("Yes, score=7.1", "YES")?
fn starts_with_yes(value: &str) -> bool {
    let v = value.trim_start().to_ascii_lowercase();
    v.strip_prefix("yes")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric()))
}

/// `X-Forefront-Antispam-Report` değerindeki `SCL` (istenmeyen güven
/// düzeyi) 5–9 arasında mı? Alanlar `AD:DEĞER;` biçimindedir.
fn forefront_spam(value: &str) -> bool {
    value.split(';').any(|field| {
        let Some((key, v)) = field.split_once(':') else {
            return false;
        };
        key.trim().eq_ignore_ascii_case("scl")
            && v.trim().parse::<u8>().is_ok_and(|n| (5..=9).contains(&n))
    })
}

/// Alıcı sunucu postayı istenmeyen diye işaretlemiş mi?
pub fn server_flagged(headers: &[(&str, &str)]) -> bool {
    let get = |name: &str| headers::first(headers, name);
    if ["X-Spam-Flag", "X-Spam-Status", "X-Spam"]
        .iter()
        .any(|name| get(name).is_some_and(|v| starts_with_yes(&v)))
    {
        return true;
    }
    if get("X-Spam-Level")
        .is_some_and(|v| v.chars().filter(|c| *c == '*').count() >= SPAM_LEVEL_STARS)
    {
        return true;
    }
    if get("X-Rspamd-Action").is_some_and(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "add header" | "rewrite subject" | "reject"
        )
    }) {
        return true;
    }
    get("X-Forefront-Antispam-Report").is_some_and(|v| forefront_spam(&v))
}

/// Yanıt adreslerinden biri gönderenin sitesi dışında mı?
pub fn reply_to_mismatch(from: &Address, reply_to: &[Address]) -> bool {
    let Some((_, own)) = from.email.trim().rsplit_once('@') else {
        return false;
    };
    reply_to.iter().any(|a| {
        a.email
            .trim()
            .rsplit_once('@')
            .is_some_and(|(_, domain)| !domain.is_empty() && !same_site(domain, own))
    })
}

/// Ek adının ağırlığı: çalıştırılabilir uzantı ya da belge uzantısının
/// ardına eklenmiş ikinci uzantı [`WEIGHT_RISKY_ATTACHMENT`], yalnız HTML
/// [`WEIGHT_HTML_ATTACHMENT`], ötekiler 0. Sondaki nokta ve boşluklar
/// (Windows bunları sessizce atar) ile parçaların çevresindeki boşluklar
/// yok sayılır.
pub fn attachment_weight(name: &str) -> u8 {
    let lower = name.to_lowercase();
    let trimmed = lower.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    let mut parts = trimmed.rsplit('.').map(str::trim);
    let (Some(ext), Some(before)) = (parts.next(), parts.next()) else {
        // Noktasız ad: uzantı yok.
        return 0;
    };
    let risky = RISKY_EXTENSIONS.contains(&ext);
    let html = HTML_EXTENSIONS.contains(&ext);
    // `before` adın gövdesi de olabilir ("pdf.exe"); o da gizleme sayılır.
    let disguised = DISGUISE_EXTENSIONS.contains(&before) && parts.next().is_some();
    if risky || (html && disguised) {
        WEIGHT_RISKY_ATTACHMENT
    } else if html {
        WEIGHT_HTML_ATTACHMENT
    } else {
        0
    }
}

/// Postanın istenmeyen puanı ve nedenleri. Nedenler puan eşiğin altında
/// kalsa da listelenir (`SpamReason::ALL` sırasıyla); "şüpheli" kararını
/// [`SpamVerdict::is_spam`] verir. Engellenen gönderende puan 100'dür.
pub fn score(input: &SpamInput<'_>) -> SpamVerdict {
    let mut hits: Vec<(SpamReason, u8)> = Vec::new();
    if server_flagged(input.headers) {
        hits.push((SpamReason::ServerFlagged, WEIGHT_SERVER_FLAGGED));
    }
    if headers::first(input.headers, AUTH_RESULTS).is_some_and(|v| auth_failed(&v)) {
        hits.push((SpamReason::AuthFailed, WEIGHT_AUTH_FAILED));
    }
    if name_has_other_address(input.from) {
        hits.push((
            SpamReason::NameHasOtherAddress,
            WEIGHT_NAME_HAS_OTHER_ADDRESS,
        ));
    }
    if lookalike_sender(input.from) {
        hits.push((SpamReason::LookalikeDomain, WEIGHT_LOOKALIKE_DOMAIN));
    }
    let attachment = input
        .attachments
        .iter()
        .map(|n| attachment_weight(n))
        .max()
        .unwrap_or(0);
    if attachment > 0 {
        hits.push((SpamReason::RiskyAttachment, attachment));
    }
    if reply_to_mismatch(input.from, input.reply_to) {
        hits.push((SpamReason::ReplyToMismatch, WEIGHT_REPLY_TO_MISMATCH));
    }
    let total: u32 = hits.iter().map(|(_, w)| u32::from(*w)).sum();
    let verdict = SpamVerdict {
        score: total.min(100) as u8,
        reasons: SpamReason::ALL
            .into_iter()
            .filter(|r| hits.iter().any(|(h, _)| h == r))
            .collect(),
    };
    if input.blocked {
        verdict.with_blocked()
    } else {
        verdict
    }
}

#[cfg(test)]
mod tests {
    use mh_core::SPAM_THRESHOLD;

    use super::*;

    fn addr(name: Option<&str>, email: &str) -> Address {
        Address::new(name, email)
    }

    /// Temiz bir gönderenle, yalnız verilen başlıklardan puan.
    fn of_headers(headers: &[(&str, &str)]) -> SpamVerdict {
        score(&SpamInput {
            headers,
            from: &addr(Some("Ayşe"), "ayse@eposta.com"),
            reply_to: &[],
            attachments: &[],
            blocked: false,
        })
    }

    fn of_attachments(names: &[&str]) -> SpamVerdict {
        score(&SpamInput {
            headers: &[],
            from: &addr(Some("Ayşe"), "ayse@eposta.com"),
            reply_to: &[],
            attachments: names,
            blocked: false,
        })
    }

    #[test]
    fn temiz_posta_puan_almaz() {
        let v = score(&SpamInput {
            headers: &[
                (
                    "Authentication-Results",
                    " mx.ornek.com; dkim=pass; spf=pass; dmarc=pass",
                ),
                ("X-Spam-Flag", " NO"),
                ("X-Spam-Status", " No, score=-1.2 required=5.0"),
                ("X-Spam-Level", " **"),
                ("X-Rspamd-Action", " no action"),
                (
                    "X-Forefront-Antispam-Report",
                    " CIP:1.2.3.4;CTRY:TR;SCL:1;SRV:;",
                ),
            ],
            from: &addr(Some("Ayşe Demir"), "ayse@eposta.com"),
            reply_to: &[
                addr(None, "ayse@eposta.com"),
                addr(None, "destek@posta.eposta.com"),
            ],
            attachments: &[
                "rapor.pdf",
                "tablo.xlsx",
                "foto.jpeg",
                "arsiv.zip",
                "notlar",
            ],
            blocked: false,
        });
        assert_eq!(v, SpamVerdict::default());
        assert!(!v.is_spam());
    }

    #[test]
    fn sunucu_isareti() {
        for headers in [
            [("X-Spam-Flag", " YES")],
            [(
                "X-Spam-Status",
                " Yes, score=7.1 required=5.0 tests=BAYES_99",
            )],
            [("X-Spam", " Yes")],
            [("X-Spam-Level", " *****")],
            [("X-Spam-Level", " ********")],
            [("X-Rspamd-Action", " add header")],
            [("X-Rspamd-Action", " Rewrite Subject")],
            [("X-Rspamd-Action", " reject")],
            [(
                "X-Forefront-Antispam-Report",
                " CIP:1.2.3.4;CTRY:;SCL:5;SRV:;",
            )],
            [(
                "X-Forefront-Antispam-Report",
                " CIP:1.2.3.4; scl : 9 ;SFV:SPM",
            )],
        ] {
            let v = of_headers(&headers);
            assert_eq!(v.reasons, [SpamReason::ServerFlagged], "{headers:?}");
            assert_eq!(v.score, WEIGHT_SERVER_FLAGGED);
            assert!(v.is_spam(), "sunucu işareti tek başına eşiği geçer");
        }
        for headers in [
            [("X-Spam-Flag", " NO")],
            [("X-Spam-Flag", " ")],
            [("X-Spam-Status", " No, score=1.0")],
            [("X-Spam-Status", " Yesterday")],
            [("X-Spam-Level", " ****")],
            [("X-Rspamd-Action", " greylist")],
            [("X-Rspamd-Action", " no action")],
            [("X-Forefront-Antispam-Report", " SCL:4;")],
            [("X-Forefront-Antispam-Report", " SCL:-1;")],
            [("X-Forefront-Antispam-Report", " BCL:9;PCL:8;")],
            [("X-Forefront-Antispam-Report", " SCL:55;")],
            [("Subject", " X-Spam-Flag: YES")],
        ] {
            assert_eq!(of_headers(&headers), SpamVerdict::default(), "{headers:?}");
        }
    }

    #[test]
    fn baslik_adi_harf_duyarsiz_ve_katlanmis_deger() {
        // Ad büyük/küçük harf karışık; değer satırlara katlanmış.
        let v = of_headers(&[("x-SPAM-flag", "\r\n\tyes")]);
        assert_eq!(v.reasons, [SpamReason::ServerFlagged]);
        let v = of_headers(&[(
            "AUTHENTICATION-results",
            " mx.ornek.com;\r\n\tdkim=fail header.d=banka.com;\r\n   spf=fail smtp.mailfrom=banka.com",
        )]);
        assert_eq!(v.reasons, [SpamReason::AuthFailed]);
        let v = of_headers(&[(
            "x-forefront-antispam-report",
            " CIP:1.2.3.4;CTRY:TR;\r\n SCL:6;\r\n SRV:;",
        )]);
        assert_eq!(v.reasons, [SpamReason::ServerFlagged]);
        let v = of_headers(&[("X-Spam-Status", " Yes,\r\n\tscore=9.9\r\n\trequired=5.0")]);
        assert_eq!(v.reasons, [SpamReason::ServerFlagged]);
    }

    #[test]
    fn yalniz_en_ustteki_baslik_sayilir() {
        // Alıcı sunucu "hayır" demiş; gönderenin yazdığı alttaki sayılmaz.
        let v = of_headers(&[("X-Spam-Flag", " NO"), ("X-Spam-Flag", " YES")]);
        assert_eq!(v, SpamVerdict::default());
        // Tersi: sunucu işaretlemiş, gönderen "NO" yazmış.
        let v = of_headers(&[("X-Spam-Flag", " YES"), ("X-Spam-Flag", " NO")]);
        assert!(v.is_spam());
        let v = of_headers(&[
            ("Authentication-Results", " mx.ornek.com; dmarc=pass"),
            ("Authentication-Results", " sahte; dmarc=fail"),
        ]);
        assert_eq!(v, SpamVerdict::default());
    }

    #[test]
    fn kimlik_dogrulama_basarisiz() {
        let v = of_headers(&[(
            "Authentication-Results",
            " mx.ornek.com; dkim=pass header.d=kotu.example; spf=pass; dmarc=fail (p=REJECT)",
        )]);
        assert_eq!(v.reasons, [SpamReason::AuthFailed]);
        assert_eq!(v.score, WEIGHT_AUTH_FAILED);
        assert!(v.is_spam(), "tek başına eşiği geçer");
        // Yalnız DKIM başarısız: işaret değil.
        let v = of_headers(&[(
            "Authentication-Results",
            " mx.ornek.com; dkim=fail; spf=pass; dmarc=pass",
        )]);
        assert_eq!(v, SpamVerdict::default());
    }

    #[test]
    fn gonderen_isaretleri_esigin_altinda_kalir() {
        let none: &[(&str, &str)] = &[];
        let v = score(&SpamInput {
            headers: none,
            from: &addr(Some("Banka destek@banka.com.tr"), "x@kotu.example"),
            reply_to: &[],
            attachments: &[],
            blocked: false,
        });
        assert_eq!(v.reasons, [SpamReason::NameHasOtherAddress]);
        assert_eq!(v.score, WEIGHT_NAME_HAS_OTHER_ADDRESS);
        assert!(!v.is_spam());

        let v = score(&SpamInput {
            headers: none,
            from: &addr(None, "a@xn--80ak6aa92e.com"),
            reply_to: &[],
            attachments: &[],
            blocked: false,
        });
        assert_eq!(v.reasons, [SpamReason::LookalikeDomain]);
        assert_eq!(v.score, WEIGHT_LOOKALIKE_DOMAIN);
        assert!(!v.is_spam());

        let v = score(&SpamInput {
            headers: none,
            from: &addr(Some("Ayşe"), "ayse@eposta.com"),
            reply_to: &[
                addr(None, "ayse@eposta.com"),
                addr(None, "odeme@baska.example"),
            ],
            attachments: &[],
            blocked: false,
        });
        assert_eq!(v.reasons, [SpamReason::ReplyToMismatch]);
        assert_eq!(v.score, WEIGHT_REPLY_TO_MISMATCH);
        assert!(!v.is_spam());
    }

    #[test]
    fn yanit_adresi() {
        let from = addr(None, "bildirim@posta.banka.com.tr");
        assert!(!reply_to_mismatch(&from, &[]));
        assert!(
            !reply_to_mismatch(&from, &[addr(None, "destek@banka.com.tr")]),
            "aynı site"
        );
        assert!(!reply_to_mismatch(
            &from,
            &[addr(None, " Destek@WWW.Banka.com.tr ")]
        ));
        assert!(reply_to_mismatch(
            &from,
            &[addr(None, "destek@banka-destek.example")]
        ));
        assert!(
            !reply_to_mismatch(&from, &[addr(None, "bozuk")]),
            "bozuk yanıt adresi sayılmaz"
        );
        assert!(!reply_to_mismatch(
            &addr(None, "bozuk"),
            &[addr(None, "a@b.com")]
        ));
    }

    #[test]
    fn riskli_ek() {
        for name in [
            "kurulum.exe",
            "KURULUM.EXE",
            "ekran.scr",
            "belge.com",
            "kisayol.pif",
            "calistir.bat",
            "calistir.cmd",
            "betik.js",
            "betik.jse",
            "makro.vbs",
            "makro.vbe",
            "betik.wsf",
            "betik.wsh",
            "sayfa.hta",
            "kisayol.lnk",
            "paket.msi",
            "uygulama.jar",
            "betik.ps1",
            "disk.iso",
            "disk.img",
            "uygulama.apk",
            "fatura.exe.",
            "fatura.exe  ",
            "fatura . exe",
        ] {
            assert_eq!(attachment_weight(name), WEIGHT_RISKY_ATTACHMENT, "{name}");
        }
        for name in [
            "rapor.pdf",
            "arsiv.zip",
            "foto.jpg",
            "tablo.xlsx",
            "exe",
            "notlar",
            "",
            ".",
            "sürüm.1.2.pdf",
            "komut.txt",
            "java.jar.txt",
        ] {
            assert_eq!(attachment_weight(name), 0, "{name}");
        }
        let v = of_attachments(&["rapor.pdf", "kurulum.exe"]);
        assert_eq!(v.reasons, [SpamReason::RiskyAttachment]);
        assert_eq!(v.score, WEIGHT_RISKY_ATTACHMENT);
        assert!(!v.is_spam(), "ek tek başına eşiği geçmez");
    }

    #[test]
    fn cift_uzanti() {
        // Belge uzantısının ardına eklenen çalıştırılabilir ya da HTML uzantısı.
        assert_eq!(attachment_weight("fatura.pdf.exe"), WEIGHT_RISKY_ATTACHMENT);
        assert_eq!(
            attachment_weight("Dekont.PDF.html"),
            WEIGHT_RISKY_ATTACHMENT
        );
        assert_eq!(attachment_weight("foto.jpg .htm"), WEIGHT_RISKY_ATTACHMENT);
        assert_eq!(attachment_weight("tablo.xlsx.lnk"), WEIGHT_RISKY_ATTACHMENT);
        // Gövdesi belge uzantısına benzeyen ad çift uzantı değildir.
        assert_eq!(attachment_weight("pdf.html"), WEIGHT_HTML_ATTACHMENT);
        // Zararsız ikinci uzantı.
        assert_eq!(attachment_weight("rapor.pdf.zip"), 0);
        assert_eq!(attachment_weight("rapor.2026.pdf"), 0);
    }

    #[test]
    fn html_eki_yari_agirlik() {
        for name in ["sayfa.html", "sayfa.HTM", "form.xhtml", "form.shtml"] {
            assert_eq!(attachment_weight(name), WEIGHT_HTML_ATTACHMENT, "{name}");
        }
        let v = of_attachments(&["sayfa.html"]);
        assert_eq!(v.reasons, [SpamReason::RiskyAttachment]);
        assert_eq!(v.score, WEIGHT_HTML_ATTACHMENT);
        // En ağır ek sayılır; ekler toplanmaz.
        let v = of_attachments(&["sayfa.html", "form.htm", "kurulum.exe", "betik.js"]);
        assert_eq!(v.score, WEIGHT_RISKY_ATTACHMENT);
    }

    #[test]
    fn esik_alti_ve_ustu() {
        let none: &[(&str, &str)] = &[];
        // HTML eki + başka alana yanıt: 35, eşiğin altında.
        let v = score(&SpamInput {
            headers: none,
            from: &addr(Some("Ayşe"), "ayse@eposta.com"),
            reply_to: &[addr(None, "x@baska.example")],
            attachments: &["form.html"],
            blocked: false,
        });
        assert_eq!(v.score, 35);
        assert_eq!(
            v.reasons,
            [SpamReason::RiskyAttachment, SpamReason::ReplyToMismatch]
        );
        assert!(v.score < SPAM_THRESHOLD && !v.is_spam());

        // Benzetilmiş alan adı + başka alana yanıt: 55, eşiğin üstünde.
        let v = score(&SpamInput {
            headers: none,
            from: &addr(None, "a@xn--80ak6aa92e.com"),
            reply_to: &[addr(None, "x@baska.example")],
            attachments: &[],
            blocked: false,
        });
        assert_eq!(v.score, 55);
        assert_eq!(
            v.reasons,
            [SpamReason::LookalikeDomain, SpamReason::ReplyToMismatch]
        );
        assert!(v.is_spam());

        // Görünen adda başka adres + riskli ek: 80.
        let v = score(&SpamInput {
            headers: none,
            from: &addr(Some("Banka destek@banka.com.tr"), "x@kotu.example"),
            reply_to: &[],
            attachments: &["dekont.pdf.exe"],
            blocked: false,
        });
        assert_eq!(v.score, 80);
        assert!(v.is_spam());

        // Tam eşik: yalnız kimlik doğrulama (50).
        assert_eq!(WEIGHT_AUTH_FAILED, SPAM_THRESHOLD);
    }

    #[test]
    fn toplam_yuze_kirpilir_ve_nedenler_sirali() {
        let v = score(&SpamInput {
            headers: &[
                ("X-Spam-Flag", " YES"),
                ("Authentication-Results", " mx.ornek.com; dmarc=fail"),
            ],
            from: &addr(Some("Banka destek@banka.com.tr"), "x@xn--80ak6aa92e.com"),
            reply_to: &[addr(None, "x@baska.example")],
            attachments: &["dekont.pdf.exe"],
            blocked: false,
        });
        assert_eq!(v.score, 100);
        assert_eq!(
            v.reasons,
            [
                SpamReason::ServerFlagged,
                SpamReason::AuthFailed,
                SpamReason::NameHasOtherAddress,
                SpamReason::LookalikeDomain,
                SpamReason::RiskyAttachment,
                SpamReason::ReplyToMismatch,
            ]
        );
    }

    #[test]
    fn engellenen_gonderen_yuz_puan() {
        let none: &[(&str, &str)] = &[];
        let v = score(&SpamInput {
            headers: none,
            from: &addr(Some("Ayşe"), "ayse@eposta.com"),
            reply_to: &[],
            attachments: &[],
            blocked: true,
        });
        assert_eq!(v.score, 100);
        assert_eq!(v.reasons, [SpamReason::Blocked]);
        // Öteki nedenler korunur; engel başa gelir.
        let v = score(&SpamInput {
            headers: none,
            from: &addr(Some("Ayşe"), "ayse@eposta.com"),
            reply_to: &[addr(None, "x@baska.example")],
            attachments: &[],
            blocked: true,
        });
        assert_eq!(v.score, 100);
        assert_eq!(
            v.reasons,
            [SpamReason::Blocked, SpamReason::ReplyToMismatch]
        );
    }

    #[test]
    fn ham_postadan_puan() {
        // Başlıklar ham postadan `headers::with_raw` ile okunur (gönderen
        // güveniyle aynı yol); gövdedeki satırlar başlık sayılmaz.
        let raw = b"X-Spam-Flag: YES\r\nAuthentication-Results: mx.ornek.com;\r\n dkim=fail;\r\n spf=fail\r\nFrom: Banka <bildirim@banka.com>\r\nSubject: x\r\n\r\nX-Rspamd-Action: reject\r\n";
        let from = addr(Some("Banka"), "bildirim@banka.com");
        let v = headers::with_raw(raw, |h| {
            score(&SpamInput {
                headers: h,
                from: &from,
                reply_to: &[],
                attachments: &[],
                blocked: false,
            })
        })
        .unwrap();
        assert_eq!(
            v.reasons,
            [SpamReason::ServerFlagged, SpamReason::AuthFailed]
        );
        assert_eq!(v.score, 100);
    }
}
