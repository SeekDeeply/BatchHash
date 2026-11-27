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
//!
//! Planning-time conflict detection is not a guarantee, because a target can be
//! created after the plan is built. The no-overwrite guarantee is therefore also
//! enforced at execution time: [`execute_plan`] (and
//! [`execute_plan_with_policy`] with `overwrite = false`) renames through
//! [`RenameBackend::rename_no_replace`], which is atomic on Linux
//! (`renameat2` + `RENAME_NOREPLACE`) and best-effort elsewhere.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use batchhash_types::{is_hex_digest_of_len, FileEntry};
use serde::{Deserialize, Serialize};

use batchhash_types::AlgorithmId;

use crate::suffix::{
    compare_existing_suffix, format_filename_multi, parse_filename, ExistingSuffixVerdict,
    MultiSuffixFormat, SuffixError, SuffixParseResult, DEFAULT_MAX_UTF16_LEN,
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
        is_dir_writable_impl(dir)
    }
}

/// Expected digest for one file, supplied by the caller as a history record.
///
/// The planner never computes hashes itself: this crate owns filename rules and
/// the rename log, not the hash engines. A caller that already hashed the file
/// (the pipeline, a stored report or the UI cache) passes the digest it recorded,
/// and the plan verifies that the file still matches before any rename happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedHash {
    /// Path the digest belongs to, exactly as [`FileEntry::display_path`] holds it.
    pub path: String,
    /// Algorithm whose digest was recorded.
    pub algorithm: AlgorithmId,
    /// Full digest in hex, any case.
    pub hex: String,
}

impl ExpectedHash {
    /// Build an expected digest record.
    pub fn new(path: impl Into<String>, algorithm: AlgorithmId, hex: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            algorithm,
            hex: hex.into(),
        }
    }
}

/// Optional digest source consulted while planning a strict rename.
///
/// Implemented by callers that can re-read a file cheaply; the planner only calls
/// it for rows it would otherwise mark [`RenameRowStatus::Ready`], so a dry run
/// stays a pure function of its inputs when strict verification is off.
pub trait HashProbe {
    /// Current digest of [`path`] for [`algorithm`], or [`None`] when the file
    /// cannot be read or the algorithm is unknown to the implementation.
    ///
    /// Returning [`None`] never renames: the row becomes
    /// [`RenameRowStatus::HashMismatch`] because an unverifiable file must not be
    /// renamed under a strict policy.
    fn current_hash(&self, path: &Path, algorithm: AlgorithmId) -> Option<String>;
}

/// [`HashProbe`] that never has anything to report.
///
/// Used when strict verification is configured but no probe was supplied, so the
/// planner can still explain the outcome per row instead of silently trusting.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHashProbe;

impl HashProbe for NoHashProbe {
    fn current_hash(&self, _path: &Path, _algorithm: AlgorithmId) -> Option<String> {
        None
    }
}

/// Options controlling [`plan_renames`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePlanOptions {
    /// Suffix format applied to every planned filename.
    ///
    /// One-part formats behave exactly like the historical single-algorithm
    /// configuration; multi-part formats additionally force the algorithm prefix
    /// on every token so the group stays parseable.
    pub format: MultiSuffixFormat,
    /// When true, an existing target file is not reported as a conflict.
    /// Defaults to false because overwriting is destructive.
    pub overwrite: bool,
    /// When true, planned targets that differ only by Unicode case are treated
    /// as conflicts. Defaults to true so case-insensitive filesystems are safe.
    pub case_insensitive_conflicts: bool,
    /// Historical digests every candidate file must still match.
    ///
    /// An empty list is the default and means **no verification at all**: no
    /// lookup, no comparison, no probe call and no [`RenameRowStatus::HashMismatch`]
    /// row, so existing callers observe exactly the behaviour they did before
    /// strict verification existed.
    pub verify_hashes: Vec<ExpectedHash>,
    /// When true, a file whose name already carries a parseable suffix has that
    /// group replaced with a freshly computed one instead of being skipped.
    ///
    /// This is the "strip existing suffix" mode: it never trusts the old token,
    /// it rewrites the name from the current digest, so a stale or foreign suffix
    /// is removed rather than nested.
    pub clear_existing_suffix: bool,
}

impl Default for RenamePlanOptions {
    fn default() -> Self {
        Self {
            format: MultiSuffixFormat::default(),
            overwrite: false,
            case_insensitive_conflicts: true,
            verify_hashes: Vec::new(),
            clear_existing_suffix: false,
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
    /// The existing suffix names an algorithm, but its hex no longer agrees
    /// with that algorithm's digest for the current bytes.
    ///
    /// This is the "the file changed since it was tagged" case. It is a skip
    /// rather than a rename: labelling the file with the new digest while the
    /// old label still claims otherwise would leave two contradictory values in
    /// one name, so the user has to decide what the old label is worth.
    ExistingSuffixMismatch,
    /// The existing suffix cannot be checked against the current digests.
    ///
    /// Either the token carries no algorithm name and its width matches several
    /// algorithms, or the algorithm it names was not computed for this file. In
    /// both cases nothing can be concluded about whether the file changed, so
    /// the row is reported instead of being silently rewritten.
    ExistingSuffixUnverified,
    /// The entry has no usable hash for the configured suffix algorithm.
    MissingHash,
    /// The file no longer matches the historical digest the caller supplied.
    ///
    /// Only produced when [`RenamePlanOptions::verify_hashes`] is non-empty; such
    /// a row is never renamed, because renaming it would label a file with a
    /// digest that is not its own.
    HashMismatch,
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

/// Normalize a path for identity comparison inside one plan.
///
/// Case-insensitive filesystems treat `A.txt` and `a.txt` as the same file, so
/// a plan that thinks they are different can order two renames into a cycle
/// that silently loses one of them.
fn normalize_key(path: &str, case_insensitive: bool) -> String {
    let replaced = path.replace('\\', "/");
    if case_insensitive {
        replaced.to_lowercase()
    } else {
        replaced
    }
}

/// Reorder ready rows so each rename runs after the row that vacates its target.
///
/// Two rows form a dependency when one's target is the other's source: `A -> B`
/// must run after `B -> C`, otherwise the first rename writes onto a file that
/// still holds the bytes the second one needs, and those bytes are gone. The
/// order produced here is a reverse topological order over that graph.
///
/// A cycle (`A -> B` and `B -> A`) has no safe order at all. Every row in one is
/// marked as a conflict rather than attempted, because running either half
/// destroys the other file and there is no intermediate name to borrow.
fn apply_dependency_order(rows: &mut [RenamePlanRow], case_insensitive: bool) {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Only ready rows take part; a blocked row will not move, so it cannot
    // vacate anything and cannot be part of a chain.
    let mut index_of: HashMap<String, usize> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        if row.status == RenameRowStatus::Ready {
            index_of.insert(normalize_key(&row.original_path, case_insensitive), index);
        }
    }
    if index_of.is_empty() {
        return;
    }

    // `depends_on[i]` lists the rows that must run before row `i`, and
    // `dependents[j]` is its inverse for the Kahn traversal.
    let mut depends_on: Vec<Vec<usize>> = vec![Vec::new(); rows.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); rows.len()];
    for (index, row) in rows.iter().enumerate() {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        let target = target_path(&row.original_path, &row.proposed_name);
        let target_key = normalize_key(&target.to_string_lossy(), case_insensitive);
        if let Some(&other) = index_of.get(&target_key) {
            if other != index {
                depends_on[index].push(other);
                dependents[other].push(index);
            }
        }
    }

    let mut indegree: Vec<usize> = depends_on.iter().map(Vec::len).collect();
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (index, row) in rows.iter().enumerate() {
        if row.status == RenameRowStatus::Ready && indegree[index] == 0 {
            queue.push_back(index);
        }
    }
    let mut ordered: Vec<usize> = Vec::new();
    while let Some(index) = queue.pop_front() {
        ordered.push(index);
        for &dependent in &dependents[index] {
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                queue.push_back(dependent);
            }
        }
    }

