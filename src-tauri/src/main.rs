//! Mailhere masaüstü kabuğu: pencereler, tepsi, bildirim kartı, anahtar
//! zinciri, güncelleme. Komutlar ve çekirdek `mh-app`tedir.

// Windows'ta sürüm derlemesinde konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod files;
mod links;
mod logfile;
mod notify_card;
mod platform;
mod prefs;
mod secrets;
mod tray;
mod updater;
mod windows;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use mh_app::state::{demo_mode, dev_env, memory_mode};
use mh_app::{Dirs, Platform};
use mh_sync::{MemorySecrets, SecretStore};
use tauri::{App, AppHandle, Manager, RunEvent};

use crate::platform::{DesktopPlatform, DesktopState};

/// İşletim sisteminin *yerel* uygulama veri klasörü. Windows'ta
/// `app_data_dir` dolaşan profildir (Roaming): posta veritabanı profil
/// sunucusuna kopyalanmasın diye `app_local_data_dir` kullanılır. Linux ve
/// macOS'ta ikisi aynı klasördür. Önceki sürümlerin dolaşan profildeki
/// veritabanı açılışta taşınır.
fn os_dirs(app: &AppHandle) -> Result<Dirs, Box<dyn std::error::Error>> {
    let data = app.path().app_local_data_dir()?;
    Ok(Dirs {
        // Windows'ta da yerel klasör (%LOCALAPPDATA%).
        cache: app
            .path()
            .app_cache_dir()
            .unwrap_or_else(|_| data.join("cache")),
        legacy_db: Some(app.path().app_data_dir()?.join("mailhere.db")),
        data,
    })
}

fn setup(app: &mut App, rt: tokio::runtime::Handle) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    // Açılışta başlama kaydı (ilk çalıştırma varsayılanı, yol tazeleme);
    // veritabanından bağımsız, çekirdekten önce.
    autostart::init(&handle);
    // Tanıtım ve deneme kipi: parolalar ve veritabanı anahtarı bellekte.
    let memory = memory_mode();
    let secrets: Arc<dyn SecretStore> = if memory {
        tracing::info!("parolalar ve veritabanı anahtarı bellekte (tanıtım ya da test kipi)");
        Arc::new(MemorySecrets::new())
    } else {
        Arc::new(secrets::KeyringSecrets)
    };
    let dirs = Dirs::resolve(memory, || os_dirs(&handle))?;
    if !memory {
        logfile::open(&dirs.data);
    }
    let platform = Arc::new(DesktopPlatform);
    let boot = mh_app::lock::Boot {
        rt: rt.clone(),
        dirs,
        secrets,
        platform: platform.clone(),
        kdf: mh_app::lock::KdfParams::DESKTOP,
    };
    // Parola kilidi açıksa çekirdek parola girilene kadar açılmaz; pencere
    // kilit ekranını gösterir, `lock_unlock` çekirdeği başlatır.
    match mh_app::lock::prepare(&handle, boot) {
        Ok(Some(boot)) => {
            // Hata iletişim kutusu gösterildi; pencere açılmaz, kutu kapanınca çıkılır.
            if mh_app::setup_core(&handle, boot, None).is_err() {
                return Ok(());
            }
        }
        Ok(None) => {}
        Err(e) => {
            platform.fatal(&handle, &e);
            return Ok(());
        }
    }

    // Açılış kaydından (`--tepside`) gelindiyse pencere açılmaz, yalnız tepsi.
    let hidden = autostart::hidden_start();
    if !hidden {
        windows::create_main(&handle)?;
    }
    if let Err(e) = tray::create(&handle) {
        tracing::warn!(error = %e, "tepsi simgesi oluşturulamadı");
    }
    if hidden {
        tray::started_hidden(&handle);
    }
    updater::start(&handle, &rt);
    tray::refresh(&handle);
    if mh_app::lock::is_locked(&handle) {
        platform::locked_start(&handle, hidden);
    }

    // Tanıtım kipinde ekran görüntüsü otomasyonu: N saniye sonra örnek kart.
    let demo_card = dev_env("MAILHERE_DEMO_NOTIFY").and_then(|v| v.trim().parse::<u64>().ok());
    if let (true, Some(secs)) = (demo_mode(), demo_card) {
        let app = handle.clone();
        rt.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            if let Err(e) = notify_card::show_demo_card(&app) {
                tracing::warn!(error = %e.message, "tanıtım kartı açılamadı");
            }
        });
    }
    Ok(())
}

