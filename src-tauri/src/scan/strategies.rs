use crate::credman::CredmanEntry;
use crate::descriptors::{Descriptor, Location};
use crate::models::{ConnectionSource, DetectedConnection, Identity, SourceType};
use crate::scan::secutil;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn source(path: &Path, descriptor: &Descriptor, source_type: SourceType) -> ConnectionSource {
    ConnectionSource {
        source_type,
        path: Some(path.display().to_string()),
        descriptor_id: Some(descriptor.id.clone()),
    }
}

fn detected(
    descriptor: &Descriptor,
    label: String,
    host: Option<String>,
    scope: Option<String>,
    path: &Path,
    fingerprint: Option<String>,
    meta: BTreeMap<String, Value>,
) -> DetectedConnection {
    DetectedConnection {
        provider: descriptor.id.clone(),
        provider_name: descriptor.name.clone(),
        identity: Identity {
            label,
            host,
            scope,
            is_active_identity: false,
        },
        source: source(path, descriptor, SourceType::ConfigFile),
        fingerprint,
        meta,
    }
}

fn read_nearby_json(path: &Path, config_root: &Path) -> Option<Value> {
    let root = config_root.canonicalize().ok()?;
    let canonical = path.canonicalize().ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    super::parsers::parse(&canonical, super::parsers::Format::Json)
        .ok()
        .map(|doc| doc.value)
}

const ACCOUNT_LABEL_KEYS: &[&str] = &[
    "email",
    "user_email",
    "user_id",
    "userId",
    "user",
    "username",
    "login",
    "client_email",
    "displayName",
    "display_name",
    "accountName",
    "account_name",
    "sso_account_name",
    "account",
    "accountId",
    "account_id",
    "sso_account_id",
    "team",
    "teamName",
    "team_name",
    "teamSlug",
    "team_slug",
    "currentTeam",
    "current_team",
    "organization",
    "organizationName",
    "organization_name",
    "org",
    "orgName",
    "org_name",
    "tenant",
    "tenantName",
    "tenant_name",
];

const NESTED_IDENTITY_KEYS: &[&str] = &[
    "email",
    "user_email",
    "user_id",
    "userId",
    "username",
    "login",
    "client_email",
    "displayName",
    "display_name",
    "name",
];

const ACCOUNT_SCOPE_KEYS: &[&str] = &[
    "currentTeam",
    "current_team",
    "teamId",
    "team_id",
    "teamSlug",
    "team_slug",
    "orgId",
    "org_id",
    "organizationId",
    "organization_id",
    "accountId",
    "account_id",
    "sso_account_id",
    "tenantId",
    "tenant_id",
    "subscriptionId",
    "subscription_id",
];

const CONTEXT_SCOPE_KEYS: &[&str] = &[
    "currentTeam",
    "current_team",
    "teamId",
    "team_id",
    "teamSlug",
    "team_slug",
    "orgId",
    "org_id",
    "organizationId",
    "organization_id",
    "accountId",
    "account_id",
    "sso_account_id",
    "tenantId",
    "tenant_id",
    "subscriptionId",
    "subscription_id",
    "project",
    "projectId",
    "project_id",
    "region",
];

const PROJECT_CONTEXT_KEYS: &[&str] = &[
    "project",
    "projectId",
    "project_id",
    "projectName",
    "project_name",
    "default_project",
    "quota_project_id",
    "site_id",
    "workspace_id",
];

fn first_account_label(value: &Value) -> Option<String> {
    first_string_for_keys(value, ACCOUNT_LABEL_KEYS)
        .or_else(|| role_arn_label(value))
        .or_else(|| {
            if has_any_key(value, PROJECT_CONTEXT_KEYS, 0) {
                None
            } else {
                first_string_for_keys(value, &["name"])
            }
        })
}

fn first_scope(value: &Value) -> Option<String> {
    first_string_for_keys(value, CONTEXT_SCOPE_KEYS)
}

fn first_account_scope(value: &Value) -> Option<String> {
    first_string_for_keys(value, ACCOUNT_SCOPE_KEYS)
}

fn first_string_for_keys(value: &Value, keys: &[&str]) -> Option<String> {
    find_string_for_keys(value, keys, 0)
}

fn find_string_for_keys(value: &Value, keys: &[&str], depth: u8) -> Option<String> {
    if depth > 5 {
        return None;
    }

    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(found) = map
                    .get(*key)
                    .and_then(|value| identity_string(value, depth))
                {
                    return Some(found);
                }
            }
            map.values()
                .find_map(|value| find_string_for_keys(value, keys, depth + 1))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_string_for_keys(value, keys, depth + 1)),
        _ => None,
    }
}

fn identity_string(value: &Value, depth: u8) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty() && text.len() <= 160).then(|| text.to_string())
        }
        Value::Object(_) | Value::Array(_) => {
            find_string_for_keys(value, NESTED_IDENTITY_KEYS, depth + 1)
        }
        _ => None,
    }
}

fn has_any_key(value: &Value, keys: &[&str], depth: u8) -> bool {
    if depth > 5 {
        return false;
    }

    match value {
        Value::Object(map) => {
            keys.iter().any(|key| map.contains_key(*key))
                || map
                    .values()
                    .any(|value| has_any_key(value, keys, depth + 1))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| has_any_key(value, keys, depth + 1)),
        _ => false,
    }
}

fn role_arn_label(value: &Value) -> Option<String> {
    first_string_for_keys(value, &["role_arn", "roleArn"]).and_then(|arn| {
        let mut parts = arn.split(':');
        let account_id = parts.nth(4)?;
        let role_name = arn.rsplit('/').next().unwrap_or("role");
        if account_id.is_empty() {
            None
        } else {
            Some(format!("{account_id}/{role_name}"))
        }
    })
}

fn insert_if_string(meta: &mut BTreeMap<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        meta.insert(key.to_string(), Value::String(value));
    }
}

