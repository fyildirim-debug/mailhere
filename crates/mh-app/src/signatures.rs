//! Hazır metinlerin denetimi: hesap imzaları (`account_update`) ve posta
//! şablonları (`settings_set`).

use std::collections::HashSet;

use mh_core::{AppError, AppResult, ErrorCode, Signature, Template};

/// Bir hesaptaki en fazla imza.
pub const MAX_SIGNATURES: usize = 20;
const MAX_SIGNATURE_NAME: usize = 60;
const MAX_SIGNATURE_BODY: usize = 10_000;

/// En fazla şablon.
pub const MAX_TEMPLATES: usize = 50;
const MAX_TEMPLATE_NAME: usize = 80;
const MAX_TEMPLATE_SUBJECT: usize = 300;
const MAX_TEMPLATE_BODY: usize = 20_000;

/// (imzalar, yeni posta varsayılanı, yanıt varsayılanı)
pub type Signatures = (Vec<Signature>, Option<String>, Option<String>);

fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((pos, _)) => s[..pos].to_owned(),
        None => s.to_owned(),
    }
}

/// Boş kimliğe yeni kimlik verir; diğerleri kırpılır.
fn id_or_new(id: &str) -> String {
    match id.trim() {
        "" => uuid::Uuid::new_v4().to_string(),
        id => id.to_owned(),
    }
}

/// İmza listesini denetler ve sadeleştirir: ad tek satıra indirilir, kırpılır
/// ve kısaltılır (boşsa "İmza N"); gövdenin sonundaki boşluklar atılır; boş
/// kimliğe yeni kimlik verilir. Listede olmayan varsayılan kaldırılır.
pub fn normalize_signatures(
    list: Vec<Signature>,
    new: Option<String>,
    reply: Option<String>,
) -> AppResult<Signatures> {
    if list.len() > MAX_SIGNATURES {
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!("Bir hesapta en çok {MAX_SIGNATURES} imza olabilir. Kullanmadıklarını sil."),
        ));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for (i, s) in list.into_iter().enumerate() {
        let id = id_or_new(&s.id);
        if !seen.insert(id.clone()) {
            return Err(AppError::new(
                ErrorCode::Invalid,
                "İki imza aynı kimliği taşıyor. Pencereyi kapatıp yeniden açarak dene.",
            ));
        }
        let name = match truncate_chars(&mh_mime::clean_line(&s.name), MAX_SIGNATURE_NAME) {
            n if n.trim().is_empty() => format!("İmza {}", i + 1),
            n => n.trim_end().to_owned(),
        };
        let body = s.body.trim_end().to_owned();
        if body.chars().count() > MAX_SIGNATURE_BODY {
            return Err(AppError::new(
                ErrorCode::Invalid,
                format!(
                    "“{name}” imzası çok uzun: en çok 10.000 karakter olabilir. İmzayı kısalt."
                ),
            ));
        }
        out.push(Signature { id, name, body });
    }
    let keep = |d: Option<String>| d.map(|d| d.trim().to_owned()).filter(|d| seen.contains(d));
    Ok((out, keep(new), keep(reply)))
}