fn main() {
    // Ortam değişkeni, iş parçacıkları başlamadan.
    notify_card::choose_backend();
    use tracing_subscriber::Layer;
    mh_app::logging::init(
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .and_then(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_ansi(false)
                    .with_writer(|| logfile::Writer),
            ),
    );
    let runtime = mh_app::logging::runtime();
    let rt = runtime.handle().clone();

    let app = tauri::Builder::default()
        // İkinci açılış yeni süreç başlatmaz; ana pencereyi öne getirir
        // (açılış kaydından gelen `--tepside` getirmez).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !autostart::is_hidden_args(args.iter().skip(1)) {
                windows::show_main(app);
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(DesktopState::default())
        .setup(move |app| setup(app, rt.clone()))
        .on_window_event(windows::on_window_event)
        // Ortak komutlar + yalnız masaüstü komutları (`commands.txt`);
        // kilitliyken `lock_*` dışındakiler kapıda reddedilir.
        .invoke_handler(mh_app::lock::gate(mh_app::shared_commands![
            windows::open_quick,
            notify_card::notify_layout,
            notify_card::notify_hide,
            notify_card::notify_focus,
            notify_card::notify_pending,
            notify_card::demo_notify,
            updater::update_check,
            updater::update_install,
            autostart::autostart_get,
            autostart::autostart_set,
            links::link_open,
        ]))
        .build(tauri::generate_context!())
        .expect("Mailhere başlatılamadı");

    app.run(|app, event| {
        // Son pencere kapansa da uygulama tepside yaşar; yalnız "Çık" kapatır.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            let quitting = app
                .try_state::<DesktopState>()
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
    use std::collections::BTreeSet;

    /// Yalnız masaüstü komutları (`commands.txt`).
    fn desktop_commands() -> Vec<&'static str> {
        include_str!("../commands.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    }

    /// `capabilities/*.json` dosyalarındaki uygulama komutu izinleri
    /// (`allow-<ad>`; eklenti izinleri `:` taşır ve sayılmaz).
    fn allowed() -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for file in [
            include_str!("../capabilities/main.json"),
            include_str!("../capabilities/notify.json"),
            include_str!("../capabilities/quick.json"),
            include_str!("../capabilities/compose.json"),
        ] {
            let cap: serde_json::Value = serde_json::from_str(file).expect("yetenek dosyası JSON");
            for p in cap["permissions"].as_array().expect("permissions dizisi") {
                let p = p.as_str().expect("izin adı");
                if let Some(name) = p.strip_prefix("allow-").filter(|_| !p.contains(':')) {
                    out.insert(name.replace('-', "_"));
                }
            }
        }
        out
    }

    #[test]
    fn masaustu_komutlari_ortaklarla_cakismaz_ve_bagli() {
        let desktop = desktop_commands();
        for name in &desktop {
            assert!(
                !mh_app::SHARED_COMMANDS.contains(name),
                "{name}: hem ortak hem masaüstü listesinde"
            );
            // Ad `shared_commands!` çağrısına `modül::ad,` biçiminde eklenmiş olmalı.
            assert!(
                include_str!("main.rs").contains(&format!("::{name},")),
                "{name}: main.rs'teki shared_commands! çağrısında yok"
            );
        }
    }

    #[test]
    fn her_komut_bir_pencerede_izinli_ve_her_izin_bir_komut() {
        let allowed = allowed();
        let known: BTreeSet<String> = mh_app::SHARED_COMMANDS
            .iter()
            .chain(desktop_commands().iter())
            .map(|s| s.to_string())
            .collect();
        for name in &known {
            assert!(
                allowed.contains(name),
                "{name}: hiçbir capabilities/*.json'da allow-* yok"
            );
        }
        for name in &allowed {
            assert!(known.contains(name), "allow-{name}: böyle bir komut yok");
        }
    }
}
