use crate::descriptors;
use crate::models::{is_retired_provider_id, ErrorPayload, Settings, Snapshot};
use crate::registry;
use crate::scan;
use crate::scan::parsers::Format;
use arboard::Clipboard;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

pub type CommandResult<T> = Result<T, ErrorPayload>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderInput {
    pub id: String,
    pub name: String,
    pub dashboard_url: Option<String>,
    pub config_paths: Vec<String>,
    pub env_vars: Vec<String>,
    pub format: Format,
}

#[derive(Debug, Serialize)]
struct CustomProviderDescriptor {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dashboard_url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    locations: Vec<CustomProviderLocation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    env_vars: Vec<CustomProviderEnvVar>,
}

#[derive(Debug, Serialize)]
struct CustomProviderLocation {
    path: String,
    format: Format,
    strategy: &'static str,
    source_type: &'static str,
}

#[derive(Debug, Serialize)]
struct CustomProviderEnvVar {
    name: String,
    label: String,
}

#[tauri::command]
pub fn get_state(app: AppHandle) -> CommandResult<Snapshot> {
    snapshot_with_autostart(Some(&app))
}

#[tauri::command]
pub fn rescan(app: AppHandle, provider: Option<String>) -> CommandResult<Snapshot> {
    rescan_internal(Some(&app), provider)
}

pub fn rescan_internal(
    app: Option<&AppHandle>,
    provider: Option<String>,
) -> CommandResult<Snapshot> {
    let snapshot = scan::scan_and_persist(provider)?;
    emit_snapshot(app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn remove(id: String) -> CommandResult<Snapshot> {
    registry::with_registry(|registry| registry.remove(&id))
        .map_err(ErrorPayload::from)?
        .map_err(ErrorPayload::from)?;
    snapshot_with_autostart(None)
}

#[tauri::command]
pub fn purge_missing() -> CommandResult<usize> {
    registry::with_registry(|registry| registry.purge_missing()).map_err(ErrorPayload::from)
}

#[tauri::command]
pub fn mark_all_seen() -> CommandResult<Snapshot> {
    registry::with_registry(|registry| registry.mark_all_seen()).map_err(ErrorPayload::from)?;
    snapshot_with_autostart(None)
}

#[tauri::command]
pub fn dismiss_history_reset_notice() -> CommandResult<Snapshot> {
    registry::with_registry(|registry| {
        registry.file.settings.history_reset_notice_dismissed = true;
    })
    .map_err(ErrorPayload::from)?;
    snapshot_with_autostart(None)
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> CommandResult<Snapshot> {
    apply_autostart(&app, settings.autostart)?;
    registry::update_settings(settings).map_err(ErrorPayload::from)?;
    snapshot_with_autostart(Some(&app))
}

#[tauri::command]
pub fn add_custom_provider(input: CustomProviderInput) -> CommandResult<Snapshot> {
    let id = normalize_provider_id(&input.id, &input.name)?;
    let name = validate_provider_name(&input.name)?;
    reject_reserved_provider_id(&id)?;

    let dashboard_url = normalize_dashboard_url(input.dashboard_url)?;
    let locations = normalize_paths(input.config_paths)?
        .into_iter()
        .map(|path| CustomProviderLocation {
            path,
            format: input.format,
            strategy: "token_file",
            source_type: "config_file",
        })
        .collect::<Vec<_>>();
    let env_vars = normalize_env_vars(input.env_vars)?
        .into_iter()
        .map(|name| CustomProviderEnvVar {
            label: format!("{name} env var"),
            name,
        })
        .collect::<Vec<_>>();

    if locations.is_empty() && env_vars.is_empty() {
        return Err(ErrorPayload::new(
            "validation_error",
            "Add at least one config path or env var",
        ));
    }

    let descriptor = CustomProviderDescriptor {
        id: id.clone(),
        name,
        dashboard_url,
        locations,
        env_vars,
    };
    let text = toml::to_string_pretty(&descriptor).map_err(|err| {
        ErrorPayload::with_detail(
            "descriptor_error",
            "Custom provider could not be serialized",
            err.to_string(),
        )
    })?;
    save_custom_descriptor(&registry::app_home(), &id, &text)?;

    scan::scan_and_persist(Some(id))
}

fn save_custom_descriptor(home: &Path, id: &str, text: &str) -> CommandResult<()> {
    let providers_dir = home.join("providers");
    fs::create_dir_all(&providers_dir).map_err(|err| {
        ErrorPayload::with_detail(
            "io_error",
            "Custom provider folder could not be created",
            err.to_string(),
        )
    })?;
    let path = providers_dir.join(format!("{id}.toml"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                ErrorPayload::new(
                    "provider_exists",
                    "A custom provider with this ID already exists. Choose another ID.",
                )
            } else {
                ErrorPayload::with_detail(
                    "io_error",
                    "Custom provider could not be saved",
                    err.to_string(),
                )
            }
        })?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|err| {
            ErrorPayload::with_detail(
                "io_error",
                "Custom provider could not be saved",
                err.to_string(),
            )
        })
}

#[tauri::command]
pub fn reset_app_data(app: AppHandle) -> CommandResult<Snapshot> {
    registry::reset_app_data().map_err(ErrorPayload::from)?;
    snapshot_with_autostart(Some(&app))
}

#[tauri::command]
pub fn copy_value(id: String, field: String) -> CommandResult<()> {
    let snapshot = snapshot_with_autostart(None)?;
    let connection = snapshot
        .connections
        .into_iter()
        .find(|connection| connection.connection.id == id)
        .ok_or_else(|| ErrorPayload::new("not_found", "Connection was not found"))?
        .connection;
    let value = match field.as_str() {
        "identity" => match &connection.identity.host {
            Some(host) => format!("{}@{host}", connection.identity.label),
            None => connection.identity.label,
        },
        "source_path" => connection.source.path.unwrap_or_default(),
        "fingerprint" => connection.fingerprint.unwrap_or_default(),
        _ => {
            return Err(ErrorPayload::new(
                "validation_error",
                "Unsupported copy field",
            ))
        }
    };
    Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(value))
        .map_err(|err| {
            ErrorPayload::with_detail("clipboard_error", "Copy failed, try again", err.to_string())
        })
}

