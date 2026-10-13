//! Akıllı süzgeç: postanın kişiden mi, otomatik bildirimden mi, bültenden mi
//! geldiğini başlıklara bakarak tahmin eder.

use mh_core::Category;

/// Otomatik gönderici olduğunu ele veren @ öncesi adlar.
const AUTOMATED_LOCAL_PARTS: &[&str] = &[
    "noreply",
    "no-reply",
    "no_reply",
    "donotreply",
    "do-not-reply",
    "do_not_reply",
    "notification",
    "notifications",
    "notify",
    "bildirim",
    "bildirimler",
    "alert",
    "alerts",
    "mailer-daemon",
    "postmaster",
];

/// Yalnız makinelerin eklediği bilinen başlıklar.
const AUTOMATED_HEADERS: &[&str] = &[
    "x-github-reason",
    "x-gitlab-notificationreason",
    "x-gitea-reason",
    "x-jenkins-job",
    "x-bitbucket-event",
];

/// Sınıflandırmanın baktığı tüm başlıklar. Postanın tamamını değil yalnız
/// seçili başlıklarını getiren kaynaklar (JMAP) bunları ister.
pub const HEADERS: &[&str] = &[
    "List-Unsubscribe",
    "List-Id",
    "Precedence",
    "Auto-Submitted",
    "X-GitHub-Reason",
    "X-GitLab-NotificationReason",
    "X-Gitea-Reason",
    "X-Jenkins-Job",
    "X-Bitbucket-Event",
];

fn header<'a>(headers: &'a [(&str, &str)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

fn is_automated_local(email: &str) -> bool {
    let local = email.split('@').next().unwrap_or("").to_ascii_lowercase();
    // "noreply+abc" ya da "notifications.tr" gibi ekleri de yakala.
    let base = local
        .split(['+', '.'])
        .next()
        .unwrap_or(local.as_str())
        .to_owned();
    AUTOMATED_LOCAL_PARTS.contains(&local.as_str())
        || AUTOMATED_LOCAL_PARTS.contains(&base.as_str())
}

/// Postayı sınıflandırır. `headers` en üst düzey başlıklardır (ad, ham değer).
pub fn classify(headers: &[(&str, &str)], from_email: &str) -> Category {
    let precedence = header(headers, "precedence").map(str::to_ascii_lowercase);
    if header(headers, "list-unsubscribe").is_some()
        || header(headers, "list-id").is_some()
        || matches!(precedence.as_deref(), Some("bulk" | "list"))
    {
        return Category::Newsletters;
    }
    let auto_submitted = header(headers, "auto-submitted")
        .map(|v| v.to_ascii_lowercase())
        .is_some_and(|v| !v.is_empty() && v != "no");
    let automated_header = headers
        .iter()
        .any(|(n, _)| AUTOMATED_HEADERS.iter().any(|a| n.eq_ignore_ascii_case(a)));
    if auto_submitted
        || automated_header
        || matches!(precedence.as_deref(), Some("junk"))
        || is_automated_local(from_email)
    {
        return Category::Notifications;
    }
    Category::People
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulten_basliklari() {
        let h = [("List-Unsubscribe", "<https://ornek.com/cik>")];
        assert_eq!(classify(&h, "bulten@ornek.com"), Category::Newsletters);
        let h = [("List-Id", "<duyuru.ornek.com>")];
        assert_eq!(classify(&h, "ayse@ornek.com"), Category::Newsletters);
        let h = [("Precedence", " Bulk ")];
        assert_eq!(classify(&h, "x@ornek.com"), Category::Newsletters);
        let h = [("precedence", "list")];
        assert_eq!(classify(&h, "x@ornek.com"), Category::Newsletters);
    }

    #[test]
    fn bildirim_basliklari() {
        let h = [("Auto-Submitted", "auto-generated")];
        assert_eq!(classify(&h, "bilgi@kargo.com.tr"), Category::Notifications);
        let h = [("Auto-Submitted", "no")];
        assert_eq!(classify(&h, "kerem@ornek.com"), Category::People);
        let h = [("X-GitHub-Reason", "ci_activity")];
        assert_eq!(classify(&h, "kerem@ornek.com"), Category::Notifications);
    }

    #[test]
    fn otomatik_gonderici_adlari() {
        for e in [
            "noreply@banka.com",
            "no-reply@ornek.com",
            "DoNotReply@ornek.com",
            "notifications@github.com",
            "bildirim@e-devlet.gov.tr",
            "alerts@izleme.com",
            "MAILER-DAEMON@mx.ornek.com",
            "postmaster@ornek.com",
            "noreply+abc@ornek.com",
        ] {
            assert_eq!(classify(&[], e), Category::Notifications, "{e}");
        }
    }

    #[test]
    fn kisiler_varsayilan() {
        let h = [("Subject", "Merhaba"), ("From", "Ayşe <ayse@eposta.com>")];
        assert_eq!(classify(&h, "ayse@eposta.com"), Category::People);
        assert_eq!(classify(&[], "noreplyalan@ornek.com"), Category::People);
    }

    #[test]
    fn baslik_listesi_eksiksiz() {
        for h in AUTOMATED_HEADERS {
            assert!(
                HEADERS.iter().any(|x| x.eq_ignore_ascii_case(h)),
                "{h} HEADERS listesinde yok"
            );
        }
    }

    #[test]
    fn bulten_bildirimden_once_gelir() {
        let h = [("List-Id", "<x>"), ("Auto-Submitted", "auto-generated")];
        assert_eq!(classify(&h, "noreply@ornek.com"), Category::Newsletters);
    }
}