    let ready_count = rows
        .iter()
        .filter(|row| row.status == RenameRowStatus::Ready)
        .count();
    if ordered.len() != ready_count {
        // Whatever did not sort is in, or downstream of, a cycle. Neither can
        // be executed safely, so all of it is refused with one clear reason.
        let settled: HashSet<usize> = ordered.into_iter().collect();
        for (index, row) in rows.iter_mut().enumerate() {
            if row.status == RenameRowStatus::Ready && !settled.contains(&index) {
                row.status = RenameRowStatus::ConflictTargetExists;
                row.reason = Some(
                    "rename cycle detected: these names would overwrite each other".to_string(),
                );
            }
        }
        return;
    }

    // Rebuild the row list with ready rows in safe order, keeping blocked rows
    // where they were so the preview still lines up with the file list.
    // `take` up front so the ready rows can be cloned out before the original
    // slice is overwritten: reading `rows[index]` while iterating `rows` would
    // borrow it both ways.
    let original: Vec<RenamePlanRow> = rows.to_vec();
    let mut next_ready = ordered.into_iter();
    let reordered: Vec<RenamePlanRow> = original
        .iter()
        .map(|row| {
            if row.status == RenameRowStatus::Ready {
                let index = next_ready.next().expect("one ordered index per ready row");
                original[index].clone()
            } else {
                row.clone()
            }
        })
        .collect();
    rows.clone_from_slice(&reordered);
}

/// Explain an [`ExistingSuffixVerdict`] in one line for the plan row.
///
/// The text is for logs and debugging; the UI keys off the row status. It still
/// names the algorithms involved, because "unverified" without saying which
/// algorithm was missing is not actionable.
fn existing_suffix_reason(verdict: &ExistingSuffixVerdict) -> String {
    match verdict {
        ExistingSuffixVerdict::Absent => "no existing suffix".to_string(),
        ExistingSuffixVerdict::Matches { algorithm, hex } => {
            format!("existing {algorithm} suffix {hex} matches the current digest")
        }
        ExistingSuffixVerdict::Mismatch {
            algorithm,
            found,
            current,
        } => format!(
            "existing {algorithm} suffix {found} does not match the current digest {current}"
        ),
        ExistingSuffixVerdict::Inferred { algorithm, agrees } => {
            if *agrees {
                format!("existing suffix agrees with the current {algorithm} digest")
            } else {
                format!("existing suffix disagrees with the current {algorithm} digest")
            }
        }
        ExistingSuffixVerdict::Ambiguous { hex, matched } => {
            if matched.is_empty() {
                format!("suffix {hex} names no algorithm and matches none that were computed")
            } else {
                let names: Vec<String> = matched.iter().map(|a| a.to_string()).collect();
                format!(
                    "suffix {hex} matches several algorithms: {}",
                    names.join(", ")
                )
            }
        }
        ExistingSuffixVerdict::Unparseable => "existing suffix cannot be interpreted".to_string(),
    }
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
    /// Rows with [`RenameRowStatus::ExistingSuffixMismatch`].
    pub suffix_mismatch: usize,
    /// Rows with [`RenameRowStatus::ExistingSuffixUnverified`].
    pub suffix_unverified: usize,
    /// Rows with [`RenameRowStatus::MissingHash`].
    pub missing_hash: usize,
    /// Rows with [`RenameRowStatus::HashMismatch`].
    pub hash_mismatch: usize,
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
///
/// This entry point performs no digest verification: it is
/// [`plan_renames_with_probe`] with [`NoHashProbe`], which is indistinguishable
/// from the historical behaviour as long as
/// [`RenamePlanOptions::verify_hashes`] is empty (the default).
pub fn plan_renames(
    entries: &[FileEntry],
    options: &RenamePlanOptions,
    fs: &dyn RenameFs,
) -> RenamePlan {
    plan_renames_with_probe(entries, options, fs, &NoHashProbe)
}

/// Build a dry-run rename plan, verifying recorded digests through [`probe`].
///
/// When [`RenamePlanOptions::verify_hashes`] is empty the probe is never called,
/// so behaviour and I/O are identical to [`plan_renames`]. When it is not empty,
/// every row that would be [`RenameRowStatus::Ready`] is re-hashed for each
/// algorithm the suffix writes, and a row whose digest no longer matches becomes
/// [`RenameRowStatus::HashMismatch`] with the expected and actual values in its
/// reason. A mismatching row is never renamed.
pub fn plan_renames_with_probe(
    entries: &[FileEntry],
    options: &RenamePlanOptions,
    fs: &dyn RenameFs,
    probe: &dyn HashProbe,
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

    // A target that another row of this same plan is about to vacate is not a
    // conflict: `A -> B` and `B -> C` together are a chain, and refusing it
    // would make the obvious batch rename impossible. Only a target that stays
    // occupied after the whole plan has run is a real conflict.
    let vacated: std::collections::HashSet<String> = rows
        .iter()
        .filter(|row| row.status == RenameRowStatus::Ready)
        .map(|row| normalize_key(&row.original_path, options.case_insensitive_conflicts))
        .collect();

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
        if !options.overwrite {
            let target = target_path(&row.original_path, &row.proposed_name);
            let target_key = normalize_key(
                &target.to_string_lossy(),
                options.case_insensitive_conflicts,
            );
            if fs.exists(&target) && !vacated.contains(&target_key) {
                row.status = RenameRowStatus::ConflictTargetExists;
                row.reason = Some("target name already exists".to_string());
            }
        }
    }

    // Order the surviving rows so a chain cannot destroy its own input, and
    // reject cycles outright: `A -> B` while `B -> A` has no safe order, and
    // executing either half overwrites the other file.
    apply_dependency_order(&mut rows, options.case_insensitive_conflicts);

    verify_row_hashes(&mut rows, options, probe);

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
    /// Rename or move [`from`] to [`to`], replacing an existing target.
    ///
    /// Implementations must not fall back to copy-and-delete; a single atomic
    /// rename (or the platform equivalent) is required. Callers that must not
    /// destroy an existing target use [`RenameBackend::rename_no_replace`]
    /// instead.
    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), RenameError>;

    /// Rename [`from`] to [`to`] without ever replacing an existing target.
    ///
    /// Returns [`RenameError::AlreadyExists`] when [`to`] exists. The default
    /// implementation checks the target first and then delegates to
    /// [`RenameBackend::rename`], so a target created between the check and the
    /// rename can still be replaced; backends with an atomic no-replace
    /// primitive ([`StdFsBackend`] on Linux) override this method. Backends with
    /// their own path model (Android SAF documents) must override it, because
    /// the default check only understands standard library paths.
    fn rename_no_replace(&mut self, from: &Path, to: &Path) -> Result<(), RenameError> {
        if target_exists(to) {
            return Err(RenameError::AlreadyExists(to.display().to_string()));
        }
        self.rename(from, to)
    }
}

