//! Local executable evidence only: never invoke commands or package managers.
use crate::descriptors::ScanPaths;
use crate::scan::secutil::redact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_COMMAND_BYTES: usize = 4096;
const MAX_PATH_DIRS: usize = 48;
const MAX_CANDIDATES: usize = 384;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolPresence {
    pub status: String,
    pub name: String,
    pub path: Option<String>,
    pub checked_at: String,
    pub reason: String,
    pub reason_code: String,
}

/// Preserve path separators while redacting each executable/path component.
/// The general token redactor permits '/' inside base64-like values, which can
/// otherwise combine ordinary directory names into a false secret match.
pub fn redact_command(command: &str) -> String {
    if command.len() > MAX_COMMAND_BYTES || command.chars().any(char::is_control) {
        return "Invalid command".to_string();
    }
    let mut safe = String::with_capacity(command.len());
    for part in command.split_inclusive(['/', '\\']) {
        if let Some(component) = part.strip_suffix(['/', '\\']) {
            safe.push_str(&redact(component));
            safe.push_str(&part[component.len()..]);
        } else {
            safe.push_str(&redact(part));
        }
    }
    safe
}

pub fn provider_command(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "aws" => "aws",
        "azure" => "az",
        "cloudflare" => "wrangler",
        "docker" => "docker",
        "gcloud" => "gcloud",
        "github" => "gh",
        "gitlab" => "glab",
        "netlify" => "netlify",
        "neon" => "neonctl",
        "npm" => "npm",
        "sentry" => "sentry-cli",
        "shopify" => "shopify",
        "stripe" => "stripe",
        "supabase" => "supabase",
        "vercel" => "vercel",
        _ => return None,
    })
}

pub fn check(
    command: &str,
    previous: Option<&ToolPresence>,
    paths: &ScanPaths,
    checked_at: &str,
) -> ToolPresence {
    let name = redact_command(command);
    let result = |status: &str, path: Option<&Path>, code: &str, reason: &str| ToolPresence {
        status: status.to_string(),
        name: name.clone(),
        path: path.and_then(display_path),
        checked_at: checked_at.to_string(),
        reason: redact(reason),
        reason_code: code.to_string(),
    };
    let plan = match candidate_plan(command, paths) {
        Ok(plan) => plan,
        Err(reason) => return result("unknown", None, "command_unresolved", reason),
    };
    let mut uncertain = !plan.complete;
    for candidate in &plan.paths {
        match probe(candidate, paths) {
            Probe::Found => {
                return result(
                    "found",
                    Some(candidate),
                    "executable_found",
                    found_reason(command, false),
                )
            }
            Probe::Unknown => uncertain = true,
            Probe::Absent => {}
        }
    }

    let historical = previous.filter(|previous| {
        (matches!(previous.status.as_str(), "found" | "missing")
            || (previous.status == "unknown"
                && matches!(
                    previous.reason_code.as_str(),
                    "executable_unchecked" | "observed_executable_search_incomplete"
                )))
            && previous.name == name
    });
    if let Some(previous) = historical {
        let Some(path) = previous.path.as_deref().map(Path::new) else {
            return result(
                "unknown",
                None,
                "previous_path_unavailable",
                "The previously found executable has no safe recorded path to check.",
            );
        };
        if !path.is_absolute()
            || path.as_os_str().len() > MAX_COMMAND_BYTES
            || display_path(path).as_deref() != previous.path.as_deref()
            || (plan.explicit && !plan.paths.iter().any(|candidate| candidate == path))
        {
            return result(
                "unknown",
                None,
                "previous_path_unavailable",
                "The previously found executable path cannot be matched to this command.",
            );
        }
        match probe(path, paths) {
            Probe::Found => return result("found", Some(path), "previous_executable_present", found_reason(command, true)),
            Probe::Unknown => return result("unknown", Some(path), "executable_unchecked", "The recorded executable could not be checked safely. Its account configuration may still be present."),
            Probe::Absent if !uncertain => return result("missing", Some(path), "observed_executable_missing", "The previously found executable is absent, and no replacement was found in the checked locations. Account configuration is checked separately."),
            Probe::Absent => return result("unknown", Some(path), "observed_executable_search_incomplete", "The previously found executable is absent, but other locations could not be checked completely. No removal is inferred."),
        }
    }
    if uncertain {
        result("unknown", None, "executable_search_incomplete", "Some executable locations could not be checked completely. No installation or removal is inferred.")
    } else {
        result("not_found", None, "executable_not_found", "No executable was found in the checked locations. This does not prove it was uninstalled; account configuration is checked separately.")
    }
}

