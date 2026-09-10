//! Local MCP registrations, not server health or proof of runtime use.
//! No commands are launched and no configured endpoints are contacted.

use crate::descriptors::{comparable_path, ScanPaths};
use crate::models::{
    Availability, Connection, ConnectionSource, ConnectionValidation, DetectedConnection, Identity,
    ProviderError, SourceType, Usage,
};
use crate::registry::connection_id;
use crate::scan::{parsers, secutil};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const PROVIDER_ID: &str = "mcp_servers";
const MAX_PROJECT_ROOTS: usize = 50;
const MAX_SERVERS_PER_SOURCE: usize = 256;

#[derive(Debug, Default)]
pub struct McpScan {
    pub detected: Vec<DetectedConnection>,
    pub validations: BTreeMap<String, ConnectionValidation>,
    pub errors: Vec<ProviderError>,
}

#[derive(Clone)]
struct Source {
    path: PathBuf,
    client: &'static str,
    format: parsers::Format,
    map_key: &'static str,
}

enum Coverage {
    Missing,
    Read,
    Unknown(&'static str),
}

fn sources(paths: &ScanPaths, project_roots: &[String]) -> Vec<Source> {
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    let mut add = |pattern: &str, client, format, map_key| {
        if let Some(path) = paths.expand_pattern(pattern) {
            if seen.insert((comparable_path(&path), client)) {
                sources.push(Source {
                    path,
                    client,
                    format,
                    map_key,
                });
            }
        }
    };
    // Prefer the ordinary path when CODEX_HOME aliases the same source. The
    // ScanPaths context ignores inherited CODEX_HOME during fixture runs.
    add(
        "~/.codex/config.toml",
        "Codex",
        parsers::Format::Toml,
        "mcp_servers",
    );
    add(
        "%CODEX_HOME%/config.toml",
        "Codex",
        parsers::Format::Toml,
        "mcp_servers",
    );
    add(
        "%APPDATA%/Claude/claude_desktop_config.json",
        "Claude Desktop",
        parsers::Format::Json,
        "mcpServers",
    );
    add(
        "~/.claude.json",
        "Claude Code",
        parsers::Format::Json,
        "mcpServers",
    );
    add(
        "~/.cursor/mcp.json",
        "Cursor",
        parsers::Format::Json,
        "mcpServers",
    );
    for root in project_roots.iter().take(MAX_PROJECT_ROOTS) {
        let Some(root) = paths.expand_pattern(root) else {
            continue;
        };
        for (relative, client, map_key) in [
            (".mcp.json", "Claude Code", "mcpServers"),
            (".cursor/mcp.json", "Cursor", "mcpServers"),
            (".vscode/mcp.json", "VS Code", "servers"),
        ] {
            add(
                &root.join(relative).to_string_lossy(),
                client,
                parsers::Format::Json,
                map_key,
            );
        }
    }
    sources
}

/// Includes absent files so creation and deletion remain observable.
pub fn watch_patterns(paths: &ScanPaths, project_roots: &[String]) -> Vec<PathBuf> {
    sources(paths, project_roots)
        .into_iter()
        .map(|source| source.path)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn scan(
    paths: &ScanPaths,
    project_roots: &[String],
    previous: &[Connection],
    checked_at: &str,
) -> McpScan {
    let mut result = McpScan::default();
    let mut checked_sources = Vec::new();
    for source in sources(paths, project_roots) {
        let coverage = read_source(paths, &source, checked_at, &mut result);
        checked_sources.push((source, coverage));
    }
    if project_roots.len() > MAX_PROJECT_ROOTS {
        result.errors.push(ProviderError {
            provider: PROVIDER_ID.into(),
            code: "mcp_project_limit".into(),
            message:
                "Only the first 50 configured project roots were checked for MCP registrations."
                    .into(),
            detail: None,
        });
    }
    for previous in previous.iter().filter(|row| row.provider == PROVIDER_ID) {
        if result.validations.contains_key(&previous.id) {
            continue;
        }
        let validation = validate_previous(paths, previous, &checked_sources, checked_at);
        result.validations.insert(previous.id.clone(), validation);
    }
    result
}

fn unknown(checked_at: &str, code: &str) -> ConnectionValidation {
    ConnectionValidation::unknown(code, "This MCP source could not be fully checked. Its registration history is protected; runtime use was not checked.", Some(checked_at.into()))
}

fn source_error(source: &Source, code: &str, result: &mut McpScan) {
    // Parser errors and configuration values can contain credentials. Keep
    // diagnostics structural and do not echo their original message or value.
    result.errors.push(ProviderError {
        provider: PROVIDER_ID.into(),
        code: code.into(),
        message: format!(
            "A {} MCP configuration could not be fully checked.",
            source.client
        ),
        detail: Some(secutil::redact(&source.path.to_string_lossy())),
    });
}

fn read_source(
    paths: &ScanPaths,
    source: &Source,
    checked_at: &str,
    result: &mut McpScan,
) -> Coverage {
    if !paths.allows(&source.path) {
        return Coverage::Unknown("mcp_source_out_of_scope");
    }
    match std::fs::symlink_metadata(&source.path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if absent_below_readable_directory(&source.path) {
                return Coverage::Missing;
            }
            source_error(source, "mcp_source_unreadable", result);
            return Coverage::Unknown("mcp_source_unreadable");
        }
        Err(_) => {
            source_error(source, "mcp_source_unreadable", result);
            return Coverage::Unknown("mcp_source_unreadable");
        }
        _ => {}
    }
    // Resolve once and repeat fixture containment on the actual read target.
    // A broken link is unknown, not evidence that its registration was removed.
    let read_path = match source.path.canonicalize() {
        Ok(path) if canonical_target_allowed(paths, &path) => path,
        _ => {
            source_error(source, "mcp_source_unreadable", result);
            return Coverage::Unknown("mcp_source_unreadable");
        }
    };
    let doc = match parsers::parse(&read_path, source.format) {
        Ok(doc) => doc,
        Err(_) => {
            source_error(source, "mcp_source_unreadable", result);
            return Coverage::Unknown("mcp_source_unreadable");
        }
    };
    let Some(root) = doc.value.as_object() else {
        source_error(source, "mcp_unsupported_shape", result);
        return Coverage::Unknown("mcp_unsupported_shape");
    };
    let Some(value) = root.get(source.map_key) else {
        return Coverage::Read;
    };
    let Some(servers) = value.as_object() else {
        source_error(source, "mcp_unsupported_shape", result);
        return Coverage::Unknown("mcp_unsupported_shape");
    };
    let mut complete = servers.len() <= MAX_SERVERS_PER_SOURCE;
    for (name, value) in servers.iter().take(MAX_SERVERS_PER_SOURCE) {
        match registration(source, name, value) {
            Some((detected, disabled)) => {
                result.validations.insert(connection_id(&detected), ConnectionValidation {
                    availability: Availability::Available,
                    usage: Usage::Referenced,
                    checked_at: Some(checked_at.into()),
                    reason: if disabled {
                        "This MCP registration is present but disabled in local configuration. The server was not started or contacted."
                    } else {
                        "This MCP registration is present in local configuration. The server was not started or contacted; runtime use is not confirmed."
                    }.into(),
                    reason_code: if disabled { "mcp_registered_disabled" } else { "mcp_registered" }.into(),
                });
                result.detected.push(detected);
            }
            None => complete = false,
        }
    }
    if complete {
        Coverage::Read
    } else {
        source_error(source, "mcp_unsupported_shape", result);
        Coverage::Unknown("mcp_unsupported_shape")
    }
}

fn canonical_target_allowed(paths: &ScanPaths, target: &Path) -> bool {
    if paths.allows(target) {
        return true;
    }
    // macOS fixtures may be named /var/... while canonical paths begin with
    // /private/var/.... Compare against the same canonical fixture root instead
    // of treating that verified system alias as an escape.
    paths.is_isolated()
        && paths
            .expand_pattern("%HOME%")
            .and_then(|home| home.canonicalize().ok())
            .is_some_and(|home| target.starts_with(home))
}

fn absent_below_readable_directory(path: &Path) -> bool {
    for ancestor in path.ancestors().skip(1) {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => return ancestor.canonicalize().is_ok_and(|path| path.is_dir()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return false,
        }
    }
    false
}

fn validate_previous(
    paths: &ScanPaths,
    previous: &Connection,
    sources: &[(Source, Coverage)],
    checked_at: &str,
) -> ConnectionValidation {
    if previous.source.source_type != SourceType::ConfigFile
        || previous.source.descriptor_id.as_deref() != Some(PROVIDER_ID)
    {
        return unknown(checked_at, "mcp_identity_unavailable");
    }
    let Some(path) = previous.source.path.as_deref().map(Path::new) else {
        return unknown(checked_at, "mcp_source_unavailable");
    };
    if !paths.allows(path) {
        return unknown(checked_at, "mcp_source_out_of_scope");
    }
    let Some(client) = previous.meta.get("mcpClient").and_then(Value::as_str) else {
        return unknown(checked_at, "mcp_identity_unavailable");
    };
    let Some(server) = previous.meta.get("mcpServerName").and_then(Value::as_str) else {
        return unknown(checked_at, "mcp_identity_unavailable");
    };
    let Some(registration) = previous
        .meta
        .get("mcpRegistrationId")
        .and_then(Value::as_str)
    else {
        return unknown(checked_at, "mcp_identity_unavailable");
    };
    if server.is_empty()
        || registration.len() != 64
        || !registration.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return unknown(checked_at, "mcp_identity_unavailable");
    }
    let Some((_, coverage)) = sources.iter().find(|(source, _)| {
        source.client == client && comparable_path(&source.path) == comparable_path(path)
    }) else {
        return unknown(checked_at, "mcp_source_out_of_scope");
    };
    match coverage {
        Coverage::Missing => ConnectionValidation::missing(checked_at, "mcp_source_missing", "The previously recorded MCP configuration file is no longer present. This does not confirm whether the server is installed elsewhere."),
        Coverage::Read => ConnectionValidation::missing(checked_at, "mcp_registration_missing", "The MCP configuration was read successfully and no longer contains this named registration. Runtime use elsewhere was not checked."),
        Coverage::Unknown(code) => unknown(checked_at, code),
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn string_map(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.values().all(Value::is_string))
}

fn string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|array| array.iter().all(Value::is_string))
}

