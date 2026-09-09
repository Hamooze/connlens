//! Reviewed, local cleanup. Paths and fingerprints are held only in short-lived
//! native tickets; callers can select IDs, never supply filesystem destinations.
use crate::descriptors::{self, Descriptor, ScanPaths};
use crate::models::{Availability, Connection, ErrorPayload, Snapshot, SourceType, Usage};
use crate::registry::{app_home, now_iso, with_registry_at, Registry};
use crate::scan::{
    self,
    parsers::{self, Format},
    secutil::redact,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant, SystemTime};

const REVIEW_TTL: Duration = Duration::from_secs(300);
const MAX_REVIEWS: usize = 8;
const MAX_FILE_BYTES: u64 = 1_048_576;

#[derive(Default)]
pub struct CleanupState {
    tickets: Mutex<BTreeMap<String, Ticket>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReview {
    pub review_id: String,
    pub checked_at: String,
    pub snapshot: Snapshot,
    pub entries: Vec<CleanupEntry>,
    pub files: Vec<CleanupFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupEntry {
    pub id: String,
    pub label: String,
    pub provider_name: String,
    pub eligible: bool,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupFile {
    pub id: String,
    pub path: String,
    pub eligible: bool,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupRequest {
    pub review_id: String,
    pub connection_ids: Vec<String>,
    pub file_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub snapshot: Snapshot,
    pub removed_ids: Vec<String>,
    pub trashed_paths: Vec<String>,
    pub retained: Vec<CleanupReason>,
    pub file_failures: Vec<CleanupReason>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupReason {
    pub id: String,
    pub reason: String,
}

struct Ticket {
    created: Instant,
    home: PathBuf,
    entries: BTreeMap<String, EntryTicket>,
    files: BTreeMap<String, FileTicket>,
}
struct EntryTicket {
    signature: [u8; 32],
    eligible: bool,
}
struct FileTicket {
    path: PathBuf,
    connection_ids: BTreeSet<String>,
    baseline: Option<FileBaseline>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileBaseline {
    stamp: FileStamp,
    hash: [u8; 32],
    format: Format,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified: SystemTime,
    identity: Vec<u64>,
    readonly: bool,
}

pub fn review(state: &CleanupState) -> Result<CleanupReview, ErrorPayload> {
    review_at(state, &app_home(), &ScanPaths::current())
}

pub fn execute(
    state: &CleanupState,
    request: CleanupRequest,
) -> Result<CleanupResult, ErrorPayload> {
    let paths = ScanPaths::current();
    // Fixture runs must never use the real desktop Trash. Tests inject a local
    // fake below; the shipped fixture boundary fails closed for file actions.
    execute_at(state, &app_home(), &paths, request, |path| {
        if paths.is_isolated() {
            Err("Desktop Trash is disabled in the isolated test environment.".to_string())
        } else {
            platform_trash(path)
        }
    })
}

fn review_at(
    state: &CleanupState,
    home: &Path,
    paths: &ScanPaths,
) -> Result<CleanupReview, ErrorPayload> {
    let review_id = new_review_id();
    let (review, ticket) = with_registry_at(home, |registry| {
        let original_ids: BTreeSet<_> = registry.file.connections.iter().map(|c| c.id.clone()).collect();
        let snapshot = scan::refresh_registry(registry, None, paths);
        let checked_at = now_iso();
        let (descriptors, descriptor_errors) = descriptors::load_all_scoped(home, paths);
        let mut ticket = Ticket {
            created: Instant::now(), home: canonical_home(home),
            entries: BTreeMap::new(), files: BTreeMap::new(),
        };
        let mut entries = Vec::new();
        let mut grouped: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
        // Group every source row, including active and hidden entries. A path
        // can never become eligible by hiding or omitting one of its owners.
        for connection in &registry.file.connections {
            if let Some(path) = connection.source.path.as_deref() {
                grouped.entry(path_key(Path::new(path))).or_default().insert(connection.id.clone());
            }
        }
        let mut files = Vec::new();
        let mut file_ids = BTreeMap::new();
        for (path, ids) in grouped {
            if !ids.iter().any(|id| original_ids.contains(id)
                && registry.file.connections.iter().any(|c| c.id == *id && missing(c))) { continue; }
            let id = digest_id(&[&review_id, &path.to_string_lossy()]);
            let baseline = if descriptor_errors.is_empty() {
                inspect_file(registry, paths, &descriptors, &path, &ids)
            } else { Err("Provider definitions could not all be checked.".to_string()) };
            let (eligible, reason, size_bytes) = match &baseline {
                Ok(value) => (true, "Empty dedicated config file. Select every associated entry to move it to Trash.".to_string(), Some(value.stamp.size)),
                Err(reason) => (false, reason.clone(), None),
            };
            files.push(CleanupFile { id: id.clone(), path: redact(&path.display().to_string()), eligible, reason, size_bytes });
            file_ids.insert(path.clone(), id.clone());
            ticket.files.insert(id, FileTicket { path, connection_ids: ids, baseline: baseline.ok() });
        }
        for connection in &registry.file.connections {
            if !original_ids.contains(&connection.id) { continue; }
            let eligible = missing(connection);
            let file_id = connection.source.path.as_deref().and_then(|path| file_ids.get(&path_key(Path::new(path)))).cloned();
            entries.push(CleanupEntry {
                id: connection.id.clone(), label: redact(&connection.identity.label),
                provider_name: redact(&connection.provider_name), eligible,
                reason: if eligible { "Confirmed missing locally. Its history entry can be removed.".to_string() }
                    else { connection.validation.reason.clone() }, file_id,
            });
            ticket.entries.insert(connection.id.clone(), EntryTicket { signature: entry_signature(connection), eligible });
        }
        (CleanupReview { review_id: review_id.clone(), checked_at, snapshot, entries, files }, ticket)
    }).map_err(ErrorPayload::from)?;
    let mut tickets = state.tickets.lock().map_err(|_| state_error())?;
    tickets.retain(|_, ticket| ticket.created.elapsed() < REVIEW_TTL);
    if tickets.len() >= MAX_REVIEWS {
        if let Some(oldest) = tickets
            .iter()
            .min_by_key(|(_, ticket)| ticket.created)
            .map(|(id, _)| id.clone())
        {
            tickets.remove(&oldest);
        }
    }
    tickets.insert(review_id, ticket);
    Ok(review)
}

fn execute_at(
    state: &CleanupState,
    home: &Path,
    paths: &ScanPaths,
    request: CleanupRequest,
    mut trash: impl FnMut(&Path) -> Result<(), String>,
) -> Result<CleanupResult, ErrorPayload> {
    let ticket = state
        .tickets
        .lock()
        .map_err(|_| state_error())?
        .remove(&request.review_id)
        .ok_or_else(stale_review)?;
    // Consume before waiting for the registry lock: duplicate clicks cannot
    // replay a partial operation, including after an IO error.
    if ticket.created.elapsed() >= REVIEW_TTL || ticket.home != canonical_home(home) {
        return Err(stale_review());
    }
    let selected: BTreeSet<_> = request.connection_ids.into_iter().collect();
    let selected_files: BTreeSet<_> = request.file_ids.into_iter().collect();
    let mut receipt = None;
    let mut history_before_removal = None;
    let result = with_registry_at(home, |registry| {
        scan::refresh_registry(registry, None, paths);
        let (descriptors, errors) = descriptors::load_all_scoped(home, paths);
        let mut retained = BTreeMap::<String, String>::new();
        let mut file_failures = Vec::new();
        let mut trashed_paths = Vec::new();
        for id in &selected {
            let reason = match (
                ticket.entries.get(id),
                registry.file.connections.iter().find(|c| c.id == *id),
            ) {
                (None, _) => Some("This entry was not in the reviewed selection.".to_string()),
                (Some(entry), Some(connection))
                    if entry.eligible
                        && missing(connection)
                        && entry.signature == entry_signature(connection) =>
                {
                    None
                }
                (_, Some(connection)) if !missing(connection) => {
                    Some(connection.validation.reason.clone())
                }
                _ => Some(
                    "This entry changed after review. Review again before removing it.".to_string(),
                ),
            };
            if let Some(reason) = reason {
                retained.insert(id.clone(), reason);
            }
        }
        for id in selected_files {
            let outcome = (|| {
                let file = ticket
                    .files
                    .get(&id)
                    .ok_or("This file was not in the review.")?;
                let baseline = file
                    .baseline
                    .as_ref()
                    .ok_or("This file was not eligible for Trash in the review.")?;
                if !file.connection_ids.is_subset(&selected) {
                    return Err(
                        "Select every associated history entry before moving its file to Trash."
                            .to_string(),
                    );
                }
                if file
                    .connection_ids
                    .iter()
                    .any(|id| retained.contains_key(id))
                {
                    return Err(
                        "An associated entry is no longer eligible for removal.".to_string()
                    );
                }
                if !errors.is_empty() {
                    return Err("Provider definitions could not all be checked.".to_string());
                }
                let current_ids: BTreeSet<_> = registry
                    .file
                    .connections
                    .iter()
                    .filter(|c| {
                        c.source
                            .path
                            .as_deref()
                            .is_some_and(|p| path_key(Path::new(p)) == file.path)
                    })
                    .map(|c| c.id.clone())
                    .collect();
                if current_ids != file.connection_ids {
                    return Err("The file's associated entries changed. Review again.".to_string());
                }
                let fresh = inspect_file(registry, paths, &descriptors, &file.path, &current_ids)?;
                if fresh != *baseline {
                    return Err(
                        "The file changed after review. Review again before moving it.".to_string(),
                    );
                }
                // Native trash APIs take paths. Recheck content and file identity
                // immediately before invoking them; never retry with permanent deletion.
                trash(&file.path)?;
                trashed_paths.push(redact(&file.path.display().to_string()));
                match fs::symlink_metadata(&file.path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => {
                        return Err(
                            "Trash reported a move, but the source path still exists or reappeared. History was kept."
                                .to_string(),
                        )
                    }
                }
                Ok::<(), String>(())
            })();
            if let Err(reason) = outcome {
                if let Some(file) = ticket.files.get(&id) {
                    for connection_id in file.connection_ids.intersection(&selected) {
                        retained.insert(connection_id.clone(), format!("History kept: {reason}"));
                    }
                }
                file_failures.push(CleanupReason { id, reason });
            }
        }
        history_before_removal = Some(snapshot(registry));
        let removed_ids: Vec<_> = selected
            .difference(&retained.keys().cloned().collect())
            .cloned()
            .collect();
        registry
            .file
            .connections
            .retain(|connection| !removed_ids.contains(&connection.id));
        let result = CleanupResult {
            snapshot: snapshot(registry),
            removed_ids,
            trashed_paths,
            retained: retained
                .into_iter()
                .map(|(id, reason)| CleanupReason { id, reason })
                .collect(),
            file_failures,
        };
        receipt = Some(result.clone());
        result
    });
    match result {
        Ok(result) => Ok(result),
        Err(error) => {
            // A file move cannot be rolled back by the history transaction. If
            // saving failed, return the actual file receipt and retain the rows.
            if let Some(mut receipt) = receipt.filter(|receipt| !receipt.trashed_paths.is_empty()) {
                for id in receipt.removed_ids.drain(..) {
                    receipt.retained.push(CleanupReason {
                        id,
                        reason: "The file moved to Trash, but saving history failed. Check again."
                            .to_string(),
                    });
                }
                if let Ok(registry) = Registry::load(home) {
                    receipt.snapshot = snapshot(&registry);
                } else if let Some(before) = history_before_removal {
                    receipt.snapshot = before;
                }
                return Ok(receipt);
            }
            Err(ErrorPayload::from(error))
        }
    }
}

fn missing(connection: &Connection) -> bool {
    connection.validation.availability == Availability::Missing
        && connection.validation.checked_at.is_some()
        && connection.validation.usage != Usage::Selected
        && !connection.identity.is_active_identity
}

fn snapshot(registry: &Registry) -> Snapshot {
    registry.snapshot(
        Vec::new(),
        crate::watchers::health(registry.file.settings.watchers_enabled),
    )
}

fn inspect_file(
    registry: &Registry,
    paths: &ScanPaths,
    descriptors: &[Descriptor],
    path: &Path,
    ids: &BTreeSet<String>,
) -> Result<FileBaseline, String> {
    if ids.is_empty() {
        return Err("The file has no reviewed entries.".to_string());
    }
    let owners: Vec<_> = registry
        .file
        .connections
        .iter()
        .filter(|c| ids.contains(&c.id))
        .collect();
    if owners.len() != ids.len()
        || owners
            .iter()
            .any(|c| !missing(c) || c.source.source_type != SourceType::ConfigFile)
    {
        return Err(
            "The file is referenced by an available, selected, unknown, or non-config entry."
                .to_string(),
        );
    }
    let safe_path = safe_path(path, &registry.home, paths)?;
    for owner in &owners {
        let original = owner
            .source
            .path
            .as_deref()
            .ok_or("An entry no longer has a source file.")?;
        if safe_path != self::safe_path(Path::new(original), &registry.home, paths)? {
            return Err("The source path changed while it was checked.".to_string());
        }
    }
    let mut matching = Vec::new();
    for descriptor in descriptors {
        for location in &descriptor.locations {
            let patterns = if let Some(relative) = location.path.strip_prefix("$PROJECT_ROOTS/") {
                registry
                    .file
                    .settings
                    .project_roots
                    .iter()
                    .map(|root| Path::new(root).join(relative).display().to_string())
                    .collect()
            } else {
                vec![location.path.clone()]
            };
            if patterns.iter().any(|pattern| {
                paths
                    .expand(pattern)
                    .iter()
                    .any(|candidate| path_key(candidate) == safe_path)
            }) {
                matching.push((descriptor, location));
            }
        }
    }
    if matching.is_empty() {
        return Err("The file is no longer an exact provider config source.".to_string());
    }
    let provider = &matching[0].0.id;
    let format = matching[0].1.format;
    if matching.iter().any(|(descriptor, location)| {
        descriptor.id != *provider
            || location.format != format
            || location.source_type != "config_file"
            || !matches!(
                location.strategy.as_str(),
                "token_file" | "vercel_auth" | "neon_auth"
            )
    }) || owners.iter().any(|connection| {
        connection.provider != *provider
            || connection.source.descriptor_id.as_deref() != Some(provider)
    }) {
        return Err("Shared, project, or multi-account configuration files are kept.".to_string());
    }
    let (before, hash) = read_baseline(&safe_path, format)?;
    let (after, second_hash) = read_baseline(&safe_path, format)?;
    if before != after || hash != second_hash {
        return Err("The file changed while it was checked. Review again.".to_string());
    }
    Ok(FileBaseline {
        stamp: after,
        hash,
        format,
    })
}

fn structurally_empty(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(values) => values.values().all(structurally_empty),
        serde_json::Value::Array(values) => values.is_empty(),
        _ => false,
    }
}

fn canonical_home(home: &Path) -> PathBuf {
    home.canonicalize().unwrap_or_else(|_| home.to_path_buf())
}
fn path_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn safe_path(path: &Path, home: &Path, paths: &ScanPaths) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("Only exact absolute config files can be moved to Trash.".to_string());
    }
    // In isolation path_key may have resolved macOS's /var alias. Compare with
    // the canonical fixture root as well as ScanPaths' lexical fence.
    let canonical = path.canonicalize().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => "The source file is already absent.".to_string(),
        _ => "The source path could not be checked.".to_string(),
    })?;
    if paths.is_isolated() && !canonical.starts_with(canonical_home(home)) {
        return Err("The source is outside the isolated test environment.".to_string());
    }
    if !paths.is_isolated() && !paths.allows(path) {
        return Err("The source is outside the allowed locations.".to_string());
    }
    let app = canonical_home(home);
    if canonical.parent() == Some(app.as_path())
        || ["providers", "inbox"]
            .iter()
            .any(|name| canonical.starts_with(app.join(name)))
    {
        return Err(
            "ConnLens registry, definitions, and registration files are protected.".to_string(),
        );
    }
    if !paths.is_isolated() {
        let user_home = directories::UserDirs::new()
            .and_then(|dirs| dirs.home_dir().canonicalize().ok())
            .ok_or("The user config boundary could not be verified.")?;
        if !canonical.starts_with(user_home) {
            return Err("Files outside your user folder are protected.".to_string());
        }
    }
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor)
            .map_err(|_| "The source path could not be checked.".to_string())?;
        if is_link(&meta) {
            // The system's /var and /tmp aliases are outside a user's control;
            // every provider-controlled ancestor and the file itself stay strict.
            #[cfg(target_os = "macos")]
            if (ancestor == Path::new("/var")
                && ancestor.canonicalize().ok().as_deref() == Some(Path::new("/private/var")))
                || (ancestor == Path::new("/tmp")
                    && ancestor.canonicalize().ok().as_deref() == Some(Path::new("/private/tmp")))
            {
                continue;
            }
            return Err("Symbolic links and reparse paths are kept.".to_string());
        }
    }
    Ok(canonical)
}

