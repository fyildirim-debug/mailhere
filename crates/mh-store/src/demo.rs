//! Tanıtım verisi: onaylı tasarımdaki üç hesap ve dokuz posta.
//!
//! Postalar gerçek RFC 5322 metni olarak üretilip olağan ayrıştırma yolundan
//! geçer; böylece okuma alanı, konuşma ve ekler gerçek veriyle çalışır.

use chrono::{DateTime, Duration, FixedOffset, NaiveTime, TimeZone};
use mail_builder::headers::raw::Raw;
use mail_builder::MessageBuilder;
use mh_core::{
    Account, AccountKind, AppError, AppResult, ErrorCode, MailboxRole, Millis, Security,
    ServerConfig,
};

use crate::messages::NewMessage;
use crate::Store;

/// Tanıtım hesaplarının sunucu adı; ağa hiç çıkılmaz.
pub const DEMO_HOST: &str = "demo.invalid";

const ME: &str = "Furkan Yıldırım";

struct DemoAccount {
    id: &'static str,
    name: &'static str,
    email: &'static str,
    color: &'static str,
    kind: AccountKind,
}

const ACCOUNTS: [DemoAccount; 3] = [
    DemoAccount {
        id: "demo-is",
        name: "İş",
        email: "furkan@ornek.com.tr",
        color: "#4C6FD8",
        kind: AccountKind::Imap,
    },
    DemoAccount {
        id: "demo-kisisel",
        name: "Kişisel",
        email: "furkan@eposta.com",
        color: "#2E9A61",
        kind: AccountKind::Imap,
    },
    DemoAccount {
        id: "demo-sunucu",
        name: "Sunucu",
        email: "posta@alanadi.com",
        color: "#D08A1E",
        kind: AccountKind::Jmap,
    },
];

const BOXES: [(&str, MailboxRole); 5] = [
    ("INBOX", MailboxRole::Inbox),
    ("Drafts", MailboxRole::Drafts),
    ("Sent", MailboxRole::Sent),
    ("Archive", MailboxRole::Archive),
    ("Trash", MailboxRole::Trash),
];

/// Bir tanıtım postası.
struct Mail {
    account: usize,
    mailbox: &'static str,
    /// (gün farkı, saat, dakika); bugün = 0.
    when: (i64, u32, u32),
    from: (&'static str, &'static str),
    to: Vec<(&'static str, &'static str)>,
    subject: &'static str,
    body: &'static [&'static str],
    attachments: &'static [(&'static str, &'static str, usize)],
    headers: &'static [(&'static str, &'static str)],
    remote_image: Option<&'static str>,
    message_id: &'static str,
    in_reply_to: Option<&'static str>,
    references: &'static [&'static str],
    unread: bool,
    starred: bool,
}

const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const PDF: &str = "application/pdf";
const KB: usize = 1024;

fn me(account: usize) -> (&'static str, &'static str) {
    (ME, ACCOUNTS[account].email)
}