/// Standard-library [`RenameBackend`] using [`std::fs::rename`].
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFsBackend;

impl StdFsBackend {
    /// Create a standard filesystem rename backend.
    pub fn new() -> Self {
        Self
    }

    /// Rename without replacing an existing target using the strongest
    /// primitive this platform offers.
    ///
    /// * Linux: `renameat2(2)` with `RENAME_NOREPLACE`, which is atomic. A kernel
    ///   or filesystem that rejects the flag (older kernels, some network and
    ///   FUSE filesystems) falls back to the best-effort path below.
    /// * Other Unix: a target check followed by `std::fs::rename`. This is
    ///   best-effort, not atomic.
    /// * Windows: `std::fs::rename` already fails when the target exists.
    fn rename_no_replace_impl(from: &Path, to: &Path) -> Result<(), RenameError> {
        #[cfg(target_os = "linux")]
        if let Some(outcome) = linux_rename_noreplace(from, to)? {
            return outcome;
        }
        #[cfg(target_os = "macos")]
        return macos_rename_noreplace(from, to);
        #[cfg(target_os = "windows")]
        return windows_rename_noreplace(from, to);
        #[cfg(unix)]
        if target_exists(to) {
            return Err(RenameError::AlreadyExists(to.display().to_string()));
        }
        #[cfg(not(target_os = "windows"))]
        std::fs::rename(from, to).map_err(|error| RenameError::from_io(to, &error))
    }
}

impl RenameBackend for StdFsBackend {
    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), RenameError> {
        std::fs::rename(from, to).map_err(|error| RenameError::from_io(from, &error))
    }

    fn rename_no_replace(&mut self, from: &Path, to: &Path) -> Result<(), RenameError> {
        StdFsBackend::rename_no_replace_impl(from, to)
    }
}

/// Whether a directory accepts new entries on this platform.
///
/// Unix: `access(dir, W_OK | X_OK)` asks the kernel the same question the
/// rename will ask. `Metadata::permissions().readonly()` cannot answer it — it
/// only reports whether the owner-write bit is set, so a directory writable via
/// group or ACL would be called unwritable, and a root-owned 0555 directory
/// writable by root would be called writable.
///
/// Windows: the readonly attribute on a directory does not mean "cannot write"
/// (it marks a customized folder and is set on most system directories), so it
/// is not consulted at all. A cheap probe is used instead: try to create and
/// remove a uniquely named file.
#[cfg(unix)]
fn is_dir_writable_impl(dir: &Path) -> bool {
    let Ok(dir_c) = path_to_cstring(dir) else {
        return false;
    };
    // SAFETY: the pointer is a NUL-terminated string that outlives the call.
    let status = unsafe { libc::access(dir_c.as_ptr(), libc::W_OK | libc::X_OK) };
    status == 0
}

#[cfg(windows)]
fn is_dir_writable_impl(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    // A real probe, because the attributes say nothing useful here and the ACL
    // is not readable without extra API calls. The name is process- and
    // counter-specific so two probes cannot collide.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let probe = dir.join(format!(".bh_write_probe_{}_{seq}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Rename without replacing an existing target, on macOS.
///
/// `renamex_np(2)` with `RENAME_EXCL` is the only atomic no-replace primitive
/// macOS offers; `std::fs::rename` maps to `rename(2)`, which silently replaces
/// the target. Without this, a plan that raced with another process would
/// destroy the file it collided with.
#[cfg(target_os = "macos")]
fn macos_rename_noreplace(from: &Path, to: &Path) -> Result<(), RenameError> {
    let from_c = path_to_cstring(from)?;
    let to_c = path_to_cstring(to)?;
    // SAFETY: both pointers are NUL-terminated and outlive the call, and
    // RENAME_EXCL is the documented "fail if the target exists" flag.
    let status = unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) };
    if status == 0 {
        return Ok(());
    }
    Err(RenameError::from_io(to, &std::io::Error::last_os_error()))
}

/// Rename without replacing an existing target, on Windows.
///
/// `std::fs::rename` is not usable here: on Windows it calls `MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING`, so renaming onto an occupied name destroys the
/// occupant. That is silent data loss in a tool whose whole job is rewriting
/// names, and it is invisible because the call reports success. Calling
/// `MoveFileExW` with flags `0` asks for the opposite: the call fails with
/// `ERROR_ALREADY_EXISTS` (or `ERROR_FILE_EXISTS`) when the target is taken.
#[cfg(target_os = "windows")]
fn windows_rename_noreplace(from: &Path, to: &Path) -> Result<(), RenameError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    /// `ERROR_FILE_EXISTS` from winerror.h.
    const ERROR_FILE_EXISTS: i32 = 80;
    /// `ERROR_ALREADY_EXISTS` from winerror.h.
    const ERROR_ALREADY_EXISTS: i32 = 183;

    let from_wide: Vec<u16> = from
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let to_wide: Vec<u16> = to
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Flags 0: neither REPLACE_EXISTING (which would destroy the target) nor
    // COPY_ALLOWED (which turns a cross-volume rename into a copy that is no
    // longer atomic). A cross-volume rename then fails loudly, which is the
    // honest outcome for an operation the caller believes is a rename.
    let flags = 0;
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call;
    // MoveFileExW only reads them.
    let ok = unsafe { MoveFileExW(from_wide.as_ptr(), to_wide.as_ptr(), flags) };
    if ok != 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if let Some(code) = error.raw_os_error() {
        if code == ERROR_FILE_EXISTS || code == ERROR_ALREADY_EXISTS {
            return Err(RenameError::AlreadyExists(to.display().to_string()));
        }
    }
    Err(RenameError::from_io(to, &error))
}