fn is_link(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0 || meta.file_type().is_symlink()
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn read_baseline(path: &Path, format: Format) -> Result<(FileStamp, [u8; 32]), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "The file could not be checked safely.".to_string())?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err("Only regular config files are eligible.".to_string());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "The file could not be opened safely.".to_string())?;
    let before = file_stamp(&file)?;
    if before.size > MAX_FILE_BYTES {
        return Err("The file exceeds the safe config size limit.".to_string());
    }
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 8192];
    let mut contents = Vec::with_capacity(before.size as usize);
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "The file could not be read.".to_string())?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_FILE_BYTES {
            return Err("The file changed while being checked.".to_string());
        }
        hasher.update(&buffer[..read]);
        contents.extend_from_slice(&buffer[..read]);
    }
    buffer.fill(0);
    // Parse the exact bytes read from the checked, nonblocking, no-follow handle.
    // Reopening a path here could follow a newly swapped link or block on a FIFO.
    let text = std::str::from_utf8(&contents)
        .map_err(|_| "The config is not valid text; the file is kept.".to_string())?;
    let doc = parsers::parse_text(text, format)
        .map_err(|_| "The config could not be safely parsed; the file is kept.".to_string())?;
    if !structurally_empty(&doc.value) {
        return Err(
            "The file still contains configuration or credentials and will be kept.".to_string(),
        );
    }
    contents.fill(0);
    let after = file_stamp(&file)?;
    if before != after || total != before.size {
        return Err("The file changed while being checked.".to_string());
    }
    Ok((after, hasher.finalize().into()))
}

