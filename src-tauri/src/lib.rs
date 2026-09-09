pub mod cleanup;
pub mod cli;
pub mod commands;
pub mod credman;
pub mod descriptors;
pub mod models;
pub mod registry;
pub mod scan;
pub mod watchers;

use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Default)]
struct PopoverState {
    last_blur: Mutex<Option<Instant>>,
}

struct TrayMenuState {
    pause: CheckMenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
}

pub(crate) fn emit_visible_snapshot(app: &AppHandle, snapshot: &models::Snapshot) {
    // show_popover sends a fresh snapshot on every open. Avoid serializing and
    // rendering background scans in the hidden webview between those opens.
    if app
        .get_webview_window("popover")
        .is_some_and(|window| window.is_visible().unwrap_or(true))
    {
        let _ = app.emit("state://updated", snapshot);
    }
}

#[tauri::command]
fn get_platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[cfg(target_os = "macos")]
fn configure_macos_app(app: &AppHandle) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    let _ = app.set_dock_visibility(false);
}

#[cfg(not(target_os = "macos"))]
fn configure_macos_app(_app: &AppHandle) {}

fn show_popover(app: &AppHandle, tray_position: Option<PhysicalPosition<f64>>) {
    if let Some(window) = app.get_webview_window("popover") {
        let _ = position_popover(&window, tray_position.or_else(|| tray_center(app)));
        let _ = window.show();
        let _ = window.set_focus();
        if let Ok(snapshot) = commands::mark_all_seen() {
            emit_visible_snapshot(app, &snapshot);
        }
    }
}

fn toggle_popover(app: &AppHandle, tray_position: Option<PhysicalPosition<f64>>) {
    if let Some(window) = app.get_webview_window("popover") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            _ => {
                // On macOS a tray click can first deliver a focus-loss event. Do
                // not reopen the popover that the same click just dismissed.
                let just_blurred = app
                    .state::<PopoverState>()
                    .last_blur
                    .lock()
                    .ok()
                    .and_then(|last| *last)
                    .is_some_and(|last| last.elapsed() < Duration::from_millis(200));
                if !just_blurred {
                    show_popover(app, tray_position);
                }
            }
        }
    }
}

fn tray_center(app: &AppHandle) -> Option<PhysicalPosition<f64>> {
    let rect = app.tray_by_id("connlens")?.rect().ok()??;
    // Tauri's tray rectangle is physical on macOS and Windows. Linux does
    // not expose its tray rectangle, so positioning falls back to a monitor.
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    Some(PhysicalPosition::new(
        position.x + size.width / 2.0,
        position.y + size.height / 2.0,
    ))
}

fn position_popover(
    window: &WebviewWindow,
    tray_position: Option<PhysicalPosition<f64>>,
) -> tauri::Result<()> {
    let Some(monitor) = popover_monitor(window, tray_position)? else {
        return Ok(());
    };
    let size = window.outer_size()?;
    let work_area = monitor.work_area();
    let margin = (10.0 * monitor.scale_factor()).round() as i32;
    let position = popover_position(
        work_area.position,
        work_area.size,
        size,
        tray_position,
        margin,
        cfg!(target_os = "macos"),
    );
    window.set_position(position)
}

fn popover_position(
    origin: PhysicalPosition<i32>,
    work_size: PhysicalSize<u32>,
    size: PhysicalSize<u32>,
    tray_position: Option<PhysicalPosition<f64>>,
    margin: i32,
    macos: bool,
) -> PhysicalPosition<i32> {
    // Clamp both dimensions, including monitors smaller than the fixed panel.
    let inset_x = margin.min((work_size.width.saturating_sub(size.width) / 2) as i32);
    let inset_y = margin.min((work_size.height.saturating_sub(size.height) / 2) as i32);
    let min_x = origin.x + inset_x;
    let max_x = origin.x + work_size.width.saturating_sub(size.width) as i32 - inset_x;
    let preferred_x = tray_position
        .map(|position| position.x.round() as i32 - (size.width as i32 / 2))
        .unwrap_or(max_x);
    let x = preferred_x.clamp(min_x, max_x.max(min_x));
    let use_top_edge = tray_position
        .map(|position| {
            let y = position.y.round() as i32;
            y <= origin.y + (work_size.height as i32 / 2)
        })
        .unwrap_or(macos);
    let y = if use_top_edge {
        // The macOS panel includes its pointer at the top of the webview.
        origin.y + if macos { 0 } else { inset_y }
    } else {
        origin.y + work_size.height.saturating_sub(size.height) as i32 - inset_y
    };
    PhysicalPosition::new(x, y.max(origin.y))
}

