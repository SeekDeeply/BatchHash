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
//! 5. When applying, refuse to touch the filesystem after cancellation, insert
//!    pending operations, execute renames and record the outcome per row.
//! 6. Annotate the report with suffix/rename fields, recompute the summary and
//!    sort entries so export is byte-reproducible.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use batchhash_hash::{
    hash_entries, BatchEvent, CancelToken, FileIo, HashEntryOptions, HashOptions, ProgressSnapshot,
};
use batchhash_rename::log_store::{
    BatchRecord, LogError, LogStore, OperationState, RenameOperation,
};
use batchhash_rename::planner::{
    execute_plan, plan_renames, RenamePlan, RenamePlanOptions, RenameResult, RenameResultStatus,
    RenameRowStatus, StdFsBackend, StdRenameFs,
};
use batchhash_rename::suffix::{parse_filename, SuffixError, SuffixFormat, SuffixParseResult};
use batchhash_types::{
    AlgorithmId, FileEntry, HardwareReport, Report, ReportEntry, ReportValidationError, RootKind,
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
    /// Entries after hashing (input order), including hashes and statuses.
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
    pub elapsed_ms: u128,
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

    events(PipelineEvent::Phase(BatchPhase::Scanning));
    let scanned = scan(&request.root_path, &request.scan)?;
    let (entries, skipped_paths) = exclude_log_artifacts(scanned.entries, scanned.skipped, request);

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
        run.report.generated_at = request
            .generated_at
            .clone()
            .unwrap_or_else(now_rfc3339);
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
                format,
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
                let batch = BatchRecord::new(
                    request.root.clone(),
                    scanned.root_kind,
                    vec![format.algorithm],
                    format,
                );
                store.create_batch(&batch)?;
                let operation_ids =
                    insert_pending_operations(&mut store, &batch, &hashed, &plan, format)?;
                let results = execute_plan(&plan, &mut StdFsBackend);
                record_rename_results(&mut store, &operation_ids, &results)?;
                apply_rename_annotations(&mut run.report, &results);
                run.batch_id = Some(batch.id);
                run.rename_results = results;
            }
            run.plan = Some(plan);
        }
    }
    run.entries = hashed;

    run.report.generated_at = request
        .generated_at
        .clone()
        .unwrap_or_else(now_rfc3339);
    run.report.recompute_summary();
    run.report.sort_entries();
    run.report.validate()?;
    run.elapsed_ms = elapsed_millis(started);
    events(PipelineEvent::Phase(BatchPhase::Completed));
    Ok(run)
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

fn proposed_relative_path(original_relative: &str, row: &batchhash_rename::planner::RenamePlanRow) -> String {
    match original_relative.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{}", row.proposed_name),
        None => row.proposed_name.clone(),
    }
}

fn apply_rename_annotations(report: &mut Report, results: &[RenameResult]) {
    for result in results {
        if let Some(report_entry) = report.entries.get_mut(result.entry_index) {
            if result.status != RenameResultStatus::Renamed {
                report_entry.renamed_to = None;
            }
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
    Ok(())
}