#[tauri::command]
pub fn open_dashboard(id: String) -> CommandResult<String> {
    let snapshot = snapshot_with_autostart(None)?;
    let connection = snapshot
        .connections
        .into_iter()
        .find(|connection| connection.connection.id == id)
        .ok_or_else(|| ErrorPayload::new("not_found", "Connection was not found"))?
        .connection;
    let descriptors = descriptors::bundled_descriptors();
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.id == connection.provider)
        .ok_or_else(|| ErrorPayload::new("url_rejected", "Provider has no dashboard URL"))?;
    let template = descriptor
        .dashboard_url
        .as_deref()
        .ok_or_else(|| ErrorPayload::new("url_rejected", "Provider has no dashboard URL"))?;
    let host = connection.identity.host.as_deref().unwrap_or_default();
    let url = template
        .replace("{label}", &connection.identity.label)
        .replace("{host}", host);
    let parsed = url::Url::parse(&url).map_err(|err| {
        ErrorPayload::with_detail("url_rejected", "Dashboard URL is invalid", err.to_string())
    })?;
    if parsed.scheme() != "https" {
        return Err(ErrorPayload::new(
            "url_rejected",
            "Dashboard URLs must use https",
        ));
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|err| {
        ErrorPayload::with_detail(
            "open_failed",
            "Dashboard could not be opened",
            err.to_string(),
        )
    })?;
    Ok(url)
}

#[tauri::command]
pub fn reveal_source(id: String) -> CommandResult<String> {
    let snapshot = snapshot_with_autostart(None)?;
    let connection = snapshot
        .connections
        .into_iter()
        .find(|connection| connection.connection.id == id)
        .ok_or_else(|| ErrorPayload::new("not_found", "Connection was not found"))?
        .connection;
    let path = connection
        .source
        .path
        .ok_or_else(|| ErrorPayload::new("path_missing", "This source cannot be revealed"))?;
    let source = PathBuf::from(&path);
    if !source.exists() {
        return Err(ErrorPayload::new("path_missing", "File no longer exists"));
    }
    tauri_plugin_opener::reveal_item_in_dir(&source).map_err(|err| {
        ErrorPayload::with_detail(
            "open_failed",
            "Source file could not be revealed",
            err.to_string(),
        )
    })?;
    Ok(path)
}

#[tauri::command]
pub fn open_external_url(url: String) -> CommandResult<String> {
    let parsed = url::Url::parse(&url).map_err(|err| {
        ErrorPayload::with_detail("url_rejected", "External URL is invalid", err.to_string())
    })?;
    if parsed.scheme() != "https" {
        return Err(ErrorPayload::new(
            "url_rejected",
            "External URLs must use https",
        ));
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|err| {
        ErrorPayload::with_detail("open_failed", "URL could not be opened", err.to_string())
    })?;
    Ok(url)
}

pub fn update_watcher_state(paused: bool) -> CommandResult<Snapshot> {
    registry::with_registry(|registry| {
        registry.file.settings.watchers_enabled = !paused;
    })
    .map_err(ErrorPayload::from)?;
    snapshot_with_autostart(None)
}

pub fn sync_autostart_setting(app: &AppHandle) -> CommandResult<()> {
    let enabled = read_autostart(app)?;
    registry::set_autostart_setting(enabled).map_err(ErrorPayload::from)
}

pub fn set_autostart(app: &AppHandle, enabled: bool) -> CommandResult<()> {
    apply_autostart(app, enabled)?;
    registry::set_autostart_setting(enabled).map_err(ErrorPayload::from)
}

fn snapshot_with_autostart(app: Option<&AppHandle>) -> CommandResult<Snapshot> {
    if let Some(app) = app {
        sync_autostart_setting(app)?;
    }
    registry::load_snapshot().map_err(ErrorPayload::from)
}

