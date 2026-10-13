//! Okuma alanındaki abonelikten çıkma bilgisi ham postadan gelir.

use mh_core::{Account, AccountKind, MailboxRole, Unsubscribe};
use mh_store::{NewMessage, Store};

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.insert_account(&Account {
        id: "a".into(),
        name: "İş".into(),
        email: "a@ornek.com".into(),
        display_name: "A".into(),
        color: "#4C6FD8".into(),
        kind: AccountKind::Imap,
        imap: None,
        smtp: None,
        jmap_url: None,
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: 1,
        auth: Default::default(),
        notify: true,
    })
    .unwrap();
    s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox, None, false)
        .unwrap();
    s
}

#[test]
fn ayrintida_abonelikten_cikma_ham_postadan_doldurulur() {
    let s = store();
    let raw = b"From: Bulten <haber@bulten.com>\r\n\
To: a@ornek.com\r\n\
Subject: Haftalik\r\n\
Message-ID: <h1@bulten.com>\r\n\
List-Unsubscribe: <https://bulten.com/cik?u=1>, <mailto:cik@bulten.com>\r\n\
List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Merhaba\r\n"
        .to_vec();
    // Yalnız başlık inmişken ham posta saklanmaz: bilgi yok.
    let mut m = NewMessage::from_raw(raw.clone(), true, "a", "a:INBOX", 5).unwrap();
    m.uid = Some(1);
    let id = s.upsert_message(&m).unwrap().id;
    assert_eq!(s.detail(id, &[]).unwrap().unsubscribe, None);

    s.store_body(id, &raw).unwrap();
    assert_eq!(
        s.detail(id, &[]).unwrap().unsubscribe,
        Some(Unsubscribe {
            url: Some("https://bulten.com/cik?u=1".into()),
            mailto: Some("mailto:cik@bulten.com".into()),
            one_click: true,
        })
    );

    // Başlığı olmayan posta.
    let plain =
        b"From: x@y.com\r\nSubject: Selam\r\nMessage-ID: <p@y.com>\r\n\r\nSelam\r\n".to_vec();
    let mut m = NewMessage::from_raw(plain, false, "a", "a:INBOX", 6).unwrap();
    m.uid = Some(2);
    let id = s.upsert_message(&m).unwrap().id;
    assert_eq!(s.detail(id, &[]).unwrap().unsubscribe, None);
}
