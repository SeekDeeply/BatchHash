//! Pure dry-run rename planner and executor.
//!
//! [`plan_renames`] never touches the filesystem for writes: it inspects the
//! entries, the suffix format and a [`RenameFs`] implementation and returns one
//! [`RenamePlanRow`] per eligible entry with a machine-readable status. Actual
//! renames go through [`execute_plan`] and a [`RenameBackend`], so Android can
//! substitute its Kotlin/SAF backend for [`StdFsBackend`].
//!
//! The planner deliberately reports conflicts instead of resolving them: an
//! existing target, two planned targets that collapse to the same name
//! (optionally case-insensitively) and a read-only directory all surface as
//! non-[`RenameRowStatus::Ready`] rows.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use batchhash_types::{is_hex_digest_of_len, FileEntry};
use serde::{Deserialize, Serialize};

use crate::suffix::{
    format_filename, parse_filename, SuffixError, SuffixFormat, SuffixParseResult,
    DEFAULT_MAX_UTF16_LEN,
};

/// Read-only filesystem probe used by the pure planner.
///
/// Implementations must be side-effect free: [`RenameFs::exists`] and
/// [`RenameFs::is_dir_writable`] are queries, never mutations.
pub trait RenameFs {
    /// Whether [`path`] currently exists.
    ///
    /// Implementations should return [`false`] when the check itself fails
    /// (for example on a permission error) rather than panicking.
    fn exists(&self, path: &Path) -> bool;

    /// Whether new entries can be created in [`dir`].
    ///
    /// Implementations should return [`false`] when the directory is missing,
    /// is not a directory, is read-only or cannot be inspected.
    fn is_dir_writable(&self, dir: &Path) -> bool;

    /// Whether this filesystem can rename [`path`].
    ///
    /// The default is [`true`]; backends that cannot rename (some Android SAF
    /// documents providers) return [`false`] so the planner can emit
    /// [`RenameRowStatus::Unsupported`].
    fn supports_rename(&self, _path: &Path) -> bool {
        true
    }
}

/// Standard-library [`RenameFs`] implementation backed by [`std::fs`].
#[derive(Debug, Clone, Copy, Default)]
pub struct StdRenameFs;

impl StdRenameFs {
    /// Create a standard filesystem probe.
    pub fn new() -> Self {
        Self
    }
}

impl RenameFs for StdRenameFs {
    fn exists(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    fn is_dir_writable(&self, dir: &Path) -> bool {
        match std::fs::metadata(dir) {
            Ok(metadata) => metadata.is_dir() && !metadata.permissions().readonly(),
            Err(_) => false,
        }
    }
}

/// Options controlling [`plan_renames`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlanOptions {
    /// Suffix format applied to every planned filename.
    pub format: SuffixFormat,
    /// When true, an existing target file is not reported as a conflict.
    /// Defaults to false because overwriting is destructive.
    pub overwrite: bool,
    /// When true, planned targets that differ only by Unicode case are treated
    /// as conflicts. Defaults to true so case-insensitive filesystems are safe.
    pub case_insensitive_conflicts: bool,
}

impl Default for RenamePlanOptions {
    fn default() -> Self {
        Self {
            format: SuffixFormat::default(),
            overwrite: false,
            case_insensitive_conflicts: true,
        }
    }
}

/// Per-row outcome of the dry-run planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameRowStatus {
    /// The row can be executed.
    Ready,
    /// The proposed target already exists or another row proposes the same
    /// target (also case-insensitively when configured).
    ConflictTargetExists,
    /// The original filename already carries a parseable suffix.
    SkippedAlreadySuffixed,
    /// The entry has no usable hash for the configured suffix algorithm.
    MissingHash,
    /// The original filename cannot be used (unparseable bracket group or a
    /// name that cannot be formatted within the platform limit).
    InvalidName,
    /// The containing directory is not writable.
    NoPermission,
    /// The filesystem or platform cannot rename this entry.
    Unsupported,
    /// An unexpected formatting or configuration error occurred.
    Error,
}

