//! End-to-end batch pipeline: scan, hash, optionally write filename suffixes,
//! build the report and record the rename log.
//!
//! This is the single orchestration entry point shared by the developer CLI and
//! (through the bridge) the Flutter UI, so both surfaces produce identical
//! reports, rename plans and undo logs.
//!
//! The pipeline is explicit about ordering and cancellation:
//!
//! 1. Scan the root (deterministic entry order).
//! 2. Hash every entry with independent per-file progress (single I/O pass).
//! 3. Build the report from the hashed entries.
//! 4. When suffix writing is requested, build a pure dry-run plan.
//! 5. When applying, reconcile any operation a previous crash left pending,
//!    refuse to touch the filesystem after cancellation (re-checked after the
//!    `applying_suffix` event and before every individual rename), insert pending
//!    operations, execute renames and record the outcome of every inserted row.
//! 6. Annotate the report with suffix/rename fields, recompute the summary and
//!    sort entries so export is byte-reproducible.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use batchhash_hash::{
    hash_entries, BatchEvent, CancelToken, FileIo, HashEntryOptions, HashOptions, ProgressSnapshot,
};
use batchhash_rename::log_store::{
    reconcile_pending_operations_with_cancel, BatchRecord, LogError, LogStore, OperationState,
    ReconcileReport, RenameOperation,
};
use batchhash_rename::planner::{
    execute_plan_with_policy_and_cancel, plan_renames, RenamePlan, RenamePlanOptions, RenameResult,
    RenameResultStatus, RenameRowStatus, StdFsBackend, StdRenameFs,
};
use batchhash_rename::suffix::{parse_filename, SuffixError, SuffixFormat, SuffixParseResult};
use batchhash_types::{
    validate_relative_path, AlgorithmId, FileEntry, HardwareReport, Report, ReportEntry,
    ReportValidationError, RootKind,
};

use crate::scan::{scan, ScanError, ScanOptions, ScanSkip};

/// Pipeline stage reported through events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchPhase {
    /// Walking the root and collecting entries.
    Scanning,
    /// Hashing entries.
    Hashing,
    /// Building the pure suffix rename plan.
    PlanningSuffix,
    /// Executing renames through the filesystem backend.
    ApplyingSuffix,
    /// Nothing left to do; the caller receives the final BatchRun.
    Completed,
}

impl BatchPhase {
    /// Stable identifier used in logs and bridge messages.
    pub const fn as_str(self) -> &'static str {
        match self {
            BatchPhase::Scanning => "scanning",
            BatchPhase::Hashing => "hashing",
            BatchPhase::PlanningSuffix => "planning_suffix",
            BatchPhase::ApplyingSuffix => "applying_suffix",
            BatchPhase::Completed => "completed",
        }
    }
}

/// Aggregate progress sample.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchProgress {
    /// Current stage.
    pub phase: BatchPhase,
    /// Entries finished in the current stage.
    pub completed: u64,
    /// Entries in the current stage.
    pub total: u64,
    /// Bytes hashed so far (hashing stage only).
    pub bytes_done: u64,
    /// Total bytes to hash (hashing stage only).
    pub bytes_total: u64,
    /// Completion percentage in 0.0..=100.0.
    pub percent: f64,
    /// Estimated seconds remaining, or None while the rate is still unknown.
    pub eta_seconds: Option<f64>,
    /// Path currently being processed.
    pub current_path: String,
    /// Wall-clock milliseconds since the pipeline started.
    pub elapsed_ms: u64,
}

/// Event stream emitted by run_batch.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// The pipeline entered a new stage.
    Phase(BatchPhase),
    /// A progress sample for the current stage.
    Progress(BatchProgress),
    /// One entry reached a terminal state.
    EntryDone(FileEntry),
}