/// Exact candidate paths for the existing watcher filter, including readable
/// symlink targets so removing a Homebrew/version-manager target is observed.
pub fn watch_candidates(command: &str, paths: &ScanPaths) -> Vec<PathBuf> {
    let Ok(plan) = candidate_plan(command, paths) else {
        return Vec::new();
    };
    let mut candidates: Vec<_> = plan
        .paths
        .into_iter()
        .filter(|path| isolated_links_safe(path, paths) && paths.allows(path))
        .collect();
    let mut seen: BTreeSet<_> = candidates.iter().cloned().collect();
    for candidate in candidates.clone() {
        if !paths.allows(&candidate) {
            continue;
        }
        if let Some(target) = candidate
            .canonicalize()
            .ok()
            .and_then(|target| scoped_target(target, paths))
        {
            if paths.allows(&target)
                && seen.insert(target.clone())
                && candidates.len() < MAX_CANDIDATES
            {
                candidates.push(target);
            }
        }
    }
    candidates
}

fn scoped_target(target: PathBuf, paths: &ScanPaths) -> Option<PathBuf> {
    if paths.allows(&target) {
        return Some(target);
    }
    if !paths.is_isolated() {
        return None;
    }
    // Preserve the fixture's /var spelling while accepting its verified
    // /private/var canonical counterpart on macOS.
    let home = paths.expand_pattern("%HOME%")?;
    let canonical_home = home.canonicalize().ok()?;
    let relative = target.strip_prefix(canonical_home).ok()?;
    let target = home.join(relative);
    paths.allows(&target).then_some(target)
}

struct CandidatePlan {
    paths: Vec<PathBuf>,
    complete: bool,
    explicit: bool,
}

fn candidate_plan(command: &str, paths: &ScanPaths) -> Result<CandidatePlan, &'static str> {
    if command.is_empty()
        || command.len() > MAX_COMMAND_BYTES
        || command.chars().any(char::is_control)
    {
        return Err("The executable name is empty, too long, or contains unsupported characters.");
    }
    let explicit = command.contains('/') || command.contains('\\');
    if explicit {
        // Relative commands depend on the launching client's working directory,
        // which this passive evidence API does not know.
        if !Path::new(command).is_absolute()
            && !command.starts_with("~/")
            && !command.starts_with('%')
        {
            return Err(
                "A relative executable path cannot be checked without its working directory.",
            );
        }
        let path = paths
            .expand_pattern(command)
            .ok_or("The executable path could not be resolved within the allowed locations.")?;
        if has_glob(command) {
            return Err("Executable paths with patterns cannot be checked as exact files.");
        }
        let (candidates, complete) = executable_variants(&path, paths);
        return Ok(CandidatePlan {
            paths: candidates,
            complete,
            explicit: true,
        });
    }
    if !command
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte))
        || command == "."
        || command == ".."
    {
        return Err("Only an executable name or an absolute executable path can be checked.");
    }
    let (mut directories, mut complete) = search_directories(paths);
    let mut seen = BTreeSet::new();
    directories.retain(|directory| seen.insert(directory.clone()));
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for directory in directories {
        let (variants, extensions_complete) = executable_variants(&directory.join(command), paths);
        complete &= extensions_complete;
        for candidate in variants {
            if candidates.len() >= MAX_CANDIDATES {
                complete = false;
                break;
            }
            if seen.insert(candidate.clone()) {
                candidates.push(candidate);
            }
        }
    }
    Ok(CandidatePlan {
        paths: candidates,
        complete,
        explicit: false,
    })
}