fn popover_monitor(
    window: &WebviewWindow,
    tray_position: Option<PhysicalPosition<f64>>,
) -> tauri::Result<Option<Monitor>> {
    if let Some(position) = tray_position {
        let x = position.x.round() as i32;
        let y = position.y.round() as i32;
        if let Some(monitor) = window.available_monitors()?.into_iter().find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            let right = position.x + size.width as i32;
            let bottom = position.y + size.height as i32;
            x >= position.x && x < right && y >= position.y && y < bottom
        }) {
            return Ok(Some(monitor));
        }
    }

    match window.current_monitor()? {
        Some(monitor) => Ok(Some(monitor)),
        None => window.primary_monitor(),
    }
}

pub(crate) fn sync_tray_settings(app: &AppHandle, settings: &models::Settings) {
    if let Some(menu) = app.try_state::<TrayMenuState>() {
        let _ = menu.pause.set_checked(!settings.watchers_enabled);
        let _ = menu.autostart.set_checked(settings.autostart);
    }
}

fn configure_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = registry::load_snapshot()
        .ok()
        .map(|snapshot| snapshot.settings);
    let open = MenuItem::with_id(app, "open", "Open ConnLens", true, None::<&str>)?;
    let rescan = MenuItem::with_id(app, "rescan", "Rescan now", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause_watchers",
        "Pause watchers",
        true,
        settings
            .as_ref()
            .is_some_and(|settings| !settings.watchers_enabled),
        None::<&str>,
    )?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Launch at startup",
        true,
        commands::read_autostart(app).unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &rescan, &pause, &autostart, &quit])?;

    let mut tray = TrayIconBuilder::with_id("connlens")
        .tooltip("ConnLens")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "linux"))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let position = rect.position.to_physical::<f64>(1.0);
                let size = rect.size.to_physical::<f64>(1.0);
                toggle_popover(
                    tray.app_handle(),
                    Some(PhysicalPosition::new(
                        position.x + size.width / 2.0,
                        position.y + size.height / 2.0,
                    )),
                );
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_popover(app, None),
            "rescan" => {
                let _ = commands::rescan_internal(Some(app), None);
            }
            "pause_watchers" => {
                if let Ok(current) = registry::load_snapshot() {
                    if let Ok(snapshot) =
                        commands::update_watcher_state(current.settings.watchers_enabled)
                    {
                        sync_tray_settings(app, &snapshot.settings);
                        emit_visible_snapshot(app, &snapshot);
                    }
                }
            }
            "autostart" => {
                let next = !commands::read_autostart(app).unwrap_or(false);
                let _ = commands::set_autostart(app, next);
                if let Ok(snapshot) = registry::load_snapshot() {
                    sync_tray_settings(app, &snapshot.settings);
                    emit_visible_snapshot(app, &snapshot);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    app.manage(TrayMenuState { pause, autostart });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Migrate before Tauri, plugins, or WebKit can create the new app directory.
    // On case-insensitive macOS volumes its bundle ID and ProjectDirs path alias.
    registry::prepare_app_home().expect("could not prepare Nemu ConnLens app data");
    // A local tray utility does not need one async worker for every CPU core.
    // Keep the owner alive throughout Tauri's event loop, as required by set().
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .min(2),
        )
        .thread_name("connlens-async")
        .enable_all()
        .build()
        .expect("could not start background runtime");
    tauri::async_runtime::set(runtime.handle().clone());
    let mut context = tauri::generate_context!();
    if descriptors::ScanPaths::current().is_isolated() {
        let home = registry::app_home();
        let home = stable_fixture_home(&home);
        let identifier = fixture_identifier(&context.config().identifier, &home);
        context.config_mut().identifier = identifier;
    }
    tauri::Builder::default()
        .manage(PopoverState::default())
        .manage(cleanup::CleanupState::default())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_popover(app, None);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            configure_macos_app(app.handle());
            let _ = commands::sync_autostart_setting(app.handle());
            if configure_tray(app.handle()).is_err() {
                // Some Linux desktops have no tray host. Keep a reachable
                // regular taskbar window instead of failing startup entirely.
                if let Some(window) = app.get_webview_window("popover") {
                    let _ = window.set_skip_taskbar(false);
                }
            }
            let _ = commands::rescan_internal(Some(app.handle()), None);
            watchers::start(app.handle().clone())?;
            show_popover(app.handle(), None);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if window.app_handle().tray_by_id("connlens").is_some() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            tauri::WindowEvent::Focused(false) if cfg!(target_os = "macos") => {
                if let Ok(mut last) = window.app_handle().state::<PopoverState>().last_blur.lock() {
                    *last = Some(Instant::now());
                }
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_platform,
            quit_app,
            commands::add_custom_provider,
            commands::copy_value,
            commands::dismiss_history_reset_notice,
            commands::get_state,
            commands::review_cleanup,
            commands::execute_cleanup,
            commands::mark_all_seen,
            commands::open_dashboard,
            commands::open_external_url,
            commands::purge_missing,
            commands::remove,
            commands::rescan,
            commands::reset_app_data,
            commands::reveal_source,
            commands::update_settings
        ])
        .build(context)
        .expect("error while building tauri application")
        .run(|_app, event| match event {
            // Finder and `open` send a native reopen event to an already-running
            // macOS app instead of launching a second process for the singleton.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => show_popover(_app, None),
            _ => {}
        });
}

