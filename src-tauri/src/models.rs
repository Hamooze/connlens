use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn is_retired_provider_id(provider: &str) -> bool {
    let provider = provider.to_ascii_lowercase();
    provider == "mcp"
        || provider.starts_with("mcp-")
        || provider == "claude"
        || provider.starts_with("claude-")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Active,
    Changed,
    Missing,
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    ConfigFile,
    CredentialManager,
    EnvVar,
    Cli,
    AgentRegistered,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub label: String,
    pub host: Option<String>,
    pub scope: Option<String>,
    #[serde(default)]
    pub is_active_identity: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSource {
    pub source_type: SourceType,
    pub path: Option<String>,
    pub descriptor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub provider: String,
    pub provider_name: String,
    pub identity: Identity,
    pub source: ConnectionSource,
    pub status: ConnectionStatus,
    pub fingerprint: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default = "default_seen")]
    pub seen: bool,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

fn default_seen() -> bool {
    true
}

impl Connection {
    pub fn removable(&self) -> bool {
        self.status == ConnectionStatus::Missing
            || matches!(
                self.source.source_type,
                SourceType::Cli | SourceType::AgentRegistered
            )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedConnection {
    pub provider: String,
    pub provider_name: String,
    pub identity: Identity,
    pub source: ConnectionSource,
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderError {
    pub provider: String,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

impl ErrorPayload {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(
        code: impl Into<String>,
        message: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: Some(detail.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotConnection {
    #[serde(flatten)]
    pub connection: Connection,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u8,
    pub connections: Vec<SnapshotConnection>,
    pub settings: Settings,
    pub last_scan: Option<String>,
    pub provider_errors: Vec<ProviderError>,
    pub watcher_health: WatcherHealth,
    pub history_reset_notice: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WatcherHealth {
    Ok,
    Degraded,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_true")]
    pub watchers_enabled: bool,
    #[serde(default = "default_poll_minutes")]
    pub poll_minutes: u16,
    #[serde(default = "default_true")]
    pub toasts_enabled: bool,
    #[serde(default = "default_true")]
    pub probes_enabled: bool,
    #[serde(default)]
    pub provider_toggles: BTreeMap<String, bool>,
    #[serde(default)]
    pub project_roots: Vec<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub collapsed_providers: BTreeMap<String, bool>,
    #[serde(default)]
    pub show_hidden: bool,
    #[serde(default)]
    pub history_reset_notice_dismissed: bool,
    #[serde(default)]
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            watchers_enabled: true,
            poll_minutes: 10,
            toasts_enabled: true,
            probes_enabled: true,
            provider_toggles: BTreeMap::new(),
            project_roots: Vec::new(),
            theme: default_theme(),
            collapsed_providers: BTreeMap::new(),
            show_hidden: false,
            history_reset_notice_dismissed: false,
            autostart: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_poll_minutes() -> u16 {
    10
}

fn default_theme() -> String {
    "dark".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegistryFile {
    pub schema_version: u8,
    pub last_scan: Option<String>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub settings: Settings,
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            last_scan: None,
            connections: Vec::new(),
            settings: Settings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanScope {
    All,
    Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub created: Vec<String>,
    pub changed: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEntry {
    pub provider: String,
    pub label: String,
    pub host: Option<String>,
    pub scope: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}