fn executable(value: &Value) -> Option<&str> {
    let command = value.as_str()?.trim();
    if command.is_empty()
        || command.len() > 4096
        || command.chars().any(char::is_control)
        || command.contains("://")
        || command.contains([';', '|', '>', '<', '`', '\"', '&', '$', '*', '?'])
    {
        return None;
    }
    // A command field is an executable, not a command line. Preserve ordinary
    // absolute Windows paths with spaces; never store shell arguments here.
    if command.chars().any(char::is_whitespace) {
        let windows_absolute = command.as_bytes().get(1) == Some(&b':')
            && command
                .as_bytes()
                .get(2)
                .is_some_and(|byte| *byte == b'\\' || *byte == b'/');
        let lower = command.to_ascii_lowercase();
        if !windows_absolute
            || lower.contains(" -")
            || [".exe ", ".com ", ".cmd ", ".bat "]
                .iter()
                .any(|marker| lower.contains(marker))
            || ![".exe", ".com", ".cmd", ".bat"]
                .iter()
                .any(|suffix| lower.ends_with(suffix))
        {
            return None;
        }
    }
    Some(command)
}

fn registration(source: &Source, name: &str, value: &Value) -> Option<(DetectedConnection, bool)> {
    if name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return None;
    }
    let entry = value.as_object()?;
    if !valid_fields(entry) {
        return None;
    }
    let disabled = entry.get("disabled").and_then(Value::as_bool) == Some(true)
        || entry.get("enabled").and_then(Value::as_bool) == Some(false);
    let command = match entry.get("command") {
        Some(value) => Some(executable(value)?),
        None => None,
    };
    let url = match entry.get("url") {
        Some(value) => Some(url::Url::parse(value.as_str()?).ok()?),
        None => None,
    };
    let kind = entry.get("type").and_then(Value::as_str);
    let (transport, host) = match (command, url.as_ref()) {
        (Some(_), None) if kind.is_none_or(|kind| kind == "stdio") => ("stdio", None),
        (None, Some(url))
            if matches!(url.scheme(), "https" | "http")
                && url.host_str().is_some()
                && kind.is_none_or(|kind| matches!(kind, "http" | "sse" | "streamable-http")) =>
        {
            (
                if kind == Some("sse") { "sse" } else { "http" },
                url.host_str().map(secutil::redact),
            )
        }
        _ => return None,
    };
    let mut key = Vec::new();
    // One source may be reached through a directory alias or a canonical
    // project root. Its registration identity must match the comparison used
    // for historical coverage, while the displayed path remains unchanged.
    let source_identity = comparable_path(&source.path);
    let source_path = source_identity.to_string_lossy();
    for value in [source_path.as_ref(), source.client, name] {
        key.extend_from_slice(value.as_bytes());
        key.push(0);
    }
    let mut meta = BTreeMap::from([
        ("mcpRegistrationId".into(), Value::String(digest(&key))),
        ("mcpClient".into(), Value::String(source.client.into())),
        ("mcpServerName".into(), Value::String(secutil::redact(name))),
        ("mcpTransport".into(), Value::String(transport.into())),
        ("mcpDisabled".into(), Value::Bool(disabled)),
    ]);
    if let Some(command) = command {
        meta.insert("mcpCommand".into(), Value::String(secutil::redact(command)));
    }
    Some((
        DetectedConnection {
            provider: PROVIDER_ID.into(),
            provider_name: "MCP servers".into(),
            identity: Identity {
                label: secutil::redact(name),
                host,
                scope: Some(source.client.into()),
                is_active_identity: false,
            },
            source: ConnectionSource {
                source_type: SourceType::ConfigFile,
                path: Some(source.path.display().to_string()),
                descriptor_id: Some(PROVIDER_ID.into()),
            },
            fingerprint: Some(format!(
                "sha256:{}",
                digest(&serde_json::to_vec(value).ok()?)
            )),
            meta,
        },
        disabled,
    ))
}

