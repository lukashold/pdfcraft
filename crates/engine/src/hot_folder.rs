//! Durable, opt-in local folder processing shared by the standalone app and agent tools.
//! No file watcher, network listener or worker is started by loading this module.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hot_folder_output::{self as output, OutputPlan, OutputProof};
use crate::hot_folder_preprocess as preprocess;
pub use crate::hot_folder_schedule::Schedule;
use crate::hot_folder_sources as sources;
use crate::{Session, ocr};

const MAX_INPUT: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_JSON: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Originals {
    #[default]
    Keep,
    Move,
    Delete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub input: PathBuf,
    pub output: PathBuf,
    pub archive: PathBuf,
    pub enabled: bool,
    pub recursive: bool,
    pub pdf: bool,
    pub images: bool,
    /// Produce JSON layout reports without invoking OCR or applying scan/image corrections.
    pub analysis_only: bool,
    /// Conservative: only entirely white rendered pages without text or annotations.
    pub remove_blank_pages: bool,
    /// Unix seconds; zero means now. Once tasks finish after their initial stable queue drains.
    pub start_at: u64,
    pub once: bool,
    pub schedule: Schedule,
    pub interval_seconds: u64,
    pub settle_seconds: u64,
    pub originals: Originals,
    pub settings: ocr::OcrSettings,
    pub output_plan: OutputPlan,
    pub preprocessing: preprocess::Settings,
    pub blank_archive: Option<PathBuf>,
    pub source: sources::SourceConfig,
}