fn mails() -> Vec<Mail> {
    let base = |account: usize| Mail {
        account,
        mailbox: "INBOX",
        when: (0, 0, 0),
        from: ("", ""),
        to: vec![me(account)],
        subject: "",
        body: &[],
        attachments: &[],
        headers: &[],
        remote_image: None,
        message_id: "",
        in_reply_to: None,
        references: &[],
        unread: false,
        starred: false,
    };
    vec![
        // m1 konuşmasının önceki iki iletisi
        Mail {
            mailbox: "Sent",
            when: (-1, 17, 20),
            from: me(0),
            to: vec![("Kerem Aydın", "kerem.aydin@ornek.com.tr")],
            subject: "Q4 bütçe taslağı",
            body: &["Kerem selam, toplantıdan önce taslağı görebilir miyim?"],
            message_id: "q4-butce-1@ornek.com.tr",
            ..base(0)
        },
        Mail {
            mailbox: "Archive",
            when: (-1, 18, 4),
            from: ("Kerem Aydın", "kerem.aydin@ornek.com.tr"),
            subject: "Re: Q4 bütçe taslağı",
            body: &["Tabii, yarın sabah ilk iş gönderiyorum."],
            message_id: "q4-butce-2@ornek.com.tr",
            in_reply_to: Some("q4-butce-1@ornek.com.tr"),
            references: &["q4-butce-1@ornek.com.tr"],
            ..base(0)
        },
        // m1
        Mail {
            when: (0, 9, 42),
            from: ("Kerem Aydın", "kerem.aydin@ornek.com.tr"),
            to: vec![me(0), ("Ayşe Demir", "ayse.demir@eposta.com")],
            subject: "Q4 bütçe taslağı",
            body: &[
                "Merhaba,",
                "Perşembe toplantısından önce bütçe taslağının son halini ekledim. Pazarlama kaleminde geçen çeyreğe göre %8'lik bir düşüş var; gerekçesini toplantıda konuşalım.",
                "Senden ricam altyapı giderlerine bir göz atman. Sunucu yenileme kalemi önceki çeyreğin üzerinde görünüyor; bir kısmını ilk çeyreğe kaydırabiliriz.",
                "Sunum özetini de ekledim, on dakikalık bir anlatım için yeterli.",
                "Kolay gelsin,\nKerem",
            ],
            attachments: &[("Butce_Q4_taslak.xlsx", XLSX, 248 * KB), ("Sunum_ozeti.pdf", PDF, 1258 * KB)],
            message_id: "q4-butce-3@ornek.com.tr",
            in_reply_to: Some("q4-butce-2@ornek.com.tr"),
            references: &["q4-butce-1@ornek.com.tr", "q4-butce-2@ornek.com.tr"],
            ..base(0)
        },
        // m2
        Mail {
            when: (0, 8, 15),
            from: ("Ayşe Demir", "ayse.demir@eposta.com"),
            subject: "Cumartesi kahvaltı?",
            body: &[
                "Selam!",
                "Cumartesi Moda'daki yeni kahvaltı yerine gidelim mi? Saat 10 gibi buluşursak kalabalığa kalmayız.",
                "Deniz ve Can da geliyor. Sen de gelirsen dört kişilik masa ayırtırım; akşama kadar haber vermen yeter.",
                "Sevgiler,\nAyşe",
            ],
            message_id: "kahvalti@eposta.com",
            unread: true,
            ..base(1)
        },
        // m3
        Mail {
            when: (0, 7, 3),
            from: ("GitHub", "notifications@github.com"),
            subject: "[mailhere] Derleme başarılı · main #128",
            body: &[
                "mailhere deposunda main dalı için 128 numaralı derleme tamamlandı.",
                "Linux x64 · geçti · 2 dk 41 sn\nWindows x64 · geçti · 3 dk 12 sn\nBirim testleri · 214 / 214\nUçtan uca testler · 38 / 38",
                "Paketler sürüm sayfasına yüklendi. Bu bildirimleri depo ayarlarından yönetebilirsin.",
            ],
            headers: &[("X-GitHub-Reason", "ci_activity")],
            message_id: "derleme-128@github.com",
            ..base(0)
        },
        // m4
        Mail {
            when: (0, 6, 30),
            from: ("Derin Kargo", "bilgi@derinkargo.com.tr"),
            subject: "Gönderiniz dağıtıma çıktı",
            body: &[
                "Merhaba,",
                "4021 8833 17 numaralı gönderiniz dağıtıma çıktı. Kurye bugün 14:00 ile 18:00 arasında adresinize uğrayacak.",
                "Evde olmayacaksanız teslimatı en yakın şubeye yönlendirebilirsiniz.",
                "Derin Kargo",
            ],
            headers: &[("Auto-Submitted", "auto-generated")],
            message_id: "dagitim-402188331@derinkargo.com.tr",
            unread: true,
            ..base(1)
        },
        // m5
        Mail {
            when: (-1, 19, 20),
            from: ("Mimar Haftalık", "bulten@mimarhaftalik.com"),
            subject: "Sayı 142 · Sakin arayüzler üzerine",
            body: &[
                "Bu hafta üç yazı seçtik.",
                "Boşluğun sesi — Ekranda hiçbir şey olmayan alanlar neden en çok şeyi anlatır?",
                "Az renkle hiyerarşi — Tek vurgu rengiyle yön bulmayı kolaylaştıran yedi örnek.",
                "Yavaş teknoloji — Bildirimleri azaltmak odaklanmayı gerçekten artırıyor mu?",
                "İyi okumalar,\nMimar Haftalık",
            ],
            headers: &[
                ("List-Id", "Mimar Haftalik <haftalik.mimarhaftalik.com>"),
                ("List-Unsubscribe", "<https://mimarhaftalik.com/abonelik/birak>"),
            ],
            remote_image: Some("https://gorsel.mimarhaftalik.com/sayi-142/kapak.jpg"),
            message_id: "sayi-142@mimarhaftalik.com",
            unread: true,
            ..base(1)
        },
        // m6
        Mail {
            when: (-1, 16, 48),
            from: ("Elif Şahin", "elif.sahin@hukukbiz.com.tr"),
            subject: "Sözleşme imzaya hazır",
            body: &[
                "Merhaba,",
                "Konuştuğumuz son düzeltmeleri işledim. 4. maddedeki hizmet süresi 24 aya çekildi, fesih bildirimi 30 güne indi.",
                "Uygunsa e-imza bağlantısını yarın sabah gönderebilirim.",
                "Saygılarımla,\nAv. Elif Şahin",
            ],
            attachments: &[("Sozlesme_v3.pdf", PDF, 386 * KB)],
            message_id: "sozlesme-v3@hukukbiz.com.tr",
            starred: true,
            ..base(0)
        },
        // m7
        Mail {
            when: (-1, 11, 2),
            from: ("Bulut Fatura", "fatura@bulutfatura.com.tr"),
            subject: "Ekim faturanız hazır",
            body: &[
                "Sayın müşterimiz,",
                "Ekim 2026 dönemine ait faturanız oluşturuldu.",
                "Tutar: 1.240,00 ₺\nSon ödeme: 24 Ekim 2026",
                "Otomatik ödeme talimatınız varsa ek bir işlem yapmanıza gerek yok.",
            ],
            attachments: &[("Fatura_2026_10.pdf", PDF, 92 * KB)],
            headers: &[("Auto-Submitted", "auto-generated")],
            message_id: "fatura-2026-10@bulutfatura.com.tr",
            ..base(2)
        },
        // m8
        Mail {
            when: (-3, 8, 0),
            from: ("Tasarım Notları", "merhaba@tasarimnotlari.co"),
            subject: "Renk, ışık ve odak",
            body: &[
                "Merhaba,",
                "Uzun ekran saatlerinde gözü yoran şey çoğu zaman parlaklık değil, karşıtlığın dağınıklığıdır.",
                "Bu sayıda beş küçük karar var: zemin tonunu kırmak, tek vurgu rengi seçmek, satır aralığını açmak, hareketi azaltmak ve bildirimleri gruplamak.",
                "Görüşmek üzere,\nTasarım Notları",
            ],
            headers: &[
                ("List-Id", "Tasarim Notlari <notlar.tasarimnotlari.co>"),
                ("List-Unsubscribe", "<mailto:ayril@tasarimnotlari.co>"),
            ],
            remote_image: Some("https://cdn.tasarimnotlari.co/sayilar/renk-isik-odak.png"),
            message_id: "renk-isik-odak@tasarimnotlari.co",
            ..base(2)
        },
        // m9 konuşmasının önceki iletisi
        Mail {
            mailbox: "Sent",
            when: (-2, 10, 30),
            from: me(0),
            to: vec![("Murat Kaya", "murat.kaya@ornek.com.tr")],
            subject: "Sunucu taşıma planı",
            body: &["Taşıma için cumartesi gece uygun mu?"],
            message_id: "tasima-1@ornek.com.tr",
            ..base(0)
        },
        // m9
        Mail {
            when: (-2, 14, 12),
            from: ("Murat Kaya", "murat.kaya@ornek.com.tr"),
            to: vec![me(0), ("Kerem Aydın", "kerem.aydin@ornek.com.tr")],
            subject: "Re: Sunucu taşıma planı",
            body: &[
                "Selam,",
                "Cumartesi gece 02:00 bakım penceresini onayladım. Yedekleri cuma akşamı alalım, taşımadan sonra iki saat gözlemde kalalım.",
                "Geri dönüş planı ekteki belgede; bir göz atarsan sevinirim.",
                "Murat",
            ],
            attachments: &[("Geri_donus_plani.pdf", PDF, 210 * KB)],
            message_id: "tasima-2@ornek.com.tr",
            in_reply_to: Some("tasima-1@ornek.com.tr"),
            references: &["tasima-1@ornek.com.tr"],
            ..base(0)
        },
    ]
}