/// `renameat2(2)` with `RENAME_NOREPLACE`.
///
/// Returns [`None`] when the filesystem does not implement the flag, so the
/// caller can fall back to a check-then-rename; returns `Some(result)` when the
/// kernel answered either way. Every other error is reported as a rename error
/// on [`to`], because a failed rename leaves the source untouched.
#[cfg(target_os = "linux")]
fn linux_rename_noreplace(
    from: &Path,
    to: &Path,
) -> Result<Option<Result<(), RenameError>>, RenameError> {
    use std::os::raw::c_char;

    let from_c = path_to_cstring(from)?;
    let to_c = path_to_cstring(to)?;
    // SAFETY: both pointers are NUL-terminated strings that live for the whole
    // call, AT_FDCWD is the documented "resolve relative to the working
    // directory" handle, and the only flag is RENAME_NOREPLACE.
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from_c.as_ptr() as *const c_char,
            libc::AT_FDCWD,
            to_c.as_ptr() as *const c_char,
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        return Ok(Some(Ok(())));
    }
    let error = std::io::Error::last_os_error();
    if is_rename_noreplace_unsupported(&error) {
        return Ok(None);
    }
    Ok(Some(Err(RenameError::from_io(to, &error))))
}

/// Whether a failed `renameat2(RENAME_NOREPLACE)` means the primitive itself is
/// unavailable rather than the rename being rejected.
#[cfg(target_os = "linux")]
fn is_rename_noreplace_unsupported(error: &std::io::Error) -> bool {
    match error.raw_os_error() {
        // The filesystem does not implement RENAME_NOREPLACE.
        Some(code) if code == libc::EINVAL => true,
        // The kernel predates renameat2(2).
        Some(code) if code == libc::ENOSYS => true,
        // Neither path can be encoded as a C string.
        _ => error.kind() == std::io::ErrorKind::InvalidInput,
    }
}

/// Convert a path for a C API call, rejecting interior NUL bytes and paths that
/// cannot be represented as bytes.
#[cfg(target_os = "linux")]
fn path_to_cstring(path: &Path) -> Result<std::ffi::CString, RenameError> {
    use std::os::unix::ffi::OsStrExt;

    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| RenameError::InvalidPath(path.display().to_string()))
}

/// Whether `path` currently exists, including a dangling symlink.
///
/// A probe that fails for another reason (a permission error, for example)
/// reports `true` so the caller never treats "cannot tell" as "free" and
/// destroys a file it could not see.
fn target_exists(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
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
    /// The path cannot be expressed as a platform path (an interior NUL byte).
    #[error("invalid path: {0}")]
    InvalidPath(String),
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
            RenameError::InvalidPath(_) => "invalid_path",
            RenameError::Io(_) => "io_error",
        }
    }
}

/// Execute every [`RenameRowStatus::Ready`] row of a plan without ever
/// replacing an existing target.
///
/// This is [`execute_plan_with_policy`] with `overwrite = false`: it is the safe
/// default, and the entry point the CLI and the bridge call.
///
/// Non-ready rows are not attempted and produce no result, so the returned
/// vector contains at most one result per ready row, in plan order. The plan
/// itself is never modified.
pub fn execute_plan(plan: &RenamePlan, backend: &mut dyn RenameBackend) -> Vec<RenameResult> {
    execute_plan_with_policy(plan, backend, false)
}

/// Execute every [`RenameRowStatus::Ready`] row of a plan through [`backend`],
/// honouring the overwrite policy.
///
/// The policy is enforced at execution time, not only at planning time: with
/// `overwrite = false` every rename goes through
/// [`RenameBackend::rename_no_replace`], so a target created *after* the plan
/// was built cannot be silently destroyed. With `overwrite = true` the backend's
/// plain rename is used, which replaces an existing target.
///
/// Non-ready rows are not attempted and produce no result, so the returned
/// vector contains at most one result per ready row, in plan order. The plan
/// itself is never modified.
pub fn execute_plan_with_policy(
    plan: &RenamePlan,
    backend: &mut dyn RenameBackend,
    overwrite: bool,
) -> Vec<RenameResult> {
    execute_plan_with_policy_and_cancel(plan, backend, overwrite, &|| false)
}

/// Execute a plan through [`backend`], stopping before the next row when
/// [`should_stop`] reports cancellation.
///
/// The stop check happens immediately before **every** destructive rename, so
/// cancellation that lands between two rows leaves exactly the rows that were
/// renamed in the returned results and none after the stop point. The check is
/// also made once before the loop, so a plan whose cancellation arrives before
/// the first write renames nothing at all.
///
/// [`should_stop`] must return `false` when nothing is cancelled; the policy
/// argument behaves exactly as in [`execute_plan_with_policy`].
/// Whether two paths name the same file and differ only in letter case.
///
/// On a case-insensitive filesystem `a.txt` and `A.txt` are one file, so a
/// direct rename from one to the other is a no-op that the OS may report as
/// success or as "target exists" depending on the platform and the primitive
/// used. Callers must route those through a temporary name instead.
pub(crate) fn is_case_only_rename(from: &Path, to: &Path) -> bool {
    if from == to {
        return false;
    }
    let (Some(from_name), Some(to_name)) = (from.file_name(), to.file_name()) else {
        return false;
    };
    if from.parent() != to.parent() {
        return false;
    }
    from_name != to_name
        && from_name.to_string_lossy().to_lowercase() == to_name.to_string_lossy().to_lowercase()
}

/// Build a temporary sibling name that `reconcile` can recognise later.
///
/// The name is derived from the file it belongs to rather than being random, so
/// a crash between the two hops leaves a file that can be traced back: the
/// reconciler sees `.bh_case_<stem>.tmp` next to a missing `<stem>` and restores
/// it. A random name would strand the user's data in a hidden file.
fn case_rename_temp_path(from: &Path) -> PathBuf {
    let parent = from.parent().unwrap_or_else(|| Path::new("."));
    let stem = from
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut candidate = parent.join(format!(".bh_case_{stem}.tmp"));
    // Two different files cannot share a name, but a leftover from a previous
    // crash can occupy this one. A suffix keeps the name traceable.
    let mut counter = 0u32;
    while candidate.exists() {
        counter += 1;
        candidate = parent.join(format!(".bh_case_{stem}.{counter}.tmp"));
    }
    candidate
}

