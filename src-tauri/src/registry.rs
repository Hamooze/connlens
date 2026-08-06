use crate::models::{
    ChangeSet, Connection, ConnectionStatus, DetectedConnection, ErrorPayload, Identity,
    RegisterEntry, RegistryFile, Settings, Snapshot, SnapshotConnection, SourceType, WatcherHealth,
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
    if let Ok(path) = std::env::var("CONNLENS_HOME") {
        return PathBuf::from(path);
    }

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("ConnLens");
    }

    directories::ProjectDirs::from("com", "brdg", "ConnLens")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".connlens"))
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
    short_hash(&[&detected.provider, &detected.identity.label, host, source])
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
    serde_json::from_str(&text).map_err(|err| RegistryError::Parse(err.to_string()))
}

fn archive_corrupt(path: &Path) {
    if path.exists() {
        let stamp = Utc::now().format("%Y%m%d%H%M%S");
        let archive = path.with_file_name(format!("registry.corrupt-{stamp}.json"));
        let _ = fs::rename(path, archive);
    }
}

impl Registry {
    pub fn load(home: &Path) -> Result<Self, RegistryError> {
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
                Err(err) => return Err(err),
            }
        }

        Ok(Self {
            home: home.to_path_buf(),
            file: RegistryFile::default(),
            history_reset_notice: false,
        })
    }

    pub fn save(&self) -> Result<(), RegistryError> {
        let _guard = lock(&self.home)?;
        fs::create_dir_all(&self.home)?;
        let path = registry_path(&self.home);
        let backup = backup_path(&self.home);
        let temp = self.home.join("registry.json.tmp");

        if path.exists() {
            let _ = fs::copy(&path, &backup);
        }

        let text = serde_json::to_string_pretty(&self.file)
            .map_err(|err| RegistryError::Parse(err.to_string()))?;
        fs::write(&temp, text)?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        fs::rename(temp, path)?;
        Ok(())
    }

    pub fn diff(&mut self, detected: Vec<DetectedConnection>, scope: Option<&str>) -> ChangeSet {
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
            let id = connection_id(&detected);
            seen_ids.insert(id.clone());

            if let Some(existing) = old_by_id.get(&id) {
                let mut updated = existing.clone();
                let fingerprint_changed = existing.fingerprint != detected.fingerprint
                    && existing.fingerprint.is_some()
                    && detected.fingerprint.is_some();
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
                } else {
                    ConnectionStatus::Active
                };
                next.push(updated);
            } else {
                let connection = Connection {
                    id: id.clone(),
                    provider: detected.provider,
                    provider_name: detected.provider_name,
                    identity: detected.identity,
                    source: detected.source,
                    status: ConnectionStatus::Active,
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
            let in_scope = scope
                .map(|provider| existing.provider == provider)
                .unwrap_or(true);
            if !seen_ids.contains(&existing.id) && in_scope {
                let mut missing = existing.clone();
                if missing.status != ConnectionStatus::Missing {
                    changes.missing.push(missing.id.clone());
                }
                missing.status = ConnectionStatus::Missing;
                missing.seen = false;
                next.push(missing);
            } else if !seen_ids.contains(&existing.id) {
                next.push(existing.clone());
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
            provider_errors,
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
            .retain(|connection| connection.status != ConnectionStatus::Missing);
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
            existing.status = ConnectionStatus::Unverified;
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

pub fn with_registry<T>(mut f: impl FnMut(&mut Registry) -> T) -> Result<T, RegistryError> {
    let home = app_home();
    let mut registry = Registry::load(&home)?;
    let result = f(&mut registry);
    registry.save()?;
    Ok(result)
}

pub fn load_snapshot() -> Result<Snapshot, RegistryError> {
    let home = app_home();
    let registry = Registry::load(&home)?;
    let health = if registry.file.settings.watchers_enabled {
        WatcherHealth::Ok
    } else {
        WatcherHealth::Paused
    };
    Ok(registry.snapshot(Vec::new(), health))
}

pub fn update_settings(patch: Settings) -> Result<Settings, RegistryError> {
    with_registry(|registry| {
        registry.file.settings = patch.clone();
        registry.file.settings.clone()
    })
}

pub fn reset_app_data() -> Result<(), RegistryError> {
    let home = app_home();
    if home.exists() {
        let _guard = lock(&home)?;
        let _ = fs::remove_file(registry_path(&home));
        let _ = fs::remove_file(backup_path(&home));
        let _ = fs::remove_dir_all(home.join("inbox"));
        let _ = fs::remove_dir_all(home.join("providers"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConnectionSource, SourceType};

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
        registry.diff(Vec::new(), Some("github"));
        assert_eq!(
            registry.file.connections[0].status,
            ConnectionStatus::Missing
        );
        assert_eq!(registry.purge_missing(), 1);
        assert!(registry.file.connections.is_empty());
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
}