fn file_stamp(file: &File) -> Result<FileStamp, String> {
    let metadata = file
        .metadata()
        .map_err(|_| "File metadata could not be read.".to_string())?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err("Only regular config files are eligible.".to_string());
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 || metadata.uid() != unsafe { libc::geteuid() } {
            return Err("Hard-linked files and files owned by another user are kept.".to_string());
        }
        vec![
            metadata.dev(),
            metadata.ino(),
            metadata.nlink(),
            metadata.mode() as u64,
            metadata.ctime() as u64,
            metadata.ctime_nsec() as u64,
        ]
    };
    #[cfg(windows)]
    let identity = {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| "File identity could not be verified.".to_string())?;
        if info.nNumberOfLinks != 1 {
            return Err("Hard-linked files are kept.".to_string());
        }
        vec![
            info.dwVolumeSerialNumber as u64,
            info.nFileIndexHigh as u64,
            info.nFileIndexLow as u64,
            info.nNumberOfLinks as u64,
            info.dwFileAttributes as u64,
            info.ftCreationTime.dwHighDateTime as u64,
            info.ftCreationTime.dwLowDateTime as u64,
        ]
    };
    #[cfg(not(any(unix, windows)))]
    return Err("File identity checks are unavailable on this platform.".to_string());
    #[cfg(any(unix, windows))]
    Ok(FileStamp {
        size: metadata.len(),
        modified: metadata
            .modified()
            .map_err(|_| "File timestamp could not be checked.")?,
        identity,
        readonly: metadata.permissions().readonly(),
    })
}