/// Şablon listesini sınırlar: ad ve gövdesi boş şablon atılır, ad tek satır
/// ve en çok 80 karakter (boşsa "Şablon"), konu tek satır ve en çok 300,
/// gövde en çok 20.000 karakter; boş kimliğe yeni kimlik verilir, yinelenen
/// kimliklerden ilki kalır; en çok 50 şablon.
pub fn normalize_templates(list: Vec<Template>) -> Vec<Template> {
    let mut seen = HashSet::new();
    list.into_iter()
        .filter(|t| !t.name.trim().is_empty() || !t.body.trim().is_empty())
        .filter_map(|t| {
            let id = id_or_new(&t.id);
            if !seen.insert(id.clone()) {
                return None;
            }
            let name = match truncate_chars(&mh_mime::clean_line(&t.name), MAX_TEMPLATE_NAME) {
                n if n.trim().is_empty() => "Şablon".to_owned(),
                n => n.trim_end().to_owned(),
            };
            Some(Template {
                id,
                name,
                subject: truncate_chars(&mh_mime::clean_line(&t.subject), MAX_TEMPLATE_SUBJECT)
                    .trim_end()
                    .to_owned(),
                body: truncate_chars(&t.body, MAX_TEMPLATE_BODY),
            })
        })
        .take(MAX_TEMPLATES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::Settings;

    fn sig(id: &str, name: &str, body: &str) -> Signature {
        Signature {
            id: id.into(),
            name: name.into(),
            body: body.into(),
        }
    }

    fn tpl(id: &str, name: &str, subject: &str, body: &str) -> Template {
        Template {
            id: id.into(),
            name: name.into(),
            subject: subject.into(),
            body: body.into(),
        }
    }

    #[test]
    fn imzalar_sadelestirilir() {
        let (list, new, reply) = normalize_signatures(
            vec![
                sig(" uzun ", "  Uzun\nimza ", "Sevgiler,\nFurkan \n\n"),
                sig("", " ", "F."),
                sig("x", &"ç".repeat(100), ""),
            ],
            Some("uzun".into()),
            Some("yok".into()),
        )
        .unwrap();
        assert_eq!(list[0], sig("uzun", "Uzun imza", "Sevgiler,\nFurkan"));
        assert_eq!(list[1].name, "İmza 2", "boş ad sırasıyla adlanır");
        assert!(!list[1].id.is_empty(), "boş kimliğe kimlik verilir");
        assert_eq!(list[2].name.chars().count(), 60);
        assert_eq!(new.as_deref(), Some("uzun"));
        assert_eq!(reply, None, "listede olmayan varsayılan kalkar");
        // Boş liste: varsayılan da yok.
        assert_eq!(
            normalize_signatures(vec![], Some("a".into()), None).unwrap(),
            (vec![], None, None)
        );
        // Tam sınırdaki gövde kabul edilir.
        let ok = "a".repeat(10_000);
        assert!(normalize_signatures(vec![sig("a", "A", &ok)], None, None).is_ok());
    }

    #[test]
    fn gecersiz_imzalar() {
        let err = |list: Vec<Signature>| normalize_signatures(list, None, None).unwrap_err();
        let e = err((0..21).map(|i| sig(&i.to_string(), "A", "b")).collect());
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("en çok 20 imza"), "{}", e.message);
        assert!(normalize_signatures(
            (0..20).map(|i| sig(&i.to_string(), "A", "b")).collect(),
            None,
            None
        )
        .is_ok());
        let e = err(vec![sig("a", "A", "1"), sig(" a", "B", "2")]);
        assert!(e.message.contains("aynı kimliği"), "{}", e.message);
        let e = err(vec![sig("a", "Uzun", &"ş".repeat(10_001))]);
        assert!(e.message.contains("10.000"), "{}", e.message);
    }

    #[test]
    fn sablonlar_ayarlarda_sinirlanir() {
        let mut list = vec![
            tpl("t1", "  Teşekkür\r\n ", " Re:\nkonu ", "Teşekkürler!"),
            tpl("", "", "Konu", "Yalnız gövde"),
            tpl("bos", " ", "Konu var", " \n "),
            tpl("t1", "Yinelenen", "", "atılır"),
            tpl(
                "uzun",
                &"a".repeat(200),
                &"k".repeat(400),
                &"g".repeat(25_000),
            ),
        ];
        list.extend((0..60).map(|i| tpl(&format!("n{i}"), "N", "", "b")));
        let s = crate::commands::normalize_settings(Settings {
            templates: list,
            ..Settings::default()
        });
        let t = &s.templates;
        assert_eq!(t.len(), 50);
        assert_eq!(t[0], tpl("t1", "Teşekkür", "Re: konu", "Teşekkürler!"));
        assert_eq!(t[1].name, "Şablon");
        assert!(!t[1].id.is_empty());
        assert_eq!(t[1].body, "Yalnız gövde");
        // Adı ve gövdesi boş şablon atılır; yinelenen kimlikten ilki kalır.
        assert!(t.iter().all(|x| x.id != "bos"));
        assert_eq!(t.iter().filter(|x| x.id == "t1").count(), 1);
        assert_eq!(t[2].id, "uzun");
        assert_eq!(t[2].name.chars().count(), 80);
        assert_eq!(t[2].subject.chars().count(), 300);
        assert_eq!(t[2].body.chars().count(), 20_000);
        assert_eq!(t[49].id, "n46");
    }
}