/// Rename through a temporary sibling, for case-only renames.
///
/// Two hops are the only reliable way to change just the letter case on a
/// case-insensitive filesystem. If the second hop fails the first is undone
/// immediately, so the file never stays under the temporary name unless the
/// rollback also fails — and in that case the error names the temporary path so
/// the user (or the reconciler) can recover it.
pub(crate) fn rename_via_temp(
    backend: &mut dyn RenameBackend,
    from: &Path,
    to: &Path,
) -> Result<(), RenameError> {
    let temp = case_rename_temp_path(from);
    backend.rename(from, &temp)?;
    match backend.rename_no_replace(&temp, to) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Put the file back before reporting: leaving it under the
            // temporary name is worse than not renaming at all.
            if let Err(rollback) = backend.rename(&temp, from) {
                return Err(RenameError::Io {
                    path: temp.display().to_string(),
                    message: format!(
                        "{error}; rolling back also failed ({rollback}), the file is at {}",
                        temp.display()
                    ),
                });
            }
            Err(error)
        }
    }
}

pub fn execute_plan_with_policy_and_cancel(
    plan: &RenamePlan,
    backend: &mut dyn RenameBackend,
    overwrite: bool,
    should_stop: &(dyn Fn() -> bool + Sync),
) -> Vec<RenameResult> {
    let mut results = Vec::new();
    for row in &plan.rows {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        if should_stop() {
            break;
        }
        let from = Path::new(&row.original_path);
        let to = target_path(&row.original_path, &row.proposed_name);
        // A case-only rename cannot be done in one hop on a case-insensitive
        // filesystem: the source and target are the same file to the OS.
        let outcome = if is_case_only_rename(from, &to) {
            rename_via_temp(backend, from, &to)
        } else if overwrite {
            backend.rename(from, &to)
        } else {
            backend.rename_no_replace(from, &to)
        };
        match outcome {
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
    // The name is rewritten from scratch in clear-suffix mode, so an existing
    // parseable group is stripped before formatting. In strict mode it stays a
    // skip: nesting a second suffix onto an already suffixed name is never what
    // the user meant.
    let mut base_name = original_name.clone();
    match parse_filename(&original_name) {
        SuffixParseResult::Parsed(parsed) => {
            if !options.clear_existing_suffix {
                // The old suffix is evidence, not noise: it names an algorithm,
                // so the file can be checked against that algorithm's current
                // digest before anything is decided. A match means the file is
                // unchanged since it was tagged and the label may be extended or
                // replaced; a mismatch means the bytes moved under the old label
                // and the user has to choose what to keep.
                let verdict = compare_existing_suffix(&original_name, &entry.hashes);
                let status = match &verdict {
                    ExistingSuffixVerdict::Mismatch { .. } => {
                        RenameRowStatus::ExistingSuffixMismatch
                    }
                    ExistingSuffixVerdict::Ambiguous { .. }
                    | ExistingSuffixVerdict::Unparseable => {
                        RenameRowStatus::ExistingSuffixUnverified
                    }
                    // A suffix whose width names exactly one algorithm is
                    // decidable, so disagreement is a real mismatch and must not
                    // be filed under "already suffixed" as if all were well.
                    ExistingSuffixVerdict::Inferred { agrees, .. } => {
                        if *agrees {
                            RenameRowStatus::SkippedAlreadySuffixed
                        } else {
                            RenameRowStatus::ExistingSuffixMismatch
                        }
                    }
                    ExistingSuffixVerdict::Absent | ExistingSuffixVerdict::Matches { .. } => {
                        RenameRowStatus::SkippedAlreadySuffixed
                    }
                };
                return RenamePlanRow {
                    entry_index,
                    original_path,
                    original_name,
                    proposed_name: String::new(),
                    status,
                    reason: Some(existing_suffix_reason(&verdict)),
                };
            }
            base_name = strip_suffix_group(&original_name, &parsed);
        }
        SuffixParseResult::Unparseable => {
            return RenamePlanRow {
                entry_index,
                original_path,
                original_name,
                proposed_name: String::new(),
                status: RenameRowStatus::InvalidName,
                reason: Some("filename has a bracket group that is not a valid suffix".to_string()),
            };
        }
        SuffixParseResult::NotSuffixed => {}
    }

    let mut digests: Vec<(AlgorithmId, &str)> = Vec::with_capacity(options.format.len());
    for part in options.format.parts() {
        let algorithm = part.algorithm;
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
        digests.push((algorithm, digest.as_str()));
    }

    match format_filename_multi(&base_name, &digests, &options.format, DEFAULT_MAX_UTF16_LEN) {
        Ok(proposed_name) => {
            // The suffix already says exactly what it should: renaming would be
            // a no-op on a case-insensitive filesystem and a pointless churn on
            // a case-sensitive one, so the row is reported as unchanged rather
            // than ready.
            let status = if proposed_name == original_name {
                RenameRowStatus::SkippedAlreadySuffixed
            } else {
                RenameRowStatus::Ready
            };
            RenamePlanRow {
                entry_index,
                original_path,
                original_name,
                proposed_name,
                status,
                reason: None,
            }
        }
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
                SuffixError::InvalidLength(_)
                | SuffixError::AlgorithmNotSuffixUsable(_)
                | SuffixError::EmptySuffixFormat
                | SuffixError::TooManySuffixParts { .. }
                | SuffixError::DuplicateSuffixAlgorithm(_) => RenameRowStatus::Error,
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

/// Strip the parseable suffix group from `name`, keeping extension and stem.
///
/// The group is located from the [`ParsedSuffix`] the parser already found, so
/// the removal cannot disagree with [`parse_filename`] about what the suffix is.
fn strip_suffix_group(name: &str, parsed: &crate::suffix::ParsedSuffix) -> String {
    let (stem, extension) = match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    };
    let group_len = parsed.token.len() + 2; // "[" + token + "]"
    match stem.len().checked_sub(group_len) {
        Some(keep) if stem[keep..].starts_with('[') => format!("{}{extension}", &stem[..keep]),
        // The parser reported a group this helper cannot locate: fall back to
        // the untouched name so the caller formats a name that still exists.
        _ => name.to_string(),
    }
}

/// Mark rows whose file no longer matches its recorded digest.
///
/// This is the whole of the strict-verification pass, and it is deliberately a
/// no-op when [`RenamePlanOptions::verify_hashes`] is empty: no probe call, no
/// allocation and no status change, which is what keeps older callers byte-for-byte
/// compatible.
fn verify_row_hashes(
    rows: &mut [RenamePlanRow],
    options: &RenamePlanOptions,
    probe: &dyn HashProbe,
) {
    if options.verify_hashes.is_empty() {
        return;
    }
    for row in rows.iter_mut() {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        for part in options.format.parts() {
            let Some(expected) =
                expected_hash_for(&options.verify_hashes, &row.original_path, part.algorithm)
            else {
                continue;
            };
            let Some(actual) = probe.current_hash(Path::new(&row.original_path), part.algorithm)
            else {
                row.status = RenameRowStatus::HashMismatch;
                row.reason = Some(format!(
                    "cannot verify the {} digest of {}: the file could not be read",
                    part.algorithm, row.original_path
                ));
                break;
            };
            if !digests_match(&actual, expected) {
                row.status = RenameRowStatus::HashMismatch;
                row.reason = Some(format!(
                    "file content changed: {} digest is {} but the record says {}",
                    part.algorithm,
                    shorten_digest(&actual),
                    shorten_digest(expected)
                ));
                break;
            }
        }
    }
}

/// Recorded digest for `path` and `algorithm`, if the caller supplied one.
fn expected_hash_for<'a>(
    expected: &'a [ExpectedHash],
    path: &str,
    algorithm: AlgorithmId,
) -> Option<&'a str> {
    expected
        .iter()
        .find(|entry| entry.path == path && entry.algorithm == algorithm)
        .map(|entry| entry.hex.as_str())
}

/// Compare two digest strings case-insensitively.
///
/// Digests are hex text: the algorithm fixes the length, and case is a display
/// choice, so an upper-case record must match a lower-case computation.
fn digests_match(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected)
}

