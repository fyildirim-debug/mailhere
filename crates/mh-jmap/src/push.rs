//! Durum değişikliği akışı (RFC 8620 §7.3, EventSource / text/event-stream).

use std::time::Duration;

use reqwest::header::{ACCEPT, CACHE_CONTROL};
use reqwest::StatusCode;
use serde_json::Value;

use crate::client::{http_for, Client};
use crate::error::{JmapError, JmapResult};

/// Tamponun üst sınırı: bozuk ya da kötü niyetli akış belleği şişirmesin.
const MAX_BUFFER: usize = 1024 * 1024;

/// Sunucudan gelen bir olay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

/// Satır satır gelen `text/event-stream` ayrıştırıcısı.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event: String,
    data: String,
    has_data: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yeni baytları ekler; tamamlanan olayları döner.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line);
            if line.is_empty() {
                if self.has_data {
                    out.push(SseEvent {
                        event: if self.event.is_empty() {
                            "message".into()
                        } else {
                            std::mem::take(&mut self.event)
                        },
                        data: std::mem::take(&mut self.data),
                    });
                }
                self.event.clear();
                self.data.clear();
                self.has_data = false;
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line.as_ref(), ""),
            };
            match field {
                "event" => self.event = value.to_owned(),
                "data" => {
                    if self.has_data {
                        self.data.push('\n');
                    }
                    self.data.push_str(value);
                    self.has_data = true;
                }
                _ => {}
            }
        }
        if self.buf.len() > MAX_BUFFER {
            self.buf.clear();
        }
        out
    }
}

/// `state` olayı bu hesabın posta ya da kutu durumunun değiştiğini söylüyor mu?
pub fn touches_account(data: &str, account_id: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        // Anlaşılmayan olay: emin olmak için eşitle.
        return true;
    };
    match v.get("changed").and_then(|c| c.get(account_id)) {
        Some(Value::Object(types)) => types
            .keys()
            .any(|k| matches!(k.as_str(), "Email" | "Mailbox" | "Thread" | "EmailDelivery")),
        Some(_) => true,
        None => false,
    }
}

/// Akışa bağlanır ve her değişiklikte `on_change` çağırır. Akış koparsa ya
/// da `idle` boyunca hiçbir bayt gelmezse hata döner. `on_connect` bağlantı
/// kurulunca bir kez çağrılır.
pub async fn listen(
    client: &Client,
    ping_secs: u32,
    idle: Duration,
    mut on_connect: impl FnMut(),
    mut on_change: impl FnMut(),
) -> JmapResult<()> {
    let url = client
        .event_source_url("Email,Mailbox", ping_secs)
        .ok_or_else(|| JmapError::Protocol("sunucu durum akışı sunmuyor".into()))?;
    let rb = http_for(&url)
        .get(url)
        .header(ACCEPT, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache");
    let creds = client.credentials();
    let mut resp = rb
        .basic_auth(&creds.username, Some(creds.password()))
        .send()
        .await?;
    match resp.status() {
        s if s.is_success() => {}
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(JmapError::Auth),
        s => return Err(JmapError::Status(s.as_u16())),
    }
    on_connect();
    let mut parser = SseParser::new();
    let account = client.account_id().to_owned();
    loop {
        let chunk = tokio::time::timeout(idle, resp.chunk()).await??;
        let Some(bytes) = chunk else {
            return Err(JmapError::Protocol("durum akışı kapandı".into()));
        };
        for ev in parser.feed(&bytes) {
            if ev.event == "state" && touches_account(&ev.data, &account) {
                on_change();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn olaylar_parca_parca_gelir() {
        let mut p = SseParser::new();
        assert!(p.feed(b"event: ping\r\ndata: {\"interval\"").is_empty());
        let ev = p.feed(b": 60}\r\n\r\n: yorum\nevent: state\ndata: {\"a\":1}\n");
        assert_eq!(
            ev,
            vec![SseEvent {
                event: "ping".into(),
                data: "{\"interval\": 60}".into()
            }]
        );
        let ev = p.feed(b"data: {\"b\":2}\n\n\n");
        assert_eq!(
            ev,
            vec![SseEvent {
                event: "state".into(),
                data: "{\"a\":1}\n{\"b\":2}".into()
            }]
        );
        // Veri olmayan olay yok sayılır; olay adı sonrakine taşınmaz.
        assert!(p.feed(b"event: bos\n\ndata: x\n\n")[0].event == "message");
    }

    #[test]
    fn hesap_degisikligi() {
        let d = r#"{"@type":"StateChange","changed":{"A":{"Email":"s2"},"B":{"Mailbox":"m"}}}"#;
        assert!(touches_account(d, "A"));
        assert!(touches_account(d, "B"));
        assert!(!touches_account(d, "C"));
        let d = r#"{"@type":"StateChange","changed":{"A":{"CalendarEvent":"x"}}}"#;
        assert!(!touches_account(d, "A"));
        assert!(touches_account("bozuk", "A"));
    }
}