impl Default for Task {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            input: PathBuf::new(),
            output: PathBuf::new(),
            archive: PathBuf::new(),
            enabled: false,
            recursive: false,
            pdf: true,
            images: true,
            analysis_only: false,
            remove_blank_pages: false,
            start_at: 0,
            once: false,
            schedule: Schedule::Interval,
            interval_seconds: 10,
            settle_seconds: 3,
            originals: Originals::Keep,
            settings: ocr::OcrSettings::default(),
            output_plan: OutputPlan::default(),
            preprocessing: preprocess::Settings::default(),
            blank_archive: None,
            source: sources::SourceConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub tasks: Vec<Task>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskStatus {
    pub completed: u64,
    pub failed: u64,
    pub waiting: usize,
    pub next_check: u64,
    pub finished: bool,
    pub batch_active: bool,
    /// A failed combined batch requires an explicit retry so no source is silently omitted.
    pub blocked: bool,
    pub last_input: Option<PathBuf>,
    pub last_output: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub updated_at: u64,
    pub tasks: BTreeMap<String, TaskStatus>,
    observations: BTreeMap<String, Observation>,
    receipts: BTreeMap<String, Receipt>,
    pub log: Vec<LogEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub at: u64,
    pub task: String,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Observation {
    fingerprint: String,
    since: u64,
    attempted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Receipt {
    outputs: Vec<PathBuf>,
    source_hash: String,
    #[serde(default)]
    source_stamp: String,
    #[serde(default)]
    output_proofs: Vec<OutputProof>,
    /// Persisted before touching originals; a retry resumes cleanup without rerunning OCR.
    #[serde(default)]
    cleanup: Option<PendingLocalAction>,
}

fn output_proof(path: &Path, root: Option<&Path>) -> Result<OutputProof, String> {
    let path = confined(path, root)?;
    let metadata = fs::symlink_metadata(&path).map_err(|e| format!("saved output is unavailable; original retained: {e}"))?;
    if !metadata.is_file() || metadata.len() > MAX_INPUT {
        return Err("saved output is not a regular file of at most 512 MiB; original retained".into());
    }
    let mut file = File::open(&path).map_err(|e| e.to_string())?.take(MAX_INPUT + 1);
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        digest.update(buffer.get(..read).ok_or("invalid output read size")?);
    }
    if bytes != metadata.len() {
        return Err("saved output changed during verification; original retained".into());
    }
    Ok(OutputProof { path, bytes, sha256: digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect() })
}

fn verify_outputs(receipt: &Receipt, root: Option<&Path>) -> Result<(), String> {
    if receipt.output_proofs.is_empty() || receipt.output_proofs.len() != receipt.outputs.len() {
        return Err("saved-output receipt has no integrity proofs; restore verified results before source cleanup".into());
    }
    for (path, proof) in receipt.outputs.iter().zip(&receipt.output_proofs) {
        if confined(path, root)? != proof.path || output_proof(path, root)? != *proof {
            return Err("saved output was replaced or changed; original retained".into());
        }
    }
    Ok(())
}

/// On Unix a synced temporary file does not make its renamed directory entry durable.
pub(crate) fn sync_parent(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(path.parent().ok_or("durable file needs a parent folder")?)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| format!("cannot persist folder entry; original retained: {e}"))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PendingLocalAction {
    Delete,
    Move { archive: PathBuf, relative: PathBuf },
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |v| v.as_secs())
}

pub fn load_config(path: &Path) -> Result<Config, String> {
    read_json(path)
}

/// Create a stopped configuration without replacing an existing file. Installers call this
/// as the chosen service account, never as root in another user's writable directory.
pub fn initialize_config(path: &Path) -> Result<(), String> {
    fn directory(path: &Path) -> Result<(), String> {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => return Ok(()),
            Ok(_) => return Err("configuration parent must be a real directory".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        directory(path.parent().ok_or("configuration needs an absolute parent")?)?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        #[cfg(not(unix))]
        let _ = &mut builder;
        builder.create(path).map_err(|e| e.to_string())?;
        sync_parent(path)
    }
    if !path.is_absolute() {
        return Err("configuration path must be absolute".into());
    }
    directory(path.parent().ok_or("configuration needs a parent")?)?;
    let path = confined(path, None)?;
    if path.exists() {
        return validate(&load_config(&path)?, None);
    }
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("configuration needs a parent")?).map_err(|e| e.to_string())?;
    temp.write_all(b"{\"tasks\":[]}\n").and_then(|_| temp.as_file().sync_all()).map_err(|e| e.to_string())?;
    match temp.persist_noclobber(&path) {
        Ok(_) => sync_parent(&path),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => validate(&load_config(&path)?, None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_config(path: &Path, config: &Config, root: Option<&Path>) -> Result<(), String> {
    validate(config, root)?;
    confined(path, root)?;
    save_json(path, config)
}

pub fn state_path(config: &Path) -> PathBuf {
    config.with_extension("state.json")
}

pub fn read_state(config: &Path) -> Result<State, String> {
    let path = state_path(config);
    if !path.exists() {
        return Ok(State::default());
    }
    read_json(&path)
}

pub fn record_worker_error(config: &Path, error: Option<&str>) -> Result<(), String> {
    let path = config.with_extension("error.json");
    if let Some(error) = error {
        save_json(&path, &error)
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub fn read_worker_error(config: &Path) -> Result<Option<String>, String> {
    let path = config.with_extension("error.json");
    if !path.exists() {
        return Ok(None);
    }
    read_json(&path).map(Some)
}

/// Canonicalize before opening anything; reject symbolic links on the final path and root escapes.
pub fn confined(path: &Path, root: Option<&Path>) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.map(Path::to_path_buf).unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?).join(path)
    };
    if let Some(root) = root {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        // Reject '..' before filesystem access; a confined call cannot probe outside the root.
        if absolute.components().any(|c| matches!(c, std::path::Component::ParentDir)) || !lexically_inside(&absolute, &root) {
            return Err("hot folder path is outside the allowed directory".into());
        }
    }
    if fs::symlink_metadata(&absolute).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("symbolic links are not allowed: {}", path.display()));
    }
    let result = if absolute.exists() {
        absolute.canonicalize()
    } else {
        absolute.parent().ok_or("path needs a parent folder")?.canonicalize().map(|p| p.join(absolute.file_name().unwrap_or_default()))
    }
    .map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(root) = root
        && !result.starts_with(root.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("hot folder path is outside the allowed directory".into());
    }
    Ok(result)
}

fn lexically_inside(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let normalize = |p: &Path| p.to_string_lossy().trim_start_matches(r"\\?\").replace('/', "\\").to_ascii_lowercase();
        let path = normalize(path);
        let root = normalize(root);
        path == root || path.starts_with(&format!("{}\\", root.trim_end_matches('\\')))
    }
    #[cfg(not(windows))]
    path.starts_with(root)
}

/// All sources and destinations must already exist and be disjoint, including across tasks.
/// This prevents recursive output loops and destructive source policies affecting another task.
pub fn validate(config: &Config, root: Option<&Path>) -> Result<(), String> {
    if config.tasks.len() > 32 {
        return Err("at most 32 hot folder tasks are allowed".into());
    }
    let mut ids = BTreeSet::new();
    let mut sources = Vec::new();
    let mut destinations = Vec::new();
    for t in &config.tasks {
        if t.id.is_empty() || t.id.len() > 100 || !t.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || !ids.insert(&t.id) {
            return Err("task IDs must be unique, using 1..100 ASCII letters, digits, '-' or '_'".into());
        }
        if t.name.trim().is_empty() || t.name.len() > 200 {
            return Err("task name is required (at most 200 bytes)".into());
        }
        if t.input.as_os_str().is_empty() || t.output.as_os_str().is_empty() || t.originals == Originals::Move && t.archive.as_os_str().is_empty() {
            return Err("choose an input, output and (when moving originals) archive folder".into());
        }
        if root.is_none() && (!t.input.is_absolute() || !t.output.is_absolute() || t.originals == Originals::Move && !t.archive.is_absolute()) {
            return Err("folder paths must be absolute; use the folder picker".into());
        }
        if !t.pdf && !t.images {
            return Err("choose PDF, images, or both".into());
        }
        if !(1..=86400).contains(&t.interval_seconds) || !(1..=300).contains(&t.settle_seconds) {
            return Err("check interval must be 1..86400 seconds; settling time 1..300 seconds".into());
        }
        let s = &t.settings;
        t.schedule.validate()?;
        t.preprocessing.validate()?;
        if !s.dpi.is_finite()
            || !(72.0..=600.0).contains(&s.dpi)
            || !s.crop_margin.is_finite()
            || !(0.0..=20.0).contains(&s.crop_margin)
            || !ocr::LANGUAGES.iter().any(|l| l.0 == s.language)
        {
            return Err("invalid OCR language, resolution or crop margin".into());
        }
        let input = confined(&t.input, root)?;
        if let sources::SourceConfig::Outlook(source) = &t.source
            && !matches!(source.action, sources::SourceAction::Keep)
            && (!t.pdf || !t.images)
        {
            return Err(
                "Outlook move/delete requires PDF and images: every supported attachment must finish before changing the whole message".into()
            );
        }
        sources::validate(&t.source, &input, root)?;
        let output = confined(&t.output, root)?;
        t.output_plan.validate(&output, root)?;
        if !input.is_dir() || !output.is_dir() {
            return Err("input and output must be existing folders".into());
        }
        if output.starts_with(&input) || input.starts_with(&output) {
            return Err("input, output and archive folders must not contain one another".into());
        }
        if t.enabled {
            sources.push(input.clone());
            sources.extend(sources::local_paths(&t.source, root)?);
            if let sources::SourceConfig::Project(source) = &t.source
                && let sources::SourceAction::Move { destination } = &source.action
            {
                destinations.push(confined(Path::new(destination), root)?);
            }
            destinations.push(output);
        }
        if let Some(folder) = &t.blank_archive {
            if root.is_none() && !folder.is_absolute() {
                return Err("blank page archive must be absolute".into());
            }
            let folder = confined(folder, root)?;
            if !folder.is_dir() || folder.starts_with(&input) || input.starts_with(&folder) {
                return Err("blank page archive must be an existing folder disjoint from the input".into());
            }
            if t.enabled {
                destinations.push(folder);
            }
        }
        for step in &t.output_plan.steps {
            if let Some(folder) = &step.folder {
                if root.is_none() && !folder.is_absolute() {
                    return Err("output step folders must be absolute".into());
                }
                let folder = confined(folder, root)?;
                if folder.starts_with(&input) || input.starts_with(&folder) {
                    return Err("output step folders must not overlap an input folder".into());
                }
                if t.enabled {
                    destinations.push(folder);
                }
            }
        }
        if t.originals == Originals::Move {
            let archive = confined(&t.archive, root)?;
            if !archive.is_dir() {
                return Err("archive must be an existing folder".into());
            }
            if archive.starts_with(&input) || input.starts_with(&archive) {
                return Err("input, output and archive folders must not contain one another".into());
            }
            if t.enabled {
                destinations.push(archive);
            }
        }
    }
    for source in &sources {
        if destinations.iter().any(|d| d.starts_with(source) || source.starts_with(d)) {
            return Err("input, output and archive folders must not contain one another".into());
        }
    }
    for (i, source) in sources.iter().enumerate() {
        if sources.iter().skip(i + 1).any(|d| d.starts_with(source) || source.starts_with(d)) {
            return Err("task input folders must not overlap".into());
        }
    }
    Ok(())
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_JSON {
        return Err("hot folder JSON exceeds 16 MiB".into());
    }
    serde_json::from_reader(file.take(MAX_JSON)).map_err(|e| format!("{}: {e}", path.display()))
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let previous = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("configuration and journal targets must be regular files".into());
        }
        Ok(metadata) => Some(metadata),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_JSON {
        return Err("hot folder journal is full; archive the completed task and its configuration".into());
    }
    let parent = path.parent().ok_or("missing parent folder")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if let Some(metadata) = &previous {
        use std::os::unix::fs::MetadataExt;
        // Keep a service-owned configuration service-readable even when an administrator GUI
        // performs the atomic replacement. Failure leaves the previous file untouched.
        std::os::unix::fs::chown(temp.path(), Some(metadata.uid()), Some(metadata.gid())).map_err(|e| e.to_string())?;
        temp.as_file().set_permissions(metadata.permissions()).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = previous;
    temp.write_all(&bytes).and_then(|_| temp.as_file().sync_all()).map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    sync_parent(path)
}

/// Run one bounded scheduler pass; persists observations, receipts, errors and schedule state.
/// One file or one bounded combined batch per task is processed per pass.
/// The lock is an OS file lock, released even if the process exits unexpectedly.
pub fn poll(config_path: &Path, root: Option<&Path>) -> Result<State, String> {
    poll_cancelable(config_path, root, &|| false)
}

/// Service-aware form: cancellation between files and OCR pages discards partial outputs.
pub fn poll_cancelable(config_path: &Path, root: Option<&Path>, should_stop: &dyn Fn() -> bool) -> Result<State, String> {
    let path = confined(config_path, root)?;
    let state_file = confined(&state_path(&path), root)?;
    let lock_path = confined(&path.with_extension("lock"), root)?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path).map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|_| "another Hot Folder worker is processing this configuration".to_string())?;
    let config = load_config(&path)?;
    validate(&config, root)?;
    let mut state = read_state(&path)?;
    let current = now();
    for task in &config.tasks {
        if should_stop() {
            break;
        }
        if !task.enabled || current < task.start_at {
            continue;
        }
        let status = state.tasks.entry(task.id.clone()).or_default();
        if !task.once && task.schedule != Schedule::Interval && status.next_check == 0 && !status.batch_active {
            status.next_check = task.schedule.next_after(current.max(task.start_at).saturating_sub(1))?;
        }
        if status.finished || status.blocked || current < status.next_check {
            continue;
        }
        status.next_check = current.saturating_add(task.interval_seconds);
        status.batch_active = true;
        if let Err(error) = step(task, &mut state, current, &path, root, should_stop) {
            let status = state.tasks.entry(task.id.clone()).or_default();
            status.error = Some(error.clone());
            let snapshot = serde_json::to_vec(task).map_err(|e| e.to_string())?;
            let still_enabled = task_unchanged(&path, &task.id, &snapshot);
            status.blocked = task.output_plan.needs_batch() && !should_stop() && still_enabled;
            status.failed = status.failed.saturating_add(1);
            state.log.push(LogEntry { at: now(), task: task.id.clone(), input: status.last_input.clone(), output: None, error: Some(error) });
        }
        if state.log.len() > 1000 {
            state.log.drain(..state.log.len().saturating_sub(1000));
        }
        let status = state.tasks.entry(task.id.clone()).or_default();
        if status.waiting == 0 {
            status.batch_active = false;
            if !task.once && task.schedule != Schedule::Interval {
                status.next_check = task.schedule.next_after(now())?;
            }
        }
        state.updated_at = now();
        save_json(&state_file, &state)?;
    }
    state.updated_at = current;
    save_json(&state_file, &state)?;
    Ok(state)
}

