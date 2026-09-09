pub mod parsers;
pub mod secutil;
pub mod strategies;

use crate::descriptors::{self, Descriptor, Location, ScanPaths};
use crate::models::{DetectedConnection, ErrorPayload, ProviderError, Settings, Snapshot};
use crate::registry::{app_home, with_registry_at};
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
    with_registry_at(home, |registry| {
        let settings = registry.file.settings.clone();
        let (mut descriptors, mut errors) = descriptors::load_all_scoped(home, paths);
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
        descriptors.retain(|descriptor| {
            provider
                .as_deref()
                .is_none_or(|target| descriptor.id == target)
                && settings.provider_toggles.get(&descriptor.id).copied() != Some(false)
        });
        let mut detected = Vec::new();
        for descriptor in &descriptors {
            detect_descriptor(
                descriptor,
                &settings.project_roots,
                paths,
                &mut detected,
                &mut errors,
            );
        }
        if !paths.is_isolated() {
            detected.extend(strategies::envvars::scan(&descriptors));
        }
        // A temporarily unreadable config is not evidence that its connection was removed.
        let unavailable = errors.iter().map(|error| error.provider.clone()).collect();
        registry.diff_with_unavailable(detected, provider.as_deref(), &unavailable);
        registry.file.provider_errors = errors.clone();
        let health = crate::watchers::health(settings.watchers_enabled);
        registry.snapshot(errors, health)
    })
    .map_err(ErrorPayload::from)
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

fn detect_descriptor(
    descriptor: &Descriptor,
    project_roots: &[String],
    paths: &ScanPaths,
    detected: &mut Vec<DetectedConnection>,
    errors: &mut Vec<ProviderError>,
) {
    let mut scanned_paths = BTreeSet::new();
    for location in &descriptor.locations {
        let candidates = location_patterns(location, project_roots)
            .into_iter()
            .flat_map(|pattern| paths.expand(&pattern))
            .collect::<BTreeSet<_>>();
        for path in candidates {
            if !path.exists() {
                continue;
            }
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !scanned_paths.insert((canonical, location.strategy.clone())) {
                continue;
            }
            match parse(&path, location.format) {
                Ok(doc) => {
                    detected.extend(dispatch_strategy(
                        descriptor, location, &path, &doc.value, false,
                    ));
                }
                Err(err) => errors.push(ProviderError {
                    provider: descriptor.id.clone(),
                    code: err.code,
                    message: format!("Could not scan {}", path.display()),
                    detail: Some(secutil::redact(&err.message)),
                }),
            }
        }
    }

    if descriptor.id == "github" && !paths.is_isolated() {
        if let Some(filter) = descriptor.credman_filter.as_deref() {
            match crate::credman::enumerate(filter) {
                Ok(entries) => {
                    detected.extend(strategies::github::from_credman(descriptor, entries));
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
            .all(|row| row.connection.status == ConnectionStatus::Active));
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
            .all(|row| row.connection.status == ConnectionStatus::Active));
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
            ConnectionStatus::Active
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
