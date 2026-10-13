//! Otomatik yanıt: JMAP `VacationResponse` (RFC 8621 §8).
//!
//! Sunucuda tek bir nesnedir (`singleton`). Yalnız oturumu
//! `urn:ietf:params:jmap:vacationresponse` yeteneğini bildiren sunucularda
//! kullanılır; istekler hesabın kendi oturumuyla (aynı kimlik doğrulama ve
//! köken kuralları) gider.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use mh_core::{Account, AppError, AppResult, ErrorCode, Millis, Vacation};
use mh_store::Store;
use mh_sync::SecretStore;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::Client;
use crate::error::{JmapError, JmapResult};
use crate::sync::parse;
use crate::types::{GetResponse, SetResponse};

/// Otomatik yanıt yeteneği.
pub const VACATION: &str = "urn:ietf:params:jmap:vacationresponse";
const SINGLETON: &str = "singleton";
const MAX_SUBJECT: usize = 300;
const MAX_BODY: usize = 20_000;
/// Bağlantı ve istek dahil üst süre.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Sunucudaki nesne (yalnız kullanılan alanlar).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct VacationResponse {
    is_enabled: bool,
    from_date: Option<String>,
    to_date: Option<String>,
    subject: Option<String>,
    text_body: Option<String>,
    html_body: Option<String>,
}

/// Sunucu otomatik yanıtı desteklemiyor (ya da hesap JMAP değil).
pub fn unsupported() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Bu hesabın sunucusu otomatik yanıtı desteklemiyor.",
    )
}

/// JMAP `UTCDate`: RFC 3339, saniye duyarlığında, `Z` ile.
pub fn utc_date(ms: Millis) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(ms).map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// RFC 3339 tarihini (herhangi bir saat dilimiyle) milisaniyeye çevirir.
pub fn parse_utc_date(s: &str) -> Option<Millis> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.timestamp_millis())
}

fn from_jmap(v: VacationResponse) -> Vacation {
    let body = match (v.text_body, v.html_body) {
        (Some(t), _) if !t.trim().is_empty() => t,
        (_, Some(h)) if !h.trim().is_empty() => mh_mime::html_to_text(&h),
        _ => String::new(),
    };
    Vacation {
        enabled: v.is_enabled,
        from_date: v.from_date.as_deref().and_then(parse_utc_date),
        to_date: v.to_date.as_deref().and_then(parse_utc_date),
        subject: mh_mime::clean_line(v.subject.as_deref().unwrap_or("")),
        body,
    }
}

/// `VacationResponse/set` güncellemesi. Boş konu ve gövde `null` gider
/// (sunucu kendi varsayılanını kullanır); HTML gövde temizlenir ki alıcı
/// burada görülen metni alsın.
fn patch(v: &Vacation) -> Value {
    let text = |s: &str| (!s.trim().is_empty()).then(|| s.to_owned());
    json!({
        "isEnabled": v.enabled,
        "fromDate": v.from_date.and_then(utc_date),
        "toDate": v.to_date.and_then(utc_date),
        "subject": text(&v.subject),
        "textBody": text(&v.body),
        "htmlBody": Value::Null,
    })
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::Invalid, message)
}

/// Denetler ve sadeleştirir: konu tek satır ve en çok 300 karakter, metin
/// en çok 20.000 karakter, bitiş başlangıçtan önce olamaz.
pub fn normalize_vacation(v: Vacation) -> AppResult<Vacation> {
    let subject = mh_mime::clean_line(&v.subject);
    if subject.chars().count() > MAX_SUBJECT {
        return Err(invalid(
            "Otomatik yanıtın konusu en çok 300 karakter olabilir. Konuyu kısalt.",
        ));
    }
    let body = v.body.replace("\r\n", "\n").trim_end().to_owned();
    if body.chars().count() > MAX_BODY {
        return Err(invalid(
            "Otomatik yanıt metni en çok 20.000 karakter olabilir. Metni kısalt.",
        ));
    }
    let date_ok = |d: Option<Millis>| d.is_none_or(|ms| utc_date(ms).is_some());
    if !date_ok(v.from_date) || !date_ok(v.to_date) {
        return Err(invalid(
            "Otomatik yanıtın tarihi geçersiz. Başlangıç ve bitiş tarihini yeniden seç.",
        ));
    }
    if let (Some(from), Some(to)) = (v.from_date, v.to_date) {
        if to < from {
            return Err(invalid(
                "Otomatik yanıtın bitiş tarihi başlangıç tarihinden önce olamaz. Tarihleri düzelt.",
            ));
        }
    }
    Ok(Vacation {
        enabled: v.enabled,
        from_date: v.from_date,
        to_date: v.to_date,
        subject,
        body,
    })
}