/// Belirli boyutta, türüne uygun başlıkla başlayan dolgu dosyası.
fn filler(mime: &str, size: usize) -> Vec<u8> {
    let mut out: Vec<u8> = if mime == PDF {
        b"%PDF-1.4\n% Mailhere tanitim dosyasi\n".to_vec()
    } else {
        b"PK\x03\x04".to_vec()
    };
    let line = b"% Bu dosya yalnizca tanitim icindir.\n";
    while out.len() < size {
        let take = line.len().min(size - out.len());
        out.extend_from_slice(&line[..take]);
    }
    out
}

fn html_body(m: &Mail, url: &str) -> String {
    let mut html = String::from(
        "<div style=\"max-width:600px;margin:0 auto;font-family:Georgia,serif;color:#222\">",
    );
    html.push_str(&format!(
        "<img src=\"{url}\" alt=\"{}\" width=\"600\" style=\"width:100%;height:auto;border-radius:8px\">",
        m.subject
    ));
    for p in m.body {
        html.push_str("<p style=\"line-height:1.6\">");
        html.push_str(
            &p.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('\n', "<br>"),
        );
        html.push_str("</p>");
    }
    html.push_str("</div>");
    html
}

fn build_raw(m: &Mail, date_secs: i64) -> std::io::Result<Vec<u8>> {
    let mut b = MessageBuilder::new()
        .from(m.from)
        .to(m.to.clone())
        .subject(m.subject)
        .date(date_secs)
        .message_id(m.message_id)
        .text_body(m.body.join("\n\n"));
    if let Some(irt) = m.in_reply_to {
        b = b.in_reply_to(irt);
    }
    if !m.references.is_empty() {
        b = b.references(m.references.to_vec());
    }
    for (name, value) in m.headers {
        b = b.header(*name, Raw::new(*value));
    }
    if let Some(url) = m.remote_image {
        b = b.html_body(html_body(m, url));
    }
    for (name, mime, size) in m.attachments {
        b = b.attachment(*mime, *name, filler(mime, *size));
    }
    b.write_to_vec()
}

