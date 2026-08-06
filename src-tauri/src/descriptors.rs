use crate::models::{is_retired_provider_id, ProviderError};
use crate::registry::app_home;
use crate::scan::parsers::Format;
use glob::glob;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const BUNDLED: &[(&str, &str)] = &[
    (
        "github.toml",
        include_str!("../resources/providers/github.toml"),
    ),
    ("aws.toml", include_str!("../resources/providers/aws.toml")),
    (
        "azure.toml",
        include_str!("../resources/providers/azure.toml"),
    ),
    (
        "cloudflare.toml",
        include_str!("../resources/providers/cloudflare.toml"),
    ),
    (
        "docker.toml",
        include_str!("../resources/providers/docker.toml"),
    ),
    (
        "gcloud.toml",
        include_str!("../resources/providers/gcloud.toml"),
    ),
    (
        "gitlab.toml",
        include_str!("../resources/providers/gitlab.toml"),
    ),
    (
        "netlify.toml",
        include_str!("../resources/providers/netlify.toml"),
    ),
    ("npm.toml", include_str!("../resources/providers/npm.toml")),
    (
        "sentry.toml",
        include_str!("../resources/providers/sentry.toml"),
    ),
    (
        "vercel.toml",
        include_str!("../resources/providers/vercel.toml"),
    ),
    (
        "neon.toml",
        include_str!("../resources/providers/neon.toml"),
    ),
    (
        "shopify.toml",
        include_str!("../resources/providers/shopify.toml"),
    ),
    (
        "stripe.toml",
        include_str!("../resources/providers/stripe.toml"),
    ),
    (
        "supabase.toml",
        include_str!("../resources/providers/supabase.toml"),
    ),
];

#[derive(Debug, Clone, Deserialize)]
pub struct Descriptor {
    pub id: String,
    pub name: String,
    pub dashboard_url: Option<String>,
    pub credman_filter: Option<String>,
    #[serde(default)]
    pub env_vars: Vec<EnvVarDescriptor>,
    #[serde(default)]
    pub locations: Vec<Location>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvVarDescriptor {
    pub name: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Location {
    pub path: String,
    pub format: Format,
    pub strategy: String,
    #[serde(default = "default_source_type")]
    pub source_type: String,
    pub scope: Option<String>,
    pub confidence: Option<String>,
}

fn default_source_type() -> String {
    "config_file".to_string()
}

pub fn load_all(home: &Path) -> (Vec<Descriptor>, Vec<ProviderError>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();

    for (name, source) in BUNDLED {
        match toml::from_str::<Descriptor>(source) {
            Ok(descriptor) => entries.push((name.to_string(), descriptor)),
            Err(err) => errors.push(ProviderError {
                provider: "descriptors".to_string(),
                code: "invalid_bundled_descriptor".to_string(),
                message: format!("Bundled descriptor {name} is invalid"),
                detail: Some(err.to_string()),
            }),
        }
    }

    let user_dir = home.join("providers");
    if let Ok(read_dir) = fs::read_dir(&user_dir) {
        let mut paths = read_dir
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
            .collect::<Vec<_>>();
        paths.sort();

        for path in paths {
            match fs::read_to_string(&path)
                .ok()
                .and_then(|text| toml::from_str::<Descriptor>(&text).ok())
            {
                Some(descriptor) => entries.push((path.display().to_string(), descriptor)),
                None => errors.push(ProviderError {
                    provider: "descriptors".to_string(),
                    code: "invalid_user_descriptor".to_string(),
                    message: "A user provider descriptor was skipped".to_string(),
                    detail: Some(path.display().to_string()),
                }),
            }
        }
    }

    let mut merged = Vec::<Descriptor>::new();
    for (_source, descriptor) in entries {
        if let Some(index) = merged
            .iter()
            .position(|existing| existing.id == descriptor.id)
        {
            merged[index] = descriptor;
        } else {
            merged.push(descriptor);
        }
    }
    merged.sort_by(|a, b| a.id.cmp(&b.id));
    merged.retain(|descriptor| !is_retired_provider_id(&descriptor.id));
    (merged, errors)
}

pub fn bundled_descriptors() -> Vec<Descriptor> {
    load_all(&app_home()).0
}

pub fn expand(pattern: &str) -> Vec<PathBuf> {
    let Some(expanded) = expand_env(pattern) else {
        return Vec::new();
    };

    let candidate = if expanded.starts_with("~/") || expanded == "~" {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(|home| expanded.replacen('~', &home, 1))
            .unwrap_or(expanded)
    } else {
        expanded
    };

    if candidate.contains('*') || candidate.contains('?') || candidate.contains('[') {
        let mut seen = BTreeSet::new();
        return glob(&candidate)
            .ok()
            .into_iter()
            .flat_map(|paths| paths.filter_map(Result::ok))
            .filter(|path| seen.insert(path.clone()))
            .take(50)
            .collect();
    }

    vec![PathBuf::from(candidate)]
}

fn expand_env(pattern: &str) -> Option<String> {
    let mut output = String::new();
    let mut rest = pattern;

    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let key = &after[..end];
        let value = std::env::var(key).ok()?;
        output.push_str(&value);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    Some(output.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undefined_env_drops_candidate() {
        assert!(expand("%CONNLENS_DOES_NOT_EXIST%/x.json").is_empty());
    }

    #[test]
    fn bundled_descriptors_have_unique_ids() {
        let (descriptors, errors) = load_all(Path::new("missing-home"));
        assert!(errors.is_empty());
        let unique = descriptors
            .iter()
            .map(|descriptor| descriptor.id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), descriptors.len());
    }

    #[test]
    fn vercel_project_links_require_explicit_project_roots() {
        let (descriptors, errors) = load_all(Path::new("missing-home"));
        assert!(errors.is_empty());
        let vercel = descriptors
            .iter()
            .find(|descriptor| descriptor.id == "vercel")
            .unwrap();
        let project_locations = vercel
            .locations
            .iter()
            .filter(|location| location.strategy == "vercel_project")
            .collect::<Vec<_>>();
        assert_eq!(project_locations.len(), 1);
        assert_eq!(
            project_locations[0].path,
            "$PROJECT_ROOTS/.vercel/project.json"
        );
    }
}
