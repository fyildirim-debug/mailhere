//! Mailhere masaüstü kabuğu: pencereler, tepsi, komutlar ve olaylar.

// Windows'ta sürüm derlemesinde konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cmd_account;
mod cmd_drafts;
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
use tracing_subscriber::EnvFilter;

fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,html5ever=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
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
            cmd_account::account_update,
            cmd_drafts::draft_save,
            cmd_drafts::draft_delete,
            updater::update_check,
            updater::update_install,
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
