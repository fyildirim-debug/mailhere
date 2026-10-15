//! Uykudan ve arka plandan dönüş: yarı ölü bağlantılar beklenmeden yenilenir.
//!
//! Dizüstü uykudan ya da telefon arka plandan dönünce IDLE ve komut
//! bağlantıları çoğunlukla ölüdür ama TCP bunu dakikalarca fark etmez; yeni
//! posta ancak yoklamada ya da IDLE yenilemesinde gelirdi. Bağlantılar baştan
//! kurulur; kuyruktaki sunucu işlemleri kalır, yeni bağlantıda uygulanır.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use mh_core::AppResult;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// İki yeniden kurma arası en az bu kadar; arada gelen istek yalnız eşitler.
const MIN_GAP: Duration = Duration::from_secs(15);
/// Uyku izleyicisinin adımı.
const TICK: Duration = Duration::from_secs(30);
/// Adım bundan geç gelirse sistem uyumuş (ya da süreç dondurulmuş) sayılır.
const SLEPT: Duration = Duration::from_secs(90);

#[derive(Default)]
pub struct Reconnect {
    last: Mutex<Option<Instant>>,
}

impl Reconnect {
    /// Sıklık sınırı: son kurmadan bu yana `MIN_GAP` geçtiyse `true` döner ve
    /// zamanı işaretler.
    fn due(&self, now: Instant) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| now.duration_since(t) < MIN_GAP) {
            return false;
        }
        *last = Some(now);
        true
    }
}

/// Tüm hesapların bağlantılarını baştan kurar; sıklık sınırına takılırsa
/// yalnız eşitler. `why` günlük içindir.
pub fn reconnect(state: &AppState, why: &str) -> AppResult<()> {
    if !state.reconnect.due(Instant::now()) {
        state.sync.sync_now();
        state.jmap.sync_now();
        return Ok(());
    }
    tracing::info!(why, "bağlantılar yeniden kuruluyor");
    // Biri başarısız olsa da öteki yeniden kurulur.
    let imap = state.sync.start_all();
    let jmap = state.jmap.start_all();
    imap.and(jmap)
}

/// İki ölçümden büyük olanı: Linux'ta tekdüze saat uykuda durur (duvar
/// saati sıçrar), Windows'ta ilerleyebilir; ikisine de bakılır. Geri alınan
/// duvar saati sıfır sayılır.
fn gap(mono: Duration, wall_from: SystemTime, wall_to: SystemTime) -> Duration {
    mono.max(wall_to.duration_since(wall_from).unwrap_or_default())
}

/// Uyku izleyicisi: adım geç gelirse bağlantılar yenilenir.
pub async fn sleep_watch(app: AppHandle) {
    sleep_watch_with(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(e) = reconnect(&state, "uyku") {
            tracing::warn!(error = %e.message, "uykudan sonra bağlantılar yenilenemedi");
        }
    })
    .await
}

/// Uyku izleyicisinin kendisi: adım `SLEPT`ten geç gelirse `on_wake`
/// çağrılır. Pencereli çekirdek `AppHandle` ile, penceresiz çekirdek
/// (`crate::headless`) doğrudan durumla kullanır.
pub async fn sleep_watch_with(on_wake: impl Fn() + Send) {
    let (mut mono, mut wall) = (Instant::now(), SystemTime::now());
    loop {
        tokio::time::sleep(TICK).await;
        let (m, w) = (Instant::now(), SystemTime::now());
        let late = gap(m.duration_since(mono), wall, w);
        (mono, wall) = (m, w);
        if late >= SLEPT {
            on_wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siklik_siniri() {
        let r = Reconnect::default();
        let t = Instant::now();
        assert!(r.due(t));
        assert!(!r.due(t + Duration::from_secs(5)));
        assert!(r.due(t + MIN_GAP));
    }

    #[test]
    fn uyku_iki_saatten_biriyle_anlasilir() {
        let w = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        // Linux: tekdüze saat 30 s, duvar saati 10 dk ilerlemiş.
        assert_eq!(
            gap(TICK, w, w + Duration::from_secs(600)),
            Duration::from_secs(600)
        );
        // Windows: tekdüze saat uykuyu da saymış.
        assert_eq!(
            gap(Duration::from_secs(600), w, w + TICK),
            Duration::from_secs(600)
        );
        // Duvar saati geri alınmış: tekdüze ölçü geçerli.
        assert_eq!(gap(TICK, w, w - Duration::from_secs(3600)), TICK);
    }
}
