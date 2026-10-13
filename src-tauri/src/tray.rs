//! Bildirim alanı simgesi ve menüsü.

use std::sync::atomic::Ordering;

use mh_core::{ComposeMode, ComposePrefill};
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::{now_ms, AppState};

const TRAY_ID: &str = "main";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Mailhere'yi aç", true, None::<&str>)?;
    let compose = MenuItem::with_id(app, "compose", "Yeni posta", true, None::<&str>)?;
    let mute = MenuItem::with_id(
        app,
        "mute",
        "Bildirimleri 1 saat sessize al",
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Çık", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &compose, &mute, &sep, &quit])?;
    let icon = Image::from_bytes(include_bytes!("../icons/64x64.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Mailhere")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(on_click)
        .build(app)?;
    Ok(())
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        "open" => crate::windows::show_main(app),
        "compose" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let prefill = ComposePrefill {
                    mode: ComposeMode::New,
                    message_id: None,
                };
                if let Err(e) = crate::windows::open_compose(&app, &prefill) {
                    tracing::warn!(error = %e, "yazma penceresi açılamadı");
                }
            });
        }
        "mute" => {
            if let Some(s) = app.try_state::<AppState>() {
                s.mute.mute_for(now_ms(), 60);
            }
            let _ = crate::windows::notify_hide(app);
        }
        "quit" => {
            if let Some(s) = app.try_state::<AppState>() {
                s.quitting.store(true, Ordering::SeqCst);
            }
            app.exit(0);
        }
        _ => {}
    }
}

fn on_click(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        crate::windows::show_main(tray.app_handle());
    }
}

/// İpucu metni.
pub fn tooltip(unread: u32) -> String {
    if unread == 0 {
        "Mailhere · Hepsi okundu".to_owned()
    } else {
        format!("Mailhere · {unread} okunmamış")
    }
}

/// Okunmamış sayısını ipucuna yazar.
pub fn refresh(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let unread = state
        .store
        .counts(now_ms())
        .map(|c| c.inbox_unread)
        .unwrap_or(0);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip(unread)));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ipucu() {
        assert_eq!(super::tooltip(0), "Mailhere · Hepsi okundu");
        assert_eq!(super::tooltip(3), "Mailhere · 3 okunmamış");
    }
}