fn valid_fields(entry: &Map<String, Value>) -> bool {
    ["enabled", "disabled"]
        .iter()
        .all(|key| entry.get(*key).is_none_or(Value::is_boolean))
        && ["args", "env_vars", "scopes", "includeTools", "excludeTools"]
            .iter()
            .all(|key| entry.get(*key).is_none_or(string_array))
        && ["env", "headers", "http_headers", "env_http_headers"]
            .iter()
            .all(|key| entry.get(*key).is_none_or(string_map))
        && ["type", "bearer_token_env_var"]
            .iter()
            .all(|key| entry.get(*key).is_none_or(Value::is_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConnectionStatus;
    use serde_json::json;
    use std::fs;

    const NOW: &str = "2026-09-10T12:00:00.000Z";

    fn write(home: &Path, relative: &str, content: &str) -> PathBuf {
        let path = home.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn previous(scan: &McpScan) -> Vec<Connection> {
        scan.detected
            .iter()
            .map(|row| Connection {
                id: connection_id(row),
                provider: row.provider.clone(),
                provider_name: row.provider_name.clone(),
                identity: row.identity.clone(),
                source: row.source.clone(),
                status: ConnectionStatus::Active,
                validation: scan.validations[&connection_id(row)].clone(),
                fingerprint: row.fingerprint.clone(),
                first_seen: NOW.into(),
                last_seen: NOW.into(),
                hidden: false,
                seen: true,
                meta: row.meta.clone(),
            })
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn project_directory_alias_and_canonical_root_keep_one_available_registration() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        let actual = home.join("project");
        let alias = home.join("project-alias");
        write(
            home,
            "project/.mcp.json",
            r#"{"mcpServers":{"fixture-server":{"command":"fixture-helper"}}}"#,
        );
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let paths = ScanPaths::isolated(home);
        let mut registry = crate::registry::Registry::load(home).unwrap();
        registry.file.settings.project_roots = vec![alias.display().to_string()];
        let initial =
            crate::scan::refresh_registry(&mut registry, Some(PROVIDER_ID.into()), &paths);
        assert_eq!(initial.connections.len(), 1);
        let id = initial.connections[0].connection.id.clone();
        let first_seen = initial.connections[0].connection.first_seen.clone();
        assert_eq!(
            initial.connections[0].connection.source.path.as_deref(),
            Some(alias.join(".mcp.json").to_string_lossy().as_ref())
        );
        registry.file.connections[0].hidden = true;
        registry.file.settings.project_roots = vec![actual.display().to_string()];
        let refreshed =
            crate::scan::refresh_registry(&mut registry, Some(PROVIDER_ID.into()), &paths);
        assert_eq!(refreshed.connections.len(), 1);
        let row = &refreshed.connections[0];
        assert_eq!(row.connection.id, id);
        assert_eq!(
            row.connection.validation.availability,
            Availability::Available
        );
        assert_eq!(row.connection.first_seen, first_seen);
        assert!(row.connection.hidden);
        assert!(!row.removable);
        assert_eq!(
            row.connection.source.path.as_deref(),
            Some(actual.join(".mcp.json").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn codex_registration_removal_and_deleted_source_have_distinct_evidence() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let path = write(
            home.path(),
            ".codex/config.toml",
            "[mcp_servers.fixture]\ncommand = 'node'\nargs = ['fixture.js']\n",
        );
        let initial = scan(&paths, &[], &[], NOW);
        assert_eq!(initial.detected.len(), 1);
        assert!(initial.errors.is_empty());
        let history = previous(&initial);
        assert_eq!(history[0].validation.availability, Availability::Available);
        assert_eq!(history[0].validation.usage, Usage::Referenced);
        fs::write(&path, "model = 'fixture-model'\n").unwrap();
        let missing = scan(&paths, &[], &history, NOW);
        assert!(missing.detected.is_empty());
        assert_eq!(
            missing.validations[&history[0].id].reason_code,
            "mcp_registration_missing"
        );
        fs::remove_file(path).unwrap();
        let removed = scan(&paths, &[], &history, NOW);
        assert_eq!(
            removed.validations[&history[0].id].reason_code,
            "mcp_source_missing"
        );
    }

    #[test]
    fn configured_clients_and_project_files_do_not_merge_same_named_servers() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        write(
            home.path(),
            ".codex/config.toml",
            "[mcp_servers.shared]\ncommand = 'node'\n",
        );
        for relative in [
            ".claude.json",
            ".cursor/mcp.json",
            "project/.mcp.json",
            "project/.cursor/mcp.json",
        ] {
            write(
                home.path(),
                relative,
                r#"{"mcpServers":{"shared":{"command":"node"}}}"#,
            );
        }
        let desktop = paths
            .expand_pattern("%APPDATA%/Claude/claude_desktop_config.json")
            .unwrap();
        fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        fs::write(desktop, r#"{"mcpServers":{"shared":{"command":"node"}}}"#).unwrap();
        write(
            home.path(),
            "project/.vscode/mcp.json",
            r#"{"servers":{"shared":{"type":"http","url":"https://fixture.invalid/mcp"}}}"#,
        );
        let roots = vec![home.path().join("project").display().to_string()];
        let found = scan(&paths, &roots, &[], NOW);
        assert_eq!(found.detected.len(), 7);
        assert!(found.errors.is_empty());
        assert_eq!(
            found
                .detected
                .iter()
                .map(connection_id)
                .collect::<BTreeSet<_>>()
                .len(),
            7
        );
        assert!(found
            .detected
            .iter()
            .all(|row| row.identity.label == "shared"));
    }

    #[test]
    fn fingerprints_include_auth_and_arguments_but_output_never_contains_them() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let secret = "private-fixture-value";
        let mut config = json!({"mcpServers": {
            "local": {"command":"node", "args":[secret], "env":{"AUTH":secret}},
            "remote": {"url":format!("https://user:{secret}@fixture.invalid/mcp?key={secret}#private"), "headers":{"Authorization":secret}}
        }});
        let path = write(home.path(), ".cursor/mcp.json", &config.to_string());
        let initial = scan(&paths, &[], &[], NOW);
        assert_eq!(initial.detected.len(), 2);
        let output = serde_json::to_string(&initial.detected).unwrap();
        assert!(!output.contains(secret));
        assert!(!output.contains("https://"));
        assert!(!output.contains("Authorization"));
        assert!(!output.contains("user:"));
        let remote = initial
            .detected
            .iter()
            .find(|row| row.identity.label == "remote")
            .unwrap();
        assert_eq!(remote.identity.host.as_deref(), Some("fixture.invalid"));
        assert!(!remote.meta.contains_key("mcpCommand"));
        config["mcpServers"]["remote"]["url"] =
            json!("https://changed.invalid/mcp?secret=replaced");
        fs::write(path, config.to_string()).unwrap();
        let updated = scan(&paths, &[], &previous(&initial), NOW);
        let changed = updated
            .detected
            .iter()
            .find(|row| row.identity.label == "remote")
            .unwrap();
        assert_eq!(connection_id(remote), connection_id(changed));
        assert_ne!(remote.fingerprint, changed.fingerprint);
    }

    #[test]
    fn disabled_registrations_still_protect_history() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        write(
            home.path(),
            ".codex/config.toml",
            "[mcp_servers.disabled]\ncommand = 'node'\nenabled = false\n",
        );
        let found = scan(&paths, &[], &[], NOW);
        assert_eq!(found.detected[0].meta["mcpDisabled"], true);
        assert_eq!(
            found.validations[&connection_id(&found.detected[0])].reason_code,
            "mcp_registered_disabled"
        );
        assert_eq!(
            found.validations[&connection_id(&found.detected[0])].availability,
            Availability::Available
        );
    }

    #[test]
    fn malformed_files_maps_and_entries_cannot_prove_absence() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let path = write(
            home.path(),
            ".cursor/mcp.json",
            r#"{"mcpServers":{"before":{"command":"node"}}}"#,
        );
        let history = previous(&scan(&paths, &[], &[], NOW));
        for invalid in [
            "{",
            "[]",
            r#"{"mcpServers":[]}"#,
            r#"{"mcpServers":{"bad":null}}"#,
            r#"{"mcpServers":{"bad":{"args":["private-fixture-value"]}}}"#,
            r#"{"mcpServers":{"bad":{"url":"not a URL"}}}"#,
        ] {
            fs::write(&path, invalid).unwrap();
            let found = scan(&paths, &[], &history, NOW);
            assert_eq!(
                found.validations[&history[0].id].availability,
                Availability::Unknown,
                "{invalid}"
            );
            assert!(!found.errors.is_empty());
            assert!(!serde_json::to_string(&found.errors)
                .unwrap()
                .contains("private-fixture-value"));
        }
        fs::write(
            &path,
            r#"{"mcpServers":{"good":{"command":"node"},"bad":null}}"#,
        )
        .unwrap();
        let partial = scan(&paths, &[], &history, NOW);
        assert_eq!(partial.detected.len(), 1);
        assert_eq!(
            partial.validations[&history[0].id].availability,
            Availability::Unknown
        );
        assert_eq!(
            partial.validations[&connection_id(&partial.detected[0])].availability,
            Availability::Available
        );
        fs::write(path, "{}").unwrap();
        assert_eq!(
            scan(&paths, &[], &history, NOW).validations[&history[0].id].availability,
            Availability::Missing
        );
    }

    #[test]
    fn removing_project_scope_keeps_history_unknown_and_fixture_scope_blocks_outside_paths() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        write(
            home.path(),
            "project/.mcp.json",
            r#"{"mcpServers":{"fixture":{"command":"node"}}}"#,
        );
        write(
            outside.path(),
            ".mcp.json",
            r#"{"mcpServers":{"outside":{"command":"node"}}}"#,
        );
        let roots = vec![
            home.path().join("project").display().to_string(),
            outside.path().display().to_string(),
        ];
        let found = scan(&paths, &roots, &[], NOW);
        assert_eq!(found.detected.len(), 1);
        let history = previous(&found);
        let unchecked = scan(&paths, &[], &history, NOW);
        assert_eq!(
            unchecked.validations[&history[0].id].reason_code,
            "mcp_source_out_of_scope"
        );
        assert!(watch_patterns(&paths, &roots)
            .iter()
            .all(|path| paths.allows(path)));
    }

    #[test]
    fn watch_patterns_include_absent_config_files_and_ignore_external_codex_home() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let patterns = watch_patterns(&paths, &[]);
        assert!(patterns.contains(&home.path().join(".codex/config.toml")));
        assert!(patterns.contains(&home.path().join(".claude.json")));
        assert!(patterns.iter().all(|path| paths.allows(path)));
        assert!(scan(&paths, &[], &[], NOW).detected.is_empty());
    }

    #[test]
    fn oversized_and_nonregular_files_do_not_prove_absence() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let path = write(
            home.path(),
            ".cursor/mcp.json",
            r#"{"mcpServers":{"fixture":{"command":"node"}}}"#,
        );
        let history = previous(&scan(&paths, &[], &[], NOW));
        fs::write(&path, " ".repeat(1_048_577)).unwrap();
        assert_eq!(
            scan(&paths, &[], &history, NOW).validations[&history[0].id].availability,
            Availability::Unknown
        );
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(
            scan(&paths, &[], &history, NOW).validations[&history[0].id].availability,
            Availability::Unknown
        );
    }

    #[test]
    fn server_limit_protects_history_outside_the_processed_entries() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let path = write(
            home.path(),
            ".cursor/mcp.json",
            r#"{"mcpServers":{"zz-history":{"command":"node"}}}"#,
        );
        let history = previous(&scan(&paths, &[], &[], NOW));
        let entries = (0..=MAX_SERVERS_PER_SOURCE)
            .map(|index| (format!("server-{index:04}"), json!({"command":"node"})))
            .collect::<Map<_, _>>();
        fs::write(path, json!({"mcpServers":entries}).to_string()).unwrap();
        let limited = scan(&paths, &[], &history, NOW);
        assert_eq!(limited.detected.len(), MAX_SERVERS_PER_SOURCE);
        assert_eq!(
            limited.validations[&history[0].id].availability,
            Availability::Unknown
        );
        assert!(!limited.errors.is_empty());
    }

    #[test]
    fn redacted_registration_names_keep_distinct_stable_keys() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let names = ["ghp_abcdefghijklmnop", "ghp_qrstuvwxyzabcdef"];
        let entries = names
            .iter()
            .map(|name| ((*name).to_string(), json!({"command":"node"})))
            .collect::<Map<_, _>>();
        write(
            home.path(),
            ".cursor/mcp.json",
            &json!({"mcpServers":entries}).to_string(),
        );
        let found = scan(&paths, &[], &[], NOW);
        assert_eq!(found.detected.len(), 2);
        assert!(found
            .detected
            .iter()
            .all(|row| row.identity.label == "[redacted]"));
        assert_ne!(
            connection_id(&found.detected[0]),
            connection_id(&found.detected[1])
        );
        let output = serde_json::to_string(&found.detected).unwrap();
        assert!(names.iter().all(|name| !output.contains(name)));
    }

    #[test]
    fn unsupported_command_lines_are_not_saved_as_executable_metadata() {
        for command in [
            "node --token=private-value",
            "sh -c something",
            "node;private-value",
            "",
            "node\nprivate-value",
            "C:\\node.exe --token=private-value.exe",
        ] {
            assert!(executable(&json!(command)).is_none());
        }
        assert_eq!(
            executable(&json!("C:\\Program Files\\nodejs\\node.exe")),
            Some("C:\\Program Files\\nodejs\\node.exe")
        );
        assert_eq!(
            executable(&json!("/usr/local/bin/node")),
            Some("/usr/local/bin/node")
        );
    }

    #[cfg(unix)]
    #[test]
    fn fixture_symlinks_cannot_read_an_outside_mcp_config() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = write(
            outside.path(),
            "mcp.json",
            r#"{"mcpServers":{"outside":{"command":"node"}}}"#,
        );
        fs::create_dir(home.path().join(".cursor")).unwrap();
        std::os::unix::fs::symlink(target, home.path().join(".cursor/mcp.json")).unwrap();
        let paths = ScanPaths::isolated(home.path());
        assert!(scan(&paths, &[], &[], NOW).detected.is_empty());
        assert!(!watch_patterns(&paths, &[]).contains(&home.path().join(".cursor/mcp.json")));
    }

    #[cfg(unix)]
    #[test]
    fn broken_parent_link_does_not_prove_registration_was_removed() {
        let home = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(home.path());
        let path = write(
            home.path(),
            ".cursor/mcp.json",
            r#"{"mcpServers":{"fixture":{"command":"node"}}}"#,
        );
        let history = previous(&scan(&paths, &[], &[], NOW));
        fs::remove_file(path).unwrap();
        fs::remove_dir(home.path().join(".cursor")).unwrap();
        std::os::unix::fs::symlink(
            home.path().join("absent-directory"),
            home.path().join(".cursor"),
        )
        .unwrap();
        assert_eq!(
            scan(&paths, &[], &history, NOW).validations[&history[0].id].availability,
            Availability::Unknown
        );
    }
}