/// Otomatik yanıtın tutulduğu JMAP hesabı; oturumda yetenek yoksa `None`.
fn vacation_account(client: &Client) -> Option<String> {
    let session = client.session();
    if !session.capabilities.contains_key(VACATION) {
        return None;
    }
    if let Some(id) = session.primary_accounts.get(VACATION) {
        return Some(id.clone());
    }
    let id = client.account_id();
    session
        .accounts
        .get(id)
        .is_none_or(|a| {
            a.account_capabilities.is_empty() || a.account_capabilities.contains_key(VACATION)
        })
        .then(|| id.to_owned())
}

fn get_call(account_id: &str) -> (String, Value) {
    (
        "VacationResponse/get".into(),
        json!({ "accountId": account_id, "ids": [SINGLETON] }),
    )
}

fn first(v: Value) -> JmapResult<Vacation> {
    let r: GetResponse<VacationResponse> = parse(v)?;
    // Nesne yoksa kapalı sayılır.
    Ok(r.list.into_iter().next().map(from_jmap).unwrap_or_default())
}

/// Sunucudaki otomatik yanıt; yetenek yoksa `None`.
pub async fn get_vacation(client: &Client) -> JmapResult<Option<Vacation>> {
    let Some(account_id) = vacation_account(client) else {
        return Ok(None);
    };
    let (method, args) = get_call(&account_id);
    first(client.call(&method, args).await?).map(Some)
}

/// Otomatik yanıtı yazar ve sunucunun kaydettiği hali döner. Değer önceden
/// [`normalize_vacation`] ile denetlenmiş olmalı.
pub async fn set_vacation(client: &Client, v: &Vacation) -> JmapResult<Vacation> {
    let account_id = vacation_account(client).ok_or_else(unsupported)?;
    let set = (
        "VacationResponse/set".to_owned(),
        json!({ "accountId": account_id, "update": { SINGLETON: patch(v) } }),
    );
    let mut res = client.calls(&[set, get_call(&account_id)]).await?;
    let got = res.pop().unwrap_or_default();
    let set: SetResponse = parse(res.pop().unwrap_or_default())?;
    if let Some(e) = set.not_updated.as_ref().and_then(|m| m.get(SINGLETON)) {
        tracing::warn!(kind = %e.kind, "otomatik yanıt kaydedilmedi");
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!(
                "Sunucu otomatik yanıtı kaydetmedi ({}). Tarihleri ve metni denetleyip yeniden dene.",
                e.kind
            ),
        )
        .into());
    }
    first(got)
}

async fn with_client<T>(
    store: &Store,
    secrets: &Arc<dyn SecretStore>,
    account: &Account,
    work: impl AsyncFnOnce(&Client) -> JmapResult<T>,
) -> AppResult<T> {
    let run = async {
        let client = crate::connect(store, secrets, account).await?;
        work(&client).await
    };
    match tokio::time::timeout(TIMEOUT, run).await {
        Ok(r) => r.map_err(|e| e.to_app()),
        Err(_) => Err(JmapError::Timeout.to_app()),
    }
}

/// Hesabın otomatik yanıtı. IMAP ve tanıtım hesabında ya da sunucu
/// desteklemiyorsa `None`.
pub async fn vacation_get(
    store: &Store,
    secrets: &Arc<dyn SecretStore>,
    account: &Account,
) -> AppResult<Option<Vacation>> {
    if !crate::is_syncable(account) {
        return Ok(None);
    }
    with_client(store, secrets, account, async |c: &Client| {
        get_vacation(c).await
    })
    .await
}

