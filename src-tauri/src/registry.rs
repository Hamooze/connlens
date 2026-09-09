use crate::models::{
    is_retired_provider_id, Availability, ChangeSet, Connection, ConnectionStatus,
    ConnectionValidation, DetectedConnection, ErrorPayload, Identity, RegisterEntry, RegistryFile,
    Settings, Snapshot, SnapshotConnection, SourceType, WatcherHealth,
};
use chrono::Utc;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("registry is locked")]
    Locked,
    #[error("registry parse error: {0}")]
    Parse(String),
    #[error("registry io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found")]
    NotFound,
    #[error("not removable")]
    NotRemovable,
    #[error("invalid register entry: {0}")]
    InvalidRegisterEntry(String),
}

impl From<RegistryError> for ErrorPayload {
    fn from(value: RegistryError) -> Self {
        match value {
            RegistryError::Locked => ErrorPayload::new(
                "registry_locked",
                "Registry is locked by another ConnLens process",
            ),
            RegistryError::Parse(detail) => {
                ErrorPayload::with_detail("parse_error", "Registry could not be parsed", detail)
            }
            RegistryError::Io(err) => ErrorPayload::with_detail(
                "io_error",
                "Registry file operation failed",
                err.to_string(),
            ),
            RegistryError::NotFound => ErrorPayload::new("not_found", "Connection was not found"),
            RegistryError::NotRemovable => ErrorPayload::new(
                "not_removable",
                "Active auto-detected connections cannot be removed",
            ),
            RegistryError::InvalidRegisterEntry(detail) => {
                ErrorPayload::with_detail("validation_error", "Register entry is invalid", detail)
            }
        }
    }
}

struct RegistryLock {
    path: PathBuf,
}

impl Drop for RegistryLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone)]
pub struct Registry {
    pub home: PathBuf,
    pub file: RegistryFile,
    pub history_reset_notice: bool,
}

pub fn app_home() -> PathBuf {
    if let Some(path) = std::env::var_os("CONNLENS_HOME") {
        return PathBuf::from(path);
    }

    default_home("nemu")
}

fn default_home(owner: &str) -> PathBuf {
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
            return PathBuf::from(local).join("ConnLens");
        }
    }

    directories::ProjectDirs::from("com", owner, "ConnLens")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".connlens"))
}

fn prepare_home(home: &Path) -> Result<(), RegistryError> {
    // Explicit fixture homes never inspect or migrate the user's default data.
    if std::env::var_os("CONNLENS_HOME").is_some() {
        return Ok(());
    }
    prepare_home_at(home, &default_home("brdg"), &default_home("nemu"))
}

pub(crate) fn prepare_app_home() -> Result<(), RegistryError> {
    prepare_home(&app_home())
}

fn prepare_home_at(home: &Path, legacy: &Path, current: &Path) -> Result<(), RegistryError> {
    if home != current || legacy == current {
        return Ok(());
    }
    move_legacy_home(legacy, current)
}