/// Everything the pipeline needs to run one batch.
#[derive(Debug, Clone)]
pub struct BatchRequest {
    /// Report root label (absolute path or SAF display path).
    pub root: String,
    /// Filesystem path that is scanned.
    pub root_path: PathBuf,
    /// Case sensitivity recorded in the report.
    pub case_sensitive: bool,
    /// Algorithms to compute; must not be empty.
    pub algorithms: Vec<AlgorithmId>,
    /// Scanner behaviour.
    pub scan: ScanOptions,
    /// Root-relative paths to restrict the batch to; empty means no restriction.
    ///
    /// A UI that previewed a rename plan for specific files executes exactly
    /// that set instead of rescanning the tree, so a run can never touch a file
    /// the plan did not show.
    pub restrict_paths: Vec<String>,
    /// Suffix format to plan when Some.
    pub suffix: Option<SuffixFormat>,
    /// Whether the planned renames are executed (requires log_db).
    pub apply_renames: bool,
    /// SQLite rename log path; required when apply_renames is true.
    pub log_db: Option<PathBuf>,
    /// Allow overwriting existing targets (destructive, off by default).
    pub overwrite: bool,
    /// Injected hardware block; the probe result when None.
    pub hardware: Option<HardwareReport>,
    /// Injected report timestamp; the current time when None.
    pub generated_at: Option<String>,
}

impl BatchRequest {
    /// Create a hash-only request for a root path.
    pub fn new(root_path: impl Into<PathBuf>, algorithms: Vec<AlgorithmId>) -> Self {
        let root_path = root_path.into();
        Self {
            root: crate::scan::display_path(&root_path),
            root_path,
            case_sensitive: default_case_sensitive(),
            algorithms,
            scan: ScanOptions::default(),
            restrict_paths: Vec::new(),
            suffix: None,
            apply_renames: false,
            log_db: None,
            overwrite: false,
            hardware: None,
            generated_at: None,
        }
    }

    /// Request suffix planning and execution with the given format.
    pub fn with_suffix(mut self, format: SuffixFormat, log_db: impl Into<PathBuf>) -> Self {
        self.suffix = Some(format);
        self.apply_renames = true;
        self.log_db = Some(log_db.into());
        self
    }

    /// Request suffix planning without touching the filesystem.
    pub fn with_suffix_dry_run(mut self, format: SuffixFormat) -> Self {
        self.suffix = Some(format);
        self.apply_renames = false;
        self
    }
}

/// Result of one pipeline run.
#[derive(Debug, Clone)]
pub struct BatchRun {
    /// Final report (summary recomputed, entries sorted).
    pub report: Report,
    /// Entries after hashing (input order), with hashes and statuses.
    pub entries: Vec<FileEntry>,
    /// Dry-run rename plan when suffix writing was requested.
    pub plan: Option<RenamePlan>,
    /// Per-row rename results when renames were executed.
    pub rename_results: Vec<RenameResult>,
    /// Log batch id when a rename log was written.
    pub batch_id: Option<String>,
    /// Paths the scanner could not represent as report entries.
    pub skipped_paths: Vec<ScanSkip>,
    /// Whether cancellation was requested.
    pub cancelled: bool,
    /// Wall-clock duration in milliseconds.
    pub elapsed_ms: u64,
}

