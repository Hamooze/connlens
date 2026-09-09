use crate::models::{is_retired_provider_id, ProviderError};
use crate::registry::app_home;
use crate::scan::parsers::Format;
use glob::glob;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

fn parsed_bundled_descriptors() -> &'static (Vec<Descriptor>, Vec<ProviderError>) {
    static PARSED: OnceLock<(Vec<Descriptor>, Vec<ProviderError>)> = OnceLock::new();
    PARSED.get_or_init(|| {
        let mut descriptors = Vec::with_capacity(BUNDLED.len());
        let mut errors = Vec::new();
        for (name, source) in BUNDLED {
            match toml::from_str::<Descriptor>(source) {
                Ok(descriptor) => descriptors.push(descriptor),
                Err(err) => errors.push(ProviderError {
                    provider: "descriptors".to_string(),
                    code: "invalid_bundled_descriptor".to_string(),
                    message: format!("Bundled descriptor {name} is invalid"),
                    detail: Some(err.to_string()),
                }),
            }
        }
        (descriptors, errors)
    })
}

pub fn load_all(home: &Path) -> (Vec<Descriptor>, Vec<ProviderError>) {
    load_all_scoped(home, &ScanPaths::current())
}

pub fn load_all_scoped(home: &Path, paths: &ScanPaths) -> (Vec<Descriptor>, Vec<ProviderError>) {
    // Only immutable, secret-free bundled templates are cached. User descriptors
    // are reloaded so edits and separate fixture homes always take effect.
    let (bundled, bundled_errors) = parsed_bundled_descriptors();
    let mut entries = bundled.clone();
    let mut errors = bundled_errors.clone();

    let user_dir = home.join("providers");
    if let Some(Ok(read_dir)) = paths.allows(&user_dir).then(|| fs::read_dir(&user_dir)) {
        let mut paths = read_dir
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                paths.allows(path) && path.extension().and_then(|ext| ext.to_str()) == Some("toml")
            })
            .collect::<Vec<_>>();
        paths.sort();

        for path in paths {
            match fs::read_to_string(&path)
                .ok()
                .and_then(|text| toml::from_str::<Descriptor>(&text).ok())
            {
                Some(descriptor) => entries.push(descriptor),
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
    for descriptor in entries {
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

/// All discovery paths share one context, so fixture runs cannot inherit user homes.
#[derive(Debug, Clone)]
pub struct ScanPaths {
    isolated_home: Option<PathBuf>,
}

impl ScanPaths {
    pub fn current() -> Self {
        Self {
            isolated_home: std::env::var_os("CONNLENS_HOME").map(PathBuf::from),
        }
    }

    #[cfg(test)]
    pub fn isolated(home: &Path) -> Self {
        Self {
            isolated_home: Some(home.to_path_buf()),
        }
    }

    pub fn is_isolated(&self) -> bool {
        self.isolated_home.is_some()
    }

    pub fn allows(&self, path: &Path) -> bool {
        let Some(home) = &self.isolated_home else {
            return true;
        };
        let Some(home) = absolute_normalized(home) else {
            return false;
        };
        let Some(path) = absolute_normalized(path) else {
            return false;
        };
        if !path.starts_with(&home) {
            return false;
        }
        let canonical_home = home.canonicalize().unwrap_or(home);
        // Check the nearest existing ancestor too, including symlinked directories.
        path.ancestors()
            .find(|ancestor| ancestor.exists())
            .and_then(|ancestor| ancestor.canonicalize().ok())
            .is_some_and(|ancestor| ancestor.starts_with(canonical_home))
    }

    pub fn expand_pattern(&self, pattern: &str) -> Option<PathBuf> {
        if pattern.starts_with("$PROJECT_ROOTS") {
            return None;
        }
        let mut output = String::new();
        let mut rest = pattern;
        while let Some(start) = rest.find('%') {
            output.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let end = after.find('%')?;
            output.push_str(&self.env_value(&after[..end])?);
            rest = &after[end + 1..];
        }
        output.push_str(rest);
        let mut output = output.replace('\\', "/");
        if output.starts_with("~/") || output == "~" {
            output = output.replacen('~', &self.env_value("HOME")?, 1);
        }
        let mut path = PathBuf::from(output);
        if let Some(home) = &self.isolated_home {
            if path.is_relative() {
                path = home.join(path);
            }
        }
        let path = absolute_normalized(&path)?;
        self.allows(&path).then_some(path)
    }

    pub fn expand(&self, pattern: &str) -> Vec<PathBuf> {
        let Some(candidate) = self.expand_pattern(pattern) else {
            return Vec::new();
        };
        let text = candidate.to_string_lossy();
        if text.contains('*') || text.contains('?') || text.contains('[') {
            let mut seen = BTreeSet::new();
            return glob(&text)
                .ok()
                .into_iter()
                .flat_map(|paths| paths.filter_map(Result::ok))
                .filter(|path| self.allows(path) && seen.insert(path.clone()))
                .take(50)
                .collect();
        }
        vec![candidate]
    }

    fn env_value(&self, key: &str) -> Option<String> {
        if let Some(home) = &self.isolated_home {
            let relative = match key {
                "CONNLENS_HOME" | "HOME" | "USERPROFILE" => "",
                "XDG_CONFIG_HOME" => ".config",
                "XDG_DATA_HOME" => ".local/share",
                "APPDATA" if cfg!(windows) => "AppData/Roaming",
                "LOCALAPPDATA" if cfg!(windows) => "AppData/Local",
                "APPDATA" | "LOCALAPPDATA" if cfg!(target_os = "macos") => {
                    "Library/Application Support"
                }
                "APPDATA" => ".config",
                "LOCALAPPDATA" => ".local/share",
                _ => return None,
            };
            return absolute_normalized(home).map(|home| home.join(relative).display().to_string());
        }
        std::env::var(key)
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| fallback_env_value(key))
    }
}

pub fn expand(pattern: &str) -> Vec<PathBuf> {
    ScanPaths::current().expand(pattern)
}

fn absolute_normalized(path: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Some(normalized)
}

fn fallback_env_value(key: &str) -> Option<String> {
    match key {
        "HOME" | "USERPROFILE" => home_dir(),
        "XDG_CONFIG_HOME" => home_dir().map(|home| join_display(&home, ".config")),
        "XDG_DATA_HOME" => home_dir().map(|home| join_display(&home, ".local/share")),
        "APPDATA" => app_config_dir(),
        "LOCALAPPDATA" => app_data_dir(),
        _ => None,
    }
}

fn home_dir() -> Option<String> {
    let keys = if cfg!(windows) {
        ["USERPROFILE", "HOME"]
    } else {
        ["HOME", "USERPROFILE"]
    };
    keys.into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().display().to_string()))
}

