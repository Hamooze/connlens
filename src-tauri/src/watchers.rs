use crate::models::{ConnectionStatus, Settings, Snapshot, WatcherHealth};
use crate::{commands, registry, scan};
use glob::{MatchOptions, Pattern};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

// CLI scans and startup snapshots must not claim a watcher is running before
// the native backend has actually registered its directories.
static NATIVE_HEALTHY: AtomicBool = AtomicBool::new(false);

pub fn health(enabled: bool) -> WatcherHealth {
    if !enabled {
        WatcherHealth::Paused
    } else if NATIVE_HEALTHY.load(Ordering::Relaxed) {
        WatcherHealth::Ok
    } else {
        WatcherHealth::Degraded
    }
}

pub fn start(app: AppHandle) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("connlens-file-watcher".into())
        .spawn(move || run(app))?;
    Ok(())
}

fn new_watcher(sender: &Sender<notify::Result<Event>>) -> Option<RecommendedWatcher> {
    notify::recommended_watcher(sender.clone()).ok()
}

fn configure_roots(
    watcher: &mut Option<RecommendedWatcher>,
    watched: &mut BTreeSet<PathBuf>,
    plan: &scan::WatchPlan,
    settings: &Settings,
) -> bool {
    let Some(watcher) = watcher else { return false };
    let desired = if settings.watchers_enabled {
        plan.roots
            .iter()
            .map(|path| normalize_event_path(path))
            .collect::<BTreeSet<_>>()
    } else {
        // Observe settings changes even while connection watching is paused.
        BTreeSet::from([normalize_event_path(&registry::app_home())])
    };
    for stale in watched.difference(&desired).cloned().collect::<Vec<_>>() {
        let _ = watcher.unwatch(&stale);
        watched.remove(&stale);
    }
    let mut healthy = !desired.is_empty();
    for path in desired.difference(watched).cloned().collect::<Vec<_>>() {
        // Never recursively observe a user's home or entire project tree.
        if watcher.watch(&path, RecursiveMode::NonRecursive).is_ok() {
            watched.insert(path);
        } else {
            healthy = false;
        }
    }
    healthy
}

#[derive(Default)]
struct PendingChanges {
    config: bool,
    registry: bool,
    refresh_plan: bool,
    failed: bool,
    replaced_paths: BTreeSet<PathBuf>,
}

impl PendingChanges {
    fn is_empty(&self) -> bool {
        !self.config && !self.registry && !self.failed
    }

    fn add(&mut self, result: notify::Result<Event>, interests: &PathInterests) {
        match result {
            Err(_) => self.failed = true,
            Ok(event) => {
                if event.kind.is_access() {
                    return;
                }
                if event.need_rescan() || event.paths.is_empty() {
                    self.registry = true;
                    self.config |= interests.enabled;
                    self.refresh_plan |= interests.enabled;
                }
                let may_replace_directory = event.kind.is_remove()
                    || matches!(
                        event.kind,
                        notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
                    );
                for path in event.paths {
                    let kind = interests.classify(&path, is_topology_event(&event.kind));
                    if may_replace_directory && kind != EventPathKind::Unrelated {
                        self.replaced_paths.insert(normalize_event_path(&path));
                    }
                    match kind {
                        EventPathKind::Config => self.config = true,
                        EventPathKind::Descriptor | EventPathKind::Topology => {
                            self.config = true;
                            self.refresh_plan = true;
                        }
                        EventPathKind::Registry => self.registry = true,
                        EventPathKind::Unrelated => {}
                    }
                }
            }
        }
    }
}

fn normalize_event_path(path: &Path) -> PathBuf {
    // Resolve aliases once when building the plan, including missing files and
    // glob patterns under /var. Event matching itself performs no filesystem IO.
    for ancestor in path.ancestors() {
        if contains_glob(ancestor) {
            continue;
        }
        if let Ok(resolved) = ancestor.canonicalize() {
            if let Ok(suffix) = path.strip_prefix(ancestor) {
                return resolved.join(suffix);
            }
        }
    }
    path.to_path_buf()
}

#[derive(Debug, PartialEq, Eq)]
enum EventPathKind {
    Config,
    Descriptor,
    Topology,
    Registry,
    Unrelated,
}

fn contains_glob(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().contains(['*', '?', '['])
}