fn search_directories(paths: &ScanPaths) -> (Vec<PathBuf>, bool) {
    let mut directories = Vec::new();
    let mut complete = true;
    if !paths.is_isolated() {
        if let Some(path) = std::env::var_os("PATH") {
            let entries: Vec<_> = std::env::split_paths(&path)
                .take(MAX_PATH_DIRS + 1)
                .collect();
            complete = entries.len() <= MAX_PATH_DIRS;
            for path in entries.into_iter().take(MAX_PATH_DIRS) {
                // Never search implicit current-directory PATH entries.
                if path.is_absolute() {
                    directories.push(path);
                } else {
                    complete = false;
                }
            }
        }
    }
    for pattern in [
        "%HOME%/.local/bin",
        "%HOME%/bin",
        "%HOME%/.cargo/bin",
        "%HOME%/.npm-global/bin",
        "%HOME%/.bun/bin",
        "%HOME%/.volta/bin",
    ] {
        if let Some(path) = paths.expand_pattern(pattern) {
            directories.push(path);
        } else {
            complete = false;
        }
    }
    #[cfg(not(windows))]
    for fixed in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        let path = if paths.is_isolated() {
            paths
                .expand_pattern("%HOME%")
                .map(|home| home.join(fixed.trim_start_matches('/')))
        } else {
            Some(PathBuf::from(fixed))
        };
        if let Some(path) = path {
            directories.push(path);
        }
    }
    #[cfg(windows)]
    {
        for pattern in [
            "%APPDATA%/npm",
            "%LOCALAPPDATA%/Microsoft/WindowsApps",
            "%LOCALAPPDATA%/Programs/Python/Scripts",
            "%LOCALAPPDATA%/Google/Cloud SDK/google-cloud-sdk/bin",
        ] {
            if let Some(path) = paths.expand_pattern(pattern) {
                directories.push(path);
            }
        }
        let program_files = if paths.is_isolated() {
            paths
                .expand_pattern("%HOME%")
                .map(|home| home.join("Program Files"))
        } else {
            std::env::var_os("ProgramFiles")
                .map(PathBuf::from)
                .or_else(|| Some(PathBuf::from("C:/Program Files")))
        };
        if let Some(root) = program_files {
            for relative in [
                "nodejs",
                "GitHub CLI",
                "Git/cmd",
                "Docker/Docker/resources/bin",
                "Amazon/AWSCLIV2",
                "Microsoft SDKs/Azure/CLI2/wbin",
            ] {
                directories.push(root.join(relative));
            }
        }
        let system = if paths.is_isolated() {
            paths
                .expand_pattern("%HOME%")
                .map(|home| home.join("Windows/System32"))
        } else {
            std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .map(|root| root.join("System32"))
        };
        if let Some(path) = system {
            directories.push(path);
        }
    }
    directories.retain(|directory| directory.is_absolute());
    (directories, complete)
}

fn executable_variants(path: &Path, _paths: &ScanPaths) -> (Vec<PathBuf>, bool) {
    #[cfg(not(windows))]
    {
        (vec![path.to_path_buf()], true)
    }
    #[cfg(windows)]
    {
        let value = if _paths.is_isolated() {
            None
        } else {
            std::env::var("PATHEXT").ok()
        };
        let extensions = value
            .as_deref()
            .unwrap_or(".COM;.EXE;.BAT;.CMD")
            .split(';')
            .filter(|extension| !extension.is_empty())
            .collect::<Vec<_>>();
        let mut complete = extensions.len() <= 16;
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for extension in extensions.into_iter().take(16) {
            if !extension.starts_with('.')
                || extension.len() > 16
                || !extension[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric())
            {
                complete = false;
                continue;
            }
            if path.extension().is_some() {
                if path.extension().is_some_and(|existing| {
                    existing
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&extension[1..])
                }) {
                    result.push(path.to_path_buf());
                    break;
                }
            } else {
                let mut file = path.as_os_str().to_os_string();
                file.push(extension);
                let candidate = PathBuf::from(file);
                if seen.insert(candidate.clone()) {
                    result.push(candidate);
                }
            }
        }
        (result, complete)
    }
}

enum Probe {
    Found,
    Absent,
    Unknown,
}