pub(crate) fn read_autostart(app: &AppHandle) -> CommandResult<bool> {
    if descriptors::ScanPaths::current().is_isolated() {
        return registry::Registry::load(&registry::app_home())
            .map(|registry| registry.file.settings.autostart)
            .map_err(ErrorPayload::from);
    }
    app.autolaunch().is_enabled().map_err(|err| {
        ErrorPayload::with_detail(
            "autostart_error",
            "Startup setting could not be read",
            err.to_string(),
        )
    })
}

fn apply_autostart(app: &AppHandle, enabled: bool) -> CommandResult<()> {
    if descriptors::ScanPaths::current().is_isolated() {
        return Ok(());
    }
    let manager = app.autolaunch();
    let current = manager.is_enabled().map_err(|err| {
        ErrorPayload::with_detail(
            "autostart_error",
            "Startup setting could not be read",
            err.to_string(),
        )
    })?;
    if current == enabled {
        return Ok(());
    }

    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|err| {
        ErrorPayload::with_detail(
            "autostart_error",
            if enabled {
                "ConnLens could not be added to startup"
            } else {
                "ConnLens could not be removed from startup"
            },
            err.to_string(),
        )
    })
}

fn emit_snapshot(app: Option<&AppHandle>, snapshot: &Snapshot) {
    if let Some(app) = app {
        crate::emit_visible_snapshot(app, snapshot);
    }
}

fn normalize_provider_id(id: &str, name: &str) -> CommandResult<String> {
    let raw = if id.trim().is_empty() { name } else { id };
    let mut out = String::new();
    let mut last_dash = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() || out.len() > 64 {
        return Err(ErrorPayload::new(
            "validation_error",
            "Provider ID must resolve to 1-64 letters, numbers, hyphens, or underscores",
        ));
    }
    Ok(out)
}

fn validate_provider_name(name: &str) -> CommandResult<String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 80 {
        return Err(ErrorPayload::new(
            "validation_error",
            "Provider name must be 1-80 characters",
        ));
    }
    Ok(name.to_string())
}

fn reject_reserved_provider_id(id: &str) -> CommandResult<()> {
    if is_retired_provider_id(id) {
        return Err(ErrorPayload::new(
            "validation_error",
            "MCP and Claude provider IDs are retired from built-in detection",
        ));
    }

    let (descriptors, _) = descriptors::load_all(Path::new("__connlens_builtin_only__"));
    if descriptors.iter().any(|descriptor| descriptor.id == id) {
        return Err(ErrorPayload::new(
            "validation_error",
            "Use a custom ID that does not match a bundled provider",
        ));
    }
    Ok(())
}

fn normalize_dashboard_url(value: Option<String>) -> CommandResult<Option<String>> {
    let Some(value) = value
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
    else {
        return Ok(None);
    };
    let parsed = url::Url::parse(&value).map_err(|err| {
        ErrorPayload::with_detail("url_rejected", "Dashboard URL is invalid", err.to_string())
    })?;
    if parsed.scheme() != "https" {
        return Err(ErrorPayload::new(
            "url_rejected",
            "Dashboard URLs must use https",
        ));
    }
    Ok(Some(value))
}

fn normalize_paths(paths: Vec<String>) -> CommandResult<Vec<String>> {
    let mut out = Vec::new();
    for path in paths {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        if path.len() > 260 || path.contains('\0') {
            return Err(ErrorPayload::new(
                "validation_error",
                "Config paths must be under 260 characters",
            ));
        }
        out.push(path.replace('\\', "/"));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn normalize_env_vars(env_vars: Vec<String>) -> CommandResult<Vec<String>> {
    let mut out = Vec::new();
    for env_var in env_vars {
        let env_var = env_var.trim().to_ascii_uppercase();
        if env_var.is_empty() {
            continue;
        }
        if env_var.len() > 80
            || !env_var
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(ErrorPayload::new(
                "validation_error",
                "Env vars may contain only A-Z, 0-9, and underscore",
            ));
        }
        out.push(env_var);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_a_custom_provider_cannot_overwrite_an_existing_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        save_custom_descriptor(dir.path(), "fixture", "original descriptor").unwrap();
        let error =
            save_custom_descriptor(dir.path(), "fixture", "replacement descriptor").unwrap_err();
        assert_eq!(error.code, "provider_exists");
        assert_eq!(
            fs::read_to_string(dir.path().join("providers/fixture.toml")).unwrap(),
            "original descriptor"
        );
    }

    #[test]
    fn provider_id_is_normalized_from_name() {
        assert_eq!(
            normalize_provider_id("", "Weird CLI!!!").unwrap(),
            "weird-cli"
        );
    }

    #[test]
    fn env_vars_are_deduped_and_normalized() {
        assert_eq!(
            normalize_env_vars(vec!["weird_token".to_string(), "WEIRD_TOKEN".to_string()]).unwrap(),
            vec!["WEIRD_TOKEN".to_string()]
        );
    }
}