/// One row of a dry-run rename plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlanRow {
    /// Index of the source entry in the slice passed to [`plan_renames`].
    pub entry_index: usize,
    /// Original path as stored in [`FileEntry::display_path`].
    pub original_path: String,
    /// Original file name (last path component) without any directory.
    pub original_name: String,
    /// Proposed new file name, or an empty string when no name could be built.
    pub proposed_name: String,
    /// Machine-readable outcome of planning.
    pub status: RenameRowStatus,
    /// Human-readable explanation for non-ready rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Aggregate counts for a [`RenamePlan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RenamePlanSummary {
    /// Number of rows in the plan (non-ok entries are excluded).
    pub total: usize,
    /// Rows with [`RenameRowStatus::Ready`].
    pub ready: usize,
    /// Rows with [`RenameRowStatus::ConflictTargetExists`].
    pub conflicts: usize,
    /// Rows with [`RenameRowStatus::SkippedAlreadySuffixed`].
    pub skipped: usize,
    /// Rows with [`RenameRowStatus::MissingHash`].
    pub missing_hash: usize,
    /// Rows with [`RenameRowStatus::InvalidName`].
    pub invalid_name: usize,
    /// Rows with [`RenameRowStatus::NoPermission`].
    pub no_permission: usize,
    /// Rows with [`RenameRowStatus::Unsupported`].
    pub unsupported: usize,
    /// Rows with [`RenameRowStatus::Error`].
    pub errors: usize,
}

/// Dry-run rename plan. [`RenamePlan::dry_run`] is always true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlan {
    /// One row per eligible entry, in source-entry order.
    pub rows: Vec<RenamePlanRow>,
    /// Aggregate counts derived from [`RenamePlan::rows`].
    pub summary: RenamePlanSummary,
    /// Always true: planning performs no filesystem writes.
    pub dry_run: bool,
}

/// Build a pure dry-run rename plan.
///
/// Entries whose [`FileEntry::status`] is not [`batchhash_types::EntryStatus::Ok`]
/// are skipped entirely. Rows are produced for every remaining entry, even when
/// they cannot be renamed, so the UI can explain each outcome. The
/// implementation performs reads through [`RenameFs`] only and never writes.
pub fn plan_renames(
    entries: &[FileEntry],
    options: &RenamePlanOptions,
    fs: &dyn RenameFs,
) -> RenamePlan {
    if let Err(error) = options.format.validate() {
        let mut rows = Vec::new();
        for (entry_index, entry) in entries.iter().enumerate() {
            if !entry.status.is_ok() {
                continue;
            }
            rows.push(RenamePlanRow {
                entry_index,
                original_path: entry.display_path.clone(),
                original_name: file_name_of(&entry.display_path).unwrap_or_default(),
                proposed_name: String::new(),
                status: RenameRowStatus::Error,
                reason: Some(format!("invalid suffix configuration: {error}")),
            });
        }
        let summary = summarize(&rows);
        return RenamePlan {
            rows,
            summary,
            dry_run: true,
        };
    }

    let mut rows: Vec<RenamePlanRow> = Vec::new();
    for (entry_index, entry) in entries.iter().enumerate() {
        if !entry.status.is_ok() {
            continue;
        }
        rows.push(build_row(entry_index, entry, options));
    }

    for row in &mut rows {
        if row.status == RenameRowStatus::Ready
            && !fs.supports_rename(Path::new(&row.original_path))
        {
            row.status = RenameRowStatus::Unsupported;
            row.reason = Some("filesystem backend does not support renaming".to_string());
        }
    }

    let mut target_counts: HashMap<String, usize> = HashMap::new();
    for row in &rows {
        if row.status == RenameRowStatus::Ready {
            *target_counts
                .entry(target_key(row, options.case_insensitive_conflicts))
                .or_insert(0) += 1;
        }
    }
    for row in &mut rows {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        let count = target_counts
            .get(&target_key(row, options.case_insensitive_conflicts))
            .copied()
            .unwrap_or(0);
        if count > 1 {
            row.status = RenameRowStatus::ConflictTargetExists;
            row.reason = Some("another entry in this plan proposes the same target".to_string());
        }
    }

    for row in &mut rows {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        let dir = parent_dir(&row.original_path);
        if !fs.is_dir_writable(dir) {
            row.status = RenameRowStatus::NoPermission;
            row.reason = Some(format!("directory is not writable: {}", dir.display()));
            continue;
        }
        if !options.overwrite && fs.exists(&target_path(&row.original_path, &row.proposed_name)) {
            row.status = RenameRowStatus::ConflictTargetExists;
            row.reason = Some("target name already exists".to_string());
        }
    }

    let summary = summarize(&rows);
    RenamePlan {
        rows,
        summary,
        dry_run: true,
    }
}

