//! Büyük sağlayıcıların herkese açık sunucu adları (elle yazılmış küçük tablo).

use mh_core::{Security, ServerConfig};

struct Provider {
    domains: &'static [&'static str],
    imap: (&'static str, u16, Security),
    smtp: (&'static str, u16, Security),
}

const PROVIDERS: &[Provider] = &[
    Provider {
        domains: &["gmail.com", "googlemail.com"],
        imap: ("imap.gmail.com", 993, Security::Tls),
        smtp: ("smtp.gmail.com", 465, Security::Tls),
    },
    Provider {
        domains: &[
            "outlook.com",
            "hotmail.com",
            "live.com",
            "msn.com",
            "outlook.com.tr",
            "hotmail.com.tr",
        ],
        imap: ("outlook.office365.com", 993, Security::Tls),
        smtp: ("smtp-mail.outlook.com", 587, Security::Starttls),
    },
    Provider {
        domains: &["yahoo.com", "yahoo.com.tr", "ymail.com"],
        imap: ("imap.mail.yahoo.com", 993, Security::Tls),
        smtp: ("smtp.mail.yahoo.com", 465, Security::Tls),
    },
    Provider {
        domains: &["icloud.com", "me.com", "mac.com"],
        imap: ("imap.mail.me.com", 993, Security::Tls),
        smtp: ("smtp.mail.me.com", 587, Security::Starttls),
    },
    Provider {
        domains: &["yandex.com", "yandex.com.tr", "yandex.ru", "ya.ru"],
        imap: ("imap.yandex.com", 993, Security::Tls),
        smtp: ("smtp.yandex.com", 465, Security::Tls),
    },
    Provider {
        domains: &["gmx.com"],
        imap: ("imap.gmx.com", 993, Security::Tls),
        smtp: ("mail.gmx.com", 587, Security::Starttls),
    },
    Provider {
        domains: &["gmx.net", "gmx.de"],
        imap: ("imap.gmx.net", 993, Security::Tls),
        smtp: ("mail.gmx.net", 587, Security::Starttls),
    },
    Provider {
        domains: &["zoho.com", "zohomail.com"],
        imap: ("imap.zoho.com", 993, Security::Tls),
        smtp: ("smtp.zoho.com", 465, Security::Tls),
    },
    Provider {
        domains: &["mail.ru"],
        imap: ("imap.mail.ru", 993, Security::Tls),
        smtp: ("smtp.mail.ru", 465, Security::Tls),
    },
];

fn server((host, port, security): (&str, u16, Security), email: &str) -> ServerConfig {
    ServerConfig {
        host: host.into(),
        port,
        security,
        username: email.into(),
    }
}

/// Alan adı tabloda varsa (IMAP, SMTP).
pub fn lookup(domain: &str, email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let p = PROVIDERS.iter().find(|p| p.domains.contains(&domain))?;
    Some((server(p.imap, email), server(p.smtp, email)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilinen_saglayicilar() {
        let (i, s) = lookup("gmail.com", "ali@gmail.com").unwrap();
        assert_eq!(
            (i.host.as_str(), i.port, i.security),
            ("imap.gmail.com", 993, Security::Tls)
        );
        assert_eq!((s.host.as_str(), s.port), ("smtp.gmail.com", 465));
        assert_eq!(i.username, "ali@gmail.com");
        let (_, s) = lookup("hotmail.com", "x@hotmail.com").unwrap();
        assert_eq!(s.security, Security::Starttls);
        for d in [
            "googlemail.com",
            "live.com",
            "msn.com",
            "yahoo.com",
            "me.com",
            "mac.com",
            "yandex.com.tr",
            "gmx.net",
            "zoho.com",
            "mail.ru",
        ] {
            assert!(lookup(d, "x@y").is_some(), "{d}");
        }
        assert!(lookup("ornek.com.tr", "x@ornek.com.tr").is_none());
    }
}