fn is_topology_event(kind: &notify::EventKind) -> bool {
    matches!(
        kind,
        notify::EventKind::Any
            | notify::EventKind::Other
            | notify::EventKind::Create(
                notify::event::CreateKind::Folder
                    | notify::event::CreateKind::Any
                    | notify::event::CreateKind::Other
            )
            | notify::EventKind::Remove(
                notify::event::RemoveKind::Folder
                    | notify::event::RemoveKind::Any
                    | notify::event::RemoveKind::Other
            )
            | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
    )
}

struct PathInterests {
    enabled: bool,
    registry_paths: BTreeSet<PathBuf>,
    descriptor_files: Vec<Pattern>,
    config_files: Vec<Pattern>,
    ancestors: Vec<Pattern>,
}

impl PathInterests {
    fn new(plan: &scan::WatchPlan, home: &Path, enabled: bool) -> Self {
        let homes = BTreeSet::from([home.to_path_buf(), normalize_event_path(home)]);
        let registry_paths = homes
            .iter()
            .map(|home| home.join("registry.json"))
            .collect();
        let descriptor_files = homes
            .iter()
            .filter_map(|home| Pattern::new(&home.join("providers/*.toml").to_string_lossy()).ok())
            .collect();
        let mut full = BTreeSet::new();
        let mut parents = BTreeSet::new();
        for pattern in &plan.patterns {
            for variant in [pattern.clone(), normalize_event_path(pattern)] {
                parents.extend(variant.ancestors().skip(1).map(Path::to_path_buf));
                full.insert(variant);
            }
        }
        Self {
            enabled,
            registry_paths,
            descriptor_files,
            config_files: full
                .iter()
                .filter_map(|path| Pattern::new(&path.to_string_lossy()).ok())
                .collect(),
            ancestors: parents
                .iter()
                .filter_map(|path| Pattern::new(&path.to_string_lossy()).ok())
                .collect(),
        }
    }

    fn classify(&self, path: &Path, topology: bool) -> EventPathKind {
        if self.registry_paths.contains(path) {
            return EventPathKind::Registry;
        }
        if self
            .registry_paths
            .iter()
            .any(|registry| registry.parent() == path.parent())
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("registry."))
        {
            return EventPathKind::Unrelated;
        }
        if !self.enabled {
            return EventPathKind::Unrelated;
        }
        let options = MatchOptions {
            case_sensitive: !cfg!(any(windows, target_os = "macos")),
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        let matches = |patterns: &[Pattern]| {
            patterns
                .iter()
                .any(|pattern| pattern.matches_path_with(path, options))
        };
        if matches(&self.descriptor_files) {
            EventPathKind::Descriptor
        } else if matches(&self.config_files) {
            if topology {
                EventPathKind::Topology
            } else {
                EventPathKind::Config
            }
        } else if topology && matches(&self.ancestors) {
            EventPathKind::Topology
        } else {
            EventPathKind::Unrelated
        }
    }
}

fn scan_settings_changed(previous: &Settings, next: &Settings) -> bool {
    previous.watchers_enabled != next.watchers_enabled
        || previous.provider_toggles != next.provider_toggles
        || previous.project_roots != next.project_roots
}

fn fallback_interval(settings: &Settings) -> Duration {
    Duration::from_secs(u64::from(settings.poll_minutes.clamp(1, 1440)) * 60)
}

fn notifications_allowed(enabled: bool, isolated: bool) -> bool {
    enabled && !isolated
}

#[derive(Default)]
struct SnapshotPublication {
    last: Option<[u8; 32]>,
}

struct SnapshotHasher(Sha256);

impl Write for SnapshotHasher {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SnapshotPublication {
    fn remember(&mut self, snapshot: &Snapshot) -> bool {
        // Retain only a digest, not another full copy of every connection.
        let mut hasher = SnapshotHasher(Sha256::new());
        let next = serde_json::to_writer(&mut hasher, snapshot)
            .ok()
            .map(|_| hasher.0.finalize().into());
        let changed = next.is_none() || next != self.last;
        self.last = next;
        changed
    }