/// Result of one attempted rename in [`execute_plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameResult {
    /// Index of the source entry in the slice passed to [`plan_renames`].
    pub entry_index: usize,
    /// Original path from the plan row.
    pub original_path: String,
    /// Target path the backend was asked to rename to.
    pub new_path: String,
    /// Execution outcome for this row.
    pub status: RenameResultStatus,
    /// Backend error message for failed rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Execution outcome of one rename attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameResultStatus {
    /// The backend renamed the file.
    Renamed,
    /// The backend rejected or failed the rename.
    Failed,
}

/// Platform rename backend.
///
/// The desktop implementation is [`StdFsBackend`]; Android supplies a backend
/// that calls the Kotlin/SAF rename API.
pub trait RenameBackend {
    /// Rename or move [`from`] to [`to`].
    ///
    /// Implementations must not fall back to copy-and-delete; a single atomic
    /// rename (or the platform equivalent) is required.
    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), RenameError>;
}

/// Standard-library [`RenameBackend`] using [`std::fs::rename`].
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFsBackend;

impl StdFsBackend {
    /// Create a standard filesystem rename backend.
    pub fn new() -> Self {
        Self
    }
}

impl RenameBackend for StdFsBackend {
    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), RenameError> {
        std::fs::rename(from, to).map_err(|error| RenameError::from_io(from, &error))
    }
}

/// Backend rename failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenameError {
    /// The source path does not exist.
    #[error("source file not found: {0}")]
    NotFound(String),
    /// The process is not allowed to rename the path.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// The target path already exists and overwriting was not allowed.
    #[error("target already exists: {0}")]
    AlreadyExists(String),
    /// The platform cannot rename this kind of path.
    #[error("rename not supported: {0}")]
    Unsupported(String),
    /// Any other I/O failure.
    #[error("rename failed: {0}")]
    Io(String),
}

impl RenameError {
    /// Build a backend-independent error from a standard I/O error.
    pub fn from_io(path: &Path, error: &std::io::Error) -> Self {
        let detail = format!("{}: {error}", path.display());
        match error.kind() {
            std::io::ErrorKind::NotFound => RenameError::NotFound(detail),
            std::io::ErrorKind::PermissionDenied => RenameError::PermissionDenied(detail),
            std::io::ErrorKind::AlreadyExists => RenameError::AlreadyExists(detail),
            std::io::ErrorKind::Unsupported => RenameError::Unsupported(detail),
            _ => RenameError::Io(detail),
        }
    }

    /// Stable machine-readable error kind, for the rename log error_kind column.
    pub fn kind_str(&self) -> &'static str {
        match self {
            RenameError::NotFound(_) => "not_found",
            RenameError::PermissionDenied(_) => "permission_denied",
            RenameError::AlreadyExists(_) => "already_exists",
            RenameError::Unsupported(_) => "unsupported",
            RenameError::Io(_) => "io_error",
        }
    }
}

/// Execute every [`RenameRowStatus::Ready`] row of a plan through [`backend`].
///
/// Non-ready rows are not attempted and produce no result, so the returned
/// vector contains at most one result per ready row, in plan order. The plan
/// itself is never modified.
pub fn execute_plan(plan: &RenamePlan, backend: &mut dyn RenameBackend) -> Vec<RenameResult> {
    let mut results = Vec::new();
    for row in &plan.rows {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        let from = Path::new(&row.original_path);
        let to = target_path(&row.original_path, &row.proposed_name);
        match backend.rename(from, &to) {
            Ok(()) => results.push(RenameResult {
                entry_index: row.entry_index,
                original_path: row.original_path.clone(),
                new_path: to.to_string_lossy().into_owned(),
                status: RenameResultStatus::Renamed,
                error: None,
            }),
            Err(error) => results.push(RenameResult {
                entry_index: row.entry_index,
                original_path: row.original_path.clone(),
                new_path: to.to_string_lossy().into_owned(),
                status: RenameResultStatus::Failed,
                error: Some(error.to_string()),
            }),
        }
    }
    results
}

