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
pub enum Availability {
    Available,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Usage {
    Selected,
    Referenced,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionValidation {
    pub availability: Availability,
    pub usage: Usage,
    pub checked_at: Option<String>,
    pub reason: String,
    pub reason_code: String,
}

impl Default for ConnectionValidation {
    fn default() -> Self {
        Self::unknown(
            "not_validated",
            "This entry has not been validated locally yet.",
            None,
        )
    }
}

impl ConnectionValidation {
    pub fn unknown(
        code: impl Into<String>,
        reason: impl Into<String>,
        checked_at: Option<String>,
    ) -> Self {
        Self {
            availability: Availability::Unknown,
            usage: Usage::Unknown,
            checked_at,
            reason: reason.into(),
            reason_code: code.into(),
        }
    }

    pub fn available(checked_at: &str, selected: bool) -> Self {
        Self {
            availability: Availability::Available,
            usage: if selected { Usage::Selected } else { Usage::Referenced },
            checked_at: Some(checked_at.to_string()),
            reason: if selected { "This identity is selected in its local configuration." } else { "A local reference to this entry is present. Actual runtime use and online access are not checked." }.to_string(),
            reason_code: if selected { "selected_locally" } else { "referenced_locally" }.to_string(),
        }
    }

    pub fn missing(checked_at: &str, code: &str, reason: &str) -> Self {
        Self {
            availability: Availability::Missing,
            usage: Usage::Unknown,
            checked_at: Some(checked_at.to_string()),
            reason: reason.to_string(),
            reason_code: code.to_string(),
        }
    }
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
    #[serde(default)]
    pub validation: ConnectionValidation,
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
        self.validation.availability == Availability::Missing
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
    // Retained for older registries; connection discovery is always local.
    #[serde(default)]
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
            probes_enabled: false,
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
    pub provider_errors: Vec<ProviderError>,
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
            provider_errors: Vec::new(),
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
