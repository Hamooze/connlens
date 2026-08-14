pub mod parsers;
pub mod secutil;
pub mod strategies;

use crate::descriptors::{self, Descriptor, Location};
use crate::models::{DetectedConnection, ErrorPayload, ProviderError, Snapshot, WatcherHealth};
use crate::registry::{app_home, Registry};
use parsers::parse;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn scan_and_persist(provider: Option<String>) -> Result<Snapshot, ErrorPayload> {
    let home = app_home();
    let mut registry = Registry::load(&home).map_err(ErrorPayload::from)?;
    let settings = registry.file.settings.clone();
    let (descriptors, mut errors) = descriptors::load_all(&home);
    let mut detected = Vec::new();

    for descriptor in &descriptors {
        if let Some(target) = provider.as_deref() {
            if descriptor.id != target {
                continue;
            }
        }
        if settings.provider_toggles.get(&descriptor.id).copied() == Some(false) {
            continue;
        }

        detect_descriptor(
            descriptor,
            &settings.project_roots,
            settings.probes_enabled,
            &mut detected,
            &mut errors,
        );
    }

    detected.extend(strategies::envvars::scan(&descriptors));
    let scope = provider.as_deref();
    registry.diff(detected, scope);
    registry.save().map_err(ErrorPayload::from)?;
    let health = if registry.file.settings.watchers_enabled {
        WatcherHealth::Ok
    } else {
        WatcherHealth::Paused
    };
    Ok(registry.snapshot(errors, health))
}

fn detect_descriptor(
    descriptor: &Descriptor,
    project_roots: &[String],
    probes_enabled: bool,
    detected: &mut Vec<DetectedConnection>,
    errors: &mut Vec<ProviderError>,
) {
    for location in &descriptor.locations {
        let mut candidates = descriptors::expand(&location.path);
        if let Some(relative) = location.path.strip_prefix("$PROJECT_ROOTS/") {
            candidates = project_roots
                .iter()
                .map(|root| PathBuf::from(root).join(relative))
                .collect();
        }

        for path in candidates {
            if !path.exists() {
                continue;
            }
            match parse(&path, location.format) {
                Ok(doc) => {
                    detected.extend(dispatch_strategy(
                        descriptor,
                        location,
                        &path,
                        &doc.value,
                        probes_enabled,
                    ));
                }
                Err(err) => errors.push(ProviderError {
                    provider: descriptor.id.clone(),
                    code: err.code,
                    message: format!("Could not scan {}", path.display()),
                    detail: Some(err.message),
                }),
            }
        }
    }

    if descriptor.id == "github" {
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