/// Failures that abort a pipeline run.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// The scan failed before any entry was produced.
    #[error("scan failed: {0}")]
    Scan(#[from] ScanError),
    /// The request is internally inconsistent.
    #[error("invalid batch request: {0}")]
    InvalidRequest(String),
    /// The suffix configuration is invalid.
    #[error("invalid suffix configuration: {0}")]
    Suffix(#[from] SuffixError),
    /// The rename log could not be opened or written.
    #[error("rename log error: {0}")]
    Log(#[from] LogError),
    /// The generated report failed its own schema validation.
    #[error("generated report failed validation: {0}")]
    Report(#[from] ReportValidationError),
    /// The rename log directory could not be created.
    #[error("cannot create rename log directory {path}: {message}")]
    LogIo {
        /// Directory that could not be created.
        path: String,
        /// Underlying operating-system error.
        message: String,
    },
}

/// Whether the platform's filesystem is case sensitive by default.
pub fn default_case_sensitive() -> bool {
    !(cfg!(windows) || cfg!(target_os = "macos"))
}

/// Current time as an RFC 3339 second-precision UTC timestamp.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Build a report from hashed entries.
///
/// Entry order is preserved; callers that export the report should call
/// Report::sort_entries or use run_batch which does it for them.
pub fn build_report(
    root: &str,
    root_kind: RootKind,
    case_sensitive: bool,
    algorithms: &[AlgorithmId],
    hardware: HardwareReport,
    entries: &[FileEntry],
) -> Report {
    let mut report = Report::new(
        root,
        root_kind,
        case_sensitive,
        algorithms.to_vec(),
        hardware,
    );
    report.entries = entries.iter().map(report_entry_from).collect();
    report.recompute_summary();
    report
}

/// Convert a hashed entry into a report entry.
pub fn report_entry_from(entry: &FileEntry) -> ReportEntry {
    ReportEntry {
        path: entry.relative_path.clone(),
        size: entry.size,
        mtime: entry.mtime.clone(),
        hashes: entry.hashes.clone(),
        status: entry.status,
        error_kind: entry.error_kind,
        error_message: entry.error_message.clone(),
        suffix_applied: None,
        suffix_algorithm: None,
        renamed_to: None,
    }
}

/// Run the full pipeline.
///
/// The event callback may be invoked from worker threads; it must be Sync and
/// should only forward events (for example into a channel or a UI isolate).
pub fn run_batch(
    request: &BatchRequest,
    cancel: &CancelToken,
    events: &(dyn Fn(PipelineEvent) + Sync),
) -> Result<BatchRun, PipelineError> {
    let started = std::time::Instant::now();
    if request.algorithms.is_empty() {
        return Err(PipelineError::InvalidRequest(
            "at least one algorithm is required".to_string(),
        ));
    }
    if let Some(format) = request.suffix {
        format.validate()?;
        if request.apply_renames && request.log_db.is_none() {
            return Err(PipelineError::InvalidRequest(
                "applying suffix renames requires a rename log path".to_string(),
            ));
        }
    }

    let restricted = validate_restrict_paths(&request.restrict_paths)?;

    events(PipelineEvent::Phase(BatchPhase::Scanning));
    let scanned = scan(&request.root_path, &request.scan)?;
    let (entries, skipped_paths) = exclude_log_artifacts(scanned.entries, scanned.skipped, request);
    let entries = restrict_entries(entries, restricted.as_ref());

    let hashing_total = entries.len() as u64;
    let bytes_total: u64 = entries.iter().map(|entry| entry.size).sum();
    let elapsed = elapsed_millis(started);
    events(PipelineEvent::Progress(BatchProgress {
        phase: BatchPhase::Scanning,
        completed: hashing_total,
        total: hashing_total,
        bytes_done: 0,
        bytes_total,
        percent: 0.0,
        eta_seconds: None,
        current_path: String::new(),
        elapsed_ms: elapsed,
    }));

    let hardware = request
        .hardware
        .clone()
        .unwrap_or_else(|| batchhash_hash::probe_auto().into());

    let mut run = BatchRun {
        report: build_report(
            &request.root,
            scanned.root_kind,
            request.case_sensitive,
            &request.algorithms,
            hardware,
            &entries,
        ),
        entries: Vec::new(),
        plan: None,
        rename_results: Vec::new(),
        batch_id: None,
        skipped_paths,
        cancelled: false,
        elapsed_ms: 0,
    };

    if cancel.is_cancelled() {
        mark_all_cancelled(&mut run.report, &entries);
        run.entries = entries;
        run.cancelled = true;
        run.report.generated_at = request.generated_at.clone().unwrap_or_else(now_rfc3339);
        run.report.sort_entries();
        run.report.validate()?;
        run.elapsed_ms = elapsed_millis(started);
        events(PipelineEvent::Phase(BatchPhase::Completed));
        return Ok(run);
    }

    events(PipelineEvent::Phase(BatchPhase::Hashing));
    let options = HashEntryOptions {
        hasher: HashOptions::new(request.algorithms.clone()),
        io: FileIo::Path,
    };
    let last_percent = AtomicU64::new(0);
    let hashing_events = |event: BatchEvent| match event {
        BatchEvent::Progress(snapshot) => {
            emit_hashing_progress(events, &snapshot, &last_percent, elapsed_millis(started));
        }
        BatchEvent::EntryDone(entry) => events(PipelineEvent::EntryDone(entry)),
        BatchEvent::Finished(_) => {}
    };
    let outcome = hash_entries(&entries, &options, cancel, &hashing_events);
    let hashed = outcome.entries;
    run.report.entries = hashed.iter().map(report_entry_from).collect();
    run.report.recompute_summary();
    run.cancelled = outcome.cancelled || cancel.is_cancelled();

    if let Some(format) = request.suffix {
        if !run.cancelled {
            events(PipelineEvent::Phase(BatchPhase::PlanningSuffix));
            let plan_options = RenamePlanOptions {
                format: format.into(),
                overwrite: request.overwrite,
                ..RenamePlanOptions::default()
            };
            let plan = plan_renames(&hashed, &plan_options, &StdRenameFs);
            annotate_suffix(&mut run.report, &hashed, &plan, format);
            if request.apply_renames && plan.summary.ready > 0 && !cancel.is_cancelled() {
                events(PipelineEvent::Phase(BatchPhase::ApplyingSuffix));
                let log_db = request
                    .log_db
                    .as_ref()
                    .ok_or_else(|| {
                        PipelineError::InvalidRequest("missing rename log path".to_string())
                    })?
                    .clone();
                let mut store = open_log_store(&log_db)?;
                // Recover operations a previous run left pending before this run
                // can add more of its own: applying a resolved row twice or
                // undoing a rename the log never confirmed would both corrupt the
                // batch, and plan_undo refuses to touch unresolved rows anyway.
                reconcile_log(&mut store, cancel)?;
                let batch = BatchRecord::new(
                    request.root.clone(),
                    scanned.root_kind,
                    vec![format.algorithm],
                    format,
                );
                store.create_batch(&batch)?;
                let operation_ids =
                    insert_pending_operations(&mut store, &batch, &hashed, &plan, format)?;
                // Cancellation can have landed while the `applying_suffix` event
                // was being delivered, so re-check before the first destructive
                // rename. The log batch and its pending rows are already written
                // and stay pending; the next open reconciles them.
                let results = if cancel.is_cancelled() {
                    run.cancelled = true;
                    Vec::new()
                } else {
                    // The cancellation check runs again before every single
                    // rename, so a cancellation that lands mid-batch stops the
                    // loop and leaves exactly the renamed rows in `results`.
                    let should_stop = || cancel.is_cancelled();
                    execute_plan_with_policy_and_cancel(
                        &plan,
                        &mut StdFsBackend,
                        request.overwrite,
                        &should_stop,
                    )
                };
                record_rename_results(&mut store, &operation_ids, &results)?;
                if cancel.is_cancelled() {
                    run.cancelled = true;
                }
                apply_rename_annotations(&mut run.report, &plan, &results);
                run.batch_id = Some(batch.id);
                run.rename_results = results;
            }
            run.plan = Some(plan);
        }
    }
    run.entries = hashed;

    run.report.generated_at = request.generated_at.clone().unwrap_or_else(now_rfc3339);
    run.report.recompute_summary();
    run.report.sort_entries();
    run.report.validate()?;
    run.elapsed_ms = elapsed_millis(started);
    events(PipelineEvent::Phase(BatchPhase::Completed));
    Ok(run)
}

/// Validate a caller-supplied restriction list and normalize it.
///
/// Each path must be the forward-slash root-relative form used by reports, so a
/// caller cannot smuggle an absolute path or a parent traversal into the batch.
/// An empty list means "no restriction" and is returned as [`None`].
fn validate_restrict_paths(paths: &[String]) -> Result<Option<BTreeSet<String>>, PipelineError> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut normalized = BTreeSet::new();
    for path in paths {
        validate_relative_path(path).map_err(|error| {
            PipelineError::InvalidRequest(format!("restricted path {path}: {error}"))
        })?;
        let canonical = normalize_relative(path);
        if canonical.is_empty() {
            return Err(PipelineError::InvalidRequest(format!(
                "restricted path {path} does not name a file"
            )));
        }
        normalized.insert(canonical);
    }
    Ok(Some(normalized))
}

/// Collapse `.`, `..`, empty and repeated separators into the scanner's form.
///
/// The scanner always emits a normalized path, so a caller that passes
/// `./a.mkv` or `sub//a.mkv` must still match. [`validate_relative_path`] has
/// already rejected anything that escapes the root or is absolute.
fn normalize_relative(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// Keep only the entries a restricted run is allowed to touch.
///
/// A restricted path that no longer exists simply has no entry, so it is dropped
/// instead of hashed or renamed; the caller sees the smaller result and knows the
/// previewed file vanished between preview and execution.
fn restrict_entries(entries: Vec<FileEntry>, allowed: Option<&BTreeSet<String>>) -> Vec<FileEntry> {
    let Some(allowed) = allowed else {
        return entries;
    };
    entries
        .into_iter()
        .filter(|entry| allowed.contains(&entry.relative_path))
        .collect()
}

/// Remove the rename log (and its SQLite side files) from the entry list.
///
/// The log lives inside the scanned root by default, so hashing it or planning a
/// suffix rename for it would both be wrong.
fn exclude_log_artifacts(
    entries: Vec<FileEntry>,
    mut skipped: Vec<ScanSkip>,
    request: &BatchRequest,
) -> (Vec<FileEntry>, Vec<ScanSkip>) {
    let Some(log_db) = request.log_db.as_ref() else {
        return (entries, skipped);
    };
    let log_display = crate::scan::display_path(log_db);
    let mut kept = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.display_path == log_display || entry.display_path.starts_with(&log_display) {
            skipped.push(ScanSkip {
                path: entry.display_path.clone(),
                reason: "rename log database".to_string(),
            });
            continue;
        }
        kept.push(entry);
    }
    (kept, skipped)
}

fn elapsed_millis(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn emit_hashing_progress(
    events: &(dyn Fn(PipelineEvent) + Sync),
    snapshot: &ProgressSnapshot,
    last_percent: &AtomicU64,
    elapsed_ms: u64,
) {
    let percent = snapshot.percent;
    let rounded = percent as u64;
    let previous = last_percent.swap(rounded, Ordering::Relaxed);
    if rounded != previous || percent >= 100.0 || snapshot.completed == snapshot.total {
        events(PipelineEvent::Progress(BatchProgress {
            phase: BatchPhase::Hashing,
            completed: snapshot.completed,
            total: snapshot.total,
            bytes_done: snapshot.bytes_done,
            bytes_total: snapshot.bytes_total,
            percent,
            eta_seconds: snapshot.eta_seconds,
            current_path: snapshot.current_path.clone(),
            elapsed_ms,
        }));
    }
}

fn mark_all_cancelled(report: &mut Report, entries: &[FileEntry]) {
    for (index, entry) in entries.iter().enumerate() {
        if let Some(report_entry) = report.entries.get_mut(index) {
            if entry.status.is_ok() {
                report_entry.status = batchhash_types::EntryStatus::Skipped;
                report_entry.error_kind = Some(batchhash_types::ErrorKind::Cancelled);
                report_entry.error_message = Some("cancelled before hashing".to_string());
                report_entry.hashes.clear();
            }
        }
    }
    report.recompute_summary();
}

fn annotate_suffix(
    report: &mut Report,
    entries: &[FileEntry],
    plan: &RenamePlan,
    format: SuffixFormat,
) {
    report.suffix_config = batchhash_types::SuffixConfig {
        length: format.length,
        case: format.case,
        enabled: true,
        algorithm: format.algorithm,
    };
    for row in &plan.rows {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        if entries.get(row.entry_index).is_none() {
            continue;
        }
        if let Some(report_entry) = report.entries.get_mut(row.entry_index) {
            report_entry.suffix_applied = suffix_token(&row.proposed_name);
            report_entry.suffix_algorithm = Some(format.algorithm);
            report_entry.renamed_to = Some(proposed_relative_path(&report_entry.path, row));
        }
    }
}

fn proposed_relative_path(
    original_relative: &str,
    row: &batchhash_rename::planner::RenamePlanRow,
) -> String {
    match original_relative.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{}", row.proposed_name),
        None => row.proposed_name.clone(),
    }
}

fn apply_rename_annotations(report: &mut Report, plan: &RenamePlan, results: &[RenameResult]) {
    for result in results {
        if let Some(report_entry) = report.entries.get_mut(result.entry_index) {
            if result.status != RenameResultStatus::Renamed {
                report_entry.renamed_to = None;
            }
        }
    }
    // Rows the run never reached (cancellation) were already annotated while
    // planning. The file was not renamed, so the report must not claim it was:
    // a report that advertises a rename which never happened is exactly the kind
    // of silent inconsistency the log reconciliation exists to prevent.
    for row in &plan.rows {
        if row.status != RenameRowStatus::Ready {
            continue;
        }
        if results
            .iter()
            .any(|result| result.entry_index == row.entry_index)
        {
            continue;
        }
        if let Some(report_entry) = report.entries.get_mut(row.entry_index) {
            report_entry.suffix_applied = None;
            report_entry.suffix_algorithm = None;
            report_entry.renamed_to = None;
        }
    }
}

fn suffix_token(name: &str) -> Option<String> {
    match parse_filename(name) {
        SuffixParseResult::Parsed(parsed) => Some(parsed.token),
        _ => None,
    }
}

fn open_log_store(path: &Path) -> Result<LogStore, PipelineError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| PipelineError::LogIo {
                path: crate::scan::display_path(parent),
                message: error.to_string(),
            })?;
        }
    }
    Ok(LogStore::open(path)?)
}