fn build_row(entry_index: usize, entry: &FileEntry, options: &RenamePlanOptions) -> RenamePlanRow {
    let original_path = entry.display_path.clone();
    let Some(original_name) = file_name_of(&original_path) else {
        return RenamePlanRow {
            entry_index,
            original_path,
            original_name: String::new(),
            proposed_name: String::new(),
            status: RenameRowStatus::InvalidName,
            reason: Some("path has no file name".to_string()),
        };
    };
    match parse_filename(&original_name) {
        SuffixParseResult::Parsed(_) => {
            return RenamePlanRow {
                entry_index,
                original_path,
                original_name,
                proposed_name: String::new(),
                status: RenameRowStatus::SkippedAlreadySuffixed,
                reason: Some("filename already carries a parseable suffix".to_string()),
            };
        }
        SuffixParseResult::Unparseable => {
            return RenamePlanRow {
                entry_index,
                original_path,
                original_name,
                proposed_name: String::new(),
                status: RenameRowStatus::InvalidName,
                reason: Some(
                    "filename has a bracket group that is not a valid suffix".to_string(),
                ),
            };
        }
        SuffixParseResult::NotSuffixed => {}
    }

    let algorithm = options.format.algorithm;
    let Some(digest) = entry.hashes.get(&algorithm) else {
        return RenamePlanRow {
            entry_index,
            original_path,
            original_name,
            proposed_name: String::new(),
            status: RenameRowStatus::MissingHash,
            reason: Some(format!("entry has no {algorithm} hash")),
        };
    };
    if !is_hex_digest_of_len(digest, algorithm.hex_len()) {
        return RenamePlanRow {
            entry_index,
            original_path,
            original_name,
            proposed_name: String::new(),
            status: RenameRowStatus::MissingHash,
            reason: Some(format!("entry hash for {algorithm} is malformed")),
        };
    }

    match format_filename(
        &original_name,
        digest,
        &options.format,
        DEFAULT_MAX_UTF16_LEN,
    ) {
        Ok(proposed_name) => RenamePlanRow {
            entry_index,
            original_path,
            original_name,
            proposed_name,
            status: RenameRowStatus::Ready,
            reason: None,
        },
        Err(error) => {
            let reason = error.to_string();
            let status = match error {
                SuffixError::AlreadySuffixed => RenameRowStatus::SkippedAlreadySuffixed,
                SuffixError::NameTooLong { .. }
                | SuffixError::UnparseableName
                | SuffixError::EmptyName => RenameRowStatus::InvalidName,
                SuffixError::DigestTooShort { .. } | SuffixError::InvalidDigest => {
                    RenameRowStatus::MissingHash
                }
                SuffixError::InvalidLength(_) | SuffixError::AlgorithmNotSuffixUsable(_) => {
                    RenameRowStatus::Error
                }
            };
            RenamePlanRow {
                entry_index,
                original_path,
                original_name,
                proposed_name: String::new(),
                status,
                reason: Some(reason),
            }
        }
    }
}

fn summarize(rows: &[RenamePlanRow]) -> RenamePlanSummary {
    let mut summary = RenamePlanSummary::default();
    for row in rows {
        summary.total += 1;
        match row.status {
            RenameRowStatus::Ready => summary.ready += 1,
            RenameRowStatus::ConflictTargetExists => summary.conflicts += 1,
            RenameRowStatus::SkippedAlreadySuffixed => summary.skipped += 1,
            RenameRowStatus::MissingHash => summary.missing_hash += 1,
            RenameRowStatus::InvalidName => summary.invalid_name += 1,
            RenameRowStatus::NoPermission => summary.no_permission += 1,
            RenameRowStatus::Unsupported => summary.unsupported += 1,
            RenameRowStatus::Error => summary.errors += 1,
        }
    }
    summary
}

