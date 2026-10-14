//! Açılış kurulumu: günlük süzgeci ve eşzamansız çalışma ortamı.
//!
//! Kabuk kendi günlük katmanını (masaüstünde `fmt`, Android'de logcat) verir;
//! süzgeç ortaktır ve gizli bilgi sızdırmaz.

use tracing::Level;
use tracing_subscriber::filter::{filter_fn, FilterExt};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// Ayrıntılı (debug/trace) günlükte ham protokol trafiğini yazan kitaplıklar:
/// `LOGIN kullanıcı "parola"`, `AUTH PLAIN …`, jeton gövdeleri. RUST_LOG ne
/// derse desin bunlarda info altı kapalıdır.
const PROTOCOL_CRATES: &[&str] = &[
    "async_imap",
    "imap_proto",
    "lettre",
    "rustls",
    "tokio_rustls",
    "reqwest",
    "hyper",
    "hyper_util",
    "h2",
];

/// Bu hedef ve düzeydeki kayıt yazılabilir mi (gizli bilgi sızdırmasın)?
pub fn log_allowed(target: &str, level: Level) -> bool {
    let protocol = PROTOCOL_CRATES.iter().any(|c| {
        target
            .strip_prefix(c)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    });
    !protocol || level <= Level::INFO
}

/// Verilen katmanı `RUST_LOG` (yoksa `info,html5ever=warn`) ve protokol
/// süzgeciyle kurar. İkinci çağrı sessizce yok sayılır.
pub fn init<L>(layer: L)
where
    L: Layer<Registry> + Send + Sync + 'static,
{
    let env =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,html5ever=warn"));
    let cap = filter_fn(|meta| log_allowed(meta.target(), *meta.level()));
    let _ = tracing_subscriber::registry()
        .with(layer.with_filter(env.and(cap)))
        .try_init();
}

/// Çok iş parçacıklı tokio ortamı; Tauri'nin eşzamansız çalışma zamanı da
/// buna bağlanır. Tüm TLS istemcileri ring kullanır; varsayılan sağlayıcı
/// yine de sabitlenir. Dönen değer sürecin sonuna dek yaşatılmalıdır.
pub fn runtime() -> tokio::runtime::Runtime {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("mailhere")
        .build()
        .expect("eşzamansız çalışma ortamı kurulamadı");
    tauri::async_runtime::set(runtime.handle().clone());
    runtime
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protokol_kitapliklari_ayrintili_yazmaz() {
        for t in [
            "async_imap",
            "async_imap::imap_stream",
            "lettre::transport::smtp",
        ] {
            assert!(!log_allowed(t, Level::TRACE), "{t}");
            assert!(!log_allowed(t, Level::DEBUG), "{t}");
            assert!(log_allowed(t, Level::INFO), "{t}");
            assert!(log_allowed(t, Level::WARN), "{t}");
        }
        assert!(log_allowed("mh_sync::engine", Level::TRACE));
        assert!(
            log_allowed("async_imapx", Level::TRACE),
            "yalnız tam ad eşleşir"
        );
    }
}