/// (gün farkı, saat, dakika) → yerel saatle zaman damgası (ms).
fn at(now: &DateTime<FixedOffset>, (days, h, min): (i64, u32, u32)) -> Millis {
    let day = now.date_naive() + Duration::days(days);
    let t = NaiveTime::from_hms_opt(h, min, 0).unwrap_or_default();
    now.offset()
        .from_local_datetime(&day.and_time(t))
        .single()
        .map(|d| d.timestamp_millis())
        .unwrap_or_else(|| now.timestamp_millis())
}

fn demo_account(a: &DemoAccount, created_at: Millis) -> Account {
    let server = |port: u16| ServerConfig {
        host: DEMO_HOST.into(),
        port,
        security: Security::Tls,
        username: a.email.into(),
    };
    Account {
        id: a.id.into(),
        name: a.name.into(),
        email: a.email.into(),
        display_name: ME.into(),
        color: a.color.into(),
        kind: a.kind,
        imap: (a.kind == AccountKind::Imap).then(|| server(993)),
        smtp: Some(server(465)),
        jmap_url: (a.kind == AccountKind::Jmap).then(|| format!("https://{DEMO_HOST}/jmap")),
        signature: None,
        created_at,
        auth: Default::default(),
    }
}

/// Hiç hesap yoksa tanıtım verisini yükler; yüklediyse `true`.
pub fn seed_demo(store: &Store, now: DateTime<FixedOffset>) -> AppResult<bool> {
    if !store.accounts()?.is_empty() {
        return Ok(false);
    }
    let now_ms = now.timestamp_millis();
    for (i, a) in ACCOUNTS.iter().enumerate() {
        store.insert_account(&demo_account(a, now_ms - 1000 + i as i64))?;
        for (path, role) in BOXES {
            store.upsert_mailbox(a.id, path, path, role)?;
        }
    }

    // Bugünün postaları "şimdi"den sonraya düşmesin: sırayı koruyarak geri çek.
    let mut mails = mails();
    let mut limit = now_ms - 60_000;
    let mut dates: Vec<Millis> = mails.iter().map(|m| at(&now, m.when)).collect();
    let mut order: Vec<usize> = (0..mails.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(dates[*i]));
    for i in order {
        dates[i] = dates[i].min(limit);
        limit = dates[i] - 60_000;
    }

    for (m, date) in mails.drain(..).zip(dates) {
        let account = ACCOUNTS[m.account].id;
        let raw = build_raw(&m, date / 1000).map_err(|e| {
            AppError::new(
                ErrorCode::Internal,
                format!("Tanıtım postası üretilemedi: {e}"),
            )
        })?;
        let mailbox = crate::mailbox_id(account, m.mailbox);
        let mut row =
            NewMessage::from_raw(raw, false, account, &mailbox, date).ok_or_else(|| {
                AppError::new(ErrorCode::Internal, "Tanıtım postası ayrıştırılamadı.")
            })?;
        row.date = date;
        row.unread = m.unread;
        row.starred = m.starred;
        store.upsert_message(&row)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::{Category, ListQuery, View};

    fn now() -> DateTime<FixedOffset> {
        // 13 Ekim 2026 Salı 10:30, İstanbul (+03:00)
        DateTime::parse_from_rfc3339("2026-10-13T10:30:00+03:00").unwrap()
    }

    fn inbox(s: &Store, n: Millis) -> Vec<mh_core::MessageSummary> {
        s.list(
            &ListQuery {
                view: View::Inbox,
                account_id: None,
                category: None,
                cursor: None,
                limit: 50,
            },
            n,
        )
        .unwrap()
        .items
    }

    #[test]
    fn tanitim_verisi_tasarimla_ayni() {
        let s = Store::open_in_memory().unwrap();
        assert!(seed_demo(&s, now()).unwrap());
        assert!(!seed_demo(&s, now()).unwrap(), "ikinci kez yüklenmez");

        let accounts = s.accounts().unwrap();
        let names: Vec<_> = accounts.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["İş", "Kişisel", "Sunucu"]);
        assert_eq!(accounts[2].kind, AccountKind::Jmap);
        assert!(accounts[2].imap.is_none());

        let n = now().timestamp_millis();
        let list = inbox(&s, n);
        let subjects: Vec<_> = list.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(
            subjects,
            vec![
                "Q4 bütçe taslağı",
                "Cumartesi kahvaltı?",
                "[mailhere] Derleme başarılı · main #128",
                "Gönderiniz dağıtıma çıktı",
                "Sayı 142 · Sakin arayüzler üzerine",
                "Sözleşme imzaya hazır",
                "Ekim faturanız hazır",
                "Re: Sunucu taşıma planı",
                "Renk, ışık ve odak",
            ]
        );
        let unread: Vec<_> = list
            .iter()
            .filter(|m| m.unread)
            .map(|m| m.subject.as_str())
            .collect();
        assert_eq!(
            unread,
            vec![
                "Cumartesi kahvaltı?",
                "Gönderiniz dağıtıma çıktı",
                "Sayı 142 · Sakin arayüzler üzerine"
            ]
        );
        let cats: Vec<_> = list.iter().map(|m| m.category).collect();
        use Category::*;
        assert_eq!(
            cats,
            vec![
                People,
                People,
                Notifications,
                Notifications,
                Newsletters,
                People,
                Notifications,
                People,
                Newsletters
            ]
        );
        assert!(list[5].starred);
        let with_att: Vec<_> = list
            .iter()
            .filter(|m| m.has_attachments)
            .map(|m| m.subject.as_str())
            .collect();
        assert_eq!(
            with_att,
            vec![
                "Q4 bütçe taslağı",
                "Sözleşme imzaya hazır",
                "Ekim faturanız hazır",
                "Re: Sunucu taşıma planı"
            ]
        );
        // 09:42 yerel saat
        assert_eq!(
            list[0].date,
            DateTime::parse_from_rfc3339("2026-10-13T09:42:00+03:00")
                .unwrap()
                .timestamp_millis()
        );

        let counts = s.counts(n).unwrap();
        assert_eq!(counts.inbox_unread, 3);
        assert_eq!(counts.per_account_unread["demo-kisisel"], 3);

        // m1: iki önceki ileti, eskiden yeniye; ekler tasarımdaki gibi.
        let d = s.detail(list[0].id, &[]).unwrap();
        assert_eq!(d.thread.len(), 2);
        assert_eq!(d.thread[0].from.email, "furkan@ornek.com.tr");
        assert_eq!(
            d.thread[1].snippet,
            "Tabii, yarın sabah ilk iş gönderiyorum."
        );
        let att: Vec<_> = d
            .attachments
            .iter()
            .map(|a| (a.name.as_str(), a.size))
            .collect();
        assert_eq!(
            att,
            vec![
                ("Butce_Q4_taslak.xlsx", 248 * 1024),
                ("Sunum_ozeti.pdf", 1258 * 1024)
            ]
        );
        assert_eq!(d.summary.to.len(), 2);
        assert!(d.body_text.unwrap().contains("%8'lik"));

        // m9: bir önceki ileti
        assert_eq!(s.detail(list[7].id, &[]).unwrap().thread.len(), 1);

        // Bültenlerde uzak görsel engelli
        let d = s.detail(list[4].id, &[]).unwrap();
        assert!(d.has_remote_content && !d.remote_allowed);
        assert!(d
            .body_html
            .unwrap()
            .contains("data-mh-src=\"https://gorsel.mimarhaftalik.com"));

        // Kahvaltı postası bulunur (tanıtım bildirimi).
        assert_eq!(
            s.find_by_subject("Cumartesi kahvaltı?")
                .unwrap()
                .unwrap()
                .from
                .email,
            "ayse.demir@eposta.com"
        );
    }

    #[test]
    fn sabah_erken_acilinca_tarih_gelecege_dusmez() {
        let s = Store::open_in_memory().unwrap();
        let early = DateTime::parse_from_rfc3339("2026-10-13T07:00:00+03:00").unwrap();
        seed_demo(&s, early).unwrap();
        let list = inbox(&s, early.timestamp_millis());
        assert_eq!(list.len(), 9);
        assert!(list.iter().all(|m| m.date < early.timestamp_millis()));
        assert_eq!(list[0].subject, "Q4 bütçe taslağı");
        assert_eq!(list[3].subject, "Gönderiniz dağıtıma çıktı");
    }
}