fn app_config_dir() -> Option<String> {
    if cfg!(windows) {
        return directories::BaseDirs::new().map(|dirs| dirs.config_dir().display().to_string());
    }
    if cfg!(target_os = "macos") {
        return home_dir().map(|home| join_display(&home, "Library/Application Support"));
    }
    std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| home_dir().map(|home| join_display(&home, ".config")))
}

fn app_data_dir() -> Option<String> {
    if cfg!(windows) {
        return directories::BaseDirs::new()
            .map(|dirs| dirs.data_local_dir().display().to_string());
    }
    if cfg!(target_os = "macos") {
        return home_dir().map(|home| join_display(&home, "Library/Application Support"));
    }
    std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| home_dir().map(|home| join_display(&home, ".local/share")))
}

fn join_display(base: &str, child: &str) -> String {
    Path::new(base).join(child).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_builtins_do_not_cache_custom_descriptors_or_leak_between_homes() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_paths = ScanPaths::isolated(first.path());
        let second_paths = ScanPaths::isolated(second.path());
        fs::create_dir(first.path().join("providers")).unwrap();
        let path = first.path().join("providers/github.toml");
        fs::write(&path, "id = 'github'\nname = 'Custom GitHub'\n").unwrap();
        let name = |home: &Path, paths: &ScanPaths| {
            load_all_scoped(home, paths)
                .0
                .into_iter()
                .find(|provider| provider.id == "github")
                .unwrap()
                .name
        };
        assert_eq!(name(first.path(), &first_paths), "Custom GitHub");
        assert_eq!(name(second.path(), &second_paths), "GitHub");
        fs::write(&path, "id = 'github'\nname = 'Updated GitHub'\n").unwrap();
        assert_eq!(name(first.path(), &first_paths), "Updated GitHub");
        fs::remove_file(path).unwrap();
        assert_eq!(name(first.path(), &first_paths), "GitHub");
    }

    #[test]
    fn isolated_paths_remap_standard_homes_and_ignore_inherited_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(dir.path());
        for pattern in [
            "%HOME%/.aws/config",
            "%USERPROFILE%/.aws/config",
            "%APPDATA%/gh/hosts.yml",
            "%LOCALAPPDATA%/example.json",
            "%XDG_CONFIG_HOME%/gh/hosts.yml",
            "~/.aws/config",
            "relative.json",
        ] {
            let expanded = paths.expand(pattern);
            assert_eq!(expanded.len(), 1, "{pattern}");
            assert!(expanded[0].starts_with(dir.path()), "{pattern}");
        }
        assert!(paths.expand("%GH_CONFIG_DIR%/hosts.yml").is_empty());
        assert!(paths.expand("../outside.json").is_empty());
        let outside = tempfile::tempdir().unwrap();
        assert!(paths
            .expand(&outside.path().join("config.json").display().to_string())
            .is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn isolated_paths_skip_symlinks_outside_the_fixture() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("config.json"), "{}").unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("linked")).unwrap();
        let paths = ScanPaths::isolated(home.path());
        assert!(paths.expand("%HOME%/linked/config.json").is_empty());
        assert!(paths.expand("%HOME%/*/config.json").is_empty());
    }

    #[test]
    fn undefined_env_drops_candidate() {
        assert!(expand("%CONNLENS_DOES_NOT_EXIST%/x.json").is_empty());
    }

    #[test]
    fn home_fallback_expands_userprofile_style_paths() {
        if ScanPaths::current().is_isolated() {
            // A not-yet-created fixture home must not fall back to the real home.
            let paths = ScanPaths::current();
            assert!(expand("%USERPROFILE%/.config/gh/hosts.yml")
                .iter()
                .all(|path| paths.allows(path)));
        } else if home_dir().is_some() {
            assert!(!expand("%USERPROFILE%/.config/gh/hosts.yml").is_empty());
        }
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