/// Hesabın otomatik yanıtını yazar. Desteklenmiyorsa `Invalid`.
pub async fn vacation_set(
    store: &Store,
    secrets: &Arc<dyn SecretStore>,
    account: &Account,
    vacation: Vacation,
) -> AppResult<Vacation> {
    if !crate::is_syncable(account) {
        return Err(unsupported());
    }
    let v = normalize_vacation(vacation)?;
    with_client(store, secrets, account, async |c: &Client| {
        set_vacation(c, &v).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Credentials;
    use crate::types::{CORE, MAIL};
    use reqwest::Url;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn tarih_bicimi() {
        // 2026-10-13T08:00:00Z
        let ms = 1_791_878_400_000;
        assert_eq!(utc_date(ms).as_deref(), Some("2026-10-13T08:00:00Z"));
        // Milisaniye atılır.
        assert_eq!(utc_date(ms + 999).as_deref(), Some("2026-10-13T08:00:00Z"));
        assert_eq!(parse_utc_date("2026-10-13T08:00:00Z"), Some(ms));
        assert_eq!(parse_utc_date("2026-10-13T11:00:00+03:00"), Some(ms));
        assert_eq!(parse_utc_date(" 2026-10-13T08:00:00.250Z "), Some(ms + 250));
        assert_eq!(parse_utc_date("13.10.2026"), None);
        assert_eq!(utc_date(i64::MAX), None);
    }

    #[test]
    fn json_donusumu() {
        let v = Vacation {
            enabled: true,
            from_date: Some(1_791_878_400_000),
            to_date: None,
            subject: "İzindeyim".into(),
            body: "Pazartesi dönüyorum.".into(),
        };
        assert_eq!(
            patch(&v),
            json!({
                "isEnabled": true,
                "fromDate": "2026-10-13T08:00:00Z",
                "toDate": null,
                "subject": "İzindeyim",
                "textBody": "Pazartesi dönüyorum.",
                "htmlBody": null,
            })
        );
        let empty = Vacation::default();
        assert_eq!(
            patch(&empty),
            json!({"isEnabled": false, "fromDate": null, "toDate": null, "subject": null, "textBody": null, "htmlBody": null})
        );
        let got: VacationResponse = serde_json::from_value(json!({
            "id": "singleton",
            "isEnabled": true,
            "fromDate": "2026-10-13T08:00:00Z",
            "toDate": "2026-10-20T08:00:00Z",
            "subject": "İzin\r\nde",
            "textBody": "Dönünce yazarım.",
            "htmlBody": "<p>yok sayılır</p>"
        }))
        .unwrap();
        assert_eq!(
            from_jmap(got),
            Vacation {
                enabled: true,
                from_date: Some(1_791_878_400_000),
                to_date: Some(1_791_878_400_000 + 7 * 86_400_000),
                subject: "İzin  de".into(),
                body: "Dönünce yazarım.".into(),
            }
        );
        // Yalnız HTML gövde düz metne çevrilir; eksik alanlar boş.
        let got: VacationResponse =
            serde_json::from_value(json!({"id": "singleton", "htmlBody": "<p>Merhaba</p>"}))
                .unwrap();
        let v = from_jmap(got);
        assert_eq!(v.body, "Merhaba");
        assert!(!v.enabled && v.from_date.is_none() && v.subject.is_empty());
    }

    #[test]
    fn dogrulama() {
        let base = Vacation {
            enabled: true,
            from_date: Some(2_000),
            to_date: Some(2_000),
            subject: " İzin\nde ".into(),
            body: "Metin\r\n\r\n".into(),
        };
        let v = normalize_vacation(base.clone()).unwrap();
        assert_eq!(v.subject, "İzin de");
        assert_eq!(v.body, "Metin");
        let err = |v: Vacation| normalize_vacation(v).unwrap_err();
        let e = err(Vacation {
            to_date: Some(1_000),
            ..base.clone()
        });
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("bitiş tarihi"), "{}", e.message);
        assert!(err(Vacation {
            subject: "k".repeat(301),
            ..base.clone()
        })
        .message
        .contains("300"));
        assert!(err(Vacation {
            body: "ş".repeat(20_001),
            ..base.clone()
        })
        .message
        .contains("20.000"));
        assert!(err(Vacation {
            from_date: Some(i64::MAX),
            to_date: None,
            ..base.clone()
        })
        .message
        .contains("tarihi geçersiz"));
        // Yalnız bir uç verilebilir.
        assert!(normalize_vacation(Vacation {
            to_date: None,
            ..base
        })
        .is_ok());
    }

    type Seen = Arc<Mutex<Vec<String>>>;

    /// Her isteğin gövdesini kaydedip `reply` (JSON) yazan düz HTTP sunucusu.
    async fn api_server(reply: Value) -> (String, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen: Seen = Arc::default();
        let s2 = seen.clone();
        let body = reply.to_string();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (seen, body) = (s2.clone(), body.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    loop {
                        let text = String::from_utf8_lossy(&buf).into_owned();
                        if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                            let len = head
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse::<usize>().ok())?
                                })
                                .unwrap_or(0);
                            if rest.len() >= len {
                                seen.lock().unwrap().push(rest.to_owned());
                                break;
                            }
                        }
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(reply.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (base, seen)
    }

    fn client(base: &str, vacation: bool) -> Client {
        let mut caps = json!({ CORE: {}, MAIL: {} });
        let mut account_caps = json!({ MAIL: {} });
        if vacation {
            caps[VACATION] = json!({});
            account_caps[VACATION] = json!({});
        }
        let session = serde_json::from_value(json!({
            "capabilities": caps,
            "accounts": { "A": { "name": "a", "accountCapabilities": account_caps } },
            "primaryAccounts": { MAIL: "A" },
            "apiUrl": "/jmap/api",
            "downloadUrl": "/d/{accountId}/{blobId}/{name}?t={type}",
            "uploadUrl": "/u/{accountId}/",
        }))
        .unwrap();
        let url = Url::parse(&format!("{base}/.well-known/jmap")).unwrap();
        Client::from_session(url, session, Credentials::new("furkan", "gizli")).unwrap()
    }

    fn singleton() -> Value {
        json!({
            "id": "singleton",
            "isEnabled": true,
            "fromDate": "2026-10-13T08:00:00Z",
            "toDate": null,
            "subject": "İzindeyim",
            "textBody": "Pazartesi dönüyorum.",
            "htmlBody": null
        })
    }

    #[tokio::test]
    async fn sunucudan_okunur_ve_yazilir() {
        let reply = json!({
            "methodResponses": [
                ["VacationResponse/set", {"accountId": "A", "updated": {"singleton": null}}, "0"],
                ["VacationResponse/get", {"accountId": "A", "state": "s1", "list": [singleton()], "notFound": []}, "1"]
            ],
            "sessionState": "x"
        });
        let (base, seen) = api_server(reply).await;
        let c = client(&base, true);
        let v = Vacation {
            enabled: true,
            from_date: Some(1_791_878_400_000),
            to_date: None,
            subject: "İzindeyim".into(),
            body: "Pazartesi dönüyorum.".into(),
        };
        assert_eq!(set_vacation(&c, &v).await.unwrap(), v);
        let sent: Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
        assert!(sent["using"].as_array().unwrap().contains(&json!(VACATION)));
        assert_eq!(
            sent["methodCalls"][0],
            json!(["VacationResponse/set", {"accountId": "A", "update": {"singleton": patch(&v)}}, "0"])
        );
        assert_eq!(
            sent["methodCalls"][1],
            json!(["VacationResponse/get", {"accountId": "A", "ids": ["singleton"]}, "1"])
        );
    }

    #[tokio::test]
    async fn kaydedilmezse_hata_doner() {
        let reply = json!({
            "methodResponses": [
                ["VacationResponse/set", {"accountId": "A", "notUpdated": {"singleton": {"type": "invalidProperties"}}}, "0"],
                ["VacationResponse/get", {"accountId": "A", "state": "s1", "list": [], "notFound": ["singleton"]}, "1"]
            ]
        });
        let (base, _) = api_server(reply).await;
        let e = set_vacation(&client(&base, true), &Vacation::default())
            .await
            .unwrap_err()
            .to_app();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("invalidProperties"), "{}", e.message);
    }

    #[tokio::test]
    async fn yetenek_yoksa_desteklenmez() {
        let (base, seen) = api_server(json!({})).await;
        let c = client(&base, false);
        assert_eq!(get_vacation(&c).await.unwrap(), None);
        let e = set_vacation(&c, &Vacation::default())
            .await
            .unwrap_err()
            .to_app();
        assert_eq!(e, unsupported());
        assert!(seen.lock().unwrap().is_empty(), "sunucuya gidilmedi");
        // Yetenek varsa ve nesne yoksa kapalı okunur.
        let reply = json!({"methodResponses": [
            ["VacationResponse/get", {"accountId": "A", "state": "s", "list": [], "notFound": ["singleton"]}, "0"]
        ]});
        let (base, _) = api_server(reply).await;
        assert_eq!(
            get_vacation(&client(&base, true)).await.unwrap(),
            Some(Vacation::default())
        );
    }

    #[tokio::test]
    async fn imap_ve_tanitim_hesabinda_kapali() {
        let store = Store::open_in_memory().unwrap();
        let secrets: Arc<dyn SecretStore> = Arc::new(mh_sync::MemorySecrets::new());
        let mut a = Account {
            id: "a".into(),
            name: "İş".into(),
            email: "a@ornek.com".into(),
            display_name: "A".into(),
            color: "#4C6FD8".into(),
            kind: mh_core::AccountKind::Imap,
            imap: None,
            smtp: None,
            jmap_url: Some("https://posta.ornek.com/jmap".into()),
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
        };
        assert_eq!(vacation_get(&store, &secrets, &a).await.unwrap(), None);
        assert_eq!(
            vacation_set(&store, &secrets, &a, Vacation::default())
                .await
                .unwrap_err(),
            unsupported()
        );
        a.kind = mh_core::AccountKind::Jmap;
        a.jmap_url = Some("https://demo.invalid/jmap".into());
        assert_eq!(vacation_get(&store, &secrets, &a).await.unwrap(), None);
    }
}