fn is_uuid_like(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    let expected = [8, 4, 4, 4, 12];
    parts.len() == expected.len()
        && parts
            .iter()
            .zip(expected)
            .all(|(part, len)| part.len() == len && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn is_vercel_id_like(value: &str) -> bool {
    value.starts_with("team_")
        || value.starts_with("usr_")
        || (value.len() >= 20
            && value.len() <= 40
            && value.chars().all(|ch| ch.is_ascii_alphanumeric()))
}

fn clean_account_label(provider: &str, value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let raw_id = match provider {
        "neon" => is_uuid_like(trimmed),
        "vercel" => is_vercel_id_like(trimmed),
        _ => false,
    };
    (!raw_id).then(|| trimmed.to_string())
}

fn first_clean_string_for_keys(provider: &str, value: &Value, keys: &[&str]) -> Option<String> {
    first_string_for_keys(value, keys).and_then(|label| clean_account_label(provider, label))
}

pub mod github {
    use super::*;

    pub fn gh_hosts(
        descriptor: &Descriptor,
        _location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let mut out = Vec::new();
        let Some(hosts) = value.as_object() else {
            return out;
        };

        for (host, host_value) in hosts {
            let Some(host_obj) = host_value.as_object() else {
                continue;
            };
            let active_user = host_obj.get("user").and_then(Value::as_str);
            if let Some(users) = host_obj.get("users").and_then(Value::as_object) {
                for (user, user_value) in users {
                    let token = user_value
                        .get("oauth_token")
                        .or_else(|| user_value.get("oauthToken"))
                        .and_then(Value::as_str);
                    let mut connection = super::detected(
                        descriptor,
                        user.to_string(),
                        Some(host.to_string()),
                        None,
                        path,
                        token.map(|token| secutil::fingerprint(token.as_bytes())),
                        BTreeMap::new(),
                    );
                    connection.identity.is_active_identity = active_user == Some(user.as_str());
                    out.push(connection);
                }
            } else if let Some(user) = active_user {
                let token = host_obj.get("oauth_token").and_then(Value::as_str);
                let mut connection = super::detected(
                    descriptor,
                    user.to_string(),
                    Some(host.to_string()),
                    None,
                    path,
                    token.map(|token| secutil::fingerprint(token.as_bytes())),
                    BTreeMap::new(),
                );
                connection.identity.is_active_identity = true;
                out.push(connection);
            }
        }
        out
    }

    pub fn from_credman(
        descriptor: &Descriptor,
        entries: Vec<CredmanEntry>,
    ) -> Vec<DetectedConnection> {
        entries
            .into_iter()
            .map(|entry| {
                let host = entry
                    .target
                    .strip_prefix("git:")
                    .unwrap_or(&entry.target)
                    .to_string();
                DetectedConnection {
                    provider: descriptor.id.clone(),
                    provider_name: descriptor.name.clone(),
                    identity: Identity {
                        label: entry.username.unwrap_or_else(|| host.clone()),
                        host: Some(host),
                        scope: Some("Credential Manager".to_string()),
                        is_active_identity: false,
                    },
                    source: ConnectionSource {
                        source_type: SourceType::CredentialManager,
                        path: None,
                        descriptor_id: Some(descriptor.id.clone()),
                    },
                    fingerprint: None,
                    meta: BTreeMap::new(),
                }
            })
            .collect()
    }
}

pub mod tokens {
    use super::*;

    pub fn token_file(
        descriptor: &Descriptor,
        location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let token = token_value(value);
        let Some(token) = token.filter(|token| !token.is_empty()) else {
            return Vec::new();
        };
        let fingerprint = secutil::fingerprint(token.as_bytes());
        let suffix = fingerprint.chars().rev().take(4).collect::<String>();
        let suffix = suffix.chars().rev().collect::<String>();
        let scope = first_scope(value).or_else(|| location.scope.clone());
        let label = first_account_label(value)
            .or_else(|| first_account_scope(value))
            .unwrap_or_else(|| format!("{} (...{suffix})", descriptor.name));
        let mut meta = BTreeMap::new();
        if let Some(confidence) = &location.confidence {
            meta.insert("confidence".to_string(), Value::String(confidence.clone()));
        }
        vec![super::detected(
            descriptor,
            label,
            descriptor.dashboard_url.clone(),
            scope,
            path,
            Some(fingerprint),
            meta,
        )]
    }

    pub(super) fn token_value(value: &Value) -> Option<&str> {
        find_token(value, 0)
    }

    fn find_token(value: &Value, depth: u8) -> Option<&str> {
        if depth > 5 {
            return None;
        }

        match value {
            Value::Object(map) => {
                for key in [
                    "token",
                    "accessToken",
                    "access_token",
                    "apiKey",
                    "api_key",
                    "authToken",
                    "auth_token",
                    "refreshToken",
                    "refresh_token",
                    "secret_key",
                    "live_mode_api_key",
                    "test_mode_api_key",
                    "oauth_token",
                ] {
                    if let Some(token) = map.get(key).and_then(Value::as_str) {
                        return Some(token);
                    }
                }
                map.values().find_map(|value| find_token(value, depth + 1))
            }
            Value::Array(values) => values.iter().find_map(|value| find_token(value, depth + 1)),
            _ => None,
        }
    }
}

pub mod neon {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct NeonProfile {
        name: Option<String>,
        label: Option<String>,
    }

    pub fn auth_file(
        descriptor: &Descriptor,
        location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let Some(token) = super::tokens::token_value(value).filter(|token| !token.is_empty())
        else {
            return Vec::new();
        };

        let id_token_payload = value
            .get("id_token")
            .and_then(Value::as_str)
            .and_then(jwt_payload);
        let user_id = value
            .get("user_id")
            .or_else(|| value.get("userId"))
            .and_then(Value::as_str)
            .or_else(|| {
                id_token_payload
                    .as_ref()
                    .and_then(|payload| payload.get("sub"))
                    .and_then(Value::as_str)
            });
        let profile = profile_for_credentials(path, user_id);

        let label = profile
            .label
            .clone()
            .or_else(|| id_token_payload.as_ref().and_then(friendly_label))
            .or_else(|| friendly_label(value))
            .or_else(|| {
                profile
                    .name
                    .as_ref()
                    .filter(|name| !name.eq_ignore_ascii_case("default"))
                    .cloned()
            })
            .unwrap_or_else(|| "Neon account".to_string());
        let fingerprint = user_id
            .or_else(|| value.get("refresh_token").and_then(Value::as_str))
            .unwrap_or(token);
        let scope = profile
            .name
            .as_ref()
            .map(|name| {
                if name.eq_ignore_ascii_case("default") {
                    "Default profile".to_string()
                } else {
                    format!("profile: {name}")
                }
            })
            .or_else(|| {
                value
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|kind| match kind {
                        "oauth" => "OAuth profile".to_string(),
                        other => format!("{other} profile"),
                    })
            })
            .or_else(|| location.scope.clone())
            .or_else(|| Some("Neon account".to_string()));

        let mut meta = BTreeMap::from([("kind".to_string(), json!("auth_profile"))]);
        if let Some(profile_name) = profile.name {
            meta.insert("profile".to_string(), Value::String(profile_name));
        }
        if let Some(user_id) = user_id {
            meta.insert(
                "userIdFingerprint".to_string(),
                Value::String(secutil::fingerprint(user_id.as_bytes())),
            );
        }
        if let Some(expires_at) = value.get("expires_at").and_then(Value::as_i64) {
            meta.insert("expiresAt".to_string(), json!(expires_at));
        }
        if let Some(token_type) = value.get("token_type").and_then(Value::as_str) {
            meta.insert(
                "tokenType".to_string(),
                Value::String(token_type.to_string()),
            );
        }
        if let Some(confidence) = &location.confidence {
            meta.insert("confidence".to_string(), Value::String(confidence.clone()));
        }

        let mut row = super::detected(
            descriptor,
            label,
            descriptor.dashboard_url.clone(),
            scope,
            path,
            Some(secutil::fingerprint(fingerprint.as_bytes())),
            meta,
        );
        row.identity.is_active_identity = true;
        vec![row]
    }

    fn friendly_label(value: &Value) -> Option<String> {
        first_clean_string_for_keys(
            "neon",
            value,
            &[
                "name",
                "email",
                "preferred_username",
                "nickname",
                "username",
                "login",
                "displayName",
                "display_name",
            ],
        )
    }

    fn profile_for_credentials(path: &Path, user_id: Option<&str>) -> NeonProfile {
        candidate_profiles(path)
            .into_iter()
            .find_map(|profiles_path| read_profile_info(&profiles_path, path, user_id))
            .unwrap_or_default()
    }

    fn candidate_profiles(path: &Path) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(parent) = path.parent() {
            candidates.push(parent.join("profiles.json"));
            if parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("neonctl"))
            {
                if let Some(config_root) = parent.parent() {
                    candidates.push(config_root.join("neon").join("profiles.json"));
                }
            }
        }
        dedupe_paths(candidates)
    }

    fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut seen = std::collections::BTreeSet::new();
        paths
            .into_iter()
            .filter(|path| seen.insert(path.display().to_string().to_ascii_lowercase()))
            .collect()
    }

    fn read_profile_info(
        profiles_path: &Path,
        credentials_path: &Path,
        user_id: Option<&str>,
    ) -> Option<NeonProfile> {
        let parent = credentials_path.parent()?;
        let root = if parent.file_name().and_then(|name| name.to_str()) == Some("neonctl") {
            parent.parent()?
        } else {
            parent
        };
        let value = read_nearby_json(profiles_path, root)?;
        let profiles = value.get("profiles").and_then(Value::as_object)?;
        let mut matching_profile = NeonProfile::default();
        let mut label_for_user = None;

        for (name, profile) in profiles {
            let profile_label = profile
                .get("label")
                .and_then(Value::as_str)
                .and_then(|label| clean_account_label("neon", label.to_string()));
            let profile_user_id = profile.get("userId").and_then(Value::as_str);
            if label_for_user.is_none()
                && user_id.is_some()
                && user_id == profile_user_id
                && profile_label.is_some()
            {
                label_for_user = profile_label.clone();
            }

            let Some(credentials) = profile.get("credentials").and_then(Value::as_str) else {
                continue;
            };
            let candidate = profiles_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(credentials);
            if !same_path(credentials_path, &candidate) {
                continue;
            }

            matching_profile.name = Some(name.to_string());
            matching_profile.label = profile_label;
        }

        if matching_profile.label.is_none() {
            matching_profile.label = label_for_user;
        }
        (matching_profile.name.is_some() || matching_profile.label.is_some())
            .then_some(matching_profile)
    }

    fn same_path(left: &Path, right: &Path) -> bool {
        canonical_or_original(left) == canonical_or_original(right)
    }

    fn canonical_or_original(path: &Path) -> PathBuf {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn jwt_payload(token: &str) -> Option<Value> {
        let payload = token.split('.').nth(1)?;
        let bytes = URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

pub mod shopify {
    use super::*;

    pub fn account_info(
        descriptor: &Descriptor,
        location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let Some(accounts) = value.as_object() else {
            return Vec::new();
        };

        accounts
            .iter()
            .filter_map(|(user_id, account)| {
                let info = account.get("info").unwrap_or(account);
                let label = first_account_label(info)
                    .or_else(|| first_account_label(account))
                    .unwrap_or_else(|| user_id.to_string());
                if label.trim().is_empty() {
                    return None;
                }

                let account_type = info
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("ShopifyAccount");
                let scope = match account_type {
                    "ServiceAccount" => "Service account",
                    _ => "Shopify account",
                };
                let mut meta = BTreeMap::from([
                    ("kind".to_string(), json!("account_info")),
                    ("accountType".to_string(), json!(account_type)),
                    (
                        "userIdFingerprint".to_string(),
                        json!(secutil::fingerprint(user_id.as_bytes())),
                    ),
                ]);
                if let Some(loaded_at) = account.get("loadedAt").and_then(Value::as_str) {
                    meta.insert("loadedAt".to_string(), Value::String(loaded_at.to_string()));
                }
                if let Some(confidence) = &location.confidence {
                    meta.insert("confidence".to_string(), Value::String(confidence.clone()));
                }

                let fingerprint =
                    secutil::fingerprint(format!("shopify-account:{user_id}:{label}").as_bytes());
                let mut row = super::detected(
                    descriptor,
                    label,
                    descriptor.dashboard_url.clone(),
                    Some(scope.to_string()),
                    path,
                    Some(fingerprint),
                    meta,
                );
                row.identity.is_active_identity = true;
                Some(row)
            })
            .collect()
    }
}

pub mod vercel {
    use super::*;

    pub fn auth_file(
        descriptor: &Descriptor,
        location: &Location,
        path: &Path,
        value: &Value,
        _probes_enabled: bool,
    ) -> Vec<DetectedConnection> {
        let Some(token) = vercel_token(value).filter(|token| !token.is_empty()) else {
            return Vec::new();
        };

        let config = read_sibling_config(path);
        let profile = profile_alias(path);
        let user_id = value.get("userId").and_then(Value::as_str);
        let current_team = config
            .as_ref()
            .and_then(|config| config.get("currentTeam"))
            .and_then(Value::as_str);
        let fingerprint = secutil::fingerprint(token.as_bytes());
        let label = primary_label(value)
            .or_else(|| profile.clone())
            .unwrap_or_else(|| "Vercel account".to_string());
        let scope = profile
            .as_ref()
            .map(|profile| {
                if *profile == label {
                    "Vercel profile".to_string()
                } else {
                    format!("profile: {profile}")
                }
            })
            .or_else(|| current_team.map(|_| "Team context".to_string()))
            .or_else(|| Some("Vercel account".to_string()));
        let mut meta = BTreeMap::from([("kind".to_string(), json!("auth_profile"))]);
        if let Some(profile) = profile {
            meta.insert("profile".to_string(), Value::String(profile));
        }
        if let Some(user_id) = user_id {
            meta.insert(
                "userIdFingerprint".to_string(),
                Value::String(secutil::fingerprint(user_id.as_bytes())),
            );
        }
        if let Some(current_team) = current_team {
            meta.insert(
                "teamFingerprint".to_string(),
                Value::String(secutil::fingerprint(current_team.as_bytes())),
            );
        }
        if let Some(expires_at) = value.get("expiresAt").and_then(Value::as_i64) {
            meta.insert("expiresAt".to_string(), json!(expires_at));
        }
        if let Some(confidence) = &location.confidence {
            meta.insert("confidence".to_string(), Value::String(confidence.clone()));
        }

        vec![super::detected(
            descriptor,
            label,
            descriptor.dashboard_url.clone(),
            scope,
            path,
            Some(fingerprint),
            meta,
        )]
    }

    fn vercel_token(value: &Value) -> Option<&str> {
        value
            .get("token")
            .or_else(|| value.get("accessToken"))
            .or_else(|| value.get("access_token"))
            .and_then(Value::as_str)
    }

    fn primary_label(value: &Value) -> Option<String> {
        first_clean_string_for_keys(
            "vercel",
            value,
            &[
                "email",
                "user_email",
                "username",
                "login",
                "name",
                "displayName",
                "display_name",
                "teamName",
                "team_name",
                "teamSlug",
                "team_slug",
                "orgName",
                "org_name",
                "organizationName",
                "organization_name",
            ],
        )
    }

    fn profile_alias(path: &Path) -> Option<String> {
        let profile = path.parent()?.file_name()?.to_str()?;
        match profile {
            "com.vercel.cli" | ".vercel" => None,
            value if value.trim().is_empty() => None,
            value => Some(value.to_string()),
        }
    }

    fn read_sibling_config(path: &Path) -> Option<Value> {
        let config_path = path.with_file_name("config.json");
        read_nearby_json(&config_path, path.parent()?)
    }
}

pub mod profiles {
    use super::*;

    pub fn profile_file(
        descriptor: &Descriptor,
        location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let mut out = Vec::new();
        let Some(sections) = value.as_object() else {
            return out;
        };

        for (section, section_value) in sections {
            let Some(section_obj) = section_value.as_object() else {
                continue;
            };
            if section_obj.is_empty() {
                continue;
            }
            let profile = section
                .strip_prefix("profile ")
                .unwrap_or(section)
                .to_string();
            let label = first_account_label(section_value).unwrap_or_else(|| profile.clone());
            let host = section_obj
                .get("host")
                .or_else(|| section_obj.get("endpoint_url"))
                .or_else(|| section_obj.get("tenantId"))
                .or_else(|| section_obj.get("tenant_id"))
                .or_else(|| section_obj.get("subscriptionId"))
                .or_else(|| section_obj.get("subscription_id"))
                .or_else(|| section_obj.get("sso_account_id"))
                .or_else(|| section_obj.get("account_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| descriptor.dashboard_url.clone());
            let fingerprint = fingerprint_from_object(section_obj);
            let mut meta = BTreeMap::new();
            if let Some(confidence) = &location.confidence {
                meta.insert("confidence".to_string(), Value::String(confidence.clone()));
            }
            meta.insert("profile".to_string(), Value::String(profile.clone()));
            if section_obj.contains_key("region") {
                meta.insert("region".to_string(), section_obj["region"].clone());
            }
            for key in PROJECT_CONTEXT_KEYS {
                if let Some(value) = section_obj.get(*key).and_then(Value::as_str) {
                    meta.insert((*key).to_string(), Value::String(value.to_string()));
                }
            }
            insert_if_string(&mut meta, "roleAccount", role_arn_label(section_value));
            let scope = if label == profile {
                first_scope(section_value).or_else(|| location.scope.clone())
            } else {
                Some(profile)
            };
            out.push(super::detected(
                descriptor,
                label,
                host,
                scope,
                path,
                fingerprint,
                meta,
            ));
        }
        out
    }

    pub fn docker_auths(
        descriptor: &Descriptor,
        _location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let Some(auths) = value.get("auths").and_then(Value::as_object) else {
            return Vec::new();
        };

        auths
            .iter()
            .map(|(host, auth)| {
                let fingerprint = auth
                    .get("auth")
                    .or_else(|| auth.get("identitytoken"))
                    .and_then(Value::as_str)
                    .map(|token| secutil::fingerprint(token.as_bytes()));
                super::detected(
                    descriptor,
                    host.trim_start_matches("https://").to_string(),
                    Some(host.to_string()),
                    value
                        .get("credsStore")
                        .and_then(Value::as_str)
                        .map(|store| format!("credential store: {store}")),
                    path,
                    fingerprint,
                    BTreeMap::new(),
                )
            })
            .collect()
    }

    pub fn vercel_project(
        descriptor: &Descriptor,
        _location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let project_id = value.get("projectId").and_then(Value::as_str);
        let org_id = value.get("orgId").and_then(Value::as_str);
        if project_id.is_none() && org_id.is_none() {
            return Vec::new();
        }

        let label = value
            .get("projectName")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| project_folder_name(path))
            .unwrap_or_else(|| "Vercel project".to_string());
        let fingerprint = project_id
            .or(org_id)
            .map(|id| secutil::fingerprint(id.as_bytes()));
        let mut meta = BTreeMap::from([("kind".to_string(), json!("project_link"))]);
        if let Some(project_id) = project_id {
            meta.insert(
                "projectFingerprint".to_string(),
                Value::String(secutil::fingerprint(project_id.as_bytes())),
            );
        }
        if let Some(org_id) = org_id {
            meta.insert(
                "orgFingerprint".to_string(),
                Value::String(secutil::fingerprint(org_id.as_bytes())),
            );
        }

        vec![super::detected(
            descriptor,
            label,
            descriptor.dashboard_url.clone(),
            Some("Linked project".to_string()),
            path,
            fingerprint,
            meta,
        )]
    }

    pub fn npmrc(
        descriptor: &Descriptor,
        location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let Some(defaults) = value.get("default").and_then(Value::as_object) else {
            return Vec::new();
        };

        let mut rows = Vec::new();
        for (key, value) in defaults {
            let key_lower = key.to_ascii_lowercase();
            if !key_lower.ends_with("_authtoken")
                && !key_lower.ends_with("_auth")
                && !key_lower.ends_with("_password")
            {
                continue;
            }
            let Some(token) = value.as_str().filter(|value| !value.is_empty()) else {
                continue;
            };
            let host = key
                .split(':')
                .next()
                .unwrap_or("//registry.npmjs.org/")
                .trim_start_matches("//")
                .trim_end_matches('/')
                .to_string();
            rows.push(super::detected(
                descriptor,
                host.clone(),
                Some(host),
                location.scope.clone(),
                path,
                Some(secutil::fingerprint(token.as_bytes())),
                BTreeMap::new(),
            ));
        }
        rows
    }

    pub fn glab_config(
        descriptor: &Descriptor,
        _location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let Some(hosts) = value.get("hosts").and_then(Value::as_object) else {
            return Vec::new();
        };

        hosts
            .iter()
            .map(|(host, host_value)| {
                let label = host_value
                    .get("user")
                    .or_else(|| host_value.get("username"))
                    .and_then(Value::as_str)
                    .unwrap_or(host)
                    .to_string();
                let fingerprint = host_value
                    .get("token")
                    .or_else(|| host_value.get("oauth_token"))
                    .and_then(Value::as_str)
                    .map(|token| secutil::fingerprint(token.as_bytes()));
                super::detected(
                    descriptor,
                    label,
                    Some(host.to_string()),
                    None,
                    path,
                    fingerprint,
                    BTreeMap::new(),
                )
            })
            .collect()
    }

    pub fn azure_profile(
        descriptor: &Descriptor,
        _location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let Some(subscriptions) = value.get("subscriptions").and_then(Value::as_array) else {
            return Vec::new();
        };

        subscriptions
            .iter()
            .filter_map(|subscription| {
                let subscription_name = subscription
                    .get("name")
                    .or_else(|| subscription.get("id"))
                    .and_then(Value::as_str)?;
                let label = first_account_label(subscription)
                    .or_else(|| {
                        subscription
                            .get("user")
                            .and_then(Value::as_object)
                            .and_then(|user| user.get("name"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| subscription_name.to_string());
                let user = subscription
                    .get("user")
                    .and_then(Value::as_object)
                    .and_then(|user| user.get("name"))
                    .and_then(Value::as_str);
                let mut meta = BTreeMap::new();
                if let Some(user) = user {
                    meta.insert("user".to_string(), Value::String(user.to_string()));
                }
                meta.insert(
                    "subscription".to_string(),
                    Value::String(subscription_name.to_string()),
                );
                Some(super::detected(
                    descriptor,
                    label,
                    subscription
                        .get("id")
                        .or_else(|| subscription.get("subscriptionId"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| descriptor.dashboard_url.clone()),
                    Some(subscription_name.to_string()),
                    path,
                    subscription
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|id| secutil::fingerprint(id.as_bytes())),
                    meta,
                ))
            })
            .collect()
    }

    fn fingerprint_from_object(map: &serde_json::Map<String, Value>) -> Option<String> {
        for key in [
            "aws_access_key_id",
            "aws_session_token",
            "token",
            "access_token",
            "auth_token",
            "api_key",
            "_authToken",
            "refresh_token",
            "project_id",
            "account_id",
            "client_id",
        ] {
            if let Some(value) = map.get(key).and_then(Value::as_str) {
                return Some(secutil::fingerprint(value.as_bytes()));
            }
        }
        None
    }

    fn project_folder_name(path: &Path) -> Option<String> {
        path.parent()?
            .parent()?
            .file_name()?
            .to_str()
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    }
}

pub mod envvars {
    use super::*;
    use crate::descriptors::Descriptor;

    pub fn scan(descriptors: &[Descriptor]) -> Vec<DetectedConnection> {
        if std::env::var_os("CONNLENS_HOME").is_some() {
            return Vec::new();
        }
        let names = descriptors
            .iter()
            .flat_map(|descriptor| {
                descriptor
                    .env_vars
                    .iter()
                    .map(|variable| variable.name.as_str())
            })
            .collect::<std::collections::BTreeSet<_>>();
        let values = read_env_hives(&names);
        scan_from_map(descriptors, &values)
    }

    pub fn scan_from_map(
        descriptors: &[Descriptor],
        values: &BTreeMap<String, String>,
    ) -> Vec<DetectedConnection> {
        let mut out = Vec::new();
        for descriptor in descriptors {
            for env in &descriptor.env_vars {
                if let Some(value) = values.get(&env.name) {
                    if value.is_empty() {
                        continue;
                    }
                    out.push(DetectedConnection {
                        provider: descriptor.id.clone(),
                        provider_name: descriptor.name.clone(),
                        identity: Identity {
                            label: env
                                .label
                                .clone()
                                .unwrap_or_else(|| format!("{} env var", env.name)),
                            host: None,
                            scope: Some(env.name.clone()),
                            is_active_identity: false,
                        },
                        source: ConnectionSource {
                            source_type: SourceType::EnvVar,
                            path: Some(env.name.clone()),
                            descriptor_id: Some(descriptor.id.clone()),
                        },
                        fingerprint: Some(secutil::fingerprint(value.as_bytes())),
                        meta: BTreeMap::from([(
                            "source".to_string(),
                            json!(if cfg!(windows) {
                                "registry_env"
                            } else {
                                "process_env"
                            }),
                        )]),
                    });
                }
            }
        }
        out
    }

    #[cfg(windows)]
    fn read_env_hives(names: &std::collections::BTreeSet<&str>) -> BTreeMap<String, String> {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;

        let mut values = BTreeMap::new();
        for (hive, subkey) in [
            (HKEY_CURRENT_USER, "Environment"),
            (
                HKEY_LOCAL_MACHINE,
                r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            ),
        ] {
            if let Ok(key) = RegKey::predef(hive).open_subkey(subkey) {
                for name in names {
                    if let Ok(value) = key.get_value::<String, _>(name) {
                        values.insert((*name).to_string(), value);
                    }
                }
            }
        }
        values
    }

    #[cfg(not(windows))]
    fn read_env_hives(names: &std::collections::BTreeSet<&str>) -> BTreeMap<String, String> {
        names
            .iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| ((*name).to_string(), value))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    fn descriptor(id: &str, name: &str) -> Descriptor {
        Descriptor {
            id: id.to_string(),
            name: name.to_string(),
            dashboard_url: None,
            credman_filter: None,
            env_vars: Vec::new(),
            locations: Vec::new(),
        }
    }

    fn unsigned_jwt(payload: Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload.to_string());
        format!("{header}.{payload}.")
    }

    #[test]
    fn gh_multi_account_shape_yields_active_flags() {
        let descriptor = descriptor("github", "GitHub");
        let value = serde_json::json!({
            "github.com": {
                "user": "alice",
                "users": {
                    "alice": {"oauth_token": "ghp_aaaaaaaaaaaaaaaa"},
                    "bob": {"oauth_token": "gho_bbbbbbbbbbbbbbbb"}
                }
            }
        });
        let rows = github::gh_hosts(
            &descriptor,
            &Location {
                path: "hosts.yml".to_string(),
                format: crate::scan::parsers::Format::Yaml,
                strategy: "gh_hosts".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("hosts.yml"),
            &value,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .filter(|row| row.identity.is_active_identity)
                .count(),
            1
        );
        assert!(!format!("{rows:?}").contains("ghp_"));
    }

    #[test]
    fn shopify_account_info_uses_cached_email_label() {
        let mut descriptor = descriptor("shopify", "Shopify");
        descriptor.dashboard_url = Some("https://admin.shopify.com".to_string());
        let value = serde_json::json!({
            "user-uuid-123": {
                "info": {
                    "type": "UserAccount",
                    "email": "shopify.user@example.test"
                },
                "loadedAt": "2026-08-08T22:33:49.604Z"
            }
        });
        let rows = shopify::account_info(
            &descriptor,
            &Location {
                path: "config.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "shopify_account_info".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: Some("account_cache".to_string()),
            },
            Path::new("shopify-app-account-info-nodejs/Config/config.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "shopify.user@example.test");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("Shopify account"));
        assert_eq!(
            rows[0].identity.host.as_deref(),
            Some("https://admin.shopify.com")
        );
        assert!(rows[0].identity.is_active_identity);
        assert!(rows[0]
            .meta
            .get("userIdFingerprint")
            .and_then(Value::as_str)
            .unwrap()
            .starts_with("sha256:"));
        assert!(!format!("{rows:?}").contains("user-uuid-123"));
    }

    #[test]
    fn vercel_profile_auth_uses_profile_alias_and_sibling_team() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join("VercelProfiles").join("barmous");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let auth_path = profile_dir.join("auth.json");
        std::fs::write(
            profile_dir.join("config.json"),
            serde_json::json!({"currentTeam": "team_fixture_123"}).to_string(),
        )
        .unwrap();

        let mut descriptor = descriptor("vercel", "Vercel");
        descriptor.dashboard_url = Some("https://vercel.com".to_string());
        let value = serde_json::json!({
            "token": "vercel_secret",
            "userId": "usr_fixture_123",
            "refreshToken": "refresh_secret",
            "expiresAt": 1786257099
        });
        let rows = vercel::auth_file(
            &descriptor,
            &Location {
                path: "auth.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "vercel_auth".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: Some("profile".to_string()),
            },
            &auth_path,
            &value,
            false,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "barmous");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("Vercel profile"));
        assert_eq!(rows[0].identity.host.as_deref(), Some("https://vercel.com"));
        assert_eq!(
            rows[0].meta.get("profile").and_then(Value::as_str),
            Some("barmous")
        );
        assert!(rows[0]
            .meta
            .get("userIdFingerprint")
            .and_then(Value::as_str)
            .unwrap()
            .starts_with("sha256:"));
        assert!(!format!("{rows:?}").contains("vercel_secret"));
        assert!(!format!("{rows:?}").contains("refresh_secret"));
        assert!(!format!("{rows:?}").contains("usr_fixture_123"));
        assert!(!format!("{rows:?}").contains("team_fixture_123"));
    }

    #[test]
    fn vercel_auth_hides_raw_user_id_without_probe() {
        let mut descriptor = descriptor("vercel", "Vercel");
        descriptor.dashboard_url = Some("https://vercel.com".to_string());
        let value = serde_json::json!({
            "token": "vercel_secret",
            "userId": "yX5emi3ltASINqWz4kLsYIPk",
            "refreshToken": "refresh_secret"
        });
        let rows = vercel::auth_file(
            &descriptor,
            &Location {
                path: "auth.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "vercel_auth".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: Some("xdg".to_string()),
            },
            Path::new("com.vercel.cli/auth.json"),
            &value,
            false,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "Vercel account");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("Vercel account"));
        assert!(!format!("{rows:?}").contains("yX5emi3ltASINqWz4kLsYIPk"));
    }

    #[test]
    fn token_file_finds_snake_case_access_tokens() {
        let descriptor = descriptor("neon", "Neon DB");
        let value = serde_json::json!({
            "access_token": "napi_secret",
            "account_id": "acct_1",
            "user": {"email": "neon.user@example.test"}
        });
        let rows = tokens::token_file(
            &descriptor,
            &Location {
                path: "credentials.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "token_file".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("credentials.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "neon.user@example.test");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("acct_1"));
        assert!(!format!("{rows:?}").contains("napi_secret"));
    }

    #[test]
    fn neon_auth_uses_jwt_claim_label() {
        let descriptor = descriptor("neon", "Neon DB");
        let user_id = "00000000-1111-2222-3333-444444444444";
        let id_token = unsigned_jwt(serde_json::json!({
            "sub": user_id,
            "name": "Neon User",
            "email": "neon.user@example.test"
        }));
        let value = serde_json::json!({
            "access_token": "napi_secret",
            "refresh_token": "refresh_secret",
            "id_token": id_token,
            "type": "oauth",
            "user_id": user_id
        });
        let rows = neon::auth_file(
            &descriptor,
            &Location {
                path: "credentials.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "neon_auth".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("credentials.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "Neon User");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("OAuth profile"));
        assert!(rows[0]
            .meta
            .get("userIdFingerprint")
            .and_then(Value::as_str)
            .unwrap()
            .starts_with("sha256:"));
        assert!(!format!("{rows:?}").contains("napi_secret"));
        assert!(!format!("{rows:?}").contains("refresh_secret"));
        assert!(!format!("{rows:?}").contains(user_id));
    }

    #[test]
    fn neon_auth_hides_uuid_when_claims_are_missing() {
        let descriptor = descriptor("neon", "Neon DB");
        let user_id = "00000000-1111-2222-3333-444444444444";
        let value = serde_json::json!({
            "access_token": "napi_secret",
            "refresh_token": "refresh_secret",
            "type": "oauth",
            "user_id": user_id
        });
        let rows = neon::auth_file(
            &descriptor,
            &Location {
                path: "credentials.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "neon_auth".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("credentials.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "Neon account");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("OAuth profile"));
        assert!(!format!("{rows:?}").contains("napi_secret"));
        assert!(!format!("{rows:?}").contains("refresh_secret"));
        assert!(!format!("{rows:?}").contains(user_id));
    }

    #[test]
    fn neon_auth_uses_profile_label_for_default_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".config");
        let neon_dir = config_dir.join("neon");
        let neonctl_dir = config_dir.join("neonctl");
        std::fs::create_dir_all(&neon_dir).unwrap();
        std::fs::create_dir_all(&neonctl_dir).unwrap();
        let credentials_path = neonctl_dir.join("credentials.json");
        std::fs::write(&credentials_path, "{}").unwrap();
        let user_id = "profile-user-fixture";
        std::fs::write(
            neon_dir.join("profiles.json"),
            serde_json::json!({
                "version": 1,
                "profiles": {
                    "DEFAULT": {"credentials": "../neonctl/credentials.json"},
                    "nemu": {
                        "credentials": "credentials.nemu.json",
                        "label": "neon.profile@example.test",
                        "userId": user_id
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let mut descriptor = descriptor("neon", "Neon DB");
        descriptor.dashboard_url = Some("https://console.neon.tech".to_string());
        let value = serde_json::json!({
            "access_token": "napi_secret",
            "refresh_token": "refresh_secret",
            "type": "oauth",
            "user_id": user_id
        });
        let rows = neon::auth_file(
            &descriptor,
            &Location {
                path: "credentials.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "neon_auth".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            &credentials_path,
            &value,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "neon.profile@example.test");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("Default profile"));
        assert_eq!(
            rows[0].meta.get("profile").and_then(Value::as_str),
            Some("DEFAULT")
        );
        assert!(!format!("{rows:?}").contains(user_id));
    }

    #[test]
    fn neon_auth_uses_named_profile_scope() {
        let dir = tempfile::tempdir().unwrap();
        let neon_dir = dir.path().join(".config").join("neon");
        std::fs::create_dir_all(&neon_dir).unwrap();
        let credentials_path = neon_dir.join("credentials.nemu.json");
        let user_id = "profile-user-fixture";
        std::fs::write(
            neon_dir.join("profiles.json"),
            serde_json::json!({
                "version": 1,
                "profiles": {
                    "nemu": {
                        "credentials": "credentials.nemu.json",
                        "label": "neon.profile@example.test",
                        "userId": user_id
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let mut descriptor = descriptor("neon", "Neon DB");
        descriptor.dashboard_url = Some("https://console.neon.tech".to_string());
        let value = serde_json::json!({
            "access_token": "napi_secret",
            "refresh_token": "refresh_secret",
            "type": "oauth",
            "user_id": user_id
        });
        let rows = neon::auth_file(
            &descriptor,
            &Location {
                path: "credentials.nemu.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "neon_auth".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: Some("profile".to_string()),
            },
            &credentials_path,
            &value,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "neon.profile@example.test");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("profile: nemu"));
        assert_eq!(
            rows[0].meta.get("profile").and_then(Value::as_str),
            Some("nemu")
        );
        assert_eq!(
            rows[0].meta.get("confidence").and_then(Value::as_str),
            Some("profile")
        );
        assert!(!format!("{rows:?}").contains(user_id));
    }

    #[test]
    fn token_file_uses_team_when_user_label_is_missing() {
        let descriptor = descriptor("vercel", "Vercel");
        let value = serde_json::json!({
            "token": "vercel_secret",
            "currentTeam": "team_acme"
        });
        let rows = tokens::token_file(
            &descriptor,
            &Location {
                path: "auth.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "token_file".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("auth.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "team_acme");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("team_acme"));
        assert!(!format!("{rows:?}").contains("vercel_secret"));
    }

    #[test]
    fn token_file_does_not_label_project_only_configs_as_accounts() {
        let descriptor = descriptor("gcloud", "Google Cloud");
        let value = serde_json::json!({
            "access_token": "gcloud_secret",
            "project_id": "local-project-123"
        });
        let rows = tokens::token_file(
            &descriptor,
            &Location {
                path: "application_default_credentials.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "token_file".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("application_default_credentials.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_ne!(rows[0].identity.label, "local-project-123");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("local-project-123"));
        assert!(rows[0].identity.label.starts_with("Google Cloud (..."));
        assert!(!format!("{rows:?}").contains("gcloud_secret"));
    }

    #[test]
    fn profile_file_prefers_account_name_over_profile_or_project() {
        let descriptor = descriptor("aws", "AWS");
        let value = serde_json::json!({
            "profile prod": {
                "sso_account_name": "BRDG Production",
                "sso_account_id": "123456789012",
                "region": "us-east-1",
                "project": "should-not-be-label"
            }
        });
        let rows = profiles::profile_file(
            &descriptor,
            &Location {
                path: "config".to_string(),
                format: crate::scan::parsers::Format::Ini,
                strategy: "profile_file".to_string(),
                source_type: "config_file".to_string(),
                scope: Some("config".to_string()),
                confidence: None,
            },
            Path::new("config"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "BRDG Production");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("prod"));
        assert_eq!(rows[0].identity.host.as_deref(), Some("123456789012"));
        assert_eq!(
            rows[0].meta.get("project").and_then(Value::as_str),
            Some("should-not-be-label")
        );
    }

    #[test]
    fn profile_file_uses_role_arn_account_fallback() {
        let descriptor = descriptor("aws", "AWS");
        let value = serde_json::json!({
            "profile admin": {
                "role_arn": "arn:aws:iam::123456789012:role/AdminAccess",
                "region": "us-east-1"
            }
        });
        let rows = profiles::profile_file(
            &descriptor,
            &Location {
                path: "config".to_string(),
                format: crate::scan::parsers::Format::Ini,
                strategy: "profile_file".to_string(),
                source_type: "config_file".to_string(),
                scope: Some("config".to_string()),
                confidence: None,
            },
            Path::new("config"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "123456789012/AdminAccess");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("admin"));
    }

    #[test]
    fn azure_profile_prefers_user_over_subscription_name() {
        let descriptor = descriptor("azure", "Azure");
        let value = serde_json::json!({
            "subscriptions": [{
                "name": "Production subscription",
                "id": "sub_123",
                "tenantId": "tenant_abc",
                "user": {"name": "azure.user@example.test"}
            }]
        });
        let rows = profiles::azure_profile(
            &descriptor,
            &Location {
                path: "azureProfile.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "azure_profile".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("azureProfile.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "azure.user@example.test");
        assert_eq!(
            rows[0].identity.scope.as_deref(),
            Some("Production subscription")
        );
        assert_eq!(rows[0].identity.host.as_deref(), Some("sub_123"));
    }

    #[test]
    fn vercel_project_links_are_fingerprinted() {
        let mut descriptor = descriptor("vercel", "Vercel");
        descriptor.dashboard_url = Some("https://vercel.com".to_string());
        let value = serde_json::json!({
            "orgId": "team_fixture_org_000000000000000000",
            "projectId": "prj_fixture_project_000000000000000000"
        });
        let rows = profiles::vercel_project(
            &descriptor,
            &Location {
                path: "project.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "vercel_project".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new("C:/Users/dev/Projects/launch/.vercel/project.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.label, "launch");
        assert_eq!(rows[0].identity.scope.as_deref(), Some("Linked project"));
        assert!(rows[0]
            .fingerprint
            .as_deref()
            .unwrap()
            .starts_with("sha256:"));
        assert!(!format!("{rows:?}").contains("prj_fixture_project"));
        assert!(!format!("{rows:?}").contains("team_fixture_org"));
    }

    #[test]
    fn npmrc_extracts_registry_token_without_secret() {
        let descriptor = descriptor("npm", "npm");
        let value = serde_json::json!({
            "default": {
                "//registry.npmjs.org/:_authToken": "npm_secret"
            }
        });
        let rows = profiles::npmrc(
            &descriptor,
            &Location {
                path: ".npmrc".to_string(),
                format: crate::scan::parsers::Format::Ini,
                strategy: "npmrc".to_string(),
                source_type: "config_file".to_string(),
                scope: None,
                confidence: None,
            },
            Path::new(".npmrc"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity.host.as_deref(), Some("registry.npmjs.org"));
        assert!(!format!("{rows:?}").contains("npm_secret"));
    }
}