fn insert_pending_operations(
    store: &mut LogStore,
    batch: &BatchRecord,
    entries: &[FileEntry],
    plan: &RenamePlan,
    format: SuffixFormat,
) -> Result<std::collections::BTreeMap<usize, i64>, PipelineError> {
    let mut ids = std::collections::BTreeMap::new();
    for row in plan
        .rows
        .iter()
        .filter(|row| row.status == RenameRowStatus::Ready)
    {
        let Some(entry) = entries.get(row.entry_index) else {
            continue;
        };
        let Some(full_hash) = entry.hashes.get(&format.algorithm).cloned() else {
            continue;
        };
        let new_path = Path::new(&row.original_path).with_file_name(&row.proposed_name);
        let operation = RenameOperation::new(
            batch.id.clone(),
            row.original_path.clone(),
            crate::scan::display_path(&new_path),
            format.algorithm,
            full_hash,
            suffix_token(&row.proposed_name).unwrap_or_default(),
        );
        let id = store.insert_operation(&operation)?;
        ids.insert(row.entry_index, id);
    }
    Ok(ids)
}

fn record_rename_results(
    store: &mut LogStore,
    ids: &std::collections::BTreeMap<usize, i64>,
    results: &[RenameResult],
) -> Result<(), PipelineError> {
    for result in results {
        let Some(id) = ids.get(&result.entry_index).copied() else {
            continue;
        };
        match result.status {
            RenameResultStatus::Renamed => {
                store.record_result(id, "renamed", None, OperationState::Applied)?;
            }
            RenameResultStatus::Failed => {
                store.record_result(
                    id,
                    "failed",
                    Some(result.error.as_deref().unwrap_or("rename_failed")),
                    OperationState::Conflict,
                )?;
            }
        }
    }
    // Rows that were never reached (cancellation, or a backend that stopped early)
    // must not be left pending: a pending row cannot be told apart from a
    // crashed rename afterwards, so record the truth — the file was not renamed.
    for (entry_index, id) in ids {
        if results
            .iter()
            .any(|result| result.entry_index == *entry_index)
        {
            continue;
        }
        store.record_result(
            *id,
            "not_attempted",
            Some("rename_not_attempted"),
            OperationState::Conflict,
        )?;
    }
    Ok(())
}

/// Reconcile every operation a previous run left `pending` against the
/// filesystem.
///
/// Reconciliation is idempotent and only touches pending rows, so it runs on
/// every store open before planning an apply or an undo. Rows it cannot resolve
/// (both paths present, or neither) become conflicts with a reason instead of an
/// unconfirmed rename, which is what `plan_undo` reports for them.
///
/// Public so the CLI and the bridge can call it after opening a log they intend
/// to undo, not only on the apply path.
pub fn reconcile_log(
    store: &mut LogStore,
    cancel: &CancelToken,
) -> Result<ReconcileReport, PipelineError> {
    let should_stop = || cancel.is_cancelled();
    reconcile_pending_operations_with_cancel(store, &StdRenameFs, &should_stop)
        .map_err(PipelineError::from)
}
