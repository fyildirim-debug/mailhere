//! Durum değişikliği akışı (RFC 8620 §7.3, EventSource / text/event-stream).

use std::time::Duration;

use reqwest::header::{ACCEPT, CACHE_CONTROL};
use reqwest::StatusCode;
use serde_json::Value;

use crate::client::{http_for, Client};
use crate::error::{JmapError, JmapResult};

/// Tek satırın üst sınırı: bozuk ya da kötü niyetli akış belleği şişirmesin.
const MAX_LINE: usize = 1024 * 1024;
/// Bir olayın biriken verisinin üst sınırı. Aşan olayın verisi atılır.
const MAX_EVENT_DATA: usize = 1024 * 1024;

/// Sunucudan gelen bir olay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

/// Satır satır gelen `text/event-stream` ayrıştırıcısı.
///
/// Bellek sınırlıdır: satır `MAX_LINE`, olay verisi `MAX_EVENT_DATA` baytı
/// aşamaz. Sınırı aşan olay boş veriyle iletilir; `state` olayıysa
/// `touches_account` boş veriyi anlayamadığından emin olmak için eşitletir.
#[derive(Debug, Default)]
pub struct SseParser {
    /// Henüz bitmemiş satır.
    buf: Vec<u8>,
    /// Bu satır sınırı aştı; satır sonuna kadar gelen baytlar atılır.
    skip_line: bool,
    event: String,
    data: String,
    has_data: bool,
    /// Bu olayın bir kısmı sınır yüzünden atıldı.
    overflow: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yeni baytları ekler; tamamlanan olayları döner.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        let mut rest = chunk;
        while !rest.is_empty() {
            let (part, ends_line) = match rest.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    let part = &rest[..pos];
                    rest = &rest[pos + 1..];
                    (part, true)
                }
                None => (std::mem::take(&mut rest), false),
            };
            if !self.skip_line {
                if self.buf.len() + part.len() > MAX_LINE {
                    self.buf = Vec::new();
                    self.skip_line = true;
                    self.overflow = true;
                } else {
                    self.buf.extend_from_slice(part);
                }
            }
            if ends_line {
                if std::mem::take(&mut self.skip_line) {
                    continue;
                }
                let line = std::mem::take(&mut self.buf);
                self.line(&line, &mut out);
            }
        }
        out
    }

    fn line(&mut self, line: &[u8], out: &mut Vec<SseEvent>) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            self.dispatch(out);
            return;
        }
        if line.starts_with(b":") {
            return;
        }
        let line = String::from_utf8_lossy(line);
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line.as_ref(), ""),
        };
        match field {
            "event" => self.event = value.to_owned(),
            "data" if self.overflow => {}
            "data" => {
                let extra = value.len() + usize::from(self.has_data);
                if self.data.len() + extra > MAX_EVENT_DATA {
                    self.data = String::new();
                    self.overflow = true;
                    return;
                }
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            _ => {}
        }
    }

    /// Boş satır: biriken olayı iletir ve durumu sıfırlar.
    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        let event = std::mem::take(&mut self.event);
        let data = std::mem::take(&mut self.data);
        if self.has_data || self.overflow {
            out.push(SseEvent {
                event: if event.is_empty() {
                    "message".into()
                } else {
                    event
                },
                // Sınırı aşan olayın verisi güvenilmez; boş iletilir.
                data: if self.overflow { String::new() } else { data },
            });
        }
        self.has_data = false;
        self.overflow = false;
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
    // Adres oturumla aynı kökendedir (`Client::resolve`); yönlendirme
    // izlenmez, parola başka sunucuya gitmez.
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
    fn buyuk_olay_verisi_sinirlanir_ve_esitletir() {
        let mut p = SseParser::new();
        // Her satır kısa ama toplam veri sınırı aşar: biriken veri atılır.
        let line = format!("data: {}\n", "x".repeat(64 * 1024));
        assert!(p.feed(b"event: state\n").is_empty());
        for _ in 0..(MAX_EVENT_DATA / (64 * 1024) + 4) {
            assert!(p.feed(line.as_bytes()).is_empty());
            assert!(p.data.len() <= MAX_EVENT_DATA, "veri sınırı aşılmaz");
        }
        let ev = p.feed(b"\n");
        assert_eq!(
            ev,
            vec![SseEvent {
                event: "state".into(),
                data: String::new()
            }],
            "aşan olay boş veriyle iletilir"
        );
        assert!(
            touches_account(&ev[0].data, "A"),
            "boş veri emin olmak için eşitletir"
        );
        // Sonraki olay normal işlenir.
        let ev = p.feed(b"event: state\ndata: {\"changed\":{}}\n\n");
        assert_eq!(ev[0].data, "{\"changed\":{}}");
    }

    #[test]
    fn cok_uzun_satir_atilir() {
        let mut p = SseParser::new();
        assert!(p.feed(b"event: state\ndata: ").is_empty());
        let chunk = vec![b'y'; 256 * 1024];
        for _ in 0..(MAX_LINE / chunk.len() + 4) {
            assert!(p.feed(&chunk).is_empty());
            assert!(p.buf.len() <= MAX_LINE, "satır tamponu sınırı aşılmaz");
        }
        // Satırın kalanı yeni bir alan diye okunmaz.
        let ev = p.feed(b"data: sahte\n\n");
        assert_eq!(
            ev,
            vec![SseEvent {
                event: "state".into(),
                data: String::new()
            }]
        );
        let ev = p.feed(b"data: {\"a\":1}\n\n");
        assert_eq!(ev[0].data, "{\"a\":1}");
        assert_eq!(ev[0].event, "message");
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