fn probe(path: &Path, paths: &ScanPaths) -> Probe {
    if !isolated_links_safe(path, paths) || !paths.allows(path) {
        return Probe::Unknown;
    }
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    return Probe::Unknown;
                }
            }
            Probe::Found
        }
        Ok(_) => Probe::Unknown,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Probe::Absent,
        Err(_) => Probe::Unknown,
    }
}

// Inspect each link before following it, including dangling directory links.
// ScanPaths' nearest-existing-ancestor fence alone cannot reject a dangling link.
fn isolated_links_safe(path: &Path, paths: &ScanPaths) -> bool {
    if !paths.is_isolated() {
        return true;
    }
    let Some(home) = paths.expand_pattern("%HOME%") else {
        return false;
    };
    let Ok(canonical_home) = home.canonicalize() else {
        return false;
    };
    scoped_links_safe(path, &home, &canonical_home, 0)
}

fn scoped_links_safe(path: &Path, home: &Path, canonical_home: &Path, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return false;
                }
            }
            part => normalized.push(part.as_os_str()),
        }
    }
    let Ok(relative) = normalized
        .strip_prefix(home)
        .or_else(|_| normalized.strip_prefix(canonical_home))
    else {
        return false;
    };
    let mut prefix = home.to_path_buf();
    for component in relative.components() {
        prefix.push(component.as_os_str());
        match fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let Ok(target) = fs::read_link(&prefix) else {
                    return false;
                };
                let target = if target.is_absolute() {
                    target
                } else {
                    prefix.parent().unwrap_or(home).join(target)
                };
                if !scoped_links_safe(&target, home, canonical_home, depth + 1) {
                    return false;
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) => return false,
        }
    }
    true
}

fn display_path(path: &Path) -> Option<String> {
    let raw = path.to_str()?;
    if raw.len() > MAX_COMMAND_BYTES {
        return None;
    }
    let redacted = redact_command(raw);
    // A masked path must not later be treated as an exact absence baseline.
    (redacted == raw && !raw.chars().any(char::is_control)).then_some(redacted)
}

fn has_glob(command: &str) -> bool {
    command.bytes().any(|byte| b"*?[]{}".contains(&byte))
}