fn file_name_of(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn parent_dir(original_path: &str) -> &Path {
    Path::new(original_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn target_path(original_path: &str, proposed_name: &str) -> PathBuf {
    parent_dir(original_path).join(proposed_name)
}

fn target_key(row: &RenamePlanRow, case_insensitive: bool) -> String {
    let target = target_path(&row.original_path, &row.proposed_name)
        .to_string_lossy()
        .into_owned();
    if case_insensitive {
        target.to_lowercase()
    } else {
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use batchhash_types::{AlgorithmId, EntryStatus, ErrorKind, RootKind};
    use crate::suffix::SuffixFormat;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn digest() -> String {
        "abcdef0123456789".repeat(4)
    }

    fn make_entry(path: &Path, with_blake3: bool) -> FileEntry {
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let display = path.to_string_lossy().into_owned();
        let mut entry = FileEntry::new("root", RootKind::Fs, name, display, 0, None);
        if with_blake3 {
            entry.hashes.insert(AlgorithmId::Blake3, digest());
        }
        entry
    }

    struct NoRenameFs;

    impl RenameFs for NoRenameFs {
        fn exists(&self, _path: &Path) -> bool {
            false
        }

        fn is_dir_writable(&self, _dir: &Path) -> bool {
            true
        }

        fn supports_rename(&self, _path: &Path) -> bool {
            false
        }
    }

    #[cfg(unix)]
    fn set_dir_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(mode);
        fs::set_permissions(path, permissions).expect("directory permissions must be settable");
    }

    #[test]
    fn planner_is_dry_run_and_ready_rows_do_not_write() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("a.txt");
        fs::write(&source, b"hello").expect("source file");
        let entry = make_entry(&source, true);

        let plan = plan_renames(&[entry], &RenamePlanOptions::default(), &StdRenameFs);

        assert!(plan.dry_run);
        assert_eq!(plan.rows.len(), 1);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
        assert_eq!(plan.rows[0].proposed_name, "a[ABCDE].txt");
        assert_eq!(plan.summary.ready, 1);
        assert_eq!(plan.summary.total, 1);
        assert!(source.exists(), "dry run must not rename the source");
        assert!(!temp.path().join("a[ABCDE].txt").exists());
    }

    #[test]
    fn planner_reports_existing_target_conflicts_and_honors_overwrite() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("a.txt");
        let target = temp.path().join("a[ABCDE].txt");
        fs::write(&source, b"hello").expect("source file");
        fs::write(&target, b"other").expect("target file");
        let entry = make_entry(&source, true);

        let plan = plan_renames(&[entry], &RenamePlanOptions::default(), &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::ConflictTargetExists);
        assert!(plan.rows[0].reason.is_some());
        assert_eq!(plan.summary.conflicts, 1);

        let overwrite = RenamePlanOptions {
            overwrite: true,
            ..RenamePlanOptions::default()
        };
        let plan = plan_renames(&[make_entry(&source, true)], &overwrite, &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
    }

    #[test]
    fn planner_detects_duplicate_targets_case_insensitively() {
        let temp = tempdir().expect("tempdir");
        let upper = make_entry(&temp.path().join("A.txt"), true);
        let lower = make_entry(&temp.path().join("a.txt"), true);

        let plan = plan_renames(&[upper, lower], &RenamePlanOptions::default(), &StdRenameFs);
        assert_eq!(plan.rows.len(), 2);
        assert_eq!(plan.rows[0].status, RenameRowStatus::ConflictTargetExists);
        assert_eq!(plan.rows[1].status, RenameRowStatus::ConflictTargetExists);

        let case_sensitive = RenamePlanOptions {
            case_insensitive_conflicts: false,
            ..RenamePlanOptions::default()
        };
        let upper = make_entry(&temp.path().join("A.txt"), true);
        let lower = make_entry(&temp.path().join("a.txt"), true);
        let plan = plan_renames(&[upper, lower], &case_sensitive, &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
        assert_eq!(plan.rows[1].status, RenameRowStatus::Ready);
    }

    #[test]
    fn planner_detects_exact_duplicate_targets() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a.txt");
        let plan = plan_renames(
            &[make_entry(&path, true), make_entry(&path, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(plan.rows[0].status, RenameRowStatus::ConflictTargetExists);
        assert_eq!(plan.rows[1].status, RenameRowStatus::ConflictTargetExists);
    }

    #[test]
    fn planner_skips_already_suffixed_names() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a[ABCDE].txt");
        let plan = plan_renames(
            &[make_entry(&path, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(
            plan.rows[0].status,
            RenameRowStatus::SkippedAlreadySuffixed
        );
        assert_eq!(plan.summary.skipped, 1);
    }

    #[test]
    fn planner_reports_missing_or_malformed_hashes() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a.txt");

        let mut wrong_algorithm = make_entry(&path, false);
        wrong_algorithm
            .hashes
            .insert(AlgorithmId::Sha256, "a".repeat(64));
        let plan = plan_renames(
            &[wrong_algorithm],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(plan.rows[0].status, RenameRowStatus::MissingHash);

        let mut malformed = make_entry(&path, false);
        malformed
            .hashes
            .insert(AlgorithmId::Blake3, "abc".to_string());
        let plan = plan_renames(&[malformed], &RenamePlanOptions::default(), &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::MissingHash);
    }

    #[test]
    fn planner_reports_unparseable_and_missing_file_names() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a[zz].txt");
        let plan = plan_renames(
            &[make_entry(&path, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(plan.rows[0].status, RenameRowStatus::InvalidName);

        let mut root_entry = FileEntry::new("root", RootKind::Fs, "", "/", 0, None);
        root_entry.hashes.insert(AlgorithmId::Blake3, digest());
        let plan = plan_renames(&[root_entry], &RenamePlanOptions::default(), &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::InvalidName);
    }

    #[test]
    fn planner_skips_non_ok_entries() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a.txt");
        let mut entry = make_entry(&path, true);
        entry.fail(ErrorKind::IoError, "cannot read");
        assert_eq!(entry.status, EntryStatus::Failed);
        let plan = plan_renames(&[entry], &RenamePlanOptions::default(), &StdRenameFs);
        assert!(plan.rows.is_empty());
        assert_eq!(plan.summary.total, 0);
    }

    #[test]
    fn planner_reports_unsupported_backend() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a.txt");
        let plan = plan_renames(
            &[make_entry(&path, true)],
            &RenamePlanOptions::default(),
            &NoRenameFs,
        );
        assert_eq!(plan.rows[0].status, RenameRowStatus::Unsupported);
        assert_eq!(plan.summary.unsupported, 1);
    }

    #[cfg(unix)]
    #[test]
    fn planner_reports_read_only_directories() {
        let temp = tempdir().expect("tempdir");
        let dir = temp.path().join("readonly");
        fs::create_dir(&dir).expect("readonly directory");
        set_dir_mode(&dir, 0o555);
        let plan = plan_renames(
            &[make_entry(&dir.join("a.txt"), true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        set_dir_mode(&dir, 0o755);
        assert_eq!(plan.rows[0].status, RenameRowStatus::NoPermission);
        assert_eq!(plan.summary.no_permission, 1);
    }

    #[test]
    fn planner_reports_invalid_suffix_configuration() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a.txt");
        let options = RenamePlanOptions {
            format: SuffixFormat {
                length: 3,
                ..SuffixFormat::default()
            },
            ..RenamePlanOptions::default()
        };
        let plan = plan_renames(&[make_entry(&path, true)], &options, &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Error);
        assert_eq!(plan.summary.errors, 1);
    }

    #[test]
    fn execute_plan_renames_ready_rows_only() {
        let temp = tempdir().expect("tempdir");
        let ready = temp.path().join("ready.txt");
        let conflicted = temp.path().join("conflicted.txt");
        fs::write(&ready, b"ready").expect("ready file");
        fs::write(&conflicted, b"conflicted").expect("conflicted file");
        fs::write(temp.path().join("conflicted[ABCDE].txt"), b"existing")
            .expect("existing target");

        let plan = plan_renames(
            &[make_entry(&ready, true), make_entry(&conflicted, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
        assert_eq!(plan.rows[1].status, RenameRowStatus::ConflictTargetExists);

        let results = execute_plan(&plan, &mut StdFsBackend::new());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, RenameResultStatus::Renamed);
        assert_eq!(
            results[0].original_path,
            ready.to_string_lossy().into_owned()
        );
        assert!(temp.path().join("ready[ABCDE].txt").exists());
        assert!(!ready.exists());
        assert!(conflicted.exists(), "conflicted row must not be moved");
    }

    #[test]
    fn execute_plan_reports_backend_failures_without_copy_fallback() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("a.txt");
        fs::write(&source, b"hello").expect("source file");
        let plan = plan_renames(
            &[make_entry(&source, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        fs::remove_file(&source).expect("remove source after planning");

        let results = execute_plan(&plan, &mut StdFsBackend::new());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, RenameResultStatus::Failed);
        assert!(results[0].error.is_some());
        assert!(!temp.path().join("a[ABCDE].txt").exists());
    }
}