fn task_unchanged(config: &Path, id: &str, snapshot: &[u8]) -> bool {
    load_config(config).is_ok_and(|config| {
        config.tasks.iter().any(|task| task.id == id && task.enabled && serde_json::to_vec(task).is_ok_and(|current| current == snapshot))
    })
}

fn fingerprint(path: &Path) -> Result<String, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("input is not a regular file".into());
    }
    let modified = meta.modified().map_err(|e| e.to_string())?.duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    Ok(format!("{}:{modified}", meta.len()))
}

fn files(task: &Task, root: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    let input = confined(&task.input, root)?;
    let mut pending = vec![(input.clone(), 0usize)];
    let mut out = Vec::new();
    let mut count = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            count = count.saturating_add(1);
            if count > MAX_ENTRIES {
                return Err("input folder exceeds 10,000 entries".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if !confined(&path, root)?.starts_with(&input) {
                return Err("input escaped its folder".into());
            }
            if kind.is_dir() && task.recursive {
                if depth >= 32 {
                    return Err("input folder nesting exceeds 32 levels".into());
                }
                pending.push((path, depth + 1));
            } else if kind.is_file() {
                let ext = path.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
                if task.pdf && ext == "pdf" || task.images && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp") {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

struct Prepared {
    source: PathBuf,
    key: String,
    stamp: String,
    hash: String,
    relative: PathBuf,
    processed: Option<Arc<Vec<u8>>>,
    blank_pages: Option<Arc<Vec<u8>>>,
    remote: Option<sources::StagedSource>,
}

fn remove_staged(path: &Path, expected: &str) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() || metadata.len() > MAX_INPUT {
        return Err("staging cleanup refused a changed or nonregular file".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?.take(MAX_INPUT + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(buffer.get(..read).ok_or("invalid staging read size")?);
    }
    let hash: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if hash != expected {
        return Err("staged file changed after processing; retained for inspection".into());
    }
    fs::remove_file(path).map_err(|e| format!("outputs and source acknowledgement saved, but staging cleanup failed: {e}"))
}

fn step(task: &Task, state: &mut State, current: u64, config_path: &Path, root: Option<&Path>, should_stop: &dyn Fn() -> bool) -> Result<(), String> {
    if state.observations.len() >= MAX_ENTRIES || state.receipts.len() >= MAX_ENTRIES {
        return Err("task journal reached 10,000 entries; archive it before continuing".into());
    }
    let input = confined(&task.input, root)?;
    let output = confined(&task.output, root)?;
    let snapshot = serde_json::to_vec(task).map_err(|e| e.to_string())?;
    let task_stopped = || should_stop() || !task_unchanged(config_path, &task.id, &snapshot);
    let store = match &task.source {
        sources::SourceConfig::Ftp(_) | sources::SourceConfig::Outlook(_) => sources::CredentialStore::for_config(config_path, root)?,
        _ => sources::CredentialStore::OsVault,
    };
    let mut remote = BTreeMap::new();
    let mut acknowledgement_proofs = Vec::new();
    let candidates = if matches!(task.source, sources::SourceConfig::Local) {
        files(task, root)?
    } else {
        let staged = sources::stage(&task.source, &input, root, &task_stopped, &store)?;
        if staged.has_more && task.output_plan.needs_batch() {
            return Err("combined remote queue exceeds 128 files or 512 MiB; choose a smaller source or per-file outputs".into());
        }
        for warning in staged.warnings {
            state.log.push(LogEntry { at: now(), task: task.id.clone(), input: None, output: None, error: Some(warning) });
        }
        for item in &staged.pending_acknowledgements {
            let path = confined(&input.join(&item.staged_relative), root)?;
            let key = format!("{}:{}:{}", task.id, path.display(), item.sha256);
            let receipt = state.receipts.get(&key).ok_or("pending source acknowledgement has no saved-output receipt; original retained")?;
            verify_outputs(receipt, root)?;
        }
        let mut items = staged.items;
        items.extend(staged.pending_acknowledgements);
        // A final Outlook acknowledgement may consume the whole message. Verify every
        // earlier ready attachment's outputs before acknowledging any of its peers.
        for item in &items {
            let path = confined(&input.join(&item.staged_relative), root)?;
            let key = format!("{}:{}:{}", task.id, path.display(), item.sha256);
            if let Some(receipt) = state.receipts.get(&key) {
                verify_outputs(receipt, root)?;
                acknowledgement_proofs.push(receipt.clone());
            }
        }
        if !matches!(task.source, sources::SourceConfig::Project(_)) {
            items.sort_by(|a, b| a.relative.cmp(&b.relative));
        }
        let mut paths = Vec::new();
        for item in items {
            let path = confined(&input.join(&item.staged_relative), root)?;
            if !path.starts_with(&input) {
                return Err("staged source escaped its inbox".into());
            }
            let key = format!("{}:{}", task.id, path.display());
            // Recover a failed/interrupted acknowledgement without rerunning OCR or saving duplicates.
            if state.receipts.contains_key(&format!("{key}:{}", item.sha256)) {
                for receipt in &acknowledgement_proofs {
                    verify_outputs(receipt, root)?;
                }
                sources::acknowledge(&task.source, &input, &item, &item.sha256, root, &task_stopped, &store)?;
                remove_staged(&path, &item.sha256)?;
                continue;
            }
            let ext = item.relative.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
            if task.pdf && ext == "pdf" || task.images && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp") {
                remote.insert(path.clone(), item);
                paths.push(path);
            }
        }
        paths
    };
    let mut ready = Vec::new();
    let mut waiting = 0usize;
    let batch = task.output_plan.needs_batch();
    for path in candidates {
        let key = format!("{}:{}", task.id, path.display());
        let stamp = fingerprint(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let observation =
            state.observations.entry(key.clone()).or_insert_with(|| Observation { fingerprint: stamp.clone(), since: current, attempted: false });
        if observation.fingerprint != stamp {
            *observation = Observation { fingerprint: stamp.clone(), since: current, attempted: false };
        }
        let pending_cleanup = state
            .receipts
            .iter()
            .any(|(receipt_key, receipt)| receipt_key.starts_with(&format!("{key}:")) && receipt.source_stamp == stamp && receipt.cleanup.is_some());
        if observation.attempted && !pending_cleanup {
            continue;
        }
        waiting = waiting.saturating_add(1);
        if (batch || ready.is_empty()) && current.saturating_sub(observation.since) >= task.settle_seconds {
            ready.push((path, key, stamp));
        }
    }
    let status = state.tasks.entry(task.id.clone()).or_default();
    status.waiting = waiting;
    if ready.is_empty() && waiting == 0 && task.once {
        status.finished = true;
    }
    // A grouped result includes the complete currently visible queue; never consume an
    // unfinished copy or publish a misleading partial group while another input is settling.
    if ready.is_empty() || batch && ready.len() != waiting {
        return Ok(());
    }
    if ready.len() > 128 {
        return Err("combined batch exceeds 128 files; split the task or use one output per file".into());
    }
    let mut prepared = Vec::new();
    let mut total = 0u64;
    for (source, key, stamp) in ready {
        if task_stopped() {
            return Err("task stopped; no partial output was saved".into());
        }
        state.tasks.entry(task.id.clone()).or_default().last_input = Some(source.clone());
        let file = File::open(&source).map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        total = total.saturating_add(length);
        if length == 0 || total > MAX_INPUT {
            if let Some(o) = state.observations.get_mut(&key) {
                o.attempted = true;
            }
            return Err("input or combined batch must contain 1 byte to 512 MiB; sources retained".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_INPUT + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_INPUT || fingerprint(&source)? != stamp {
            return Err("input changed while being read; waiting for a stable copy".into());
        }
        let hash: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        let receipt_key = format!("{key}:{hash}");
        if let Some(receipt) = state.receipts.get(&receipt_key).cloned() {
            if let Some(cleanup) = &receipt.cleanup {
                verify_outputs(&receipt, root)?;
                finish_original(&source, &hash, cleanup, task, root, &task_stopped)?;
                if let Some(receipt) = state.receipts.get_mut(&receipt_key) {
                    receipt.cleanup = None;
                }
                save_json(&state_path(config_path), state)?;
            }
            if let Some(o) = state.observations.get_mut(&key) {
                o.attempted = true;
            }
            state.tasks.entry(task.id.clone()).or_default().waiting = state.tasks.get(&task.id).map_or(0, |s| s.waiting.saturating_sub(1));
            continue;
        }
        let processed = match prepare(task, &source, bytes.clone(), config_path, &task_stopped) {
            Ok(bytes) => bytes,
            Err(e) => {
                if !task_stopped()
                    && let Some(o) = state.observations.get_mut(&key)
                {
                    o.attempted = true;
                }
                return Err(e);
            }
        };
        if fingerprint(&source)? != stamp {
            return Err("input changed during processing; sources retained".into());
        }
        prepared.push(Prepared {
            relative: remote.get(&source).map(|item| item.relative.clone()).unwrap_or(source.strip_prefix(&input).map_err(|e| e.to_string())?.into()),
            remote: remote.get(&source).cloned(),
            processed: processed.document,
            blank_pages: processed.blank_pages,
            source,
            key,
            stamp,
            hash,
        });
    }
    if prepared.is_empty() {
        return Ok(());
    }
    let stopped = &task_stopped;
    if stopped() {
        return Err("task stopped; no partial output was saved".into());
    }
    for file in &prepared {
        if fingerprint(&file.source)? != file.stamp {
            return Err("input changed before saving; sources retained".into());
        }
    }
    let inputs: Vec<_> = prepared
        .iter()
        .filter_map(|p| p.processed.as_ref().map(|bytes| output::Input { relative: p.relative.clone(), bytes: bytes.clone() }))
        .collect();
    let primary = if inputs.is_empty() {
        Ok(output::OutputReceipt::default())
    } else {
        let mut plan = task.output_plan.clone();
        for step in &mut plan.steps {
            if task.analysis_only {
                step.format = output::Format::Analysis;
            }
            if step.format == output::Format::Epub && step.language == "und" {
                step.language = task.settings.language.clone();
            }
        }
        output::save(&inputs, &output, &plan, &task.name, current, root, &stopped)
    };
    let mut result = match primary {
        Ok(result) => result,
        Err(error) => {
            for path in error.outputs {
                state.log.push(LogEntry { at: now(), task: task.id.clone(), input: None, output: Some(path), error: Some(error.message.clone()) });
            }
            for file in &prepared {
                if let Some(o) = state.observations.get_mut(&file.key) {
                    o.attempted = !stopped();
                }
            }
            return Err(format!("{}; all originals retained", error.message));
        }
    };
    if let Some(archive) = &task.blank_archive {
        let blanks: Vec<_> = prepared
            .iter()
            .filter_map(|p| p.blank_pages.as_ref().map(|bytes| output::Input { relative: p.relative.clone(), bytes: bytes.clone() }))
            .collect();
        if !blanks.is_empty() {
            let plan = OutputPlan { steps: vec![output::SaveStep { name_template: "{name}_blank_pages".into(), ..Default::default() }] };
            match output::save(&blanks, archive, &plan, &task.name, current, root, &stopped) {
                Ok(saved) => {
                    result.outputs.extend(saved.outputs);
                    result.proofs.extend(saved.proofs);
                    result.warnings.extend(saved.warnings);
                }
                Err(error) => {
                    for path in result.outputs.into_iter().chain(error.outputs) {
                        state.log.push(LogEntry {
                            at: now(),
                            task: task.id.clone(),
                            input: None,
                            output: Some(path),
                            error: Some(error.message.clone()),
                        });
                    }
                    for file in &prepared {
                        if let Some(o) = state.observations.get_mut(&file.key) {
                            o.attempted = !stopped();
                        }
                    }
                    return Err(format!("blank-page archive failed: {}; all originals retained", error.message));
                }
            }
        }
    }
    for proof in &result.proofs {
        if output_proof(&proof.path, root)? != *proof {
            return Err("published output no longer matches the export; all originals retained".into());
        }
    }
    for file in &prepared {
        let cleanup = if file.remote.is_some() {
            None
        } else {
            match task.originals {
                Originals::Keep => None,
                Originals::Delete => Some(PendingLocalAction::Delete),
                Originals::Move => Some(PendingLocalAction::Move { archive: confined(&task.archive, root)?, relative: file.relative.clone() }),
            }
        };
        state.receipts.insert(
            format!("{}:{}", file.key, file.hash),
            Receipt {
                outputs: result.outputs.clone(),
                source_hash: file.hash.clone(),
                source_stamp: file.stamp.clone(),
                output_proofs: result.proofs.clone(),
                cleanup,
            },
        );
        if let Some(o) = state.observations.get_mut(&file.key) {
            o.attempted = true;
        }
        state.log.push(LogEntry {
            at: now(),
            task: task.id.clone(),
            input: Some(file.source.clone()),
            output: result.outputs.first().cloned(),
            error: None,
        });
    }
    for path in result.outputs.iter().skip(1) {
        state.log.push(LogEntry { at: now(), task: task.id.clone(), input: None, output: Some(path.clone()), error: None });
    }
    for warning in result.warnings {
        state.log.push(LogEntry { at: now(), task: task.id.clone(), input: None, output: None, error: Some(warning) });
    }
    let status = state.tasks.entry(task.id.clone()).or_default();
    status.last_output = result.outputs.first().cloned();
    status.completed = status.completed.saturating_add(prepared.len() as u64);
    status.error = None;
    status.waiting = status.waiting.saturating_sub(prepared.len());
    // All outputs and their receipts are durable before any source move/delete is attempted.
    save_json(&state_path(config_path), state)?;
    for file in prepared {
        if stopped() {
            return Err("task stopped after outputs were saved; remaining original actions will resume on retry".into());
        }
        let receipt_key = format!("{}:{}", file.key, file.hash);
        if file.remote.is_some() || state.receipts.get(&receipt_key).is_some_and(|r| r.cleanup.is_some()) {
            verify_outputs(state.receipts.get(&receipt_key).ok_or("output receipt unavailable; original retained")?, root)?;
        }
        if let Some(remote) = &file.remote {
            // A previous ready attachment's output may have changed during lengthy OCR
            // of this final attachment. Recheck the whole pending group at the action.
            for receipt in &acknowledgement_proofs {
                verify_outputs(receipt, root)?;
            }
            sources::acknowledge(&task.source, &input, remote, &file.hash, root, &stopped, &store)?;
            remove_staged(&file.source, &file.hash)?;
            continue;
        }
        let key = format!("{}:{}", file.key, file.hash);
        if let Some(action) = state.receipts.get(&key).and_then(|r| r.cleanup.clone()) {
            finish_original(&file.source, &file.hash, &action, task, root, &stopped)?;
            if let Some(receipt) = state.receipts.get_mut(&key) {
                receipt.cleanup = None;
            }
            save_json(&state_path(config_path), state)?;
        }
    }
    Ok(())
}

fn prepare(
    task: &Task,
    source: &Path,
    bytes: Vec<u8>,
    config_path: &Path,
    should_stop: &dyn Fn() -> bool,
) -> Result<preprocess::BlankResult, String> {
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    let pdf = if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        Arc::new(bytes)
    } else {
        Session::new().create_from_images(&[(name.to_string(), bytes)]).map_err(|e| e.to_string())?
    };
    if task.analysis_only {
        let selected = if task.preprocessing.pages.trim().is_empty() {
            pdf
        } else {
            // COS-only page selection; do not open an interactive document or run its scripts.
            Session::new().combine_ranges(&[(name.to_string(), pdf, Some(task.preprocessing.pages.clone()))]).map_err(|e| e.to_string())?
        };
        return Ok(preprocess::BlankResult { document: Some(selected), blank_pages: None });
    }
    let pdf = preprocess::prepare(pdf, &task.preprocessing, f64::from(task.settings.dpi), should_stop)?;
    let split = if task.remove_blank_pages {
        preprocess::separate_blank_pages(pdf, should_stop)?
    } else {
        preprocess::BlankResult { document: Some(pdf), blank_pages: None }
    };
    let blanks = split.blank_pages;
    let Some(pdf) = split.document else {
        return if task.blank_archive.is_some() {
            Ok(preprocess::BlankResult { document: None, blank_pages: blanks })
        } else {
            Err("all pages are blank; select a blank-page archive to keep them separately".into())
        };
    };
    if !task.settings.recognize && !task.settings.auto_rotate && !task.settings.deskew && !task.settings.auto_crop {
        return Ok(preprocess::BlankResult { document: Some(pdf), blank_pages: blanks });
    }
    let model = ocr::engine()?;
    let mut cancelled = false;
    let mut session = Session::new();
    let id = session.open(name.as_ref(), None, pdf, None).map_err(|e| e.to_string())?;
    let doc = session.get(id).ok_or("document unavailable")?;
    if let Some(reason) = &doc.read_only_reason {
        return Err(reason.clone());
    }
    let infos = doc.info.pages.clone();
    let job = session.ocr_job(id, &[], task.settings.clone()).ok_or("document unavailable")?;
    let mut pages = job.run(&model, |_, _| {
        let enabled = !should_stop() && load_config(config_path).is_ok_and(|c| c.tasks.iter().any(|t| t.id == task.id && t.enabled));
        cancelled |= !enabled;
        enabled
    });
    preprocess::constrain_words(&mut pages, &infos, &task.preprocessing.regions);

    if cancelled || should_stop() {
        return Err("task stopped; no partial output was saved".into());
    }
    if let Some(why) = pages.iter().filter_map(|p| p.skipped.as_deref()).find(|s| *s != "the page already contains text") {
        return Err(format!("scan processing failed; sources retained: {why}"));
    }
    session.apply_ocr(id, &pages).map_err(|e| e.to_string())?;
    Ok(preprocess::BlankResult { document: Some(session.save_bytes(id).map_err(|e| e.to_string())?), blank_pages: blanks })
}

#[cfg(test)]
fn remove_blank_pages(bytes: Arc<Vec<u8>>) -> Result<Arc<Vec<u8>>, String> {
    preprocess::separate_blank_pages(bytes, &|| false)?.document.ok_or_else(|| "all pages are blank".into())
}

fn safe_subfolder(base: &Path, relative: &Path) -> Result<(), String> {
    let mut path = base.to_path_buf();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err("invalid output subfolder".into());
        }
        path.push(part);
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("output subfolder is a symbolic link".into());
        }
        fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        sync_parent(&path)?;
        if !path.canonicalize().map_err(|e| e.to_string())?.starts_with(base) {
            return Err("output subfolder escaped its folder".into());
        }
    }
    Ok(())
}

fn finish_original(
    source: &Path,
    hash: &str,
    action: &PendingLocalAction,
    task: &Task,
    root: Option<&Path>,
    stopped: &dyn Fn() -> bool,
) -> Result<(), String> {
    if stopped() {
        return Err("task stopped; original retained".into());
    }
    // A changed task policy must never retroactively authorize deletion of old inputs.
    match action {
        PendingLocalAction::Delete if task.originals != Originals::Delete => return Ok(()),
        PendingLocalAction::Move { archive, .. } if task.originals != Originals::Move || confined(&task.archive, root)? != *archive => return Ok(()),
        _ => {}
    }
    if !fs::symlink_metadata(source).map_err(|e| e.to_string())?.is_file() {
        return Err("original is not a regular file".into());
    }
    let mut bytes = Vec::new();
    File::open(source).map_err(|e| e.to_string())?.take(MAX_INPUT + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let actual: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    if bytes.len() as u64 > MAX_INPUT || actual != hash {
        return Err("source changed after output was saved; original retained".into());
    }
    if let PendingLocalAction::Move { archive, relative } = action {
        let archive = confined(archive, root)?;
        let parent = relative.parent().unwrap_or(Path::new(""));
        safe_subfolder(&archive, parent)?;
        save_unique(
            &archive.join(parent),
            source.file_stem().unwrap_or_default().to_string_lossy().as_ref(),
            source.extension().unwrap_or_default().to_string_lossy().as_ref(),
            &bytes,
        )?;
    }
    if stopped() {
        return Err("task stopped after archive copy; original retained".into());
    }
    remove_staged(source, hash)
}

fn save_unique(folder: &Path, stem: &str, extension: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let stem: String = stem.chars().take(100).collect();
    let mut temp = tempfile::NamedTempFile::new_in(folder).map_err(|e| e.to_string())?;
    temp.write_all(bytes).and_then(|_| temp.as_file().sync_all()).map_err(|e| e.to_string())?;
    for n in 0..1000 {
        let name = if n == 0 { format!("{stem}.{extension}") } else { format!("{stem} ({n}).{extension}") };
        let target = folder.join(name);
        if let Ok(meta) = fs::symlink_metadata(&target) {
            if meta.is_file() && meta.len() == bytes.len() as u64 {
                let mut existing = Vec::new();
                File::open(&target).map_err(|e| e.to_string())?.take(MAX_INPUT + 1).read_to_end(&mut existing).map_err(|e| e.to_string())?;
                if existing == bytes {
                    sync_parent(&target)?;
                    return Ok(target);
                }
            }
            continue;
        }
        match temp.persist_noclobber(&target) {
            Ok(_) => {
                sync_parent(&target)?;
                return Ok(target);
            }
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => temp = e.file,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("too many existing output filenames; no file was overwritten".into())
}

/// Retry failed inputs / reschedule completed once tasks without forgetting successful receipts.
pub fn reset(config_path: &Path, task_id: &str, root: Option<&Path>) -> Result<(), String> {
    reset_for_run(config_path, task_id, root, false)
}

pub fn run_now(config_path: &Path, task_id: &str, root: Option<&Path>) -> Result<(), String> {
    reset_for_run(config_path, task_id, root, true)
}

fn reset_for_run(config_path: &Path, task_id: &str, root: Option<&Path>, immediate: bool) -> Result<(), String> {
    let path = confined(config_path, root)?;
    if !load_config(&path)?.tasks.iter().any(|t| t.id == task_id) {
        return Err("no such Hot Folder task".into());
    }
    let lock_path = confined(&path.with_extension("lock"), root)?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path).map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|_| "worker is busy; stop the task and retry shortly".to_string())?;
    let mut state = read_state(&path)?;
    state.tasks.remove(task_id);
    if immediate {
        state.tasks.insert(task_id.into(), TaskStatus { batch_active: true, ..Default::default() });
    }
    state.observations.retain(|k, _| !k.starts_with(&format!("{task_id}:")));
    save_json(&confined(&state_path(&path), root)?, &state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, Task) {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input");
        let output = dir.path().join("output");
        let archive = dir.path().join("archive");
        for path in [&input, &output, &archive] {
            fs::create_dir(path).unwrap();
        }
        let task = Task { id: "scan".into(), name: "Scan task".into(), input, output, archive, enabled: true, ..Default::default() };
        let path = dir.path().join("tasks.json");
        save_config(&path, &Config { tasks: vec![task.clone()] }, None).unwrap();
        (dir, path, task)
    }

    fn text_pdf() -> Arc<Vec<u8>> {
        Session::new().create_from_text("test", "A searchable original remains intact").unwrap()
    }

    fn settled(task: &Task, state: &mut State, path: &Path) -> Result<(), String> {
        step(task, state, 10, path, None, &|| false)?;
        step(task, state, 13, path, None, &|| false)
    }

    #[test]
    fn hot_folder_initialize_is_atomic_and_preserves_existing_configs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/tasks.json");
        initialize_config(&path).unwrap();
        assert!(load_config(&path).unwrap().tasks.is_empty());
        fs::write(&path, b"broken existing settings").unwrap();
        assert!(initialize_config(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken existing settings");
    }

    #[test]
    fn hot_folder_batch_waits_for_all_inputs_and_saves_multiple_outputs_before_deletion() {
        let (_dir, config, mut task) = setup();
        task.originals = Originals::Delete;
        task.settings.recognize = false;
        task.settings.auto_rotate = false;
        task.settings.deskew = false;
        task.output_plan.steps = vec![
            output::SaveStep { grouping: output::Grouping::All, ..Default::default() },
            output::SaveStep { format: output::Format::Txt, grouping: output::Grouping::All, ..Default::default() },
        ];
        save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
        fs::write(task.input.join("a.pdf"), text_pdf().as_slice()).unwrap();
        let mut state = State::default();
        step(&task, &mut state, 10, &config, None, &|| false).unwrap();
        fs::write(task.input.join("b.pdf"), text_pdf().as_slice()).unwrap();
        step(&task, &mut state, 13, &config, None, &|| false).unwrap();
        assert!(fs::read_dir(&task.output).unwrap().next().is_none());
        step(&task, &mut state, 16, &config, None, &|| false).unwrap();
        assert!(fs::read_dir(&task.input).unwrap().next().is_none());
        assert_eq!(fs::read_dir(&task.output).unwrap().count(), 2);
        let pdf = fs::read(task.output.join("Scan task.pdf")).unwrap();
        let mut session = Session::new();
        let id = session.open("combined", None, Arc::new(pdf), None).unwrap();
        assert_eq!(session.get(id).unwrap().info.pages.len(), 2);
        assert_eq!(read_state(&config).unwrap().receipts.len(), 2);
    }

    #[test]
    fn hot_folder_failed_second_output_preserves_all_sources_and_publishes_nothing() {
        let (_dir, config, mut task) = setup();
        task.originals = Originals::Delete;
        task.settings.recognize = false;
        task.settings.auto_rotate = false;
        task.settings.deskew = false;
        task.output_plan.steps.push(output::SaveStep {
            format: output::Format::Txt,
            folder: Some(task.archive.join("missing")),
            ..Default::default()
        });
        save_json(&config, &Config { tasks: vec![task.clone()] }).unwrap();
        fs::write(task.input.join("source.pdf"), text_pdf().as_slice()).unwrap();
        let mut state = State::default();
        assert!(settled(&task, &mut state, &config).is_err());
        assert!(task.input.join("source.pdf").exists());
        assert!(fs::read_dir(&task.output).unwrap().next().is_none());
        assert!(state.receipts.is_empty());
    }

    #[test]
    fn hot_folder_rejects_overlapping_folders_and_invalid_options() {
        let (_dir, _, mut t) = setup();
        t.output = t.input.clone();
        assert!(validate(&Config { tasks: vec![t.clone()] }, None).unwrap_err().contains("must not contain"));
        t.output = t.archive.clone();
        t.settings.language = "unsupported".into();
        assert!(validate(&Config { tasks: vec![t.clone()] }, None).is_err());
        t.settings.language = "de".into();
        t.settle_seconds = 0;
        assert!(validate(&Config { tasks: vec![t] }, None).is_err());
    }

    #[test]
    fn hot_folder_settling_and_durable_receipts_prevent_duplicates_and_overwrites() {
        if !ocr::available() {
            return;
        }
        let (_dir, config, task) = setup();
        let original = text_pdf();
        fs::write(task.input.join("document.pdf"), original.as_slice()).unwrap();
        fs::write(task.output.join("document.pdf"), b"existing result").unwrap();
        let mut state = State::default();
        step(&task, &mut state, 10, &config, None, &|| false).unwrap();
        step(&task, &mut state, 12, &config, None, &|| false).unwrap();
        assert_eq!(fs::read_dir(&task.output).unwrap().count(), 1);
        step(&task, &mut state, 13, &config, None, &|| false).unwrap();
        assert_eq!(fs::read(task.output.join("document.pdf")).unwrap(), b"existing result");
        assert_eq!(fs::read(task.input.join("document.pdf")).unwrap(), *original);
        let output = task.output.join("document (1).pdf");
        assert!(output.exists());
        let mut restored = read_state(&config).unwrap();
        restored.observations.clear();
        settled(&task, &mut restored, &config).unwrap();
        assert_eq!(fs::read_dir(&task.output).unwrap().count(), 2);
        assert_eq!(restored.receipts.len(), 1);
    }

    #[test]
    fn hot_folder_changes_restart_settling_and_failures_preserve_originals() {
        let (_dir, config, mut task) = setup();
        task.originals = Originals::Delete;
        save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
        let source = task.input.join("broken.pdf");
        fs::write(&source, b"partial").unwrap();
        let mut state = State::default();
        step(&task, &mut state, 10, &config, None, &|| false).unwrap();
        fs::write(&source, b"still not a complete PDF").unwrap();
        step(&task, &mut state, 13, &config, None, &|| false).unwrap();
        assert!(fs::read_dir(&task.output).unwrap().next().is_none());
        assert!(step(&task, &mut state, 16, &config, None, &|| false).is_err());
        assert!(source.exists());
        assert!(state.receipts.is_empty());
    }

    #[test]
    fn hot_folder_move_and_delete_happen_only_after_success() {
        if !ocr::available() {
            return;
        }
        for policy in [Originals::Move, Originals::Delete] {
            let (_dir, config, mut task) = setup();
            task.originals = policy;
            save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
            let original = text_pdf();
            fs::write(task.input.join("source.pdf"), original.as_slice()).unwrap();
            let mut state = State::default();
            settled(&task, &mut state, &config).unwrap();
            assert!(!task.input.join("source.pdf").exists());
            assert!(task.output.join("source.pdf").exists());
            assert_eq!(read_state(&config).unwrap().receipts.len(), 1);
            if policy == Originals::Move {
                assert_eq!(fs::read(task.archive.join("source.pdf")).unwrap(), *original);
            }
        }
    }

    #[test]
    fn hot_folder_retry_finishes_a_journaled_archive_without_duplicate_outputs() {
        let (_dir, config, mut task) = setup();
        task.originals = Originals::Move;
        save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
        let original = text_pdf();
        let path = task.input.join("source.pdf");
        fs::write(&path, original.as_slice()).unwrap();
        // Simulate a crash after outputs and archive copy, before removing the original.
        fs::write(task.output.join("source.pdf"), original.as_slice()).unwrap();
        fs::write(task.archive.join("source.pdf"), original.as_slice()).unwrap();
        let hash: String = Sha256::digest(original.as_slice()).iter().map(|b| format!("{b:02x}")).collect();
        let key = format!("{}:{}:{hash}", task.id, confined(&path, None).unwrap().display());
        let mut state = State::default();
        state.receipts.insert(
            key.clone(),
            Receipt {
                outputs: vec![task.output.join("source.pdf")],
                source_hash: hash,
                source_stamp: fingerprint(&path).unwrap(),
                output_proofs: vec![output_proof(&task.output.join("source.pdf"), None).unwrap()],
                cleanup: Some(PendingLocalAction::Move { archive: confined(&task.archive, None).unwrap(), relative: "source.pdf".into() }),
            },
        );
        let observation_key = format!("{}:{}", task.id, confined(&path, None).unwrap().display());
        state.observations.insert(observation_key, Observation { fingerprint: fingerprint(&path).unwrap(), since: 1, attempted: true });
        settled(&task, &mut state, &config).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&task.archive).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&task.output).unwrap().count(), 1);
        assert!(read_state(&config).unwrap().receipts[&key].cleanup.is_none());
    }

    #[test]
    fn hot_folder_cleanup_recovery_refuses_missing_replaced_and_legacy_outputs() {
        for corruption in ["missing", "replaced", "legacy"] {
            let (_dir, config, mut task) = setup();
            task.originals = Originals::Delete;
            save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
            let original = text_pdf();
            let source = task.input.join("source.pdf");
            let output = task.output.join("source.pdf");
            fs::write(&source, original.as_slice()).unwrap();
            fs::write(&output, original.as_slice()).unwrap();
            let proof = output_proof(&output, None).unwrap();
            let hash: String = Sha256::digest(original.as_slice()).iter().map(|b| format!("{b:02x}")).collect();
            let key = format!("{}:{}", task.id, confined(&source, None).unwrap().display());
            let stamp = fingerprint(&source).unwrap();
            let mut state = State::default();
            state.receipts.insert(
                format!("{key}:{hash}"),
                Receipt {
                    outputs: vec![output.clone()],
                    source_hash: hash,
                    source_stamp: stamp.clone(),
                    output_proofs: if corruption == "legacy" { vec![] } else { vec![proof] },
                    cleanup: Some(PendingLocalAction::Delete),
                },
            );
            state.observations.insert(key, Observation { fingerprint: stamp, since: 1, attempted: true });
            match corruption {
                "missing" => fs::remove_file(&output).unwrap(),
                "replaced" => fs::write(&output, b"a different result").unwrap(),
                _ => {}
            }
            assert!(step(&task, &mut state, 10, &config, None, &|| false).is_err());
            assert_eq!(fs::read(source).unwrap(), *original);
        }
    }

    #[test]
    fn hot_folder_replacement_during_publication_cannot_become_the_expected_output() {
        let (_dir, config, mut task) = setup();
        task.originals = Originals::Delete;
        task.settings.recognize = false;
        task.settings.auto_rotate = false;
        task.settings.deskew = false;
        task.output_plan.steps.push(output::SaveStep { format: output::Format::Txt, ..Default::default() });
        save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
        let original = text_pdf();
        let source = task.input.join("source.pdf");
        let output = task.output.join("source.pdf");
        fs::write(&source, original.as_slice()).unwrap();
        let mut state = State::default();
        step(&task, &mut state, 10, &config, None, &|| false).unwrap();
        let replaced = std::cell::Cell::new(false);
        let interfere = || {
            if output.exists() && !replaced.replace(true) {
                fs::write(&output, b"replaced before all saving steps completed").unwrap();
            }
            false
        };
        let error = step(&task, &mut state, 13, &config, None, &interfere).unwrap_err();
        assert!(error.contains("no longer matches"), "{error}");
        assert!(state.receipts.is_empty());
        assert_eq!(fs::read(source).unwrap(), *original);
    }

    #[test]
    fn hot_folder_project_recovery_verifies_every_output_before_any_acknowledgement() {
        let (dir, config, mut task) = setup();
        let originals = dir.path().join("originals");
        fs::create_dir(&originals).unwrap();
        let original = text_pdf();
        for name in ["z.pdf", "a.pdf"] {
            fs::write(originals.join(name), original.as_slice()).unwrap();
        }
        let manifest = originals.join("project.json");
        sources::write_project_manifest(
            &manifest,
            &sources::ProjectManifest { files: vec!["z.pdf".into(), "a.pdf".into()], ..Default::default() },
            None,
        )
        .unwrap();
        task.source = sources::SourceConfig::Project(sources::ProjectSource { manifest, action: sources::SourceAction::Delete });
        task.settings.recognize = false;
        task.settings.auto_rotate = false;
        task.settings.deskew = false;
        task.output_plan.steps = vec![output::SaveStep { grouping: output::Grouping::All, ..Default::default() }];
        save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
        let mut state = State::default();
        step(&task, &mut state, 10, &config, None, &|| false).unwrap();
        assert!(step(&task, &mut state, 13, &config, None, &|| state_path(&config).exists()).is_err());
        assert_eq!(state.receipts.len(), 2);
        let output = task.output.join("Scan task.pdf");
        let saved = fs::read(&output).unwrap();
        fs::write(&output, b"replaced output").unwrap();
        let mut restored = read_state(&config).unwrap();
        assert!(step(&task, &mut restored, 16, &config, None, &|| false).is_err());
        assert!(originals.join("z.pdf").exists() && originals.join("a.pdf").exists());
        fs::write(&output, saved).unwrap();
        step(&task, &mut restored, 19, &config, None, &|| false).unwrap();
        assert!(!originals.join("z.pdf").exists() && !originals.join("a.pdf").exists());
        assert_eq!(fs::read_dir(&task.output).unwrap().count(), 1);
    }

    #[test]
    fn hot_folder_pausing_or_changing_policy_during_preprocessing_leaves_the_input_retryable() {
        for policy_change in [false, true] {
            let (_dir, config, mut task) = setup();
            task.originals = Originals::Delete;
            task.preprocessing.grayscale = true;
            task.settings.recognize = false;
            task.settings.auto_rotate = false;
            task.settings.deskew = false;
            task.settings.dpi = 72.0;
            save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
            fs::write(task.input.join("source.pdf"), text_pdf().as_slice()).unwrap();
            let mut state = State::default();
            step(&task, &mut state, 10, &config, None, &|| false).unwrap();
            let calls = std::cell::Cell::new(0);
            let pause = || {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    let mut changed = task.clone();
                    if policy_change {
                        changed.originals = Originals::Keep;
                    } else {
                        changed.enabled = false;
                    }
                    save_config(&config, &Config { tasks: vec![changed] }, None).unwrap();
                }
                false
            };
            assert!(step(&task, &mut state, 13, &config, None, &pause).is_err());
            assert!(state.observations.values().all(|observation| !observation.attempted));
            assert!(task.input.join("source.pdf").exists());
            if policy_change {
                task.originals = Originals::Keep;
            }
            save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
            step(&task, &mut state, 16, &config, None, &|| false).unwrap();
            assert!(task.output.join("source.pdf").exists());
            assert_eq!(task.input.join("source.pdf").exists(), policy_change);
        }
    }

    #[test]
    fn hot_folder_recurses_and_makes_real_scans_searchable() {
        if !ocr::available() {
            return;
        }
        let (_dir, config, mut task) = setup();
        task.recursive = true;
        task.settings.auto_rotate = false;
        task.settings.deskew = false;
        let mut s = Session::new();
        save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
        let id = s.open("text", None, text_pdf(), None).unwrap();
        let png = crate::export::Exporter::new(s.get(id).unwrap()).png(0, 150.0).unwrap();
        fs::create_dir(task.input.join("child")).unwrap();
        fs::write(task.input.join("child/scan.png"), png).unwrap();
        let mut state = State::default();
        settled(&task, &mut state, &config).unwrap();
        let bytes = fs::read(task.output.join("child/scan.pdf")).unwrap();
        let id = s.open("result", None, Arc::new(bytes), None).unwrap();
        assert!(crate::export::Exporter::new(s.get(id).unwrap()).text(0).unwrap().contains("searchable"));
    }

    #[test]
    fn hot_folder_recognition_regions_exclude_new_text_without_removing_visible_content() {
        if !ocr::available() {
            return;
        }
        let (_dir, config, mut task) = setup();
        task.settings.auto_rotate = false;
        task.settings.deskew = false;
        task.preprocessing.regions.push(preprocess::Region { exclude: true, ..Default::default() });
        save_config(&config, &Config { tasks: vec![task.clone()] }, None).unwrap();
        let mut session = Session::new();
        let id = session.open("source", None, text_pdf(), None).unwrap();
        let png = crate::export::Exporter::new(session.get(id).unwrap()).png(0, 150.0).unwrap();
        fs::write(task.input.join("scan.png"), &png).unwrap();
        let mut state = State::default();
        settled(&task, &mut state, &config).unwrap();
        let bytes = fs::read(task.output.join("scan.pdf")).unwrap();
        let id = session.open("output", None, Arc::new(bytes), None).unwrap();
        let mut export = crate::export::Exporter::new(session.get(id).unwrap());
        assert!(export.text(0).unwrap().trim().is_empty());
        let rendered = image::load_from_memory(&export.png(0, 72.0).unwrap()).unwrap().to_rgb8();
        assert!(rendered.pixels().any(|p| p.0.iter().any(|v| *v < 200)));
    }

    #[test]
    fn hot_folder_stops_without_writing_partial_output() {
        if !ocr::available() {
            return;
        }
        let (_dir, config, task) = setup();
        fs::write(task.input.join("source.pdf"), text_pdf().as_slice()).unwrap();
        let mut state = State::default();
        step(&task, &mut state, 10, &config, None, &|| false).unwrap();
        assert!(step(&task, &mut state, 13, &config, None, &|| true).unwrap_err().contains("stopped"));
        assert!(fs::read_dir(&task.output).unwrap().next().is_none());
        assert!(task.input.join("source.pdf").exists());
    }

    #[test]
    fn hot_folder_schedule_lock_and_root_are_enforced() {
        let (dir, config, mut task) = setup();
        task.start_at = now() + 3600;
        save_config(&config, &Config { tasks: vec![task] }, None).unwrap();
        assert!(poll(&config, Some(dir.path())).unwrap().tasks.is_empty());
        let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(config.with_extension("lock")).unwrap();
        lock.lock().unwrap();
        assert!(poll(&config, None).unwrap_err().contains("another"));
        assert!(confined(&dir.path().join("../outside.pdf"), Some(dir.path())).is_err());
    }

    #[test]
    fn blank_page_removal_keeps_text_and_refuses_an_empty_document() {
        let s = Session::new();
        assert!(remove_blank_pages(s.create_blank(100.0, 100.0, 2).unwrap()).unwrap_err().contains("all pages"));
        let text = text_pdf();
        assert_eq!(remove_blank_pages(text.clone()).unwrap(), text);
    }
}
