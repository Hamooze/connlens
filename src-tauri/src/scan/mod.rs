pub mod parsers;
pub mod secutil;
pub mod strategies;

use crate::descriptors::{self, Descriptor, Location, ScanPaths};
use crate::models::{
    Connection, ConnectionValidation, DetectedConnection, ErrorPayload, ProviderError, Settings,
    Snapshot, SourceType,
};
use crate::registry::{app_home, now_iso, with_registry_at, Registry};
use parsers::parse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn scan_and_persist(provider: Option<String>) -> Result<Snapshot, ErrorPayload> {
    scan_with_paths(&app_home(), provider, &ScanPaths::current())
}

fn scan_with_paths(
    home: &Path,
    provider: Option<String>,
    paths: &ScanPaths,
) -> Result<Snapshot, ErrorPayload> {
    with_registry_at(home, |registry| refresh_registry(registry, provider, paths))
        .map_err(ErrorPayload::from)
}

/// Refresh inside the caller's existing registry transaction. No registry lock is
/// acquired here, allowing cleanup to validate and remove against one fresh state.
pub(crate) fn refresh_registry(
    registry: &mut Registry,
    provider: Option<String>,
    paths: &ScanPaths,
) -> Snapshot {
    let settings = registry.file.settings.clone();
    let (catalog, mut errors) = descriptors::load_all_scoped(&registry.home, paths);
    let catalog_incomplete = !errors.is_empty();
    if let Some(target) = provider.as_deref() {
        errors.extend(
            registry
                .file
                .provider_errors
                .iter()
                .filter(|error| error.provider != target && error.provider != "descriptors")
                .cloned(),
        );
    }
    let descriptors = catalog
        .iter()
        .filter(|descriptor| {
            provider
                .as_deref()
                .is_none_or(|target| descriptor.id == target)
                && settings.provider_toggles.get(&descriptor.id).copied() != Some(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut detected = Vec::new();
    let mut coverage = Vec::new();
    for descriptor in &descriptors {
        detect_descriptor(
            descriptor,
            &settings.project_roots,
            paths,
            &mut detected,
            &mut errors,
            &mut coverage,
        );
    }
    if !paths.is_isolated() {
        detected.extend(strategies::envvars::scan(&descriptors));
    }
    let checked_at = now_iso();
    // Present references receive positive validation in diff_with_validation.
    // Reserve the historical-source checks for entries absent from this scan.
    let detected_ids: BTreeSet<_> = detected
        .iter()
        .map(crate::registry::connection_id)
        .collect();
    let validations = registry
        .file
        .connections
        .iter()
        .filter(|connection| !detected_ids.contains(&connection.id))
        .map(|connection| {
            (
                connection.id.clone(),
                validate_unseen(
                    connection,
                    provider.as_deref(),
                    &settings,
                    &catalog,
                    catalog_incomplete,
                    paths,
                    &coverage,
                    &detected,
                    &checked_at,
                ),
            )
        })
        .collect();
    registry.diff_with_validation(detected, provider.as_deref(), &validations);
    registry.file.provider_errors = errors.clone();
    registry.snapshot(errors, crate::watchers::health(settings.watchers_enabled))
}

#[derive(Clone)]
enum SourceCheck {
    Absent,
    Read,
    Unknown(&'static str),
}

struct LocationCoverage {
    provider: String,
    pattern: Option<PathBuf>,
    strategy: String,
    complete: bool,
    sources: BTreeMap<PathBuf, SourceCheck>,
}

fn pattern_matches(pattern: &Path, source: &Path) -> bool {
    let pattern = descriptors::comparable_path(pattern);
    pattern == source
        || glob::Pattern::new(&pattern.to_string_lossy()).is_ok_and(|pattern| {
            pattern.matches_path_with(
                source,
                glob::MatchOptions {
                    case_sensitive: !cfg!(windows),
                    require_literal_separator: true,
                    require_literal_leading_dot: false,
                },
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn validate_unseen(
    connection: &Connection,
    provider: Option<&str>,
    settings: &Settings,
    catalog: &[Descriptor],
    catalog_incomplete: bool,
    paths: &ScanPaths,
    coverage: &[LocationCoverage],
    detected: &[DetectedConnection],
    checked_at: &str,
) -> ConnectionValidation {
    let unknown =
        |code, reason| ConnectionValidation::unknown(code, reason, Some(checked_at.to_string()));
    if matches!(
        connection.source.source_type,
        SourceType::Cli | SourceType::AgentRegistered
    ) {
        return unknown(
            "manual_record",
            "This manually registered entry has no automatically validated local source.",
        );
    }
    if provider.is_some_and(|target| target != connection.provider) {
        return unknown(
            "outside_scan_scope",
            "This provider was not included in the current validation.",
        );
    }
    if settings.provider_toggles.get(&connection.provider).copied() == Some(false) {
        return unknown(
            "provider_disabled",
            "Validation for this provider is disabled.",
        );
    }
    match connection.source.source_type {
        SourceType::EnvVar => return unknown("environment_not_visible", "An absent inherited environment variable does not establish whether this entry is available elsewhere."),
        SourceType::CredentialManager => return unknown("credential_store_unsupported", "This credential store cannot currently provide reliable absence evidence."),
        _ => {}
    }
    if catalog_incomplete {
        return unknown(
            "descriptor_errors",
            "Some provider definitions could not be read, so absence cannot be confirmed.",
        );
    }
    if !catalog
        .iter()
        .any(|descriptor| descriptor.id == connection.provider)
        || connection.source.descriptor_id.as_deref() != Some(connection.provider.as_str())
    {
        return unknown(
            "descriptor_unavailable",
            "The provider definition for this source is unavailable.",
        );
    }
    let Some(source) = connection.source.path.as_deref().map(Path::new) else {
        return unknown(
            "source_unavailable",
            "No local source path is recorded for this entry.",
        );
    };
    if !paths.allows(source) {
        return unknown(
            "source_out_of_scope",
            "This source is outside the permitted scan locations.",
        );
    }
    let source = descriptors::comparable_path(source);
    let matches = coverage
        .iter()
        .filter(|item| {
            item.provider == connection.provider
                && item
                    .pattern
                    .as_ref()
                    .is_some_and(|pattern| pattern_matches(pattern, &source))
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return unknown(
            "source_out_of_scope",
            "The current provider settings no longer include this source.",
        );
    }
    if matches.iter().any(|item| !item.complete) {
        return unknown(
            "scan_incomplete",
            "The source search was incomplete or reached its file limit.",
        );
    }
    if detected.iter().any(|row| {
        row.provider == connection.provider
            && row.identity.label == connection.identity.label
            && row.identity.host == connection.identity.host
            && row.identity.scope == connection.identity.scope
            && row
                .source
                .path
                .as_deref()
                .is_some_and(|path| descriptors::comparable_path(Path::new(path)) != source)
    }) {
        return unknown("source_moved", "A matching local reference was found at another source; the previous entry needs review.");
    }
    let same_source = detected
        .iter()
        .filter(|row| {
            row.provider == connection.provider
                && row
                    .source
                    .path
                    .as_deref()
                    .is_some_and(|path| descriptors::comparable_path(Path::new(path)) == source)
        })
        .collect::<Vec<_>>();
    if same_source.iter().any(|row| {
        (connection.fingerprint.is_some() && row.fingerprint == connection.fingerprint)
            || ["profile", "userIdFingerprint"].iter().any(|key| {
                connection
                    .meta
                    .get(*key)
                    .is_some_and(|value| row.meta.get(*key) == Some(value))
            })
    }) || (!same_source.is_empty()
        && matches.iter().any(|item| {
            matches!(
                item.strategy.as_str(),
                "token_file" | "neon_auth" | "vercel_auth"
            )
        }))
    {
        return unknown(
            "identity_changed",
            "The source still contains a local reference whose identity details changed.",
        );
    }
    let mut checked = false;
    let mut source_absent = false;
    for item in matches {
        if !supported_strategy(&item.strategy) {
            return unknown(
                "strategy_unsupported",
                "The current provider reader cannot validate this source.",
            );
        }
        match item.sources.get(&source) {
            Some(SourceCheck::Read) => checked = true,
            Some(SourceCheck::Absent) => {
                checked = true;
                source_absent = true;
            }
            Some(SourceCheck::Unknown(code)) => {
                return unknown(
                    code,
                    "The source or its supporting configuration could not be fully validated.",
                )
            }
            None => match std::fs::metadata(&source) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    checked = true;
                    source_absent = true;
                }
                _ => {
                    return unknown(
                        "source_not_checked",
                        "The historical source was not successfully read in this scan.",
                    )
                }
            },
        }
    }
    if checked {
        if source_absent {
            ConnectionValidation::missing(
                checked_at,
                "source_missing",
                "The previously recorded local source is no longer present.",
            )
        } else {
            ConnectionValidation::missing(
                checked_at,
                "reference_missing",
                "The local source was read successfully and no longer contains this reference.",
            )
        }
    } else {
        unknown(
            "source_not_checked",
            "The source was not successfully checked.",
        )
    }
}

fn location_patterns(location: &Location, project_roots: &[String]) -> Vec<String> {
    if let Some(relative) = location.path.strip_prefix("$PROJECT_ROOTS/") {
        project_roots
            .iter()
            .map(|root| Path::new(root).join(relative).display().to_string())
            .collect()
    } else {
        vec![location.path.clone()]
    }
}

#[derive(Debug, Default)]
pub struct WatchPlan {
    pub roots: Vec<PathBuf>,
    pub patterns: Vec<PathBuf>,
}

/// Build once when scan settings or the directory topology changes. No registry
/// reload is needed when the native worker already has the current settings.
pub fn watch_plan_with_settings(settings: &Settings) -> WatchPlan {
    watch_plan_with_paths(&app_home(), &ScanPaths::current(), settings)
}

pub fn watch_roots() -> Vec<PathBuf> {
    watch_roots_with_paths(&app_home(), &ScanPaths::current())
}

fn watch_roots_with_paths(home: &Path, paths: &ScanPaths) -> Vec<PathBuf> {
    let settings = match crate::registry::Registry::load(home) {
        Ok(registry) => registry.file.settings,
        Err(_) => return vec![home.to_path_buf()],
    };
    watch_plan_with_paths(home, paths, &settings).roots
}

fn watch_plan_with_paths(home: &Path, paths: &ScanPaths, settings: &Settings) -> WatchPlan {
    let mut patterns = BTreeSet::new();
    for relative in ["providers/*.toml", "inbox/*"] {
        if let Some(pattern) = paths.expand_pattern(&home.join(relative).display().to_string()) {
            patterns.insert(pattern);
        }
    }
    if settings.watchers_enabled {
        for descriptor in descriptors::load_all_scoped(home, paths).0 {
            if settings.provider_toggles.get(&descriptor.id).copied() == Some(false) {
                continue;
            }
            for location in descriptor.locations {
                for pattern in location_patterns(&location, &settings.project_roots) {
                    if let Some(pattern) = paths.expand_pattern(&pattern) {
                        // Labels also depend on these local auxiliary files.
                        let mut auxiliary = Vec::new();
                        if location.strategy == "vercel_auth" {
                            auxiliary.push(pattern.with_file_name("config.json"));
                        } else if location.strategy == "neon_auth" {
                            if let Some(parent) = pattern.parent() {
                                auxiliary.push(parent.join("profiles.json"));
                                if parent
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .is_some_and(|name| name.eq_ignore_ascii_case("neonctl"))
                                {
                                    if let Some(config_root) = parent.parent() {
                                        auxiliary.push(config_root.join("neon/profiles.json"));
                                    }
                                }
                            }
                        }
                        patterns.extend(auxiliary.into_iter().filter(|path| paths.allows(path)));
                        patterns.insert(pattern);
                    }
                }
            }
        }
    }
    let mut candidates = vec![home.to_path_buf()];
    for pattern in &patterns {
        let mut fixed = PathBuf::new();
        for component in pattern.components() {
            if component
                .as_os_str()
                .to_string_lossy()
                .contains(['*', '?', '['])
            {
                break;
            }
            fixed.push(component.as_os_str());
        }
        // Watch a newly created profile directory before its auth file exists.
        if fixed != *pattern {
            for ancestor in pattern
                .ancestors()
                .skip(1)
                .take_while(|ancestor| *ancestor != fixed)
            {
                candidates.extend(paths.expand(&ancestor.display().to_string()));
            }
        }
        candidates.push(fixed);
        candidates.extend(paths.expand(&pattern.display().to_string()));
    }
    let roots = candidates
        .into_iter()
        .filter_map(|candidate| {
            candidate
                .ancestors()
                .find(|path| path.is_dir() && paths.allows(path))
                .map(Path::to_path_buf)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    WatchPlan {
        roots,
        patterns: patterns.into_iter().collect(),
    }
}

fn supported_strategy(strategy: &str) -> bool {
    matches!(
        strategy,
        "azure_profile"
            | "docker_auths"
            | "glab_config"
            | "gh_hosts"
            | "neon_auth"
            | "npmrc"
            | "profile_file"
            | "shopify_account_info"
            | "token_file"
            | "vercel_auth"
            | "vercel_project"
    )
}

// Positive references can still be displayed from partly recognized documents;
// only a recognized complete shape supplies evidence that a prior row is absent.
fn recognized_shape(
    strategy: &str,
    value: &serde_json::Value,
    rows: &[DetectedConnection],
) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.is_empty() {
        return true;
    }
    match strategy {
        "gh_hosts" => object.values().all(|host| {
            host.as_object().is_some_and(|host| {
                host.is_empty()
                    || if let Some(users) = host.get("users") {
                        users
                            .as_object()
                            .is_some_and(|users| users.values().all(serde_json::Value::is_object))
                            && host.get("user").is_none_or(serde_json::Value::is_string)
                    } else {
                        host.get("user").is_some_and(serde_json::Value::is_string)
                    }
            })
        }),
        "profile_file" => object.values().all(serde_json::Value::is_object),
        "npmrc" => object
            .get("default")
            .is_some_and(serde_json::Value::is_object),
        "docker_auths" => object
            .get("auths")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|auths| auths.values().all(serde_json::Value::is_object)),
        "glab_config" => object
            .get("hosts")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|hosts| hosts.values().all(serde_json::Value::is_object)),
        "azure_profile" => object
            .get("subscriptions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|rows| {
                rows.iter().all(|row| {
                    row.is_object()
                        && row
                            .get("name")
                            .or_else(|| row.get("id"))
                            .is_some_and(serde_json::Value::is_string)
                })
            }),
        "shopify_account_info" => object.values().all(|account| {
            account.is_object() && account.get("info").is_none_or(serde_json::Value::is_object)
        }),
        "token_file" | "neon_auth" | "vercel_auth" | "vercel_project" => !rows.is_empty(),
        _ => false,
    }
}

fn auxiliary_readable(path: &Path, strategy: &str, paths: &ScanPaths) -> bool {
    let mut candidates = Vec::new();
    if strategy == "vercel_auth" {
        candidates.push(path.with_file_name("config.json"));
    }
    if strategy == "neon_auth" {
        if let Some(parent) = path.parent() {
            candidates.push(parent.join("profiles.json"));
            if parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("neonctl"))
            {
                if let Some(root) = parent.parent() {
                    candidates.push(root.join("neon/profiles.json"));
                }
            }
        }
    }
    candidates.iter().all(|candidate| {
        if !paths.allows(candidate) {
            return false;
        }
        match std::fs::metadata(candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Ok(metadata) if metadata.is_file() => {
                parse(candidate, parsers::Format::Json).is_ok_and(|doc| doc.value.is_object())
            }
            Ok(_) => false,
            Err(_) => false,
        }
    })
}

fn detect_descriptor(
    descriptor: &Descriptor,
    project_roots: &[String],
    paths: &ScanPaths,
    detected: &mut Vec<DetectedConnection>,
    errors: &mut Vec<ProviderError>,
    coverage: &mut Vec<LocationCoverage>,
) {
    let mut scanned = BTreeMap::<(PathBuf, String), SourceCheck>::new();
    for location in &descriptor.locations {
        for pattern in location_patterns(location, project_roots) {
            let expansion = paths.expand_checked(&pattern);
            let mut item = LocationCoverage {
                provider: descriptor.id.clone(),
                pattern: paths.expand_pattern(&pattern),
                strategy: location.strategy.clone(),
                complete: expansion.complete,
                sources: BTreeMap::new(),
            };
            for path in expansion.paths {
                let canonical = descriptors::comparable_path(&path);
                let key = (canonical.clone(), location.strategy.clone());
                if let Some(result) = scanned.get(&key) {
                    item.sources.insert(canonical, result.clone());
                    continue;
                }
                let result = if !supported_strategy(&location.strategy) {
                    SourceCheck::Unknown("strategy_unsupported")
                } else {
                    match std::fs::metadata(&path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            SourceCheck::Absent
                        }
                        Err(_) => SourceCheck::Unknown("source_unreadable"),
                        Ok(metadata) if !metadata.is_file() => {
                            SourceCheck::Unknown("not_regular_file")
                        }
                        Ok(_) => match parse(&path, location.format) {
                            Ok(doc) if auxiliary_readable(&path, &location.strategy, paths) => {
                                let rows = dispatch_strategy(
                                    descriptor, location, &path, &doc.value, false,
                                );
                                let complete =
                                    recognized_shape(&location.strategy, &doc.value, &rows);
                                detected.extend(rows);
                                if complete {
                                    SourceCheck::Read
                                } else {
                                    SourceCheck::Unknown("source_shape_unknown")
                                }
                            }
                            Ok(_) => SourceCheck::Unknown("auxiliary_unreadable"),
                            Err(err) => {
                                errors.push(ProviderError {
                                    provider: descriptor.id.clone(),
                                    code: err.code,
                                    message: format!("Could not scan {}", path.display()),
                                    detail: Some(secutil::redact(&err.message)),
                                });
                                SourceCheck::Unknown("source_unreadable")
                            }
                        },
                    }
                };
                scanned.insert(key, result.clone());
                item.sources.insert(canonical, result);
            }
            coverage.push(item);
        }
    }
    if descriptor.id == "github" && !paths.is_isolated() {
        if let Some(filter) = descriptor.credman_filter.as_deref() {
            match crate::credman::enumerate(filter) {
                Ok(entries) => {
                    detected.extend(strategies::github::from_credman(descriptor, entries))
                }
                Err(err) => errors.push(ProviderError {
                    provider: descriptor.id.clone(),
                    code: "credman_unavailable".to_string(),
                    message: "Windows Credential Manager could not be enumerated".to_string(),
                    detail: Some(err),
                }),
            }
        }
    }
}

fn dispatch_strategy(
    descriptor: &Descriptor,
    location: &Location,
    path: &Path,
    value: &serde_json::Value,
    probes_enabled: bool,
) -> Vec<DetectedConnection> {
    match location.strategy.as_str() {
        "azure_profile" => strategies::profiles::azure_profile(descriptor, location, path, value),
        "docker_auths" => strategies::profiles::docker_auths(descriptor, location, path, value),
        "glab_config" => strategies::profiles::glab_config(descriptor, location, path, value),
        "gh_hosts" => strategies::github::gh_hosts(descriptor, location, path, value),
        "neon_auth" => strategies::neon::auth_file(descriptor, location, path, value),
        "npmrc" => strategies::profiles::npmrc(descriptor, location, path, value),
        "profile_file" => strategies::profiles::profile_file(descriptor, location, path, value),
        "shopify_account_info" => {
            strategies::shopify::account_info(descriptor, location, path, value)
        }
        "token_file" => strategies::tokens::token_file(descriptor, location, path, value),
        "vercel_auth" => {
            strategies::vercel::auth_file(descriptor, location, path, value, probes_enabled)
        }
        "vercel_project" => strategies::profiles::vercel_project(descriptor, location, path, value),
        _ => Vec::new(),
    }
}

pub fn provider_catalog() -> Vec<BTreeMap<String, String>> {
    descriptors::bundled_descriptors()
        .into_iter()
        .map(|descriptor| {
            BTreeMap::from([
                ("id".to_string(), descriptor.id),
                ("name".to_string(), descriptor.name),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConnectionStatus, SourceType};
    use crate::registry::Registry;
    use std::fs;

    fn write(home: &Path, path: &str, value: &str) {
        let path = home.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }

    fn scan(home: &Path, provider: Option<&str>) -> Snapshot {
        scan_with_paths(
            home,
            provider.map(str::to_string),
            &ScanPaths::isolated(home),
        )
        .unwrap()
    }

    fn github(home: &Path, credential: &str) {
        write(
            home,
            ".config/gh/hosts.yml",
            &format!("github.com:\n  user: fixture-user\n  oauth_token: {credential}\n"),
        );
    }

    #[test]
    fn isolated_scan_persists_change_acknowledgement_missing_and_purge() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        github(home, "fixture-first-credential");
        let initial = scan(home, None);
        assert!(initial.provider_errors.is_empty());
        assert_eq!(initial.connections.len(), 1);
        assert_eq!(
            initial.connections[0].connection.status,
            ConnectionStatus::Active
        );
        assert!(initial
            .connections
            .iter()
            .all(|row| row.connection.source.source_type != SourceType::EnvVar));
        let id = initial.connections[0].connection.id.clone();
        let first_seen = initial.connections[0].connection.first_seen.clone();
        github(home, "fixture-second-credential");
        let changed = scan(home, Some("github"));
        assert_eq!(changed.connections[0].connection.id, id);
        assert_eq!(changed.connections[0].connection.first_seen, first_seen);
        assert_eq!(
            changed.connections[0].connection.status,
            ConnectionStatus::Changed
        );
        assert_eq!(
            scan(home, Some("github")).connections[0].connection.status,
            ConnectionStatus::Changed
        );
        with_registry_at(home, |registry| registry.mark_all_seen()).unwrap();
        assert_eq!(
            scan(home, None).connections[0].connection.status,
            ConnectionStatus::Active
        );
        fs::remove_file(home.join(".config/gh/hosts.yml")).unwrap();
        let missing = scan(home, None);
        assert_eq!(
            missing.connections[0].connection.status,
            ConnectionStatus::Missing
        );
        assert!(missing.connections[0].removable);
        let persisted = fs::read_to_string(home.join("registry.json")).unwrap();
        assert!(!persisted.contains("fixture-first-credential"));
        assert!(!persisted.contains("fixture-second-credential"));
        assert_eq!(
            with_registry_at(home, |registry| registry.purge_missing()).unwrap(),
            1
        );
        assert!(Registry::load(home).unwrap().file.connections.is_empty());
    }

    #[test]
    fn provider_scope_disabled_provider_and_parse_errors_preserve_other_history() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        github(home, "fixture-credential");
        write(
            home,
            ".aws/config",
            "[profile sample]\nsso_account_name = Fixture account\nsso_account_id = 123456789012\n",
        );
        let all = scan(home, None);
        assert_eq!(all.connections.len(), 2);
        fs::remove_file(home.join(".aws/config")).unwrap();
        let scoped = scan(home, Some("github"));
        assert!(scoped
            .connections
            .iter()
            .all(|row| row.connection.status != ConnectionStatus::Missing));
        write(home, ".config/gh/hosts.yml", "github.com: [invalid");
        let invalid = scan(home, Some("github"));
        assert_eq!(invalid.provider_errors.len(), 1);
        let cached = Registry::load(home)
            .unwrap()
            .snapshot(Vec::new(), crate::models::WatcherHealth::Degraded);
        assert_eq!(cached.provider_errors, invalid.provider_errors);
        assert_eq!(
            scan(home, Some("aws")).provider_errors,
            invalid.provider_errors
        );
        assert!(invalid
            .connections
            .iter()
            .all(|row| row.connection.status != ConnectionStatus::Missing));
        with_registry_at(home, |registry| {
            registry
                .file
                .settings
                .provider_toggles
                .insert("github".into(), false);
        })
        .unwrap();
        fs::remove_file(home.join(".config/gh/hosts.yml")).unwrap();
        let disabled = scan(home, None);
        assert_eq!(
            disabled
                .connections
                .iter()
                .find(|row| row.connection.provider == "github")
                .unwrap()
                .connection
                .status,
            ConnectionStatus::Unverified
        );
        assert_eq!(
            disabled
                .connections
                .iter()
                .find(|row| row.connection.provider == "aws")
                .unwrap()
                .connection
                .status,
            ConnectionStatus::Missing
        );
        assert!(disabled.provider_errors.is_empty());
    }

    fn row<'a>(snapshot: &'a Snapshot, provider: &str) -> &'a crate::models::SnapshotConnection {
        snapshot
            .connections
            .iter()
            .find(|row| row.connection.provider == provider)
            .unwrap()
    }

    fn custom(home: &Path, strategy: &str, pattern: &str) {
        write(home, "providers/example.toml", &format!(
            "id = 'example'\nname = 'Example'\n[[locations]]\npath = '{pattern}'\nformat = 'json'\nstrategy = '{strategy}'\n"));
    }

    #[test]
    fn validation_distinguishes_selected_reference_unknown_missing_and_reappearance() {
        use crate::models::{Availability, Usage};
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        write(
            home,
            ".config/gh/hosts.yml",
            "github.com:\n  user: alice\n  users:\n    alice: {}\n    bob: {}\n",
        );
        let initial = scan(home, None);
        let alice = initial
            .connections
            .iter()
            .find(|row| row.connection.identity.label == "alice")
            .unwrap();
        let bob = initial
            .connections
            .iter()
            .find(|row| row.connection.identity.label == "bob")
            .unwrap();
        assert_eq!(alice.connection.validation.usage, Usage::Selected);
        assert_eq!(bob.connection.validation.usage, Usage::Referenced);
        write(
            home,
            ".config/gh/hosts.yml",
            "github.com:\n  users:\n    malformed: 4\n",
        );
        let malformed = scan(home, None);
        let old = malformed
            .connections
            .iter()
            .find(|row| row.connection.id == alice.connection.id)
            .unwrap();
        assert_eq!(
            old.connection.validation.availability,
            Availability::Unknown
        );
        assert!(!old.connection.identity.is_active_identity);
        assert!(!old.removable);
        fs::remove_file(home.join(".config/gh/hosts.yml")).unwrap();
        let missing = scan(home, None);
        assert!(missing
            .connections
            .iter()
            .all(|row| row.connection.validation.availability == Availability::Missing));
        write(home, ".config/gh/hosts.yml", "github.com:\n  user: alice\n");
        let back = scan(home, None);
        let old = back
            .connections
            .iter()
            .find(|row| row.connection.id == alice.connection.id)
            .unwrap();
        assert_eq!(
            old.connection.validation.availability,
            Availability::Available
        );
        assert_eq!(old.connection.validation.usage, Usage::Selected);
        assert_eq!(old.connection.first_seen, alice.connection.first_seen);
        assert!(!old.removable);
    }

    #[test]
    fn disappeared_descriptor_unsupported_reader_and_changed_path_do_not_prove_missing() {
        use crate::models::Availability;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        custom(home, "token_file", "%HOME%/.example/auth.json");
        write(
            home,
            ".example/auth.json",
            r#"{"email":"fixture@example.test","token":"fixture-local-value"}"#,
        );
        scan(home, None);
        fs::remove_file(home.join("providers/example.toml")).unwrap();
        assert_eq!(
            row(&scan(home, None), "example")
                .connection
                .validation
                .reason_code,
            "descriptor_unavailable"
        );
        custom(home, "future_reader", "%HOME%/.example/auth.json");
        let unsupported = scan(home, None);
        assert_eq!(
            row(&unsupported, "example")
                .connection
                .validation
                .availability,
            Availability::Unknown
        );
        assert!(!row(&unsupported, "example").removable);
        custom(home, "token_file", "%HOME%/.example/moved.json");
        assert_eq!(
            row(&scan(home, None), "example")
                .connection
                .validation
                .reason_code,
            "source_out_of_scope"
        );
        custom(home, "token_file", "%HOME%/.example/auth.json");
        with_registry_at(home, |registry| {
            registry
                .file
                .settings
                .provider_toggles
                .insert("example".into(), false);
        })
        .unwrap();
        assert_eq!(
            row(&scan(home, None), "example")
                .connection
                .validation
                .reason_code,
            "provider_disabled"
        );
    }

    #[test]
    fn empty_dedicated_source_is_missing_but_auxiliary_failure_is_unknown() {
        use crate::models::{Availability, Usage};
        for strategy in ["token_file", "vercel_auth", "neon_auth"] {
            let fixture = tempfile::tempdir().unwrap();
            let home = fixture.path();
            custom(home, strategy, "%HOME%/.example/auth.json");
            write(
                home,
                ".example/auth.json",
                r#"{"email":"fixture@example.test","token":"fixture-local-value","access_token":"fixture-local-value"}"#,
            );
            let initial = scan(home, None);
            assert_eq!(
                row(&initial, "example").connection.validation.usage,
                Usage::Referenced
            );
            write(home, ".example/auth.json", "{}");
            assert_eq!(
                row(&scan(home, None), "example")
                    .connection
                    .validation
                    .availability,
                Availability::Missing,
                "{strategy}"
            );
            if strategy != "token_file" {
                write(
                    home,
                    if strategy == "vercel_auth" {
                        ".example/config.json"
                    } else {
                        ".example/profiles.json"
                    },
                    "{broken",
                );
                let unknown = scan(home, None);
                assert_eq!(
                    row(&unknown, "example").connection.validation.reason_code,
                    "auxiliary_unreadable"
                );
                assert!(!row(&unknown, "example").removable);
            }
        }
    }

    #[test]
    fn truncated_glob_never_confirms_absence() {
        use crate::models::Availability;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        custom(home, "token_file", "%HOME%/.example/*/auth.json");
        write(
            home,
            ".example/z-last/auth.json",
            r#"{"token":"fixture-local-value","email":"last@example.test"}"#,
        );
        let initial = scan(home, None);
        let id = row(&initial, "example").connection.id.clone();
        for number in 0..50 {
            write(home, &format!(".example/a-{number:02}/auth.json"), "{}");
        }
        let capped = scan(home, None);
        let old = capped
            .connections
            .iter()
            .find(|row| row.connection.id == id)
            .unwrap();
        assert_eq!(
            old.connection.validation.availability,
            Availability::Unknown
        );
        assert_eq!(old.connection.validation.reason_code, "scan_incomplete");
        assert!(!old.removable);
    }

    #[test]
    fn removed_project_root_keeps_history_with_unknown_availability() {
        use crate::models::Availability;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        write(
            home,
            "project/.vercel/project.json",
            r#"{"projectId":"fixture-project"}"#,
        );
        with_registry_at(home, |registry| {
            registry.file.settings.project_roots = vec![home.join("project").display().to_string()]
        })
        .unwrap();
        let first = scan(home, None);
        assert_eq!(first.connections.len(), 1);
        with_registry_at(home, |registry| {
            registry.file.settings.project_roots.clear()
        })
        .unwrap();
        let unchecked = scan(home, None);
        assert_eq!(unchecked.connections.len(), 1);
        assert_eq!(
            unchecked.connections[0].connection.validation.availability,
            Availability::Unknown
        );
        assert!(!unchecked.connections[0].removable);
    }

    #[test]
    fn renamed_profile_and_malformed_account_containers_do_not_prove_absence() {
        use crate::models::Availability;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        write(
            home,
            ".aws/config",
            "[profile sample]\nsso_account_name = Old display\n",
        );
        let before = scan(home, None);
        let id = row(&before, "aws").connection.id.clone();
        write(
            home,
            ".aws/config",
            "[profile sample]\nsso_account_name = New display\n",
        );
        let after = scan(home, None);
        let old = after
            .connections
            .iter()
            .find(|row| row.connection.id == id)
            .unwrap();
        assert_eq!(
            old.connection.validation.availability,
            Availability::Unknown
        );
        assert_eq!(old.connection.validation.reason_code, "identity_changed");
        assert!(!old.removable);
        for (strategy, value) in [
            (
                "shopify_account_info",
                serde_json::json!({"old-account": 4}),
            ),
            (
                "shopify_account_info",
                serde_json::json!({"old-account": {"info": []}}),
            ),
            (
                "azure_profile",
                serde_json::json!({"subscriptions": [{"id": "good"}, 4]}),
            ),
            (
                "docker_auths",
                serde_json::json!({"auths": {"old-host": []}}),
            ),
            (
                "glab_config",
                serde_json::json!({"hosts": {"old-host": []}}),
            ),
        ] {
            assert!(!recognized_shape(strategy, &value, &[]), "{strategy}");
        }
    }

    #[test]
    fn unreadable_directory_source_is_unknown_and_never_removed() {
        use crate::models::Availability;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        github(home, "fixture-local-value");
        scan(home, None);
        fs::remove_file(home.join(".config/gh/hosts.yml")).unwrap();
        fs::create_dir(home.join(".config/gh/hosts.yml")).unwrap();
        let result = scan(home, None);
        assert_eq!(
            row(&result, "github").connection.validation.availability,
            Availability::Unknown
        );
        assert_eq!(
            row(&result, "github").connection.validation.reason_code,
            "not_regular_file"
        );
        assert!(!row(&result, "github").removable);
    }

    #[test]
    fn watcher_roots_include_new_profile_directories_before_credentials_exist() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let paths = ScanPaths::isolated(home);
        let profile = paths
            .expand("%APPDATA%/VercelProfiles/new-profile")
            .pop()
            .unwrap();
        fs::create_dir_all(&profile).unwrap();
        let roots = watch_roots_with_paths(home, &paths);
        assert!(roots.contains(&profile));
        assert!(roots.iter().all(|path| path.starts_with(home)));
        fs::remove_dir(&profile).unwrap();
        let roots = watch_roots_with_paths(home, &paths);
        assert!(!roots.contains(&profile));
        assert!(roots.contains(&profile.parent().unwrap().to_path_buf()));
    }

    #[test]
    fn watch_plan_tracks_auxiliary_labels_and_respects_disabled_providers() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        let paths = ScanPaths::isolated(home);
        let mut settings = Settings::default();
        let plan = watch_plan_with_paths(home, &paths, &settings);
        let vercel_config = paths
            .expand_pattern("%APPDATA%/com.vercel.cli/config.json")
            .unwrap();
        let neon_profiles = paths
            .expand_pattern("%XDG_CONFIG_HOME%/neon/profiles.json")
            .unwrap();
        assert!(plan.patterns.contains(&vercel_config));
        assert!(plan.patterns.contains(&neon_profiles));
        assert!(plan.patterns.contains(&home.join("providers/*.toml")));
        assert!(plan.patterns.iter().all(|path| path.starts_with(home)));
        settings.provider_toggles.insert("vercel".into(), false);
        let disabled = watch_plan_with_paths(home, &paths, &settings);
        assert!(!disabled.patterns.contains(&vercel_config));
        assert!(disabled.patterns.contains(&neon_profiles));
        settings.watchers_enabled = false;
        let paused = watch_plan_with_paths(home, &paths, &settings);
        assert_eq!(paused.patterns.len(), 2);
        assert!(paused.roots.iter().all(|path| path.starts_with(home)));
    }

    #[test]
    fn custom_descriptors_and_explicit_project_roots_are_scanned_inside_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        write(
            home,
            "providers/example.toml",
            r#"id = "example"
name = "Example service"
[[locations]]
path = "%HOME%/.example/auth.json"
format = "json"
strategy = "token_file"
[[env_vars]]
name = "PATH"
"#,
        );
        write(
            home,
            ".example/auth.json",
            r#"{"email":"fixture@example.test","token":"fixture-custom-credential"}"#,
        );
        write(
            home,
            "project/.vercel/project.json",
            r#"{"orgId":"team_fixture","projectId":"prj_fixture"}"#,
        );
        with_registry_at(home, |registry| {
            registry.file.settings.project_roots = vec![home.join("project").display().to_string()]
        })
        .unwrap();
        let snapshot = scan(home, None);
        assert!(snapshot.provider_errors.is_empty());
        assert_eq!(snapshot.connections.len(), 2);
        assert!(snapshot
            .connections
            .iter()
            .any(|row| row.connection.provider == "example"));
        assert!(snapshot
            .connections
            .iter()
            .any(|row| row.connection.provider == "vercel"));
        assert!(snapshot
            .connections
            .iter()
            .all(|row| row.connection.source.source_type == SourceType::ConfigFile));
        assert!(!fs::read_to_string(home.join("registry.json"))
            .unwrap()
            .contains("fixture-custom-credential"));
    }
}
