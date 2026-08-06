pub mod cli;
pub mod commands;
pub mod credman;
pub mod descriptors;
pub mod models;
pub mod registry;
pub mod scan;

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

fn toggle_popover(app: &AppHandle, tray_position: Option<PhysicalPosition<f64>>) {
    if let Some(window) = app.get_webview_window("popover") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            _ => {
                let _ = position_popover(&window, tray_position);
                let _ = window.show();
                let _ = window.set_focus();
                let _ = commands::mark_all_seen();
            }
        }
    }
}

fn position_popover(
    window: &WebviewWindow,
    tray_position: Option<PhysicalPosition<f64>>,
) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };
    let size = window.outer_size()?;
    let work_area = monitor.work_area();
    let margin = 10;
    let right = work_area.position.x + work_area.size.width as i32;
    let bottom = work_area.position.y + work_area.size.height as i32;
    let preferred_x = tray_position
        .map(|position| position.x.round() as i32 - size.width as i32 + 26)
        .unwrap_or(right - size.width as i32 - margin);
    let min_x = work_area.position.x + margin;
    let max_x = right - size.width as i32 - margin;
    let x = preferred_x.clamp(min_x, max_x.max(min_x));
    let y = bottom - size.height as i32 - margin;
    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}

fn configure_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open ConnLens", true, None::<&str>)?;
    let rescan = MenuItem::with_id(app, "rescan", "Rescan now", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause_watchers",
        "Pause watchers",
        true,
        false,
        None::<&str>,
    )?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Launch at startup",
        false,
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &rescan, &pause, &autostart, &quit])?;

    let mut tray = TrayIconBuilder::with_id("connlens")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), Some(position));
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => toggle_popover(app, None),
            "rescan" => {
                let _ = commands::rescan_internal(Some(app), None);
            }
            "pause_watchers" => {
                let paused = registry::with_registry(|registry| {
                    registry.file.settings.watchers_enabled =
                        !registry.file.settings.watchers_enabled;
                    registry.file.settings.watchers_enabled
                })
                .map(|enabled| !enabled)
                .unwrap_or(false);
                let _ = commands::update_watcher_state(paused);
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            toggle_popover(app, None);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            configure_tray(app.handle())?;
            let _ = commands::rescan_internal(Some(app.handle()), None);
            toggle_popover(app.handle(), None);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_custom_provider,
            commands::copy_value,
            commands::dismiss_history_reset_notice,
            commands::get_state,
            commands::mark_all_seen,
            commands::open_dashboard,
            commands::purge_missing,
            commands::remove,
            commands::rescan,
            commands::reset_app_data,
            commands::reveal_source,
            commands::update_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