    fn publish(&mut self, app: &AppHandle, snapshot: &Snapshot) {
        if self.remember(snapshot) {
            crate::emit_visible_snapshot(app, snapshot);
        }
    }
}

fn connections_changed(previous: &Snapshot, next: &Snapshot) -> bool {
    if previous.connections.len() != next.connections.len() {
        return true;
    }
    let by_id = previous
        .connections
        .iter()
        .map(|connection| (&connection.connection.id, &connection.connection))
        .collect::<HashMap<_, _>>();
    next.connections.iter().any(|next| {
        by_id.get(&next.connection.id).is_none_or(|old| {
            old.fingerprint != next.connection.fingerprint
                || old.identity != next.connection.identity
                || (old.status == ConnectionStatus::Missing)
                    != (next.connection.status == ConnectionStatus::Missing)
        })
    })
}

fn run(app: AppHandle) {
    let isolated = crate::descriptors::ScanPaths::current().is_isolated();
    let home = registry::app_home();
    let (sender, receiver) = mpsc::channel();
    let mut watcher = new_watcher(&sender);
    let mut watched = BTreeSet::new();
    let initial = registry::load_snapshot().ok();
    let mut settings = initial
        .as_ref()
        .map(|snapshot| snapshot.settings.clone())
        .unwrap_or_default();
    let mut plan = scan::watch_plan_with_settings(&settings);
    let mut interests = PathInterests::new(&plan, &home, settings.watchers_enabled);
    let mut healthy = configure_roots(&mut watcher, &mut watched, &plan, &settings);
    NATIVE_HEALTHY.store(healthy, Ordering::Relaxed);
    let mut publication = SnapshotPublication::default();
    if let Some(mut snapshot) = initial {
        snapshot.watcher_health = health(settings.watchers_enabled);
        crate::sync_tray_settings(&app, &settings);
        publication.publish(&app, &snapshot);
    }
    let mut last_fallback = Instant::now();

    loop {
        let timeout = if healthy {
            Duration::from_secs(3600)
        } else {
            fallback_interval(&settings).saturating_sub(last_fallback.elapsed())
        };
        let mut pending = PendingChanges::default();
        let fallback = if timeout.is_zero() {
            true
        } else {
            match receiver.recv_timeout(timeout) {
                Ok(event) => {
                    pending.add(event, &interests);
                    // Irrelevant writes must not start a debounce timer, read the
                    // registry, enumerate descriptors, or rescan connection files.
                    if pending.is_empty() {
                        continue;
                    }
                    // Coalesce atomic saves and event bursts, with a fixed ceiling
                    // so continuous writes cannot postpone a rescan indefinitely.
                    let started = Instant::now();
                    while started.elapsed() < Duration::from_secs(1) {
                        match receiver.recv_timeout(Duration::from_millis(180)) {
                            Ok(event) => pending.add(event, &interests),
                            Err(_) => break,
                        }
                    }
                    false
                }
                Err(RecvTimeoutError::Timeout) => !healthy,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };
        if !fallback && pending.is_empty() {
            continue;
        }
        let Ok(mut before) = registry::load_snapshot() else {
            healthy = false;
            NATIVE_HEALTHY.store(false, Ordering::Relaxed);
            last_fallback = Instant::now();
            continue;
        };
        let settings_changed = before.settings != settings;
        let scan_settings_changed = scan_settings_changed(&settings, &before.settings);
        let need_scan = pending.config || fallback || scan_settings_changed;
        settings = before.settings.clone();
        // A directory can be removed and recreated within one debounce batch.
        // Its path still exists, but inode-based backends lost the old watch.
        for path in watched
            .iter()
            .filter(|path| pending.replaced_paths.contains(*path))
            .cloned()
            .collect::<Vec<_>>()
        {
            if let Some(watcher) = &mut watcher {
                let _ = watcher.unwatch(&path);
            }
            watched.remove(&path);
        }
        if pending.refresh_plan || scan_settings_changed || fallback {
            plan = scan::watch_plan_with_settings(&settings);
            interests = PathInterests::new(&plan, &home, settings.watchers_enabled);
        }
        if fallback {
            // Retry native watching before using a timed scan. Polling runs
            // only while a native watcher or one of its registrations failed.
            watcher = new_watcher(&sender);
            watched.clear();
            last_fallback = Instant::now();
            healthy = configure_roots(&mut watcher, &mut watched, &plan, &settings);
        } else if pending.refresh_plan || scan_settings_changed {
            healthy = configure_roots(&mut watcher, &mut watched, &plan, &settings) && healthy;
        }
        if pending.failed {
            healthy = false;
            last_fallback = Instant::now();
        }
        NATIVE_HEALTHY.store(healthy, Ordering::Relaxed);
        if settings_changed {
            crate::sync_tray_settings(&app, &settings);
        }
        before.watcher_health = health(settings.watchers_enabled);
        if settings.watchers_enabled && need_scan {
            match commands::rescan_internal(Some(&app), None) {
                Ok(after) => {
                    publication.remember(&after);
                    if notifications_allowed(after.settings.toasts_enabled, isolated)
                        && connections_changed(&before, &after)
                    {
                        let _ = app
                            .notification()
                            .builder()
                            .title("ConnLens")
                            .body("Your local connections have changed. Open ConnLens to review.")
                            .show();
                    }
                }
                Err(_) => {
                    healthy = false;
                    NATIVE_HEALTHY.store(false, Ordering::Relaxed);
                    last_fallback = Instant::now();
                    before.watcher_health = health(settings.watchers_enabled);
                    publication.publish(&app, &before);
                }
            }
        } else {
            publication.publish(&app, &before);
        }
    }
    NATIVE_HEALTHY.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_interests(home: &Path, enabled: bool) -> PathInterests {
        let plan = scan::WatchPlan {
            roots: vec![home.to_path_buf()],
            patterns: vec![
                home.join(".aws/credentials"),
                home.join("profiles/*/auth.json"),
                home.join("providers/*.toml"),
                home.join("inbox/*"),
            ],
        };
        PathInterests::new(&plan, home, enabled)
    }

    fn write_event(path: &Path) -> notify::Result<Event> {
        Ok(
            Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )))
            .add_path(path.to_path_buf()),
        )
    }

    #[test]
    fn registry_saves_do_not_trigger_rescan_loops() {
        let home = Path::new("/fixture/connlens");
        let interests = fixture_interests(home, true);
        assert_eq!(
            interests.classify(&home.join("registry.json"), false),
            EventPathKind::Registry
        );
        assert_eq!(
            interests.classify(&home.join("registry.json.tmp"), false),
            EventPathKind::Unrelated
        );
        assert_eq!(
            interests.classify(&home.join("registry.lock"), false),
            EventPathKind::Unrelated
        );
        assert_eq!(
            interests.classify(&home.join("providers/custom.toml"), false),
            EventPathKind::Descriptor
        );
        assert_eq!(
            interests.classify(&home.join(".aws/credentials"), false),
            EventPathKind::Config
        );
    }

    #[test]
    fn unrelated_ancestor_writes_do_not_schedule_any_work() {
        let fixture = tempfile::tempdir().unwrap();
        let interests = fixture_interests(fixture.path(), true);
        let mut pending = PendingChanges::default();
        for relative in [
            "notes.txt",
            "registry.lock",
            "registry.json.tmp",
            "profiles/work/log.txt",
            ".aws/cache.json",
        ] {
            pending.add(write_event(&fixture.path().join(relative)), &interests);
        }
        assert!(pending.is_empty());
        assert!(!pending.refresh_plan);
        assert!(pending.replaced_paths.is_empty());
    }

    #[test]
    fn target_writes_scan_without_rebuilding_the_watch_plan() {
        let fixture = tempfile::tempdir().unwrap();
        let interests = fixture_interests(fixture.path(), true);
        let mut pending = PendingChanges::default();
        pending.add(
            write_event(&fixture.path().join("profiles/new/auth.json")),
            &interests,
        );
        assert!(pending.config);
        assert!(!pending.refresh_plan);
        assert_eq!(
            interests.classify(&fixture.path().join("profiles/new/nested/auth.json"), false),
            EventPathKind::Unrelated
        );
    }

    #[test]
    fn new_profile_directories_and_descriptor_edits_refresh_the_plan() {
        let fixture = tempfile::tempdir().unwrap();
        let interests = fixture_interests(fixture.path(), true);
        let mut pending = PendingChanges::default();
        pending.add(
            Ok(
                Event::new(notify::EventKind::Create(notify::event::CreateKind::Folder))
                    .add_path(fixture.path().join("profiles/new")),
            ),
            &interests,
        );
        assert!(pending.config && pending.refresh_plan);
        let mut descriptor = PendingChanges::default();
        descriptor.add(
            write_event(&fixture.path().join("providers/custom.toml")),
            &interests,
        );
        assert!(descriptor.config && descriptor.refresh_plan);
        assert_eq!(
            interests.classify(&fixture.path().join("inbox/entry.json"), false),
            EventPathKind::Config
        );
    }

    #[test]
    fn directory_replacement_rearms_an_existing_watch() {
        let fixture = tempfile::tempdir().unwrap();
        let directory = fixture.path().join(".aws");
        std::fs::create_dir(&directory).unwrap();
        let interests = fixture_interests(fixture.path(), true);
        let mut pending = PendingChanges::default();
        pending.add(
            Ok(
                Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Folder))
                    .add_path(directory.clone()),
            ),
            &interests,
        );
        assert!(pending.refresh_plan);
        assert!(pending
            .replaced_paths
            .contains(&normalize_event_path(&directory)));
    }

    #[test]
    fn paused_watchers_only_process_registry_settings_events() {
        let fixture = tempfile::tempdir().unwrap();
        let interests = fixture_interests(fixture.path(), false);
        let mut pending = PendingChanges::default();
        pending.add(
            write_event(&fixture.path().join(".aws/credentials")),
            &interests,
        );
        assert!(pending.is_empty());
        pending.add(
            write_event(&fixture.path().join("registry.json")),
            &interests,
        );
        assert!(pending.registry && !pending.config);
    }

    #[cfg(unix)]
    #[test]
    fn aliased_fixture_paths_match_resolved_backend_events() {
        let fixture = tempfile::tempdir().unwrap();
        let actual = fixture.path().join("actual");
        std::fs::create_dir(&actual).unwrap();
        let alias = fixture.path().join("alias");
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let interests = fixture_interests(&alias, true);
        let resolved = actual.canonicalize().unwrap();
        assert_eq!(
            interests.classify(&resolved.join(".aws/credentials"), false),
            EventPathKind::Config
        );
        assert_eq!(
            interests.classify(&resolved.join("profiles/new"), true),
            EventPathKind::Topology
        );
        assert_eq!(
            interests.classify(&resolved.join("registry.json"), false),
            EventPathKind::Registry
        );
    }

    #[test]
    fn duplicate_snapshot_publications_are_suppressed_without_caching_rows() {
        let mut snapshot = Snapshot {
            schema_version: 1,
            connections: vec![],
            settings: Settings::default(),
            last_scan: None,
            provider_errors: vec![],
            watcher_health: WatcherHealth::Ok,
            history_reset_notice: false,
        };
        let mut publication = SnapshotPublication::default();
        assert!(publication.remember(&snapshot));
        assert!(!publication.remember(&snapshot));
        snapshot.settings.watchers_enabled = false;
        assert!(publication.remember(&snapshot));
        snapshot.watcher_health = WatcherHealth::Paused;
        assert!(publication.remember(&snapshot));
        assert!(!publication.remember(&snapshot));
    }

    #[test]
    fn presentation_settings_do_not_trigger_scans_but_resume_does() {
        let previous = Settings::default();
        let mut next = previous.clone();
        next.theme = "light".into();
        next.autostart = !previous.autostart;
        assert!(!scan_settings_changed(&previous, &next));
        next.watchers_enabled = !previous.watchers_enabled;
        assert!(scan_settings_changed(&previous, &next));
    }

    #[test]
    fn fallback_interval_cannot_busy_loop() {
        let mut settings = Settings::default();
        settings.poll_minutes = 0;
        assert_eq!(fallback_interval(&settings), Duration::from_secs(60));
    }

    #[test]
    fn isolated_runs_never_send_desktop_notifications() {
        assert!(!notifications_allowed(true, true));
        assert!(!notifications_allowed(false, false));
        assert!(notifications_allowed(true, false));
    }

    #[test]
    fn native_watcher_observes_a_fixture_write() {
        let fixture = tempfile::tempdir().unwrap();
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(sender).unwrap();
        watcher
            .watch(fixture.path(), RecursiveMode::NonRecursive)
            .unwrap();
        let path = fixture.path().join("test-provider.json");
        std::fs::write(&path, "{\"profile\":\"test\"}").unwrap();
        let started = Instant::now();
        let mut observed = false;
        while started.elapsed() < Duration::from_secs(5) {
            if let Ok(Ok(event)) = receiver.recv_timeout(Duration::from_millis(250)) {
                if !event.kind.is_access()
                    && event
                        .paths
                        .iter()
                        .any(|event_path| event_path.file_name() == path.file_name())
                {
                    observed = true;
                    break;
                }
            }
        }
        assert!(
            observed,
            "native watcher did not observe the isolated fixture write"
        );
    }
}