fn entry_signature(connection: &Connection) -> [u8; 32] {
    // Exclude scan timestamps and validation; every execution validates anew.
    let value = serde_json::to_vec(&(
        &connection.id,
        &connection.provider,
        &connection.identity,
        &connection.source,
        &connection.fingerprint,
        &connection.first_seen,
    ))
    .unwrap_or_default();
    Sha256::digest(value).into()
}

fn new_review_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    digest_id(&[
        &now_iso(),
        &std::process::id().to_string(),
        &NEXT.fetch_add(1, Ordering::Relaxed).to_string(),
    ])
}
fn digest_id(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}
fn stale_review() -> ErrorPayload {
    ErrorPayload::new(
        "cleanup_review_expired",
        "This cleanup review expired or was already used. Review again.",
    )
}
fn state_error() -> ErrorPayload {
    ErrorPayload::new(
        "cleanup_unavailable",
        "Cleanup is temporarily unavailable. Open a new review.",
    )
}

#[cfg(target_os = "macos")]
fn platform_trash(path: &Path) -> Result<(), String> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut context = trash::TrashContext::new();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context
        .delete(path)
        .map_err(|_| "macOS could not move the file to Trash. Its history was kept.".to_string())
}

#[cfg(target_os = "linux")]
fn platform_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|_| {
        "The desktop Trash is unavailable for this file. Its history was kept.".to_string()
    })
}

