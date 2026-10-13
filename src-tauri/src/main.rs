//! Mailhere masaüstü kabuğu: pencereler, tepsi, komutlar ve olaylar.

// Windows'ta sürüm derlemesinde konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cmd_account;
mod cmd_cert;
mod cmd_drafts;
mod cmd_jmap;
mod cmd_oauth;
mod commands;
mod notify;
mod secrets;
mod sink;
mod state;
mod tray;
mod updater;
mod windows;

use std::sync::atomic::Ordering;

use tauri::{Manager, RunEvent};
use tracing::Level;
use tracing_subscriber::filter::{filter_fn, FilterExt};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

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
fn log_allowed(target: &str, level: Level) -> bool {
    let protocol = PROTOCOL_CRATES.iter().any(|c| {
        target
            .strip_prefix(c)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    });
    !protocol || level <= Level::INFO
}

fn init_logging() {
    let env =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,html5ever=warn"));
    let cap = filter_fn(|meta| log_allowed(meta.target(), *meta.level()));
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_filter(env.and(cap)),
        )
        .try_init();
}

fn main() {
    init_logging();
    // Tüm TLS istemcileri ring kullanır; yine de varsayılan sağlayıcıyı sabitle.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("mailhere")
        .build()
        .expect("eşzamansız çalışma ortamı kurulamadı");
    tauri::async_runtime::set(runtime.handle().clone());
    let rt = runtime.handle().clone();

    let app = tauri::Builder::default()
        // İkinci açılış yeni süreç başlatmaz; ana pencereyi öne getirir.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_main(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(cmd_oauth::OAuthState::default())
        .setup(move |app| state::setup(app, rt.clone()))
        .on_window_event(windows::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::accounts_list,
            commands::account_discover,
            commands::account_add,
            commands::account_remove,
            commands::mailboxes_list,
            commands::counts,
            commands::messages_list,
            commands::message_get,
            commands::messages_flag,
            commands::messages_move,
            commands::messages_snooze,
            commands::search,
            commands::remote_allow,
            commands::attachment_open,
            commands::send,
            commands::send_cancel,
            commands::settings_get,
            commands::settings_set,
            commands::sync_now,
            commands::open_main,
            commands::open_quick,
            commands::open_compose,
            commands::notify_layout,
            commands::notify_hide,
            commands::notify_pending,
            commands::notify_snooze,
            commands::demo_notify,
            cmd_jmap::jmap_discover,
            cmd_jmap::account_add_jmap,
            cmd_account::account_update,
            cmd_drafts::draft_save,
            cmd_drafts::draft_delete,
            updater::update_check,
            updater::update_install,
            cmd_oauth::oauth_providers,
            cmd_oauth::oauth_sign_in,
            cmd_oauth::account_add_oauth,
            cmd_cert::cert_inspect,
            cmd_cert::cert_trust,
        ])
        .build(tauri::generate_context!())
        .expect("Mailhere başlatılamadı");

    app.run(|app, event| {
        // Son pencere kapansa da uygulama tepside yaşar; yalnız "Çık" kapatır.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            let quitting = app
                .try_state::<state::AppState>()
                .is_some_and(|s| s.quitting.load(Ordering::SeqCst));
            if !quitting {
                api.prevent_exit();
            }
        }
    });
    drop(runtime);
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