fn fixture_identifier(identifier: &str, home: &std::path::Path) -> String {
    // Single-instance plugins key their socket/mutex/D-Bus name by identifier.
    // A fixture must never activate or quit against the installed app instance.
    format!(
        "{identifier}.fixture{}",
        registry::short_hash(&[&home.to_string_lossy()])
    )
}

fn stable_fixture_home(home: &std::path::Path) -> std::path::PathBuf {
    let absolute = if home.is_absolute() {
        home.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(home)
    };
    // Resolve existing ancestors even before the fixture directory is created,
    // so /var and /private/var yield the same singleton key on every launch.
    for ancestor in absolute.ancestors() {
        if let Ok(resolved) = ancestor.canonicalize() {
            if let Ok(suffix) = absolute.strip_prefix(ancestor) {
                return resolved.join(suffix);
            }
        }
    }
    absolute
}

#[cfg(test)]
mod tests {
    use super::popover_position;
    use tauri::{PhysicalPosition as Point, PhysicalSize as Size};

    #[test]
    fn macos_is_centered_below_the_menu_bar() {
        assert_eq!(
            popover_position(
                Point::new(0, 50),
                Size::new(3024, 1914),
                Size::new(720, 1040),
                Some(Point::new(2500.0, 25.0)),
                20,
                true
            ),
            Point::new(2140, 50)
        );
    }

    #[test]
    fn windows_bottom_taskbar_respects_work_area() {
        assert_eq!(
            popover_position(
                Point::new(0, 0),
                Size::new(1920, 1040),
                Size::new(390, 520),
                Some(Point::new(1870.0, 1060.0)),
                10,
                false
            ),
            Point::new(1520, 510)
        );
    }

    #[test]
    fn linux_top_panel_and_negative_monitor_coordinates_are_supported() {
        assert_eq!(
            popover_position(
                Point::new(-1920, 30),
                Size::new(1920, 1050),
                Size::new(390, 520),
                Some(Point::new(-1910.0, 15.0)),
                10,
                false
            ),
            Point::new(-1910, 40)
        );
    }

    #[test]
    fn small_work_area_never_places_window_above_or_left_of_screen() {
        assert_eq!(
            popover_position(
                Point::new(50, 30),
                Size::new(300, 400),
                Size::new(390, 520),
                None,
                10,
                false
            ),
            Point::new(50, 30)
        );
    }

    #[test]
    fn fixture_single_instances_are_separate_from_the_installed_app() {
        let identifier = "com.nemu.connlens";
        let fixture_a = std::path::Path::new("/fixture/a");
        let fixture_b = std::path::Path::new("/fixture/b");
        assert_ne!(super::fixture_identifier(identifier, fixture_a), identifier);
        assert_eq!(
            super::fixture_identifier(identifier, fixture_a),
            super::fixture_identifier(identifier, fixture_a)
        );
        assert_ne!(
            super::fixture_identifier(identifier, fixture_a),
            super::fixture_identifier(identifier, fixture_b)
        );
    }

    #[test]
    fn fixture_home_identity_is_stable_before_and_after_directory_creation() {
        let parent = tempfile::tempdir().unwrap();
        let home = parent.path().join("new-fixture");
        let before = super::stable_fixture_home(&home);
        std::fs::create_dir(&home).unwrap();
        assert_eq!(before, super::stable_fixture_home(&home));
    }
}