fn found_reason(command: &str, previous: bool) -> &'static str {
    let lower = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .to_ascii_lowercase();
    let name = lower.as_str();
    let name = name
        .strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".cmd"))
        .or_else(|| name.strip_suffix(".bat"))
        .unwrap_or(name);
    if [
        "npx", "npm", "pnpm", "yarn", "bun", "bunx", "uv", "uvx", "docker", "podman", "node",
        "python", "python3", "deno",
    ]
    .contains(&name)
    {
        "The launcher executable is present. Packages, containers, MCP servers, and runtime health were not checked or executed."
    } else if previous {
        "The previously observed executable is still present. Current shell resolution and runtime health were not checked."
    } else {
        "An executable is present in a checked location. It was not run or checked for runtime health. Account configuration is checked separately."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, ScanPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = ScanPaths::isolated(dir.path());
        (dir, paths)
    }
    fn executable(home: &Path, name: &str) -> PathBuf {
        let name = if cfg!(windows) {
            format!("{name}.EXE")
        } else {
            name.to_string()
        };
        let path = home.join(".local").join("bin").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "fixture executable; must never run").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        path
    }

    #[test]
    fn fresh_negative_is_not_found_and_fixture_never_sees_system_tools() {
        let (_dir, paths) = fixture();
        let result = check("sh", None, &paths, "fixture-time");
        assert_eq!(result.status, "not_found");
        assert_eq!(result.checked_at, "fixture-time");
        assert!(result.path.is_none());
    }

    #[test]
    fn deletion_stays_missing_across_repeated_checks_and_reinstall_returns_found() {
        let (dir, paths) = fixture();
        let path = executable(dir.path(), "fixture-cli");
        let first = check("fixture-cli", None, &paths, "1");
        assert_eq!(first.status, "found");
        fs::remove_file(&path).unwrap();
        let second = check("fixture-cli", Some(&first), &paths, "2");
        assert_eq!(second.status, "missing");
        assert_eq!(
            check("fixture-cli", Some(&second), &paths, "3").status,
            "missing"
        );
        executable(dir.path(), "fixture-cli");
        assert_eq!(
            check("fixture-cli", Some(&second), &paths, "4").status,
            "found"
        );
    }

    #[test]
    fn replacement_in_another_checked_location_prevents_missing() {
        let (dir, paths) = fixture();
        let original = executable(dir.path(), "fixture-cli");
        let first = check("fixture-cli", None, &paths, "1");
        let replacement = dir
            .path()
            .join(".cargo")
            .join("bin")
            .join(original.file_name().unwrap());
        fs::create_dir_all(replacement.parent().unwrap()).unwrap();
        fs::rename(original, &replacement).unwrap();
        let result = check("fixture-cli", Some(&first), &paths, "2");
        assert_eq!(result.status, "found");
        assert_eq!(result.path.as_deref(), replacement.to_str());
    }

    #[test]
    fn explicit_paths_are_checked_without_shell_interpretation() {
        let (dir, paths) = fixture();
        let path = executable(dir.path(), "with spaces");
        assert_eq!(
            check(path.to_str().unwrap(), None, &paths, "1").status,
            "found"
        );
        for command in [
            "./fixture-cli",
            "sh -c fixture-cli",
            "$(fixture-cli)",
            "gh\nversion",
            "foo*",
            "${UNRESOLVED}",
        ] {
            assert_eq!(check(command, None, &paths, "1").status, "unknown");
        }
    }

    #[test]
    fn ordinary_macos_temporary_paths_remain_exact_removal_baselines() {
        let path =
            "/var/folders/bl/vzjcvbz95c3dk19cb0fw46180000gn/T/.tmp0GH4ri/.cargo/bin/fixture-cli";
        assert_ne!(
            redact(path),
            path,
            "The general redactor reproduces the original failure"
        );
        assert_eq!(redact_command(path), path);
        assert_eq!(display_path(Path::new(path)).as_deref(), Some(path));
        let windows = r"C:\Users\fixture\AppData\Local\Programs\Fixture\bin\fixture-cli.EXE";
        assert_eq!(redact_command(windows), windows);
        assert_eq!(display_path(Path::new(windows)).as_deref(), Some(windows));
    }

    #[test]
    fn secret_components_stay_redacted_and_cannot_be_removal_baselines() {
        for secret in ["ghp_abcdefghijklmnop".to_string(), "A".repeat(48)] {
            for separator in ['/', '\\'] {
                let path =
                    format!("{separator}fixture{separator}{secret}{separator}bin{separator}cli");
                let safe = redact_command(&path);
                assert!(safe.contains("[redacted]"));
                assert!(!safe.contains(&secret));
                assert!(display_path(Path::new(&path)).is_none());
            }
        }
    }

    #[test]
    fn absolute_command_names_preserve_the_same_safe_path_across_removal() {
        let (dir, paths) = fixture();
        let nested = dir
            .path()
            .join("var")
            .join("folders")
            .join("bl")
            .join("vzjcvbz95c3dk19cb0fw46180000gn")
            .join("T")
            .join("fixture");
        let executable = executable(&nested, "fixture-cli");
        let command = executable.to_str().unwrap();
        let first = check(command, None, &paths, "1");
        assert_eq!(first.status, "found");
        assert_eq!(first.name, command);
        assert_eq!(first.path.as_deref(), Some(command));
        fs::remove_file(&executable).unwrap();
        assert_eq!(check(command, Some(&first), &paths, "2").status, "missing");
    }

    #[test]
    fn launcher_found_does_not_claim_a_server_package_is_installed() {
        let (dir, paths) = fixture();
        executable(dir.path(), "npx");
        let result = check("npx", None, &paths, "1");
        assert_eq!(result.status, "found");
        assert!(result.reason.contains("launcher"));
        assert!(result.reason.contains("not checked or executed"));
    }

    #[test]
    fn missing_previous_path_cannot_establish_removal() {
        let (_dir, paths) = fixture();
        let previous = ToolPresence {
            status: "found".into(),
            name: "fixture-cli".into(),
            path: None,
            checked_at: "0".into(),
            reason: String::new(),
            reason_code: String::new(),
        };
        assert_eq!(
            check("fixture-cli", Some(&previous), &paths, "1").status,
            "unknown"
        );
    }

    #[test]
    fn oversized_or_sensitive_commands_are_not_echoed() {
        let (_dir, paths) = fixture();
        let large = "x".repeat(MAX_COMMAND_BYTES + 1);
        assert_eq!(check(&large, None, &paths, "1").name, "Invalid command");
        let result = check("ghp_abcdefghijklmnop", None, &paths, "1");
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("ghp_abcdefghijklmnop"));
    }

    #[test]
    fn all_watch_candidates_are_inside_the_fixture() {
        let (dir, paths) = fixture();
        let candidates = watch_candidates("fixture-cli", &paths);
        assert!(!candidates.is_empty());
        assert!(candidates.iter().all(|path| path.starts_with(dir.path())));
        assert!(candidates.iter().all(|path| paths.allows(path)));
    }

    #[cfg(unix)]
    #[test]
    fn existing_non_executable_file_is_unknown() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, paths) = fixture();
        let path = executable(dir.path(), "fixture-cli");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(check("fixture-cli", None, &paths, "1").status, "unknown");
    }

    #[cfg(unix)]
    #[test]
    fn observed_path_survives_a_temporary_unknown_check() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, paths) = fixture();
        let path = executable(dir.path(), "fixture-cli");
        let first = check("fixture-cli", None, &paths, "1");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let uncertain = check("fixture-cli", Some(&first), &paths, "2");
        assert_eq!(uncertain.status, "unknown");
        assert_eq!(uncertain.path, first.path);
        assert_eq!(uncertain.reason_code, "executable_unchecked");
        fs::remove_file(path).unwrap();
        assert_eq!(
            check("fixture-cli", Some(&uncertain), &paths, "3").status,
            "missing"
        );
    }

    #[test]
    fn an_unproven_unknown_path_does_not_establish_a_removal_baseline() {
        let (dir, paths) = fixture();
        let previous = ToolPresence {
            status: "unknown".into(),
            name: "fixture-cli".into(),
            path: Some(dir.path().join("never-observed").to_str().unwrap().into()),
            checked_at: "0".into(),
            reason: String::new(),
            reason_code: "executable_search_incomplete".into(),
        };
        assert_eq!(
            check("fixture-cli", Some(&previous), &paths, "1").status,
            "not_found"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_link_is_missing_but_external_fixture_link_is_unknown() {
        use std::os::unix::fs::symlink;
        let (dir, paths) = fixture();
        let target = executable(dir.path(), "actual-cli");
        let link = dir.path().join(".local/bin/fixture-cli");
        symlink(&target, &link).unwrap();
        let first = check("fixture-cli", None, &paths, "1");
        assert_eq!(first.status, "found");
        assert!(watch_candidates("fixture-cli", &paths).contains(&target));
        fs::remove_file(target).unwrap();
        assert_eq!(
            check("fixture-cli", Some(&first), &paths, "2").status,
            "missing"
        );
        fs::remove_file(&link).unwrap();
        let (other, _) = fixture();
        let outside = executable(other.path(), "outside-cli");
        symlink(outside, &link).unwrap();
        assert_eq!(
            check("fixture-cli", Some(&first), &paths, "3").status,
            "unknown"
        );
        assert!(!watch_candidates("fixture-cli", &paths).contains(&link));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_external_file_and_directory_links_are_never_followed() {
        use std::os::unix::fs::symlink;
        let (dir, paths) = fixture();
        let (outside, _) = fixture();
        let link = dir.path().join(".local/bin/fixture-cli");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(outside.path().join("absent-cli"), &link).unwrap();
        assert_eq!(check("fixture-cli", None, &paths, "1").status, "unknown");
        assert!(!watch_candidates("fixture-cli", &paths).contains(&link));
        fs::remove_file(&link).unwrap();
        fs::remove_dir(link.parent().unwrap()).unwrap();
        symlink(outside.path().join("absent-bin"), link.parent().unwrap()).unwrap();
        assert_eq!(check("fixture-cli", None, &paths, "2").status, "unknown");
        assert!(!watch_candidates("fixture-cli", &paths).contains(&link));
    }
}
