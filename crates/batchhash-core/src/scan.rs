//! Filesystem scanning shared by the CLI, the Flutter bridge and tests.
//!
//! The scanner produces unhashed [FileEntry] values with forward-slash relative
//! paths, so the same entry list can flow into the hash scheduler, the report
//! builder and the rename planner without platform-specific path handling.
//!
//! Rules (see docs/file-entry-root-spec.md):
//!
//! * the batch root is a single directory (or a single file for the one-file case),
//! * directory entries are visited in sorted order so the entry list is deterministic,
//! * symlinks are skipped unless ScanOptions::follow_links is set; directory loops
//!   are detected with a visited set,
//! * an unreadable child becomes a failed entry instead of aborting the batch,
//! * paths that cannot be represented as report-relative paths are reported in
//!   ScanOutcome::skipped instead of being silently dropped or mangled.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use batchhash_types::{validate_relative_path, EntryStatus, ErrorKind, FileEntry, RootKind};

/// Maximum directory depth the scanner walks before reporting an error.
pub const DEFAULT_MAX_DEPTH: usize = 64;

/// Scanner behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    /// Follow symbolic links (files and directories). Loops are still detected.
    pub follow_links: bool,
    /// Maximum directory depth; deeper subtrees produce an error entry.
    pub max_depth: usize,
    /// Root identifier copied into every entry (reports only use one root).
    pub root_id: String,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_links: false,
            max_depth: DEFAULT_MAX_DEPTH,
            root_id: "root".to_string(),
        }
    }
}

impl ScanOptions {
    /// Create default options for a root.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable symlink following.
    pub fn with_follow_links(mut self, follow_links: bool) -> Self {
        self.follow_links = follow_links;
        self
    }
}

/// One path that the scanner could not represent as a batch entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSkip {
    /// Absolute (display) path that was skipped.
    pub path: String,
    /// Human-readable reason.
    pub reason: String,
}

/// Result of a scan.
#[derive(Debug, Clone)]
pub struct ScanOutcome {
    /// Unhashed entries in deterministic path order.
    pub entries: Vec<FileEntry>,
    /// Root kind of this scan (filesystem for every current platform except SAF).
    pub root_kind: RootKind,
    /// Paths that were skipped because they cannot be represented in a report.
    pub skipped: Vec<ScanSkip>,
}

/// Failures that abort a scan before any entry is produced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScanError {
    /// The root itself cannot be read.
    #[error("cannot read batch root {path}: {message}")]
    Root {
        /// Root path as displayed to the user.
        path: String,
        /// Underlying operating-system error.
        message: String,
    },
    /// The root is neither a regular file nor a directory.
    #[error("batch root {path} is neither a file nor a directory")]
    UnsupportedRoot {
        /// Root path as displayed to the user.
        path: String,
    },
}

/// Scan a root and return unhashed entries.
///
/// A single file root produces exactly one entry whose relative path is the
/// file name.
pub fn scan(root: &Path, options: &ScanOptions) -> Result<ScanOutcome, ScanError> {
    let metadata = fs::metadata(root).map_err(|error| ScanError::Root {
        path: display_path(root),
        message: error.to_string(),
    })?;
    let mut outcome = ScanOutcome {
        entries: Vec::new(),
        root_kind: RootKind::Fs,
        skipped: Vec::new(),
    };
    if metadata.is_file() {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| display_path(root));
        push_entry(root, &name, &metadata, options, &mut outcome);
        return Ok(outcome);
    }
    if !metadata.is_dir() {
        return Err(ScanError::UnsupportedRoot {
            path: display_path(root),
        });
    }
    let mut visited = BTreeSet::new();
    if let Ok(canonical) = fs::canonicalize(root) {
        visited.insert(canonical);
    } else {
        visited.insert(root.to_path_buf());
    }
    walk(root, root, options, 0, &mut visited, &mut outcome);
    outcome
        .entries
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(outcome)
}

/// Convert an OS path into the forward-slash relative form used by reports.
pub fn relative_slash(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .filter(|part| !part.is_empty() && part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Display form of an operating-system path.
pub fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn rfc3339_from(time: Option<SystemTime>) -> Option<String> {
    let time = time?;
    let converted: chrono::DateTime<chrono::Utc> = time.into();
    Some(converted.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn push_entry(
    path: &Path,
    relative: &str,
    metadata: &fs::Metadata,
    options: &ScanOptions,
    outcome: &mut ScanOutcome,
) {
    if let Err(error) = validate_relative_path(relative) {
        outcome.skipped.push(ScanSkip {
            path: display_path(path),
            reason: error.to_string(),
        });
        return;
    }
    let root_kind = outcome.root_kind;
    outcome.entries.push(FileEntry::new(
        options.root_id.clone(),
        root_kind,
        relative.to_string(),
        display_path(path),
        metadata.len(),
        rfc3339_from(metadata.modified().ok()),
    ));
}

fn push_failure(
    path: &Path,
    relative: &str,
    kind: ErrorKind,
    message: String,
    options: &ScanOptions,
    outcome: &mut ScanOutcome,
) {
    if validate_relative_path(relative).is_err() {
        outcome.skipped.push(ScanSkip {
            path: display_path(path),
            reason: message,
        });
        return;
    }
    let root_kind = outcome.root_kind;
    let mut entry = FileEntry::new(
        options.root_id.clone(),
        root_kind,
        relative.to_string(),
        display_path(path),
        0,
        None,
    );
    entry.status = EntryStatus::Failed;
    entry.error_kind = Some(kind);
    entry.error_message = Some(message);
    outcome.entries.push(entry);
}

fn walk(
    root: &Path,
    current: &Path,
    options: &ScanOptions,
    depth: usize,
    visited: &mut BTreeSet<PathBuf>,
    outcome: &mut ScanOutcome,
) {
    if depth > options.max_depth {
        let relative = current
            .strip_prefix(root)
            .map(relative_slash)
            .unwrap_or_else(|_| relative_slash(current));
        push_failure(
            current,
            &relative,
            ErrorKind::Unsupported,
            format!("directory tree deeper than {} levels", options.max_depth),
            options,
            outcome,
        );
        return;
    }
    let read_dir = match fs::read_dir(current) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            let relative = current
                .strip_prefix(root)
                .map(relative_slash)
                .unwrap_or_else(|_| relative_slash(current));
            let relative = if relative.is_empty() {
                ".".to_string()
            } else {
                relative
            };
            push_failure(
                current,
                &relative,
                ErrorKind::PermissionDenied,
                error.to_string(),
                options,
                outcome,
            );
            return;
        }
    };
    let mut children: Vec<_> = read_dir.filter_map(|item| item.ok()).collect();
    children.sort_by_key(|item| item.file_name());
    for child in children {
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map(relative_slash)
            .unwrap_or_else(|_| child.file_name().to_string_lossy().into_owned());
        let file_type = match child.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                push_failure(
                    &path,
                    &relative,
                    ErrorKind::IoError,
                    error.to_string(),
                    options,
                    outcome,
                );
                continue;
            }
        };
        if file_type.is_symlink() && !options.follow_links {
            continue;
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                push_failure(
                    &path,
                    &relative,
                    ErrorKind::NotFound,
                    error.to_string(),
                    options,
                    outcome,
                );
                continue;
            }
        };
        if metadata.is_dir() {
            let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !visited.insert(key) {
                // Symlink loop or an already visited subtree.
                continue;
            }
            walk(root, &path, options, depth + 1, visited, outcome);
            continue;
        }
        push_entry(&path, &relative, &metadata, options, outcome);
    }
}