fn move_legacy_home(legacy: &Path, current: &Path) -> Result<(), RegistryError> {
    // Never merge with or overwrite an existing Nemu namespace, including an
    // empty directory or a symbolic link supplied by another process.
    match fs::symlink_metadata(current) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    match fs::symlink_metadata(legacy) {
        Ok(metadata) if metadata.is_dir() && !is_directory_link(&metadata) => {}
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Legacy ConnLens data is not a regular directory; it was left unchanged",
            )
            .into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }

    // Hold the existing registry lock during the move. Do not copy the tree:
    // the atomic rename preserves every file and permission without following
    // any links inside it, and a failed move leaves the old namespace intact.
    let Some(mut guard) = lock_legacy_for_migration(legacy, current)? else {
        return Ok(());
    };
    if let Some(parent) = current.parent() {
        fs::create_dir_all(parent)?;
    }
    match rename_directory_no_replace(legacy, current) {
        Ok(()) => {
            guard.path = lock_path(current);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn lock_legacy_for_migration(
    legacy: &Path,
    current: &Path,
) -> Result<Option<RegistryLock>, RegistryError> {
    let path = lock_path(legacy);
    let start = Instant::now();
    loop {
        if fs::symlink_metadata(current).is_ok() {
            return Ok(None);
        }
        // Unlike the normal registry lock, never create the legacy directory.
        // Another new process may have just moved it into the Nemu namespace.
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let _ = writeln!(file, "pid={}", std::process::id());
                return Ok(Some(RegistryLock { path }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if start.elapsed() >= Duration::from_secs(2) {
                    return Err(RegistryError::Locked);
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && fs::symlink_metadata(current).is_ok() =>
            {
                return Ok(None)
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn is_directory_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn rename_directory_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        #[cfg(target_os = "macos")]
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use windows::{
            core::HSTRING,
            Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS},
        };
        // No REPLACE_EXISTING and no COPY_ALLOWED: fail safely if a rename is
        // not available instead of overwriting or performing a partial copy.
        unsafe {
            MoveFileExW(
                &HSTRING::from(from.as_os_str()),
                &HSTRING::from(to.as_os_str()),
                MOVE_FILE_FLAGS(0),
            )
        }
        .map_err(|_| std::io::Error::last_os_error())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (from, to);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Atomic namespace migration is unavailable",
        ))
    }
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn short_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

pub fn connection_id(detected: &DetectedConnection) -> String {
    let host = detected.identity.host.as_deref().unwrap_or_default();
    let source = detected.source.path.as_deref().unwrap_or_default();
    let scope = detected.identity.scope.as_deref().unwrap_or_default();
    short_hash(&[
        &detected.provider,
        &detected.identity.label,
        host,
        source,
        scope,
    ])
}

fn registry_path(home: &Path) -> PathBuf {
    home.join("registry.json")
}

fn backup_path(home: &Path) -> PathBuf {
    home.join("registry.json.bak")
}

fn lock_path(home: &Path) -> PathBuf {
    home.join("registry.lock")
}

fn lock(home: &Path) -> Result<RegistryLock, RegistryError> {
    prepare_home(home)?;
    lock_unprepared(home)
}

fn lock_unprepared(home: &Path) -> Result<RegistryLock, RegistryError> {
    fs::create_dir_all(home)?;
    let path = lock_path(home);
    let start = Instant::now();

    loop {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let _ = writeln!(file, "pid={}", std::process::id());
                return Ok(RegistryLock { path });
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if start.elapsed() >= Duration::from_secs(2) {
                    return Err(RegistryError::Locked);
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(RegistryError::Io(err)),
        }
    }
}

fn read_file(path: &Path) -> Result<RegistryFile, RegistryError> {
    let text = fs::read_to_string(path)?;
    let mut file: RegistryFile =
        serde_json::from_str(&text).map_err(|err| RegistryError::Parse(err.to_string()))?;
    file.settings.probes_enabled = false;
    for connection in &mut file.connections {
        if connection.validation.availability != Availability::Available {
            connection.identity.is_active_identity = false;
            connection.validation.usage = crate::models::Usage::Unknown;
            if connection.validation.availability == Availability::Unknown {
                connection.status = ConnectionStatus::Unverified;
            }
        }
    }
    Ok(file)
}

fn archive_corrupt(path: &Path) {
    if path.exists() {
        let stamp = Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let archive = path.with_file_name(format!("{name}.corrupt-{stamp}"));
        let _ = fs::rename(path, archive);
    }
}

impl Registry {
    pub fn load(home: &Path) -> Result<Self, RegistryError> {
        prepare_home(home)?;
        fs::create_dir_all(home)?;
        let primary = registry_path(home);
        let backup = backup_path(home);

        if primary.exists() {
            match read_file(&primary) {
                Ok(file) => {
                    return Ok(Self {
                        home: home.to_path_buf(),
                        file,
                        history_reset_notice: false,
                    });
                }
                Err(_) if backup.exists() => match read_file(&backup) {
                    Ok(file) => {
                        archive_corrupt(&primary);
                        return Ok(Self {
                            home: home.to_path_buf(),
                            file,
                            history_reset_notice: true,
                        });
                    }
                    Err(_) => {
                        archive_corrupt(&primary);
                        archive_corrupt(&backup);
                        return Ok(Self {
                            home: home.to_path_buf(),
                            file: RegistryFile::default(),
                            history_reset_notice: true,
                        });
                    }
                },
                Err(RegistryError::Parse(_)) => {
                    archive_corrupt(&primary);
                    return Ok(Self {
                        home: home.to_path_buf(),
                        file: RegistryFile::default(),
                        history_reset_notice: true,
                    });
                }
                Err(err) => return Err(err),
            }
        }
        if backup.exists() {
            return Ok(Self {
                home: home.to_path_buf(),
                file: read_file(&backup)?,
                history_reset_notice: true,
            });
        }

        Ok(Self {
            home: home.to_path_buf(),
            file: RegistryFile::default(),
            history_reset_notice: false,
        })
    }

    pub fn save(&self) -> Result<(), RegistryError> {
        let _guard = lock(&self.home)?;
        self.save_unlocked()
    }

    fn save_unlocked(&self) -> Result<(), RegistryError> {
        fs::create_dir_all(&self.home)?;
        let path = registry_path(&self.home);
        let backup = backup_path(&self.home);
        let temp = self.home.join("registry.json.tmp");

        if path.exists() {
            let _ = fs::copy(&path, &backup);
        }

        let text = serde_json::to_string_pretty(&self.file)
            .map_err(|err| RegistryError::Parse(err.to_string()))?;
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(temp, path)?;
        Ok(())
    }

    pub fn diff(&mut self, detected: Vec<DetectedConnection>, scope: Option<&str>) -> ChangeSet {
        self.diff_with_validation(detected, scope, &BTreeMap::new())
    }

    pub fn diff_with_validation(
        &mut self,
        detected: Vec<DetectedConnection>,
        _scope: Option<&str>,
        validations: &BTreeMap<String, ConnectionValidation>,
    ) -> ChangeSet {
        let now = now_iso();
        let mut changes = ChangeSet::default();
        let mut seen_ids = BTreeSet::new();
        let old_by_id = self
            .file
            .connections
            .iter()
            .cloned()
            .map(|connection| (connection.id.clone(), connection))
            .collect::<BTreeMap<_, _>>();
        let mut next = Vec::new();

        for detected in detected {
            if is_retired_provider_id(&detected.provider) {
                continue;
            }

            let id = connection_id(&detected);
            if !seen_ids.insert(id.clone()) {
                continue;
            }
            // Preserve history when migrating IDs to include the profile scope.
            let existing = old_by_id.get(&id).or_else(|| {
                self.file.connections.iter().find(|existing| {
                    existing.provider == detected.provider
                        && existing.identity.label == detected.identity.label
                        && existing.identity.host == detected.identity.host
                        && existing.identity.scope == detected.identity.scope
                        && existing.source == detected.source
                })
            });
            if let Some(existing) = existing {
                seen_ids.insert(existing.id.clone());
                let mut updated = existing.clone();
                updated.id = id.clone();
                let fingerprint_changed = existing.fingerprint != detected.fingerprint
                    && existing.fingerprint.is_some()
                    && detected.fingerprint.is_some();
                updated.validation =
                    ConnectionValidation::available(&now, detected.identity.is_active_identity);
                updated.provider_name = detected.provider_name;
                updated.identity = detected.identity;
                updated.source = detected.source;
                updated.meta = detected.meta;
                updated.last_seen = if existing.first_seen > now {
                    existing.first_seen.clone()
                } else {
                    now.clone()
                };
                updated.fingerprint = detected.fingerprint;
                updated.status = if fingerprint_changed {
                    updated.seen = false;
                    changes.changed.push(id.clone());
                    ConnectionStatus::Changed
                } else if existing.status == ConnectionStatus::Changed && !existing.seen {
                    ConnectionStatus::Changed
                } else {
                    ConnectionStatus::Active
                };
                next.push(updated);
            } else {
                let validation =
                    ConnectionValidation::available(&now, detected.identity.is_active_identity);
                let connection = Connection {
                    id: id.clone(),
                    provider: detected.provider,
                    provider_name: detected.provider_name,
                    identity: detected.identity,
                    source: detected.source,
                    status: ConnectionStatus::Active,
                    validation,
                    fingerprint: detected.fingerprint,
                    first_seen: now.clone(),
                    last_seen: now.clone(),
                    hidden: false,
                    seen: false,
                    meta: detected.meta,
                };
                changes.created.push(id);
                next.push(connection);
            }
        }

        for existing in &self.file.connections {
            if is_retired_provider_id(&existing.provider) {
                continue;
            }

            if !seen_ids.contains(&existing.id) {
                let mut updated = existing.clone();
                updated.validation = validations.get(&existing.id).cloned().unwrap_or_else(|| {
                    ConnectionValidation::unknown(
                        "not_checked",
                        "This source was not validated in the current scan.",
                        Some(now.clone()),
                    )
                });
                updated.identity.is_active_identity = false;
                if updated.validation.availability == Availability::Missing {
                    if existing.validation.availability != Availability::Missing {
                        changes.missing.push(updated.id.clone());
                        updated.seen = false;
                    }
                    updated.status = ConnectionStatus::Missing;
                } else {
                    updated.status = ConnectionStatus::Unverified;
                }
                next.push(updated);
            }
        }

        next.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| status_rank(&a.status).cmp(&status_rank(&b.status)))
                .then_with(|| {
                    b.identity
                        .is_active_identity
                        .cmp(&a.identity.is_active_identity)
                })
                .then_with(|| a.identity.label.cmp(&b.identity.label))
        });
        self.file.connections = next;
        self.file.last_scan = Some(now);
        changes
    }

    pub fn snapshot(
        &self,
        provider_errors: Vec<crate::models::ProviderError>,
        watcher_health: WatcherHealth,
    ) -> Snapshot {
        let connections = self
            .file
            .connections
            .iter()
            .filter(|connection| !is_retired_provider_id(&connection.provider))
            .cloned()
            .map(|connection| SnapshotConnection {
                removable: connection.removable(),
                connection,
            })
            .collect();

        Snapshot {
            schema_version: 1,
            connections,
            settings: self.file.settings.clone(),
            last_scan: self.file.last_scan.clone(),
            provider_errors: if provider_errors.is_empty() {
                self.file.provider_errors.clone()
            } else {
                provider_errors
            },
            watcher_health,
            history_reset_notice: self.history_reset_notice
                && !self.file.settings.history_reset_notice_dismissed,
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<(), RegistryError> {
        let index = self
            .file
            .connections
            .iter()
            .position(|connection| connection.id == id)
            .ok_or(RegistryError::NotFound)?;
        if !self.file.connections[index].removable() {
            return Err(RegistryError::NotRemovable);
        }
        self.file.connections.remove(index);
        Ok(())
    }

    pub fn purge_missing(&mut self) -> usize {
        let before = self.file.connections.len();
        self.file
            .connections
            .retain(|connection| connection.validation.availability != Availability::Missing);
        before - self.file.connections.len()
    }

    pub fn mark_all_seen(&mut self) {
        for connection in &mut self.file.connections {
            connection.seen = true;
            if connection.status == ConnectionStatus::Changed {
                connection.status = ConnectionStatus::Active;
            }
        }
    }

    pub fn register_entry(&mut self, entry: RegisterEntry) -> Result<String, RegistryError> {
        validate_register_entry(&entry)?;
        let now = now_iso();
        let mut meta = entry.meta;
        if let Some(note) = entry.note {
            meta.insert("note".to_string(), Value::String(note));
        }
        let detected = DetectedConnection {
            provider: entry.provider.clone(),
            provider_name: title_case(&entry.provider),
            identity: Identity {
                label: entry.label,
                host: entry.host,
                scope: entry.scope,
                is_active_identity: false,
            },
            source: crate::models::ConnectionSource {
                source_type: SourceType::AgentRegistered,
                path: None,
                descriptor_id: None,
            },
            fingerprint: None,
            meta,
        };
        let id = connection_id(&detected);
        if let Some(existing) = self
            .file
            .connections
            .iter_mut()
            .find(|connection| connection.id == id)
        {
            existing.last_seen = now;
            existing.identity = detected.identity;
            existing.meta = detected.meta;
            existing.status = ConnectionStatus::Unverified;
            existing.validation = ConnectionValidation::unknown(
                "manual_record",
                "This is a manually registered entry; local availability and use are not verified.",
                Some(existing.last_seen.clone()),
            );
            existing.seen = false;
            return Ok(id);
        }

        self.file.connections.push(Connection {
            id: id.clone(),
            provider: detected.provider,
            provider_name: detected.provider_name,
            identity: detected.identity,
            source: detected.source,
            status: ConnectionStatus::Unverified,
            validation: ConnectionValidation::unknown(
                "manual_record",
                "This is a manually registered entry; local availability and use are not verified.",
                Some(now.clone()),
            ),
            fingerprint: None,
            first_seen: now.clone(),
            last_seen: now,
            hidden: false,
            seen: false,
            meta: detected.meta,
        });
        Ok(id)
    }
}

fn status_rank(status: &ConnectionStatus) -> u8 {
    match status {
        ConnectionStatus::Active => 0,
        ConnectionStatus::Changed => 1,
        ConnectionStatus::Unverified => 2,
        ConnectionStatus::Missing => 3,
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => "Unknown".to_string(),
    }
}

pub fn validate_register_entry(entry: &RegisterEntry) -> Result<(), RegistryError> {
    if entry.provider.trim().is_empty() || entry.provider.len() > 64 {
        return Err(RegistryError::InvalidRegisterEntry(
            "provider must be 1-64 characters".to_string(),
        ));
    }
    if entry.label.trim().is_empty() || entry.label.len() > 160 {
        return Err(RegistryError::InvalidRegisterEntry(
            "label must be 1-160 characters".to_string(),
        ));
    }
    if entry
        .provider
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
    {
        return Err(RegistryError::InvalidRegisterEntry(
            "provider may contain only letters, numbers, hyphen, and underscore".to_string(),
        ));
    }
    Ok(())
}

pub fn with_registry<T>(f: impl FnOnce(&mut Registry) -> T) -> Result<T, RegistryError> {
    with_registry_at(&app_home(), f)
}

pub fn with_registry_at<T>(
    home: &Path,
    f: impl FnOnce(&mut Registry) -> T,
) -> Result<T, RegistryError> {
    // Hold one lock across the complete transaction, including scan and UI updates.
    let _guard = lock(home)?;
    let mut registry = Registry::load(home)?;
    let previous = registry.file.clone();
    let result = f(&mut registry);
    if registry.file != previous || !registry_path(home).exists() {
        registry.save_unlocked()?;
    }
    Ok(result)
}

pub fn load_snapshot() -> Result<Snapshot, RegistryError> {
    let home = app_home();
    let registry = Registry::load(&home)?;
    let health = crate::watchers::health(registry.file.settings.watchers_enabled);
    Ok(registry.snapshot(Vec::new(), health))
}

pub fn update_settings(mut patch: Settings) -> Result<Settings, RegistryError> {
    patch.probes_enabled = false;
    patch.poll_minutes = patch.poll_minutes.clamp(1, 1440);
    with_registry(|registry| {
        registry.file.settings = patch.clone();
        registry.file.settings.clone()
    })
}

pub fn set_autostart_setting(enabled: bool) -> Result<(), RegistryError> {
    with_registry(|registry| registry.file.settings.autostart = enabled)
}

pub fn reset_app_data() -> Result<(), RegistryError> {
    reset_app_data_at(&app_home())
}

fn reset_app_data_at(home: &Path) -> Result<(), RegistryError> {
    if !home.exists() {
        return Ok(());
    }
    let _guard = lock(home)?;
    // Return filesystem failures to the UI instead of reporting a partial reset as success.
    for directory in [home.join("inbox"), home.join("providers")] {
        if let Err(error) = fs::remove_dir_all(directory) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error.into());
            }
        }
    }
    for path in [
        backup_path(home),
        home.join("registry.json.tmp"),
        registry_path(home),
    ] {
        if let Err(error) = fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error.into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConnectionSource, SourceType};

    #[test]
    fn namespace_migration_preserves_the_complete_app_data_tree() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy-owner");
        let current = dir.path().join("nemu");
        with_registry_at(&legacy, |registry| {
            registry.file.settings.theme = "light".to_string();
            registry.file.settings.watchers_enabled = false;
        })
        .unwrap();
        fs::create_dir_all(legacy.join("providers/nested")).unwrap();
        fs::create_dir_all(legacy.join("inbox")).unwrap();
        fs::write(
            legacy.join("providers/nested/custom.toml"),
            "fixture provider",
        )
        .unwrap();
        fs::write(legacy.join("inbox/entry.json"), "fixture registration").unwrap();
        fs::write(legacy.join("registry.json.corrupt-fixture"), [0, 1, 2, 255]).unwrap();
        let original_registry = fs::read(registry_path(&legacy)).unwrap();

        prepare_home_at(&current, &legacy, &current).unwrap();

        assert!(!legacy.exists());
        assert_eq!(
            fs::read(registry_path(&current)).unwrap(),
            original_registry
        );
        assert_eq!(
            fs::read_to_string(current.join("providers/nested/custom.toml")).unwrap(),
            "fixture provider"
        );
        assert_eq!(
            fs::read_to_string(current.join("inbox/entry.json")).unwrap(),
            "fixture registration"
        );
        assert_eq!(
            fs::read(current.join("registry.json.corrupt-fixture")).unwrap(),
            [0, 1, 2, 255]
        );
        assert!(!lock_path(&current).exists());
        let registry = Registry::load(&current).unwrap();
        assert_eq!(registry.file.settings.theme, "light");
        assert!(!registry.file.settings.watchers_enabled);
        prepare_home_at(&current, &legacy, &current).unwrap();
        assert!(!legacy.exists());
    }

    #[test]
    fn namespace_migration_never_merges_or_overwrites_existing_data() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let current = dir.path().join("nemu");
        fs::create_dir(&legacy).unwrap();
        fs::create_dir(&current).unwrap();
        fs::write(legacy.join("registry.json"), "legacy data").unwrap();
        fs::write(current.join("registry.json"), "Nemu data").unwrap();
        move_legacy_home(&legacy, &current).unwrap();
        assert_eq!(
            fs::read_to_string(legacy.join("registry.json")).unwrap(),
            "legacy data"
        );
        assert_eq!(
            fs::read_to_string(current.join("registry.json")).unwrap(),
            "Nemu data"
        );
        assert!(!lock_path(&legacy).exists());
    }

    #[test]
    fn explicit_fixture_home_does_not_migrate_default_namespaces() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let current = dir.path().join("nemu");
        let fixture = dir.path().join("isolated");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("registry.json"), "original").unwrap();
        prepare_home_at(&fixture, &legacy, &current).unwrap();
        assert!(!current.exists());
        assert!(!fixture.exists());
        assert_eq!(
            fs::read_to_string(legacy.join("registry.json")).unwrap(),
            "original"
        );
        // Linux and the normal Windows storage override do not change path.
        prepare_home_at(&legacy, &legacy, &legacy).unwrap();
        assert_eq!(
            fs::read_to_string(legacy.join("registry.json")).unwrap(),
            "original"
        );
    }

    #[test]
    fn simultaneous_namespace_migrations_preserve_data_without_recreating_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let current = dir.path().join("nemu");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("registry.json"), "original").unwrap();
        let held_lock = lock_unprepared(&legacy).unwrap();
        let barrier = std::sync::Barrier::new(3);
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                move_legacy_home(&legacy, &current)
            });
            let second = scope.spawn(|| {
                barrier.wait();
                move_legacy_home(&legacy, &current)
            });
            barrier.wait();
            // Both starters must wait for the existing transaction to release.
            std::thread::sleep(Duration::from_millis(100));
            assert!(!current.exists());
            drop(held_lock);
            first.join().unwrap().unwrap();
            second.join().unwrap().unwrap();
        });
        assert!(!legacy.exists());
        assert!(!lock_path(&current).exists());
        assert_eq!(
            fs::read_to_string(current.join("registry.json")).unwrap(),
            "original"
        );
    }

    #[test]
    fn busy_legacy_namespace_does_not_create_an_empty_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let current = dir.path().join("nemu");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("registry.json"), "original").unwrap();
        let _held_lock = lock_unprepared(&legacy).unwrap();
        assert!(matches!(
            move_legacy_home(&legacy, &current),
            Err(RegistryError::Locked)
        ));
        assert!(!current.exists());
        assert_eq!(
            fs::read_to_string(legacy.join("registry.json")).unwrap(),
            "original"
        );
    }

    #[test]
    fn atomic_migration_rename_cannot_replace_even_an_empty_destination() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let current = dir.path().join("nemu");
        fs::create_dir(&legacy).unwrap();
        fs::create_dir(&current).unwrap();
        fs::write(legacy.join("registry.json"), "original").unwrap();
        assert!(rename_directory_no_replace(&legacy, &current).is_err());
        assert!(legacy.join("registry.json").exists());
        assert_eq!(fs::read_dir(current).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn namespace_migration_preserves_embedded_links_without_following_them() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let current = dir.path().join("nemu");
        let external = dir.path().join("outside.json");
        fs::create_dir(&legacy).unwrap();
        fs::write(&external, "outside unchanged").unwrap();
        symlink(&external, legacy.join("linked.json")).unwrap();
        move_legacy_home(&legacy, &current).unwrap();
        assert!(fs::symlink_metadata(current.join("linked.json"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_link(current.join("linked.json")).unwrap(),
            external
        );
        assert_eq!(fs::read_to_string(external).unwrap(), "outside unchanged");
    }

    #[cfg(unix)]
    #[test]
    fn namespace_migration_refuses_a_link_as_the_legacy_root() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let current = dir.path().join("nemu");
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &legacy).unwrap();
        assert!(move_legacy_home(&legacy, &current).is_err());
        assert!(!current.exists());
        assert!(fs::symlink_metadata(&legacy)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(!lock_path(&outside).exists());
    }

    #[test]
    fn reset_removes_owned_data_and_reports_filesystem_failures() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        with_registry_at(home, |registry| {
            registry.file.settings.theme = "light".into()
        })
        .unwrap();
        with_registry_at(home, |registry| {
            registry.file.settings.theme = "dark".into()
        })
        .unwrap();
        fs::write(
            home.join("providers"),
            "a file cannot be removed as a provider directory",
        )
        .unwrap();
        assert!(reset_app_data_at(home).is_err());
        assert!(registry_path(home).exists());
        assert!(!lock_path(home).exists());
        fs::remove_file(home.join("providers")).unwrap();
        fs::create_dir(home.join("providers")).unwrap();
        fs::write(home.join("providers/fixture.toml"), "fixture descriptor").unwrap();
        reset_app_data_at(home).unwrap();
        assert!(!registry_path(home).exists());
        assert!(!backup_path(home).exists());
        assert!(!home.join("providers").exists());
        assert!(!lock_path(home).exists());
        assert!(Registry::load(home).unwrap().file.connections.is_empty());
    }

    #[test]
    fn parallel_transactions_preserve_both_settings_updates() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for provider in ["github", "aws"] {
                let home = dir.path();
                scope.spawn(move || {
                    with_registry_at(home, |registry| {
                        std::thread::sleep(Duration::from_millis(10));
                        registry
                            .file
                            .settings
                            .provider_toggles
                            .insert(provider.to_string(), false);
                    })
                    .unwrap();
                });
            }
        });
        let registry = Registry::load(dir.path()).unwrap();
        assert_eq!(registry.file.settings.provider_toggles.len(), 2);
    }

    #[test]
    fn profiles_with_same_account_and_path_keep_distinct_ids() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let mut first = detected("same-account", "account-host", "config", None);
        first.identity.scope = Some("development".to_string());
        let mut second = first.clone();
        second.identity.scope = Some("production".to_string());
        registry.diff(vec![first.clone(), first, second], None);
        assert_eq!(registry.file.connections.len(), 2);
        assert_ne!(
            registry.file.connections[0].id,
            registry.file.connections[1].id
        );
    }

    #[test]
    fn manually_registered_connections_survive_scans_and_update_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let entry = RegisterEntry {
            provider: "fixture".into(),
            label: "Manual account".into(),
            host: None,
            scope: None,
            note: Some("First note".into()),
            meta: BTreeMap::new(),
        };
        let id = registry.register_entry(entry.clone()).unwrap();
        registry.diff(Vec::new(), None);
        assert_eq!(
            registry.file.connections[0].status,
            ConnectionStatus::Unverified
        );
        let mut updated = entry;
        updated.note = Some("Updated note".into());
        assert_eq!(registry.register_entry(updated).unwrap(), id);
        assert_eq!(
            registry.file.connections[0]
                .meta
                .get("note")
                .and_then(Value::as_str),
            Some("Updated note")
        );
    }

    #[test]
    fn missing_primary_recovers_backup_and_legacy_probes_stay_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RegistryFile::default();
        file.settings.probes_enabled = true;
        file.settings.theme = "light".into();
        fs::write(
            backup_path(dir.path()),
            serde_json::to_string(&file).unwrap(),
        )
        .unwrap();
        let recovered = Registry::load(dir.path()).unwrap();
        assert!(recovered.history_reset_notice);
        assert_eq!(recovered.file.settings.theme, "light");
        assert!(!recovered.file.settings.probes_enabled);
    }

    fn detected(
        label: &str,
        host: &str,
        path: &str,
        fingerprint: Option<&str>,
    ) -> DetectedConnection {
        DetectedConnection {
            provider: "github".to_string(),
            provider_name: "GitHub".to_string(),
            identity: Identity {
                label: label.to_string(),
                host: Some(host.to_string()),
                scope: None,
                is_active_identity: false,
            },
            source: ConnectionSource {
                source_type: SourceType::ConfigFile,
                path: Some(path.to_string()),
                descriptor_id: Some("github".to_string()),
            },
            fingerprint: fingerprint.map(str::to_string),
            meta: BTreeMap::new(),
        }
    }

    #[test]
    fn stable_id_preserves_first_seen() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        registry.diff(
            vec![detected(
                "alice",
                "github.com",
                "hosts.yml",
                Some("sha256:aaaa1111"),
            )],
            None,
        );
        let first_seen = registry.file.connections[0].first_seen.clone();
        let id = registry.file.connections[0].id.clone();
        registry.diff(
            vec![detected(
                "alice",
                "github.com",
                "hosts.yml",
                Some("sha256:aaaa1111"),
            )],
            None,
        );
        assert_eq!(registry.file.connections[0].id, id);
        assert_eq!(registry.file.connections[0].first_seen, first_seen);
    }

    #[test]
    fn missing_rows_are_retained_until_purged() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        registry.diff(
            vec![detected(
                "alice",
                "github.com",
                "hosts.yml",
                Some("sha256:aaaa1111"),
            )],
            None,
        );
        let id = registry.file.connections[0].id.clone();
        let proof = BTreeMap::from([(
            id,
            ConnectionValidation::missing(
                &now_iso(),
                "source_missing",
                "Fixture source was checked and is absent.",
            ),
        )]);
        registry.diff_with_validation(Vec::new(), Some("github"), &proof);
        assert_eq!(
            registry.file.connections[0].status,
            ConnectionStatus::Missing
        );
        assert_eq!(registry.purge_missing(), 1);
        assert!(registry.file.connections.is_empty());
    }

    #[test]
    fn project_link_history_is_retained_without_source_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let mut project = detected(
            "linked-app",
            "https://vercel.com",
            "app/.vercel/project.json",
            Some("sha256:project"),
        );
        project.provider = "vercel".to_string();
        project.provider_name = "Vercel".to_string();
        project.identity.scope = Some("Linked project".to_string());
        project.meta.insert(
            "kind".to_string(),
            Value::String("project_link".to_string()),
        );

        registry.diff(vec![project], Some("vercel"));
        assert_eq!(registry.file.connections.len(), 1);

        registry.diff(Vec::new(), Some("vercel"));
        assert_eq!(registry.file.connections.len(), 1);
        assert_eq!(
            registry.file.connections[0].validation.availability,
            Availability::Unknown
        );
        assert!(!registry.file.connections[0].removable());
    }

    #[test]
    fn fingerprint_fallback_history_is_retained_when_identity_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let source = "C:/Users/dev/.config/neonctl/credentials.json";
        let mut fallback = detected(
            "Neon DB (...66e4)",
            "https://console.neon.tech",
            source,
            Some("sha256:old"),
        );
        fallback.provider = "neon".to_string();
        fallback.provider_name = "Neon DB".to_string();
        let mut user = detected(
            "usr_fixture_1",
            "https://console.neon.tech",
            source,
            Some("sha256:new"),
        );
        user.provider = "neon".to_string();
        user.provider_name = "Neon DB".to_string();

        registry.diff(vec![fallback], Some("neon"));
        assert_eq!(registry.file.connections.len(), 1);

        registry.diff(vec![user], Some("neon"));
        assert_eq!(registry.file.connections.len(), 2);
        assert_eq!(registry.file.connections[0].identity.label, "usr_fixture_1");
        assert_eq!(
            registry.file.connections[1].validation.availability,
            Availability::Unknown
        );
    }

    #[test]
    fn raw_identity_history_is_retained_when_label_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let source = "C:/Users/dev/.config/neonctl/credentials.json";
        let mut raw = detected(
            "bd340f52-aba8-42aa-a283-9f1f026e7eeb",
            "https://console.neon.tech",
            source,
            Some("sha256:old"),
        );
        raw.provider = "neon".to_string();
        raw.provider_name = "Neon DB".to_string();
        let mut clean = detected(
            "Neon account",
            "https://console.neon.tech",
            source,
            Some("sha256:new"),
        );
        clean.provider = "neon".to_string();
        clean.provider_name = "Neon DB".to_string();

        registry.diff(vec![raw], Some("neon"));
        assert_eq!(registry.file.connections.len(), 1);

        registry.diff(vec![clean], Some("neon"));
        assert_eq!(registry.file.connections.len(), 2);
        assert_eq!(registry.file.connections[0].identity.label, "Neon account");
        assert_eq!(
            registry.file.connections[1].validation.availability,
            Availability::Unknown
        );
    }

    #[test]
    fn generic_identity_history_is_retained_when_label_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let source = "C:/Users/dev/.config/neonctl/credentials.json";
        let mut generic = detected(
            "Neon account",
            "https://console.neon.tech",
            source,
            Some("sha256:old"),
        );
        generic.provider = "neon".to_string();
        generic.provider_name = "Neon DB".to_string();
        let mut clean = detected(
            "neon.profile@example.test",
            "https://console.neon.tech",
            source,
            Some("sha256:new"),
        );
        clean.provider = "neon".to_string();
        clean.provider_name = "Neon DB".to_string();

        registry.diff(vec![generic], Some("neon"));
        assert_eq!(registry.file.connections.len(), 1);

        registry.diff(vec![clean], Some("neon"));
        assert_eq!(registry.file.connections.len(), 2);
        assert_eq!(
            registry.file.connections[0].identity.label,
            "neon.profile@example.test"
        );
    }

    #[test]
    fn azure_config_history_is_retained_when_profile_account_appears() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let config_source = "C:/Users/dev/.azure/config";
        let profile_source = "C:/Users/dev/.azure/azureProfile.json";
        let mut cloud = detected(
            "AzureCloud",
            "https://portal.azure.com",
            config_source,
            None,
        );
        cloud.provider = "azure".to_string();
        cloud.provider_name = "Azure".to_string();
        cloud.identity.scope = Some("cloud".to_string());
        let mut default = detected("default", "https://portal.azure.com", config_source, None);
        default.provider = "azure".to_string();
        default.provider_name = "Azure".to_string();
        let mut account = detected(
            "azure.user@example.test",
            "33235475-cbc2-48bb-b74d-cbbba011a03c",
            profile_source,
            Some("sha256:new"),
        );
        account.provider = "azure".to_string();
        account.provider_name = "Azure".to_string();
        account.identity.scope = Some("Azure subscription 1".to_string());

        registry.diff(vec![cloud, default], Some("azure"));
        assert_eq!(registry.file.connections.len(), 2);

        registry.diff(vec![account], Some("azure"));
        assert_eq!(registry.file.connections.len(), 3);
        assert_eq!(
            registry.file.connections[0].identity.label,
            "azure.user@example.test"
        );
    }

    #[test]
    fn corrupt_primary_falls_back_to_backup() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        registry.diff(
            vec![detected("alice", "github.com", "hosts.yml", None)],
            None,
        );
        registry.save().unwrap();
        fs::copy(
            dir.path().join("registry.json"),
            dir.path().join("registry.json.bak"),
        )
        .unwrap();
        fs::write(dir.path().join("registry.json"), "{not-json").unwrap();
        let recovered = Registry::load(dir.path()).unwrap();
        assert!(recovered.history_reset_notice);
        assert_eq!(recovered.file.connections.len(), 1);
    }

    #[test]
    fn legacy_missing_status_without_validation_never_authorizes_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        registry.diff(
            vec![detected("legacy", "github.com", "hosts.yml", None)],
            None,
        );
        let mut json = serde_json::to_value(&registry.file).unwrap();
        let row = &mut json["connections"][0];
        row.as_object_mut().unwrap().remove("validation");
        row["status"] = serde_json::json!("missing");
        row["identity"]["isActiveIdentity"] = serde_json::json!(true);
        fs::write(
            dir.path().join("registry.json"),
            serde_json::to_vec(&json).unwrap(),
        )
        .unwrap();
        let mut restored = Registry::load(dir.path()).unwrap();
        let row = &restored.file.connections[0];
        assert_eq!(row.validation.availability, Availability::Unknown);
        assert_eq!(row.validation.usage, crate::models::Usage::Unknown);
        assert!(row.validation.checked_at.is_none());
        assert!(!row.identity.is_active_identity);
        assert!(!row.removable());
        assert_eq!(restored.purge_missing(), 0);
    }

    #[test]
    fn active_auto_detected_rows_are_not_removable() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        registry.diff(
            vec![detected("alice", "github.com", "hosts.yml", None)],
            None,
        );
        let id = registry.file.connections[0].id.clone();
        assert!(matches!(
            registry.remove(&id),
            Err(RegistryError::NotRemovable)
        ));
    }

    #[test]
    fn retired_provider_rows_are_hidden_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::load(dir.path()).unwrap();
        let row = detected("Claude Code", "localhost", ".claude.json", None);
        registry.file.connections.push(Connection {
            id: "old-mcp-row".to_string(),
            provider: "mcp-claude-code".to_string(),
            provider_name: "Claude Code".to_string(),
            identity: row.identity,
            source: row.source,
            status: ConnectionStatus::Active,
            validation: ConnectionValidation::default(),
            fingerprint: None,
            first_seen: "2026-08-06T00:00:00.000Z".to_string(),
            last_seen: "2026-08-06T00:00:00.000Z".to_string(),
            hidden: false,
            seen: true,
            meta: BTreeMap::new(),
        });

        let snapshot = registry.snapshot(Vec::new(), WatcherHealth::Ok);
        assert!(snapshot.connections.is_empty());
        registry.diff(Vec::new(), None);
        assert!(registry.file.connections.is_empty());
    }
}