/// First 12 characters of a digest, for a reason message a human can read.
fn shorten_digest(digest: &str) -> String {
    digest.chars().take(12).collect()
}

fn summarize(rows: &[RenamePlanRow]) -> RenamePlanSummary {
    let mut summary = RenamePlanSummary::default();
    for row in rows {
        summary.total += 1;
        match row.status {
            RenameRowStatus::Ready => summary.ready += 1,
            RenameRowStatus::ConflictTargetExists => summary.conflicts += 1,
            RenameRowStatus::SkippedAlreadySuffixed => summary.skipped += 1,
            RenameRowStatus::ExistingSuffixMismatch => summary.suffix_mismatch += 1,
            RenameRowStatus::ExistingSuffixUnverified => summary.suffix_unverified += 1,
            RenameRowStatus::MissingHash => summary.missing_hash += 1,
            RenameRowStatus::HashMismatch => summary.hash_mismatch += 1,
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
    use crate::suffix::SuffixFormat;
    use batchhash_types::{AlgorithmId, EntryStatus, ErrorKind, RootKind};
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

    /// Build an entry whose blake3 digest starts with `prefix`.
    fn entry_with_blake3_prefix(name: &str, prefix: &str) -> FileEntry {
        let mut entry = FileEntry::new("root", RootKind::Fs, name, name, 0, None);
        let digest: String = std::iter::repeat(prefix)
            .flat_map(|value| value.chars())
            .chain(std::iter::repeat('0'))
            .take(64)
            .collect();
        entry.hashes.insert(AlgorithmId::Blake3, digest);
        entry
    }

    #[test]
    fn existing_suffix_that_matches_is_still_a_skip() {
        // The label agrees with the current digest, so nothing is stale. It is
        // reported as an ordinary skip: the file needs no correction.
        let entry = entry_with_blake3_prefix("a[blake3:abcdef01].txt", "abcdef01");
        let plan = plan_renames(&[entry], &RenamePlanOptions::default(), &NoRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::SkippedAlreadySuffixed);
        assert_eq!(plan.summary.skipped, 1);
    }

    #[test]
    fn existing_suffix_that_disagrees_is_reported_as_a_mismatch() {
        // The name claims blake3:ffffffff but the bytes hash to abcdef01...,
        // which is exactly "the content changed after it was tagged".
        let entry = entry_with_blake3_prefix("a[blake3:ffffffff].txt", "abcdef01");
        let plan = plan_renames(&[entry], &RenamePlanOptions::default(), &NoRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::ExistingSuffixMismatch);
        assert_eq!(plan.summary.suffix_mismatch, 1);
        assert_eq!(plan.summary.skipped, 0);
    }

    #[test]
    fn existing_suffix_naming_an_uncomputed_algorithm_is_unverified() {
        // sha256 is not among the entry's hashes, so the label cannot be
        // checked at all; claiming a match or a mismatch would both be guesses.
        let entry = entry_with_blake3_prefix("a[sha256:abcdef01].txt", "abcdef01");
        let plan = plan_renames(&[entry], &RenamePlanOptions::default(), &NoRenameFs);
        assert_eq!(
            plan.rows[0].status,
            RenameRowStatus::ExistingSuffixUnverified
        );
        assert_eq!(plan.summary.suffix_unverified, 1);
    }

    /// Entry whose proposed name is fixed, so a plan can be built for an
    /// arbitrary chain without depending on digest formatting.
    fn entry_named(name: &str) -> FileEntry {
        FileEntry::new("root", RootKind::Fs, name, name, 0, None)
    }

    /// A filesystem where every named file exists and every directory is
    /// writable, used to exercise conflict detection without a real tree.
    struct ExistingFs(std::collections::HashSet<String>);

    impl RenameFs for ExistingFs {
        fn exists(&self, path: &Path) -> bool {
            self.0.contains(&path.to_string_lossy().to_string())
        }

        fn is_dir_writable(&self, _dir: &Path) -> bool {
            true
        }

        fn supports_rename(&self, _path: &Path) -> bool {
            true
        }
    }

    #[test]
    fn a_chain_is_ordered_so_no_row_overwrites_its_own_input() {
        // A -> B and B -> C in one batch: if A -> B ran first it would land on
        // the file B -> C still needs to read, and that content is gone.
        let mut entries = vec![entry_named("a.txt"), entry_named("b.txt")];
        // Give them digests that produce the desired targets by using distinct
        // algorithms is fragile; instead assert the ordering rule directly.
        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut rows = vec![
            RenamePlanRow {
                entry_index: 0,
                original_path: "/root/a.txt".to_string(),
                original_name: "a.txt".to_string(),
                proposed_name: "b.txt".to_string(),
                status: RenameRowStatus::Ready,
                reason: None,
            },
            RenamePlanRow {
                entry_index: 1,
                original_path: "/root/b.txt".to_string(),
                original_name: "b.txt".to_string(),
                proposed_name: "c.txt".to_string(),
                status: RenameRowStatus::Ready,
                reason: None,
            },
        ];
        apply_dependency_order(&mut rows, false);
        assert_eq!(rows[0].original_path, "/root/b.txt", "B -> C runs first");
        assert_eq!(rows[1].original_path, "/root/a.txt", "A -> B runs second");
        assert!(rows.iter().all(|row| row.status == RenameRowStatus::Ready));
    }

    #[test]
    fn a_cycle_is_refused_instead_of_losing_a_file() {
        // A -> B and B -> A has no safe execution order: whichever runs first
        // destroys the other file.
        let mut rows = vec![
            RenamePlanRow {
                entry_index: 0,
                original_path: "/root/a.txt".to_string(),
                original_name: "a.txt".to_string(),
                proposed_name: "b.txt".to_string(),
                status: RenameRowStatus::Ready,
                reason: None,
            },
            RenamePlanRow {
                entry_index: 1,
                original_path: "/root/b.txt".to_string(),
                original_name: "b.txt".to_string(),
                proposed_name: "a.txt".to_string(),
                status: RenameRowStatus::Ready,
                reason: None,
            },
        ];
        apply_dependency_order(&mut rows, false);
        for row in &rows {
            assert_eq!(row.status, RenameRowStatus::ConflictTargetExists);
            assert!(
                row.reason.as_deref().unwrap_or_default().contains("cycle"),
                "reason explains the cycle: {:?}",
                row.reason
            );
        }
    }

    #[test]
    fn a_target_vacated_by_the_plan_is_not_a_disk_conflict() {
        // b.txt exists on disk, but the plan renames it away, so a.txt -> b.txt
        // is legal. Treating it as a conflict made batch renames impossible.
        let mut entries = vec![entry_named("a.txt"), entry_named("b.txt")];
        entries[0].hashes.insert(AlgorithmId::Blake3, digest());
        entries[1].hashes.insert(AlgorithmId::Blake3, digest());
        let fs = ExistingFs(["/root/b.txt".to_string()].into_iter().collect());
        let mut rows = vec![
            RenamePlanRow {
                entry_index: 0,
                original_path: "/root/a.txt".to_string(),
                original_name: "a.txt".to_string(),
                proposed_name: "b.txt".to_string(),
                status: RenameRowStatus::Ready,
                reason: None,
            },
            RenamePlanRow {
                entry_index: 1,
                original_path: "/root/b.txt".to_string(),
                original_name: "b.txt".to_string(),
                proposed_name: "c.txt".to_string(),
                status: RenameRowStatus::Ready,
                reason: None,
            },
        ];
        // The ordering pass alone must not flag either row as a conflict.
        apply_dependency_order(&mut rows, false);
        assert!(rows.iter().all(|row| row.status == RenameRowStatus::Ready));
        let _ = fs;
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
        assert_eq!(plan.rows[0].proposed_name, "a[ABCDEF].txt");
        assert_eq!(plan.summary.ready, 1);
        assert_eq!(plan.summary.total, 1);
        assert!(source.exists(), "dry run must not rename the source");
        assert!(!temp.path().join("a[ABCDEF].txt").exists());
    }

    #[test]
    fn planner_reports_existing_target_conflicts_and_honors_overwrite() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("a.txt");
        let target = temp.path().join("a[ABCDEF].txt");
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
    fn planner_reports_a_suffix_it_cannot_attribute_to_an_algorithm() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("a[ABCDEF].txt");
        let plan = plan_renames(
            &[make_entry(&path, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        // Six hex characters is not the digest width of any algorithm, so the
        // token names nothing and no digest can be compared against it. It is
        // reported as unverifiable rather than "already suffixed": the user has
        // to be told the name carries a value that cannot be checked.
        assert_eq!(
            plan.rows[0].status,
            RenameRowStatus::ExistingSuffixUnverified
        );
        assert_eq!(plan.summary.suffix_unverified, 1);
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
                // MD5, not the default BLAKE3: the ceiling is the selected
                // algorithm's own width, so 33 is only invalid for a 32-
                // character digest.
                algorithm: AlgorithmId::Md5,
                length: 33,
                ..SuffixFormat::default()
            }
            .into(),
            ..RenamePlanOptions::default()
        };
        let plan = plan_renames(&[make_entry(&path, true)], &options, &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Error);
        assert_eq!(plan.summary.errors, 1);
    }

    #[test]
    fn planner_writes_a_multi_algorithm_group_with_forced_prefixes() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("movie.mkv");
        fs::write(&source, b"movie").expect("source file");
        let mut entry = make_entry(&source, true);
        entry.hashes.insert(
            AlgorithmId::Md5,
            "0123456789abcdef0123456789abcdef".to_string(),
        );
        let options = RenamePlanOptions {
            format: MultiSuffixFormat::new(vec![
                SuffixFormat {
                    algorithm: AlgorithmId::Blake3,
                    ..SuffixFormat::default()
                },
                SuffixFormat {
                    algorithm: AlgorithmId::Md5,
                    ..SuffixFormat::default()
                },
            ])
            .expect("two parts"),
            ..RenamePlanOptions::default()
        };
        let plan = plan_renames(&[entry], &options, &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
        assert_eq!(
            plan.rows[0].proposed_name,
            "movie[blake3-ABCDEF+md5-012345].mkv"
        );
        assert!(source.exists(), "planning must not rename");
    }

    #[test]
    fn planner_reports_missing_hash_for_any_multi_algorithm_part() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("movie.mkv");
        fs::write(&source, b"movie").expect("source file");
        let entry = make_entry(&source, true); // Blake3 only
        let options = RenamePlanOptions {
            format: MultiSuffixFormat::new(vec![
                SuffixFormat {
                    algorithm: AlgorithmId::Blake3,
                    ..SuffixFormat::default()
                },
                SuffixFormat {
                    algorithm: AlgorithmId::Md5,
                    ..SuffixFormat::default()
                },
            ])
            .expect("two parts"),
            ..RenamePlanOptions::default()
        };
        let plan = plan_renames(&[entry], &options, &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::MissingHash);
        assert!(plan.rows[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("md5")));
        assert_eq!(plan.summary.missing_hash, 1);
    }

    #[test]
    fn clear_existing_suffix_rewrites_the_name_instead_of_skipping() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("movie[999999].mkv");
        fs::write(&source, b"movie").expect("source file");
        let entry = make_entry(&source, true);

        // Without the switch the old token is not rewritten. `999999` is six
        // hex characters, which matches no digest width, so it cannot be
        // verified and is reported as such instead of being trusted.
        let plan = plan_renames(
            std::slice::from_ref(&entry),
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(
            plan.rows[0].status,
            RenameRowStatus::ExistingSuffixUnverified
        );

        // With it, the old token is replaced by a freshly computed one.
        let options = RenamePlanOptions {
            clear_existing_suffix: true,
            ..RenamePlanOptions::default()
        };
        let plan = plan_renames(&[entry], &options, &StdRenameFs);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
        assert_eq!(plan.rows[0].proposed_name, "movie[ABCDEF].mkv");
    }

    #[test]
    fn strict_plan_marks_changed_files_as_hash_mismatch() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("movie.mkv");
        fs::write(&source, b"movie").expect("source file");
        let entry = make_entry(&source, true);
        let options = RenamePlanOptions {
            verify_hashes: vec![ExpectedHash::new(
                entry.display_path.clone(),
                AlgorithmId::Blake3,
                digest(),
            )],
            ..RenamePlanOptions::default()
        };
        // The probe reports a different digest than the record.
        struct ChangedProbe;
        impl HashProbe for ChangedProbe {
            fn current_hash(&self, _path: &Path, _algorithm: AlgorithmId) -> Option<String> {
                Some("f".repeat(64))
            }
        }
        let plan = plan_renames_with_probe(&[entry], &options, &StdRenameFs, &ChangedProbe);
        assert_eq!(plan.rows[0].status, RenameRowStatus::HashMismatch);
        assert_eq!(plan.summary.hash_mismatch, 1);
        assert_eq!(plan.summary.ready, 0);
        assert!(plan.rows[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("changed")));
    }

    #[test]
    fn strict_plan_keeps_matching_files_ready_and_never_renames_a_mismatch() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("movie.mkv");
        fs::write(&source, b"movie").expect("source file");
        let entry = make_entry(&source, true);
        let options = RenamePlanOptions {
            verify_hashes: vec![ExpectedHash::new(
                entry.display_path.clone(),
                AlgorithmId::Blake3,
                digest().to_uppercase(), // case must not matter
            )],
            ..RenamePlanOptions::default()
        };
        struct SameProbe;
        impl HashProbe for SameProbe {
            fn current_hash(&self, _path: &Path, _algorithm: AlgorithmId) -> Option<String> {
                Some(digest())
            }
        }
        let plan = plan_renames_with_probe(&[entry], &options, &StdRenameFs, &SameProbe);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
        assert_eq!(plan.summary.hash_mismatch, 0);

        let results = execute_plan(&plan, &mut StdFsBackend::new());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, RenameResultStatus::Renamed);
    }

    #[test]
    fn strict_plan_treats_an_unreadable_file_as_a_mismatch() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("movie.mkv");
        fs::write(&source, b"movie").expect("source file");
        let entry = make_entry(&source, true);
        let options = RenamePlanOptions {
            verify_hashes: vec![ExpectedHash::new(
                entry.display_path.clone(),
                AlgorithmId::Blake3,
                digest(),
            )],
            ..RenamePlanOptions::default()
        };
        // NoHashProbe cannot report a digest at all; under a strict policy that
        // must block the rename rather than trust the record.
        let plan = plan_renames_with_probe(&[entry], &options, &StdRenameFs, &NoHashProbe);
        assert_eq!(plan.rows[0].status, RenameRowStatus::HashMismatch);
        assert!(plan.rows[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("could not be read")));
    }

    #[test]
    fn empty_verify_hashes_is_exactly_the_legacy_behaviour() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("movie.mkv");
        fs::write(&source, b"movie").expect("source file");
        let entry = make_entry(&source, true);

        // A probe that would panic proves the legacy path never consults it.
        struct PanicProbe;
        impl HashProbe for PanicProbe {
            fn current_hash(&self, _path: &Path, _algorithm: AlgorithmId) -> Option<String> {
                panic!("the probe must not be called when verify_hashes is empty");
            }
        }
        let options = RenamePlanOptions::default();
        assert!(options.verify_hashes.is_empty());
        let plan = plan_renames_with_probe(&[entry], &options, &StdRenameFs, &PanicProbe);
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);
        assert_eq!(plan.summary.hash_mismatch, 0);
    }

    #[test]
    fn execute_plan_renames_ready_rows_only() {
        let temp = tempdir().expect("tempdir");
        let ready = temp.path().join("ready.txt");
        let conflicted = temp.path().join("conflicted.txt");
        fs::write(&ready, b"ready").expect("ready file");
        fs::write(&conflicted, b"conflicted").expect("conflicted file");
        fs::write(temp.path().join("conflicted[ABCDEF].txt"), b"existing")
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
        assert!(temp.path().join("ready[ABCDEF].txt").exists());
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
        assert!(!temp.path().join("a[ABCDEF].txt").exists());
    }

    #[test]
    fn execute_plan_never_replaces_a_target_created_after_planning() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("a.txt");
        let target = temp.path().join("a[ABCDEF].txt");
        fs::write(&source, b"hello").expect("source file");

        let plan = plan_renames(
            &[make_entry(&source, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);

        // The target appears after the plan was built: the planning-time check
        // cannot see it, so execution itself must refuse to replace it.
        fs::write(&target, b"other").expect("late target file");

        let results = execute_plan(&plan, &mut StdFsBackend::new());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, RenameResultStatus::Failed);
        assert!(
            results[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("already exists")),
            "error: {:?}",
            results[0].error
        );
        assert_eq!(fs::read(&target).expect("late target"), b"other");
        assert!(source.exists(), "a refused rename leaves the source alone");
    }

    // `std::fs::rename` replaces a target on Unix; on Windows it refuses, so the
    // overwrite policy only replaces there through a platform backend.
    #[cfg(unix)]
    #[test]
    fn execute_plan_with_policy_overwrites_only_when_asked() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("a.txt");
        let target = temp.path().join("a[ABCDEF].txt");
        fs::write(&source, b"hello").expect("source file");

        let plan = plan_renames(
            &[make_entry(&source, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(plan.rows[0].status, RenameRowStatus::Ready);

        // Created after planning, so only the execution-time policy can decide.
        fs::write(&target, b"other").expect("late target file");

        let results = execute_plan_with_policy(&plan, &mut StdFsBackend::new(), true);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, RenameResultStatus::Renamed);
        assert_eq!(fs::read(&target).expect("overwritten target"), b"hello");
        assert!(!source.exists());
    }

    #[test]
    fn execute_plan_with_policy_and_cancel_stops_between_rows() {
        let temp = tempdir().expect("tempdir");
        let first = temp.path().join("first.txt");
        let second = temp.path().join("second.txt");
        fs::write(&first, b"first").expect("first file");
        fs::write(&second, b"second").expect("second file");

        let plan = plan_renames(
            &[make_entry(&first, true), make_entry(&second, true)],
            &RenamePlanOptions::default(),
            &StdRenameFs,
        );
        assert_eq!(plan.summary.ready, 2);

        // The cancellation lands after the first rename has been attempted.
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let should_stop = || {
            let seen = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            seen >= 1
        };

        let mut backend = StdFsBackend::new();
        let results = execute_plan_with_policy_and_cancel(&plan, &mut backend, false, &should_stop);
        assert_eq!(results.len(), 1, "only the first row may be attempted");
        assert_eq!(results[0].status, RenameResultStatus::Renamed);
        assert!(temp.path().join("first[ABCDEF].txt").exists());
        assert!(!first.exists());
        assert!(
            second.exists(),
            "cancellation must stop the loop before the next rename"
        );
        assert!(!temp.path().join("second[ABCDEF].txt").exists());
    }
}