#[cfg(windows)]
fn platform_trash(path: &Path) -> Result<(), String> {
    windows_trash::recycle(path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform_trash(_: &Path) -> Result<(), String> {
    Err("Trash is unavailable on this platform.".to_string())
}

#[cfg(windows)]
mod windows_trash {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};
    use windows::{
        core::{implement, Ref, HRESULT, HSTRING, PCWSTR},
        Win32::{
            Foundation::E_ABORT,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                FileOperation, IFileOperation, IFileOperationProgressSink,
                IFileOperationProgressSink_Impl, IShellItem, SHCreateItemFromParsingName,
                FOFX_ADDUNDORECORD, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FOF_NORECURSION,
                FOF_NO_CONNECTED_ELEMENTS, FOF_NO_UI, TSF_DELETE_RECYCLE_IF_POSSIBLE,
            },
        },
    };

    pub(super) fn recycle(path: &Path) -> Result<(), String> {
        let path = path.to_owned();
        // Tauri may already own a COM apartment. A dedicated STA avoids changing
        // its threading model and guarantees balanced COM initialization.
        std::thread::spawn(move || unsafe { recycle_sta(&path) })
            .join()
            .map_err(|_| {
                "Windows Trash operation did not complete. History was kept.".to_string()
            })?
    }

    unsafe fn recycle_sta(path: &Path) -> Result<(), String> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|_| "Windows Recycle Bin is unavailable.".to_string())?;
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }
        let _guard = ComGuard;
        let completed = Arc::new(AtomicBool::new(false));
        let sink: IFileOperationProgressSink = RecycleSink {
            completed: completed.clone(),
        }
        .into();
        let result = (|| -> windows::core::Result<()> {
            let operation: IFileOperation =
                unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER) }?;
            // RECYCLEONDELETE is the Windows 8+ recycle-only contract. ALLOWUNDO
            // alone can permanently delete on unsupported volumes, so is not used.
            unsafe {
                operation.SetOperationFlags(
                    FOFX_RECYCLEONDELETE
                        | FOFX_EARLYFAILURE
                        | FOFX_ADDUNDORECORD
                        | FOF_NO_UI
                        | FOF_NORECURSION
                        | FOF_NO_CONNECTED_ELEMENTS,
                )
            }?;
            let item: IShellItem =
                unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None) }?;
            unsafe { operation.DeleteItem(&item, &sink) }?;
            unsafe { operation.PerformOperations() }?;
            if unsafe { operation.GetAnyOperationsAborted() }?.as_bool()
                || !completed.load(Ordering::SeqCst)
            {
                return Err(E_ABORT.into());
            }
            Ok(())
        })();
        result.map_err(|_| {
            "Windows could not confirm moving the file to Recycle Bin. History was kept."
                .to_string()
        })
    }

    #[implement(IFileOperationProgressSink)]
    struct RecycleSink {
        completed: Arc<AtomicBool>,
    }

    #[allow(non_snake_case)]
    impl IFileOperationProgressSink_Impl for RecycleSink_Impl {
        fn StartOperations(&self) -> windows::core::Result<()> {
            Ok(())
        }
        fn FinishOperations(&self, result: HRESULT) -> windows::core::Result<()> {
            result.ok()
        }
        fn PreDeleteItem(&self, flags: u32, _: Ref<'_, IShellItem>) -> windows::core::Result<()> {
            if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
                return Err(E_ABORT.into());
            }
            Ok(())
        }
        fn PostDeleteItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            result: HRESULT,
            created: Ref<'_, IShellItem>,
        ) -> windows::core::Result<()> {
            result.ok()?;
            // A recycle operation creates a shell item in the Recycle Bin. Never
            // report a permanent deletion, skipped item, or ambiguous completion.
            if created.as_ref().is_none() {
                return Err(E_ABORT.into());
            }
            self.completed.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn PreRenameItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn PostRenameItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<'_, IShellItem>,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn PreMoveItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn PostMoveItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<'_, IShellItem>,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn PreCopyItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn PostCopyItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
            _: HRESULT,
            _: Ref<'_, IShellItem>,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn PreNewItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn PostNewItem(
            &self,
            _: u32,
            _: Ref<'_, IShellItem>,
            _: &PCWSTR,
            _: &PCWSTR,
            _: u32,
            _: HRESULT,
            _: Ref<'_, IShellItem>,
        ) -> windows::core::Result<()> {
            Err(E_ABORT.into())
        }
        fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
            Ok(())
        }
        fn ResetTimer(&self) -> windows::core::Result<()> {
            Ok(())
        }
        fn PauseTimer(&self) -> windows::core::Result<()> {
            Ok(())
        }
        fn ResumeTimer(&self) -> windows::core::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fixture {
        dir: tempfile::TempDir,
        paths: ScanPaths,
        state: CleanupState,
        source: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = ScanPaths::isolated(dir.path());
            fs::create_dir_all(dir.path().join("providers")).unwrap();
            fs::create_dir_all(dir.path().join("config")).unwrap();
            fs::write(
                dir.path().join("providers/fixture.toml"),
                r#"
id = "fixture"
name = "Fixture"
[[locations]]
path = "%HOME%/config/auth.json"
format = "json"
strategy = "token_file"
source_type = "config_file"
"#,
            )
            .unwrap();
            let source = dir.path().join("config/auth.json");
            let fixture = Self {
                dir,
                paths,
                state: CleanupState::default(),
                source,
            };
            fixture.detect("old@example.test");
            fs::write(&fixture.source, "{}").unwrap();
            fixture
        }
        fn detect(&self, label: &str) {
            fs::write(
                &self.source,
                serde_json::to_vec(&serde_json::json!({"token":"fixture-token", "email":label}))
                    .unwrap(),
            )
            .unwrap();
            with_registry_at(self.dir.path(), |registry| {
                scan::refresh_registry(registry, None, &self.paths);
            })
            .unwrap();
        }
        fn review(&self) -> CleanupReview {
            review_at(&self.state, self.dir.path(), &self.paths).unwrap()
        }
        fn request(&self, review: &CleanupReview, files: bool) -> CleanupRequest {
            CleanupRequest {
                review_id: review.review_id.clone(),
                connection_ids: review
                    .entries
                    .iter()
                    .filter(|entry| entry.eligible)
                    .map(|entry| entry.id.clone())
                    .collect(),
                file_ids: if files {
                    review
                        .files
                        .iter()
                        .filter(|file| file.eligible)
                        .map(|file| file.id.clone())
                        .collect()
                } else {
                    Vec::new()
                },
            }
        }
        fn execute(
            &self,
            request: CleanupRequest,
            trash: impl FnMut(&Path) -> Result<(), String>,
        ) -> Result<CleanupResult, ErrorPayload> {
            execute_at(&self.state, self.dir.path(), &self.paths, request, trash)
        }
    }

    #[test]
    fn history_only_cleanup_preserves_the_source_file() {
        let fixture = Fixture::new();
        let review = fixture.review();
        assert_eq!(review.entries.iter().filter(|e| e.eligible).count(), 1);
        assert_eq!(review.files.iter().filter(|f| f.eligible).count(), 1);
        let result = fixture
            .execute(fixture.request(&review, false), |_| {
                panic!("No file was selected")
            })
            .unwrap();
        assert_eq!(result.removed_ids.len(), 1);
        assert!(result.snapshot.connections.is_empty());
        assert_eq!(fs::read_to_string(&fixture.source).unwrap(), "{}");
    }

    #[test]
    fn reviewed_empty_file_moves_once_using_only_the_backend_path() {
        let fixture = Fixture::new();
        let review = fixture.review();
        let request = fixture.request(&review, true);
        let duplicate = request.clone();
        let fake_trash = fixture.dir.path().join("test-trash.json");
        let result = fixture
            .execute(request, |path| {
                assert_eq!(path, fixture.source.canonicalize().unwrap());
                fs::rename(path, &fake_trash).map_err(|_| "Fixture move failed".to_string())
            })
            .unwrap();
        assert_eq!(result.removed_ids.len(), 1);
        assert_eq!(result.trashed_paths.len(), 1);
        assert!(result.file_failures.is_empty());
        assert_eq!(fs::read_to_string(fake_trash).unwrap(), "{}");
        assert_eq!(
            fixture
                .execute(duplicate, |_| panic!("Used ticket must not run"))
                .unwrap_err()
                .code,
            "cleanup_review_expired"
        );
    }

    #[test]
    fn trash_failure_preserves_corresponding_history_and_file() {
        let fixture = Fixture::new();
        let review = fixture.review();
        let result = fixture
            .execute(fixture.request(&review, true), |_| {
                Err("Trash unavailable".to_string())
            })
            .unwrap();
        assert_eq!(result.file_failures.len(), 1);
        assert_eq!(result.retained.len(), 1);
        assert!(result.removed_ids.is_empty());
        assert_eq!(result.snapshot.connections.len(), 1);
        assert!(fixture.source.exists());
    }

    #[test]
    fn changed_empty_file_rejects_stale_baseline() {
        let fixture = Fixture::new();
        let review = fixture.review();
        fs::write(&fixture.source, "{\n}").unwrap();
        let result = fixture
            .execute(fixture.request(&review, true), |_| {
                panic!("Changed files must stay")
            })
            .unwrap();
        assert_eq!(result.file_failures.len(), 1);
        assert!(result.file_failures[0]
            .reason
            .contains("changed after review"));
        assert!(result.removed_ids.is_empty());
        assert!(fixture.source.exists());
    }

    #[test]
    fn credentials_restored_after_review_preserve_all_entries() {
        let fixture = Fixture::new();
        let review = fixture.review();
        fixture.detect("new@example.test");
        let result = fixture
            .execute(fixture.request(&review, true), |_| {
                panic!("Available sources must stay")
            })
            .unwrap();
        assert!(result.removed_ids.is_empty());
        assert_eq!(result.file_failures.len(), 1);
        assert_eq!(result.snapshot.connections.len(), 2);
        assert!(fixture.source.exists());
    }

    #[test]
    fn file_requires_selection_of_every_historical_owner() {
        let fixture = Fixture::new();
        fixture.detect("second@example.test");
        fs::write(&fixture.source, "{}").unwrap();
        let review = fixture.review();
        assert_eq!(review.entries.iter().filter(|e| e.eligible).count(), 2);
        let mut request = fixture.request(&review, true);
        request.connection_ids.truncate(1);
        let result = fixture
            .execute(request, |_| panic!("All owners must be selected"))
            .unwrap();
        assert!(result.removed_ids.is_empty());
        assert_eq!(result.retained.len(), 1);
        assert_eq!(result.snapshot.connections.len(), 2);
    }

    #[test]
    fn expired_ticket_and_unknown_ids_cannot_remove_history() {
        let fixture = Fixture::new();
        let review = fixture.review();
        fixture
            .state
            .tickets
            .lock()
            .unwrap()
            .get_mut(&review.review_id)
            .unwrap()
            .created = Instant::now() - REVIEW_TTL;
        assert_eq!(
            fixture
                .execute(fixture.request(&review, true), |_| panic!("Expired"))
                .unwrap_err()
                .code,
            "cleanup_review_expired"
        );
        let review = fixture.review();
        let result = fixture
            .execute(
                CleanupRequest {
                    review_id: review.review_id,
                    connection_ids: vec!["not-reviewed".to_string()],
                    file_ids: vec!["not-a-file".to_string()],
                },
                |_| panic!("Unknown IDs"),
            )
            .unwrap();
        assert_eq!(result.retained.len(), 1);
        assert_eq!(result.file_failures.len(), 1);
        assert!(result.removed_ids.is_empty());
    }

    #[test]
    fn scalar_or_nonempty_configuration_is_never_empty() {
        for value in [
            serde_json::json!(null),
            serde_json::json!(""),
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!({"token":""}),
            serde_json::json!({"enabled":false}),
            serde_json::json!([{}]),
        ] {
            assert!(!structurally_empty(&value), "{value}");
        }
        assert!(structurally_empty(&serde_json::json!({})));
        assert!(structurally_empty(
            &serde_json::json!({"profiles":{}, "accounts":[]})
        ));
    }

    #[test]
    fn unknown_or_active_configuration_is_never_offered_for_file_cleanup() {
        let fixture = Fixture::new();
        fs::write(&fixture.source, r#"{"unrecognized":"keep this setting"}"#).unwrap();
        let review = fixture.review();
        assert!(review.entries.iter().all(|entry| !entry.eligible));
        assert!(review.files.iter().all(|file| !file.eligible));
        fixture.detect("new@example.test");
        let review = fixture.review();
        assert!(review.files.iter().all(|file| !file.eligible));
    }

    #[test]
    fn source_outside_fixture_and_app_internal_paths_are_protected() {
        let fixture = Fixture::new();
        let external = tempfile::tempdir().unwrap();
        let external_file = external.path().join("outside.json");
        fs::write(&external_file, "{}").unwrap();
        assert!(safe_path(&external_file, fixture.dir.path(), &fixture.paths).is_err());
        assert!(safe_path(
            &fixture.dir.path().join("registry.json"),
            fixture.dir.path(),
            &fixture.paths
        )
        .is_err());
        assert!(safe_path(
            &fixture.dir.path().join("providers/fixture.toml"),
            fixture.dir.path(),
            &fixture.paths
        )
        .is_err());
        assert!(read_baseline(&fixture.dir.path().join("config"), Format::Json).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_hardlinks_are_kept_even_when_their_content_is_empty() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let original = fixture.dir.path().join("config/original.json");
        fs::rename(&fixture.source, &original).unwrap();
        symlink(&original, &fixture.source).unwrap();
        let review = fixture.review();
        assert!(review.files.iter().all(|file| !file.eligible));
        fs::remove_file(&fixture.source).unwrap();
        fs::hard_link(&original, &fixture.source).unwrap();
        let review = fixture.review();
        assert!(review.files.iter().all(|file| !file.eligible));
    }

    #[cfg(unix)]
    #[test]
    fn fifo_is_rejected_without_opening_or_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("config.fifo");
        let path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(read_baseline(&fifo, Format::Json)
            .unwrap_err()
            .contains("regular"));
    }

    #[test]
    fn incomplete_trash_success_keeps_history() {
        let fixture = Fixture::new();
        let review = fixture.review();
        let called = Cell::new(false);
        let result = fixture
            .execute(fixture.request(&review, true), |_| {
                called.set(true);
                Ok(())
            })
            .unwrap();
        assert!(called.get());
        assert_eq!(result.file_failures.len(), 1);
        assert!(result.removed_ids.is_empty());
        assert!(fixture.source.exists());
    }

    #[test]
    fn replacement_with_identical_bytes_still_rejects_stale_identity() {
        let fixture = Fixture::new();
        let review = fixture.review();
        let replacement = fixture.dir.path().join("config/replacement.json");
        fs::write(&replacement, "{}").unwrap();
        fs::remove_file(&fixture.source).unwrap();
        fs::rename(replacement, &fixture.source).unwrap();
        let result = fixture
            .execute(fixture.request(&review, true), |_| {
                panic!("Replaced file must stay")
            })
            .unwrap();
        assert!(result.removed_ids.is_empty());
        assert!(result.file_failures[0]
            .reason
            .contains("changed after review"));
    }

    #[test]
    fn file_shared_by_another_descriptor_is_kept() {
        let fixture = Fixture::new();
        fs::write(
            fixture.dir.path().join("providers/shared.toml"),
            r#"
id = "shared"
name = "Shared source"
[[locations]]
path = "%HOME%/config/auth.json"
format = "json"
strategy = "token_file"
source_type = "config_file"
"#,
        )
        .unwrap();
        let review = fixture.review();
        assert!(review.entries.iter().any(|entry| entry.eligible));
        assert_eq!(review.files.len(), 1);
        assert!(!review.files[0].eligible);
        assert!(review.files[0].reason.contains("Shared"));
    }

    #[cfg(unix)]
    #[test]
    fn source_parent_symlink_is_not_hidden_by_canonical_grouping() {
        let fixture = Fixture::new();
        let target = fixture.dir.path().join("other-config");
        fs::rename(fixture.dir.path().join("config"), &target).unwrap();
        std::os::unix::fs::symlink(&target, fixture.dir.path().join("config")).unwrap();
        let review = fixture.review();
        assert!(review.files.iter().all(|file| !file.eligible));
    }

    #[test]
    fn successful_trash_followed_by_history_save_failure_preserves_the_receipt() {
        let fixture = Fixture::new();
        let review = fixture.review();
        let fake_trash = fixture.dir.path().join("test-trash.json");
        let result = fixture
            .execute(fixture.request(&review, true), |path| {
                fs::rename(path, &fake_trash).unwrap();
                // Block only the fixture registry's atomic save destination.
                fs::create_dir(fixture.dir.path().join("registry.json.tmp")).unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(result.trashed_paths.len(), 1);
        assert!(result.removed_ids.is_empty());
        assert_eq!(result.retained.len(), 1);
        assert!(result.retained[0].reason.contains("saving history failed"));
        assert_eq!(result.snapshot.connections.len(), 1);
        assert_eq!(
            Registry::load(fixture.dir.path())
                .unwrap()
                .file
                .connections
                .len(),
            1
        );
        assert_eq!(fs::read_to_string(fake_trash).unwrap(), "{}");
    }

    #[test]
    fn recreated_source_keeps_history_and_reports_the_completed_move() {
        let fixture = Fixture::new();
        let review = fixture.review();
        let fake_trash = fixture.dir.path().join("test-trash.json");
        let result = fixture
            .execute(fixture.request(&review, true), |path| {
                fs::rename(path, &fake_trash).unwrap();
                fs::write(path, r#"{"token":"fixture-recreated-token"}"#).unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(result.trashed_paths.len(), 1);
        assert!(result.removed_ids.is_empty());
        assert_eq!(result.retained.len(), 1);
        assert!(result.file_failures[0].reason.contains("reappeared"));
        assert_eq!(result.snapshot.connections.len(), 1);
        assert_eq!(fs::read_to_string(fake_trash).unwrap(), "{}");
        assert!(fs::read_to_string(&fixture.source)
            .unwrap()
            .contains("fixture-recreated-token"));
    }
}
