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

pub mod mcp {
    use super::*;

    pub fn mcp_servers(
        descriptor: &Descriptor,
        location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let mut out = Vec::new();
        collect_servers(
            descriptor,
            location,
            path,
            value.get("mcpServers").unwrap_or(value),
            location.scope.clone(),
            &mut out,
        );

        if let Some(projects) = value.get("projects").and_then(Value::as_object) {
            for (project_path, project_value) in projects {
                let scope = Path::new(project_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("Project: {name}"))
                    .unwrap_or_else(|| "Project MCP".to_string());
                collect_servers(
                    descriptor,
                    location,
                    path,
                    project_value.get("mcpServers").unwrap_or(project_value),
                    Some(scope),
                    &mut out,
                );
            }
        }

        out
    }

    fn collect_servers(
        descriptor: &Descriptor,
        _location: &Location,
        path: &Path,
        value: &Value,
        scope: Option<String>,
        out: &mut Vec<DetectedConnection>,
    ) {
        let Some(servers) = value.as_object() else {
            return;
        };

        for (name, server) in servers {
            let command = server.get("command").and_then(Value::as_str);
            let url = server.get("url").and_then(Value::as_str);
            let mut meta = BTreeMap::new();
            meta.insert(
                "kind".to_string(),
                Value::String(
                    if command.is_some() {
                        "command"
                    } else if url.is_some() {
                        "url"
                    } else {
                        "unknown"
                    }
                    .to_string(),
                ),
            );
            if let Some(command) = command {
                meta.insert("command".to_string(), Value::String(command.to_string()));
            }
            if let Some(url) = url {
                meta.insert("url".to_string(), Value::String(url.to_string()));
            }
            if let Some(env) = server.get("env").and_then(Value::as_object) {
                let safe_env = env
                    .iter()
                    .map(|(key, value)| {
                        let fp = value
                            .as_str()
                            .map(|raw| secutil::fingerprint(raw.as_bytes()))
                            .unwrap_or_else(|| "sha256:00000000".to_string());
                        (key.clone(), Value::String(fp))
                    })
                    .collect();
                meta.insert("env".to_string(), Value::Object(safe_env));
            }

            let mut connection = super::detected(
                descriptor,
                name.to_string(),
                url.or(command).map(str::to_string),
                scope.clone(),
                path,
                None,
                meta,
            );
            connection.source.descriptor_id = Some(descriptor.id.clone());
            out.push(connection);
        }
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
        let scope = value
            .get("currentTeam")
            .or_else(|| value.get("teamId"))
            .or_else(|| value.get("team_id"))
            .or_else(|| value.get("projectId"))
            .or_else(|| value.get("project_id"))
            .or_else(|| value.get("accountId"))
            .or_else(|| value.get("account_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| location.scope.clone());
        let label = value
            .get("email")
            .or_else(|| value.get("username"))
            .or_else(|| value.get("user"))
            .or_else(|| value.get("account"))
            .and_then(Value::as_str)
            .map(str::to_string)
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

    fn token_value(value: &Value) -> Option<&str> {
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
            let label = section
                .strip_prefix("profile ")
                .unwrap_or(section)
                .to_string();
            let host = section_obj
                .get("host")
                .or_else(|| section_obj.get("endpoint_url"))
                .or_else(|| section_obj.get("region"))
                .or_else(|| section_obj.get("account"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| descriptor.dashboard_url.clone());
            let fingerprint = fingerprint_from_object(section_obj);
            let mut meta = BTreeMap::new();
            if let Some(confidence) = &location.confidence {
                meta.insert("confidence".to_string(), Value::String(confidence.clone()));
            }
            if section_obj.contains_key("region") {
                meta.insert("region".to_string(), section_obj["region"].clone());
            }
            out.push(super::detected(
                descriptor,
                label,
                host,
                location.scope.clone(),
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
        location: &Location,
        path: &Path,
        value: &Value,
    ) -> Vec<DetectedConnection> {
        let Some(subscriptions) = value.get("subscriptions").and_then(Value::as_array) else {
            return Vec::new();
        };

        subscriptions
            .iter()
            .filter_map(|subscription| {
                let label = subscription
                    .get("name")
                    .or_else(|| subscription.get("id"))
                    .and_then(Value::as_str)?;
                let user = subscription
                    .get("user")
                    .and_then(Value::as_object)
                    .and_then(|user| user.get("name"))
                    .and_then(Value::as_str);
                let mut meta = BTreeMap::new();
                if let Some(user) = user {
                    meta.insert("user".to_string(), Value::String(user.to_string()));
                }
                Some(super::detected(
                    descriptor,
                    label.to_string(),
                    descriptor.dashboard_url.clone(),
                    subscription
                        .get("tenantId")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| location.scope.clone()),
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
}

pub mod envvars {
    use super::*;
    use crate::descriptors::Descriptor;

    pub fn scan(descriptors: &[Descriptor]) -> Vec<DetectedConnection> {
        let values = read_env_hives();
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
                        meta: BTreeMap::from([("source".to_string(), json!("registry_env"))]),
                    });
                }
            }
        }
        out
    }

    #[cfg(windows)]
    fn read_env_hives() -> BTreeMap<String, String> {
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
                for name in key
                    .enum_values()
                    .filter_map(Result::ok)
                    .map(|(name, _)| name)
                {
                    if let Ok(value) = key.get_value::<String, _>(&name) {
                        values.insert(name, value);
                    }
                }
            }
        }
        values
    }

    #[cfg(not(windows))]
    fn read_env_hives() -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn mcp_env_values_are_fingerprinted() {
        let descriptor = descriptor("mcp-cursor", "Cursor MCP");
        let value = serde_json::json!({
            "mcpServers": {
                "postgres": {
                    "command": "npx",
                    "env": {"NEON_API_KEY": "secret-secret-secret"}
                }
            }
        });
        let rows = mcp::mcp_servers(
            &descriptor,
            &Location {
                path: "cursor-mcp.json".to_string(),
                format: crate::scan::parsers::Format::Json,
                strategy: "mcp_servers".to_string(),
                source_type: "config_file".to_string(),
                scope: Some("Cursor".to_string()),
                confidence: None,
            },
            Path::new("cursor-mcp.json"),
            &value,
        );
        assert_eq!(rows.len(), 1);
        assert!(!format!("{rows:?}").contains("secret-secret-secret"));
    }

    #[test]
    fn token_file_finds_snake_case_access_tokens() {
        let descriptor = descriptor("neon", "Neon DB");
        let value = serde_json::json!({"access_token": "napi_secret", "account_id": "acct_1"});
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
        assert_eq!(rows[0].identity.scope.as_deref(), Some("acct_1"));
        assert!(!format!("{rows:?}").contains("napi_secret"));
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
