//! Serialisable request/response API of the native bridge.
//!
//! Every function here is pure Rust and covered by unit tests; [crate::c_api]
//! only marshals JSON across the ABI. Keeping the two layers apart is what makes
//! the bridge testable without any Dart code in the loop.

use serde::{Deserialize, Serialize};

use batchhash_core::pipeline::{run_batch, BatchRequest};
use batchhash_core::scan::{scan, ScanOptions};
use batchhash_hash::{FileIo, HashEntryOptions, HashOptions};
use batchhash_rename::log_store::{apply_undo, LogStore, OperationState};
use batchhash_rename::planner::{RenameRowStatus, StdFsBackend, StdRenameFs};
use batchhash_rename::suffix::{SuffixFormat, SuffixParseResult};
use batchhash_report::format::{detect_report_format, export_report, import_report, ReportFormat};
use batchhash_types::{
    AlgorithmId, BackendPolicy, EntryStatus, ErrorKind, FileEntry, HardwareReport, HexCase, Report,
};

/// Version of the request/response contract.
pub const CONTRACT_VERSION: u32 = 1;

/// Error returned by every bridge call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiError {
    /// Machine readable kind, for example "invalid_request" or "io".
    pub kind: String,
    /// Human readable message.
    pub message: String,
}

impl ApiError {
    /// Build an error with an explicit kind.
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }

    /// Build an "invalid_request" error.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ApiError {}

/// One algorithm as exposed to the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlgorithmDto {
    /// Stable identifier used in requests, reports and filenames.
    pub id: String,
    /// Human readable display name.
    pub display_name: String,
    /// Digest length in hex characters.
    pub hex_len: u32,
    /// Whether the algorithm is cryptographically broken (MD5, SHA-1).
    pub cryptographic: bool,
    /// Whether the algorithm may be used for filename suffixes.
    pub suffix_usable: bool,
}

/// Hardware capability report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareDto {
    /// CPU architecture, or "unknown".
    pub arch: String,
    /// Detected instruction-set features.
    pub detected_features: Vec<String>,
    /// Backend selection policy that produced this report.
    pub backend_policy: String,
    /// BLAKE3 backend reported by the library, or "unknown".
    pub blake3_backend_reported: String,
    /// SHA-256 backend reported by the library, or "unknown".
    pub sha256_backend_reported: String,
    /// Where the observation came from.
    pub source: String,
    /// Worker thread count used for batches.
    pub multithread: u32,
}

/// One hash value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HashDto {
    /// Algorithm id.
    pub algorithm: String,
    /// Lowercase hex digest.
    pub hex: String,
}

/// One file entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryDto {
    /// Root-relative path with forward slashes.
    pub path: String,
    /// Absolute (or platform) display path.
    pub display_path: String,
    /// File size in bytes.
    pub size: u64,
    /// Modification time as RFC 3339, when available.
    pub mtime: Option<String>,
    /// "ok" | "failed" | "skipped".
    pub status: String,
    /// Machine readable error kind when the entry is not ok.
    pub error_kind: Option<String>,
    /// Human readable error message when the entry is not ok.
    pub error_message: Option<String>,
    /// Hashes in canonical registry order.
    pub hashes: Vec<HashDto>,
}

/// One path the scanner could not represent as an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkippedDto {
    /// Display path that was skipped.
    pub path: String,
    /// Why it was skipped.
    pub reason: String,
}

/// Request for [probe].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeRequest {
    /// Force the software backend policy in the report.
    #[serde(default)]
    pub forced_soft: bool,
}

/// Request for [scan].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanRequest {
    /// Filesystem path of the batch root.
    pub root: String,
    /// Follow symbolic links.
    #[serde(default)]
    pub follow_links: bool,
    /// Maximum directory depth (defaults to the scanner default).
    #[serde(default)]
    pub max_depth: Option<usize>,
}

/// Result of [scan].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanResultDto {
    /// Scanned root.
    pub root: String,
    /// Entries in deterministic path order (unhashed).
    pub entries: Vec<EntryDto>,
    /// Paths that could not be represented.
    pub skipped: Vec<SkippedDto>,
    /// Sum of the entry sizes.
    pub total_bytes: u64,
    /// Whether the root is a single file.
    pub single_file: bool,
}

/// Suffix configuration for a batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuffixDto {
    /// Algorithm whose digest supplies the token.
    pub algorithm: String,
    /// Number of hex characters (4..=16).
    #[serde(default = "default_suffix_length")]
    pub length: u8,
    /// "upper" | "lower" (defaults to upper).
    #[serde(default)]
    pub case: Option<String>,
    /// Emit a self-describing "algo:hex" token.
    #[serde(default)]
    pub prefix: bool,
}

fn default_suffix_length() -> u8 {
    5
}

/// Request for [run_batch_request].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchRequestDto {
    /// Filesystem path of the batch root.
    pub root: String,
    /// Root label written into the report (defaults to the path).
    #[serde(default)]
    pub root_label: Option<String>,
    /// Algorithms to compute, at least one.
    pub algorithms: Vec<String>,
    /// Follow symbolic links.
    #[serde(default)]
    pub follow_links: bool,
    /// Case sensitivity recorded in the report (defaults per platform).
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    /// Prefix format to plan when present.
    #[serde(default)]
    pub suffix: Option<SuffixDto>,
    /// Whether planned renames are executed.
    #[serde(default)]
    pub apply_renames: bool,
    /// SQLite rename log path (required when apply_renames is true).
    #[serde(default)]
    pub log_db: Option<String>,
    /// Allow overwriting existing targets (destructive).
    #[serde(default)]
    pub overwrite: bool,
}

/// One row of the rename plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanRowDto {
    /// Original file name.
    pub original_name: String,
    /// Proposed file name.
    pub proposed_name: String,
    /// ready | conflict | skipped | missing_hash | invalid_name | no_permission | unsupported | error
    pub status: String,
    /// Explanation for non-ready rows.
    pub reason: Option<String>,
}

/// Rename plan summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSummaryDto {
    /// Rows in the plan.
    pub total: u32,
    /// Rows that can be executed.
    pub ready: u32,
    /// Rows blocked by an existing or duplicated target.
    pub conflicts: u32,
    /// Rows skipped because a suffix is already present.
    pub skipped: u32,
    /// Rows without a usable hash.
    pub missing_hash: u32,
    /// Rows with an unusable file name.
    pub invalid_name: u32,
    /// Rows in a non-writable directory.
    pub no_permission: u32,
    /// Rows the platform cannot rename.
    pub unsupported: u32,
    /// Rows that failed for another reason.
    pub errors: u32,
}

/// Result of a batch run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchResultDto {
    /// Report summary numbers.
    pub total: u64,
    /// Entries that hashed successfully.
    pub ok: u64,
    /// Entries that failed.
    pub failed: u64,
    /// Entries that were skipped.
    pub skipped_count: u64,
    /// Total bytes in the batch.
    pub total_bytes: u64,
    /// Hashed entries in input order.
    pub entries: Vec<EntryDto>,
    /// Paths the scanner could not represent.
    pub skipped: Vec<SkippedDto>,
    /// Rename plan rows, when suffix writing was requested.
    pub plan: Option<Vec<PlanRowDto>>,
    /// Rename plan summary, when suffix writing was requested.
    pub plan_summary: Option<PlanSummaryDto>,
    /// Rename log batch id when renames were applied.
    pub batch_id: Option<String>,
    /// Renames that succeeded.
    pub renamed: u64,
    /// Renames that failed.
    pub rename_failed: u64,
    /// Whether the run was cancelled.
    pub cancelled: bool,
    /// Duration in milliseconds.
    pub elapsed_ms: u64,
    /// Canonical report JSON, ready for export or diffing.
    pub report_json: String,
}

/// Request for [undo].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UndoRequestDto {
    /// SQLite rename log path.
    pub db: String,
    /// Batch id; defaults to the newest batch in the log.
    #[serde(default)]
    pub batch_id: Option<String>,
    /// Whether to execute the undo (false returns the plan only).
    #[serde(default)]
    pub apply: bool,
}

/// One undo row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UndoRowDto {
    /// Current (renamed) path.
    pub from: String,
    /// Original path.
    pub to: String,
    /// ready | conflict | already_undone
    pub status: String,
    /// Explanation for non-ready rows.
    pub reason: Option<String>,
}

/// Result of [undo].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UndoResultDto {
    /// Batch that was inspected.
    pub batch_id: String,
    /// Rows of the undo plan.
    pub rows: Vec<UndoRowDto>,
    /// Rows that were actually undone (zero for a dry run).
    pub undone: u64,
    /// Whether the undo was executed.
    pub applied: bool,
}

/// All registered algorithms in canonical order.
pub fn algorithms() -> Vec<AlgorithmDto> {
    batchhash_types::REGISTRY
        .iter()
        .map(|spec| AlgorithmDto {
            id: spec.stable_id.to_string(),
            display_name: spec.display_name.to_string(),
            hex_len: u32::try_from(spec.hex_len).unwrap_or(u32::MAX),
            cryptographic: spec.cryptographic,
            suffix_usable: spec.suffix_usable,
        })
        .collect()
}

/// Report the hardware capabilities.
pub fn probe(request: &ProbeRequest) -> HardwareDto {
    let policy = if request.forced_soft {
        BackendPolicy::ForcedSoft
    } else {
        BackendPolicy::Auto
    };
    let hardware: HardwareReport = batchhash_hash::probe(policy).into();
    HardwareDto {
        arch: hardware.arch,
        detected_features: hardware.detected_features,
        backend_policy: backend_policy_str(hardware.backend_policy).to_string(),
        blake3_backend_reported: hardware.blake3_backend_reported,
        sha256_backend_reported: hardware.sha256_backend_reported,
        source: hardware.source,
        multithread: u32::try_from(hardware.multithread).unwrap_or(u32::MAX),
    }
}

fn backend_policy_str(policy: BackendPolicy) -> &'static str {
    match policy {
        BackendPolicy::Auto => "auto",
        BackendPolicy::ForcedSoft => "forced-soft",
    }
}

/// Scan a root without hashing anything.
pub fn scan_request(request: &ScanRequest) -> Result<ScanResultDto, ApiError> {
    let options = ScanOptions {
        follow_links: request.follow_links,
        max_depth: request
            .max_depth
            .unwrap_or(batchhash_core::scan::DEFAULT_MAX_DEPTH),
        ..ScanOptions::default()
    };
    let single_file = std::path::Path::new(&request.root).is_file();
    let outcome = scan(std::path::Path::new(&request.root), &options).map_err(|error| {
        ApiError::new(
            match error {
                batchhash_core::scan::ScanError::Root { .. } => "io",
                batchhash_core::scan::ScanError::UnsupportedRoot { .. } => "unsupported",
            },
            error.to_string(),
        )
    })?;
    let total_bytes: u64 = outcome.entries.iter().map(|entry| entry.size).sum();
    Ok(ScanResultDto {
        root: request.root.clone(),
        entries: outcome.entries.iter().map(entry_dto).collect(),
        skipped: outcome
            .skipped
            .iter()
            .map(|skip| SkippedDto {
                path: skip.path.clone(),
                reason: skip.reason.clone(),
            })
            .collect(),
        total_bytes,
        single_file,
    })
}

/// Run the full pipeline (scan, hash, optional suffix write, report, log).
pub fn run_batch_request(request: &BatchRequestDto) -> Result<BatchResultDto, ApiError> {
    let algorithms = parse_algorithms(&request.algorithms)?;
    let suffix = match &request.suffix {
        Some(suffix) => Some(parse_suffix(suffix)?),
        None => None,
    };
    let mut pipeline_request =
        BatchRequest::new(std::path::PathBuf::from(&request.root), algorithms);
    if let Some(label) = &request.root_label {
        pipeline_request.root = label.clone();
    }
    if let Some(case_sensitive) = request.case_sensitive {
        pipeline_request.case_sensitive = case_sensitive;
    }
    pipeline_request.scan = ScanOptions {
        follow_links: request.follow_links,
        ..ScanOptions::default()
    };
    pipeline_request.suffix = suffix;
    pipeline_request.apply_renames = request.apply_renames && suffix.is_some();
    pipeline_request.log_db = request.log_db.as_ref().map(std::path::PathBuf::from);
    pipeline_request.overwrite = request.overwrite;

    let cancel = batchhash_hash::CancelToken::new();
    let noop = |_event: batchhash_core::pipeline::PipelineEvent| {};
    let run = run_batch(&pipeline_request, &cancel, &noop)
        .map_err(|error| ApiError::new("pipeline", error.to_string()))?;

    let report_json = export_report(&run.report, ReportFormat::Json)
        .map_err(|error| ApiError::new("export", error.to_string()))?;
    let mut renamed = 0u64;
    let mut rename_failed = 0u64;
    for result in &run.rename_results {
        match result.status {
            batchhash_rename::planner::RenameResultStatus::Renamed => renamed += 1,
            batchhash_rename::planner::RenameResultStatus::Failed => rename_failed += 1,
        }
    }
    let plan = run.plan.as_ref().map(|plan| {
        plan.rows
            .iter()
            .map(|row| PlanRowDto {
                original_name: row.original_name.clone(),
                proposed_name: row.proposed_name.clone(),
                status: plan_row_status_str(row.status).to_string(),
                reason: row.reason.clone(),
            })
            .collect::<Vec<_>>()
    });
    let plan_summary = run.plan.as_ref().map(|plan| PlanSummaryDto {
        total: u32::try_from(plan.summary.total).unwrap_or(u32::MAX),
        ready: u32::try_from(plan.summary.ready).unwrap_or(u32::MAX),
        conflicts: u32::try_from(plan.summary.conflicts).unwrap_or(u32::MAX),
        skipped: u32::try_from(plan.summary.skipped).unwrap_or(u32::MAX),
        missing_hash: u32::try_from(plan.summary.missing_hash).unwrap_or(u32::MAX),
        invalid_name: u32::try_from(plan.summary.invalid_name).unwrap_or(u32::MAX),
        no_permission: u32::try_from(plan.summary.no_permission).unwrap_or(u32::MAX),
        unsupported: u32::try_from(plan.summary.unsupported).unwrap_or(u32::MAX),
        errors: u32::try_from(plan.summary.errors).unwrap_or(u32::MAX),
    });
    Ok(BatchResultDto {
        total: run.report.summary.total,
        ok: run.report.summary.ok,
        failed: run.report.summary.failed,
        skipped_count: run.report.summary.skipped,
        total_bytes: run.report.summary.total_bytes,
        entries: run.entries.iter().map(entry_dto).collect(),
        skipped: run
            .skipped_paths
            .iter()
            .map(|skip| SkippedDto {
                path: skip.path.clone(),
                reason: skip.reason.clone(),
            })
            .collect(),
        plan,
        plan_summary,
        batch_id: run.batch_id.clone(),
        renamed,
        rename_failed,
        cancelled: run.cancelled,
        elapsed_ms: run.elapsed_ms,
        report_json,
    })
}

/// Plan and optionally execute an undo for one batch.
pub fn undo(request: &UndoRequestDto) -> Result<UndoResultDto, ApiError> {
    let mut store =
        LogStore::open(&request.db).map_err(|error| ApiError::new("log", error.to_string()))?;
    let batch_id = match &request.batch_id {
        Some(id) => id.clone(),
        None => store
            .list_batches()
            .map_err(|error| ApiError::new("log", error.to_string()))?
            .last()
            .map(|batch| batch.id.clone())
            .ok_or_else(|| ApiError::invalid("the rename log is empty"))?,
    };
    let plan = store
        .plan_undo(&batch_id, &StdRenameFs)
        .map_err(|error| ApiError::new("log", error.to_string()))?;
    let rows: Vec<UndoRowDto> = plan
        .rows
        .iter()
        .map(|row| UndoRowDto {
            from: row.from.clone(),
            to: row.to.clone(),
            status: undo_status_str(row.status).to_string(),
            reason: row.reason.clone(),
        })
        .collect();
    if !request.apply {
        return Ok(UndoResultDto {
            batch_id,
            rows,
            undone: 0,
            applied: false,
        });
    }
    let results = apply_undo(&plan, &mut StdFsBackend, &mut store)
        .map_err(|error| ApiError::new("log", error.to_string()))?;
    Ok(UndoResultDto {
        batch_id,
        rows,
        undone: u64::try_from(results.len()).unwrap_or(u64::MAX),
        applied: true,
    })
}

/// Re-export a report JSON document into another format.
pub fn export(report_json: &str, format: &str) -> Result<String, ApiError> {
    let report: Report = serde_json::from_str(report_json)
        .map_err(|error| ApiError::new("invalid_report", error.to_string()))?;
    let format = parse_format(format)?;
    export_report(&report, format).map_err(|error| ApiError::new("export", error.to_string()))
}

/// Parse a canonical suffix out of a single file name.
pub fn parse_suffix_token(name: &str) -> Option<String> {
    match batchhash_rename::suffix::parse_filename(name) {
        SuffixParseResult::Parsed(parsed) => Some(parsed.token),
        _ => None,
    }
}

fn parse_algorithms(ids: &[String]) -> Result<Vec<AlgorithmId>, ApiError> {
    if ids.is_empty() {
        return Err(ApiError::invalid("at least one algorithm is required"));
    }
    let mut algorithms = Vec::with_capacity(ids.len());
    for id in ids {
        let algorithm = AlgorithmId::from_stable_id(id)
            .ok_or_else(|| ApiError::invalid(format!("unknown algorithm '{id}'")))?;
        if !algorithms.contains(&algorithm) {
            algorithms.push(algorithm);
        }
    }
    Ok(algorithms)
}

fn parse_suffix(suffix: &SuffixDto) -> Result<SuffixFormat, ApiError> {
    let algorithm = AlgorithmId::from_stable_id(&suffix.algorithm)
        .ok_or_else(|| ApiError::invalid(format!("unknown algorithm '{}'", suffix.algorithm)))?;
    let case = match suffix.case.as_deref() {
        None | Some("upper") => HexCase::Upper,
        Some("lower") => HexCase::Lower,
        Some(other) => {
            return Err(ApiError::invalid(format!(
                "invalid suffix case '{other}' (upper or lower)"
            )))
        }
    };
    let format = SuffixFormat {
        length: suffix.length,
        case,
        algorithm,
        prefix_enabled: suffix.prefix,
    };
    format
        .validate()
        .map_err(|error| ApiError::invalid(error.to_string()))?;
    Ok(format)
}

fn parse_format(name: &str) -> Result<ReportFormat, ApiError> {
    ReportFormat::ALL
        .into_iter()
        .find(|format| {
            format.as_str() == name
                || format.extension() == name
                || (*format == ReportFormat::Markdown && name == "md")
        })
        .ok_or_else(|| ApiError::invalid(format!("unknown format '{name}'")))
}

fn entry_dto(entry: &FileEntry) -> EntryDto {
    EntryDto {
        path: entry.relative_path.clone(),
        display_path: entry.display_path.clone(),
        size: entry.size,
        mtime: entry.mtime.clone(),
        status: status_str(entry.status).to_string(),
        error_kind: entry.error_kind.map(error_kind_str).map(str::to_string),
        error_message: entry.error_message.clone(),
        hashes: entry
            .hashes
            .iter()
            .map(|(algorithm, hex)| HashDto {
                algorithm: algorithm.stable_id().to_string(),
                hex: hex.clone(),
            })
            .collect(),
    }
}

fn status_str(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Ok => "ok",
        EntryStatus::Failed => "failed",
        EntryStatus::Skipped => "skipped",
    }
}

fn error_kind_str(kind: ErrorKind) -> &'static str {
    kind.as_str()
}

fn plan_row_status_str(status: RenameRowStatus) -> &'static str {
    match status {
        RenameRowStatus::Ready => "ready",
        RenameRowStatus::ConflictTargetExists => "conflict",
        RenameRowStatus::SkippedAlreadySuffixed => "skipped",
        RenameRowStatus::MissingHash => "missing_hash",
        RenameRowStatus::InvalidName => "invalid_name",
        RenameRowStatus::NoPermission => "no_permission",
        RenameRowStatus::Unsupported => "unsupported",
        RenameRowStatus::Error => "error",
    }
}

fn undo_status_str(status: batchhash_rename::log_store::UndoStatus) -> &'static str {
    match status {
        batchhash_rename::log_store::UndoStatus::Ready => "ready",
        batchhash_rename::log_store::UndoStatus::Conflict => "conflict",
        batchhash_rename::log_store::UndoStatus::AlreadyUndone => "already_undone",
    }
}

// ---------------------------------------------------------------------------
// Real-time UI surface: hash explicit entries, plan renames, import, diff, history
// ---------------------------------------------------------------------------

/// One entry supplied by the UI (already scanned, possibly already hashed).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryInputDto {
    /// Root-relative path with forward slashes.
    pub path: String,
    /// Display path; defaults to root joined with path.
    #[serde(default)]
    pub display_path: Option<String>,
    /// Size in bytes (0 when unknown).
    #[serde(default)]
    pub size: u64,
    /// Modification time as RFC 3339, when known.
    #[serde(default)]
    pub mtime: Option<String>,
    /// Hashes already known for this entry.
    #[serde(default)]
    pub hashes: Vec<HashDto>,
}

/// Request for [hash_entries_request]: hash specific paths under a root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HashEntriesRequest {
    /// Batch root used to resolve and label the entries.
    pub root: String,
    /// Root-relative paths to hash.
    pub paths: Vec<String>,
    /// Algorithms to compute.
    pub algorithms: Vec<String>,
    /// Follow symbolic links while hashing (path mode only).
    #[serde(default)]
    pub follow_links: bool,
}

/// Request for [plan_renames_request]: pure suffix planning over known entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanRenamesRequest {
    /// Entries to plan, with the hash required by the suffix algorithm.
    pub entries: Vec<EntryInputDto>,
    /// Algorithm whose digest supplies the suffix token.
    pub algorithm: String,
    /// Number of hex characters (4..=16).
    #[serde(default = "default_suffix_length")]
    pub length: u8,
    /// "upper" | "lower" (defaults to upper).
    #[serde(default)]
    pub case: Option<String>,
    /// Emit a self-describing "algo:hex" token.
    #[serde(default)]
    pub prefix: bool,
    /// Allow overwriting existing targets.
    #[serde(default)]
    pub overwrite: bool,
}

/// Result of [plan_renames_request].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenamePlanDto {
    /// One row per candidate entry.
    pub rows: Vec<PlanRowDto>,
    /// Aggregate counters.
    pub summary: PlanSummaryDto,
}

/// Request for [import_report_request].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportReportRequest {
    /// Filesystem path of the report to import.
    pub path: String,
}

/// Request for [diff_reports_request].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffReportsRequest {
    /// Canonical JSON of the left report.
    pub left_json: String,
    /// Canonical JSON of the right report.
    pub right_json: String,
    /// Case sensitivity of comparison keys (platform default when absent).
    #[serde(default)]
    pub case_sensitive: Option<bool>,
}

/// Request for [diff_directory_request].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffDirectoryRequest {
    /// Canonical JSON of the stored report.
    pub report_json: String,
    /// Directory scanned in real time.
    pub directory: String,
    /// Case sensitivity of comparison keys (platform default when absent).
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    /// Follow symbolic links while scanning.
    #[serde(default)]
    pub follow_links: bool,
}

/// Diff counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffSummaryDto {
    /// Rows in the diff.
    pub total: u64,
    /// Identical on both sides.
    pub unchanged: u64,
    /// Present on both sides with different hashes.
    pub changed: u64,
    /// Only on the right side.
    pub added: u64,
    /// Only on the left side.
    pub removed: u64,
    /// One or both sides are not ok.
    pub unknown: u64,
    /// Basename collision.
    pub ambiguous: u64,
    /// Duplicate comparison key inside one side.
    pub duplicate_key: u64,
    /// Path outside the comparison root.
    pub out_of_root: u64,
    /// Same hash but different size.
    pub corrupt: u64,
    /// Changed between the pre and post hash metadata reads.
    pub unstable: u64,
    /// Whether the right side scan was cancelled.
    pub incomplete: bool,
}

/// One diff row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffRowDto {
    /// Comparison key.
    pub path: String,
    /// unchanged | changed | added | removed | unknown | ambiguous | duplicate_key | out_of_root | corrupt | unstable
    pub status: String,
    /// Explanation of the status.
    pub reason: String,
    /// Algorithms that differ.
    pub changed_algorithms: Vec<String>,
    /// Left side entry, when present.
    pub left: Option<EntryDto>,
    /// Right side entry, when present.
    pub right: Option<EntryDto>,
}

/// Result of a diff request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffResultDto {
    /// Counters.
    pub summary: DiffSummaryDto,
    /// Rows sorted by comparison key.
    pub rows: Vec<DiffRowDto>,
    /// Canonical diff JSON, ready for export.
    pub diff_json: String,
}

/// Request for [list_batches_request].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListBatchesRequest {
    /// SQLite rename log path.
    pub db: String,
}

/// One batch of the rename log, as shown in the history page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchInfoDto {
    /// Batch id.
    pub id: String,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Root that was renamed.
    pub root: String,
    /// Algorithms recorded for the batch.
    pub algorithms: Vec<String>,
    /// Batch status string.
    pub status: String,
    /// Rename operations recorded for the batch.
    pub file_count: u64,
    /// Operations currently marked applied.
    pub applied_count: u64,
    /// Operations already undone.
    pub undone_count: u64,
}

fn entry_from_input(input: &EntryInputDto, root: &str) -> FileEntry {
    let display = match &input.display_path {
        Some(display) => display.clone(),
        None => {
            let joined = std::path::Path::new(root).join(&input.path);
            joined.to_string_lossy().into_owned()
        }
    };
    let mut entry = FileEntry::new(
        "root",
        batchhash_types::RootKind::Fs,
        input.path.clone(),
        display,
        input.size,
        input.mtime.clone(),
    );
    for hash in &input.hashes {
        if let Some(algorithm) = AlgorithmId::from_stable_id(&hash.algorithm) {
            entry.hashes.insert(algorithm, hash.hex.clone());
        }
    }
    entry
}

/// Hash an explicit list of root-relative paths.
pub fn hash_entries_request(request: &HashEntriesRequest) -> Result<BatchResultDto, ApiError> {
    let algorithms = parse_algorithms(&request.algorithms)?;
    let mut entries = Vec::with_capacity(request.paths.len());
    let mut skipped = Vec::new();
    for path in &request.paths {
        if let Err(error) = batchhash_types::validate_relative_path(path) {
            skipped.push(SkippedDto {
                path: path.clone(),
                reason: error.to_string(),
            });
            continue;
        }
        let display = std::path::Path::new(&request.root).join(path);
        let metadata = std::fs::metadata(&display);
        let (size, mtime) = match metadata {
            Ok(metadata) => (
                metadata.len(),
                metadata
                    .modified()
                    .ok()
                    .map(|time| {
                        let converted: chrono::DateTime<chrono::Utc> = time.into();
                        converted.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                    }),
            ),
            Err(error) => {
                let mut entry = FileEntry::new(
                    "root",
                    batchhash_types::RootKind::Fs,
                    path.clone(),
                    display.to_string_lossy().into_owned(),
                    0,
                    None,
                );
                entry.fail(
                    batchhash_types::ErrorKind::NotFound,
                    format!("cannot stat the entry: {error}"),
                );
                entries.push(entry);
                continue;
            }
        };
        entries.push(FileEntry::new(
            "root",
            batchhash_types::RootKind::Fs,
            path.clone(),
            display.to_string_lossy().into_owned(),
            size,
            mtime,
        ));
    }

    let options = HashEntryOptions {
        hasher: HashOptions::new(algorithms.clone()),
        io: FileIo::Path,
    };
    let cancel = batchhash_hash::CancelToken::new();
    let outcome = batchhash_hash::hash_entries(&entries, &options, &cancel, &|_event| {});
    let hashed = outcome.entries;
    let hardware: HardwareReport = batchhash_hash::probe_auto().into();
    let mut report = batchhash_core::build_report(
        &request.root,
        batchhash_types::RootKind::Fs,
        batchhash_core::default_case_sensitive(),
        &algorithms,
        hardware,
        &hashed,
    );
    report.generated_at = batchhash_core::now_rfc3339();
    report.sort_entries();
    report
        .validate()
        .map_err(|error| ApiError::new("invalid_report", error.to_string()))?;
    let report_json = export_report(&report, ReportFormat::Json)
        .map_err(|error| ApiError::new("export", error.to_string()))?;
    Ok(BatchResultDto {
        total: report.summary.total,
        ok: report.summary.ok,
        failed: report.summary.failed,
        skipped_count: report.summary.skipped,
        total_bytes: report.summary.total_bytes,
        entries: hashed.iter().map(entry_dto).collect(),
        skipped,
        plan: None,
        plan_summary: None,
        batch_id: None,
        renamed: 0,
        rename_failed: 0,
        cancelled: outcome.cancelled,
        elapsed_ms: u64::try_from(outcome.elapsed_ms).unwrap_or(u64::MAX),
        report_json,
    })
}

/// Plan suffix renames from entries that already carry their hashes.
pub fn plan_renames_request(request: &PlanRenamesRequest) -> Result<RenamePlanDto, ApiError> {
    let suffix = parse_suffix(&SuffixDto {
        algorithm: request.algorithm.clone(),
        length: request.length,
        case: request.case.clone(),
        prefix: request.prefix,
    })?;
    let entries: Vec<FileEntry> = request
        .entries
        .iter()
        .map(|entry| entry_from_input(entry, "."))
        .collect();
    let options = batchhash_rename::planner::RenamePlanOptions {
        format: suffix,
        overwrite: request.overwrite,
        ..batchhash_rename::planner::RenamePlanOptions::default()
    };
    let plan = batchhash_rename::planner::plan_renames(
        &entries,
        &options,
        &batchhash_rename::planner::StdRenameFs,
    );
    Ok(RenamePlanDto {
        rows: plan
            .rows
            .iter()
            .map(|row| PlanRowDto {
                original_name: row.original_name.clone(),
                proposed_name: row.proposed_name.clone(),
                status: plan_row_status_str(row.status).to_string(),
                reason: row.reason.clone(),
            })
            .collect(),
        summary: PlanSummaryDto {
            total: u32::try_from(plan.summary.total).unwrap_or(u32::MAX),
            ready: u32::try_from(plan.summary.ready).unwrap_or(u32::MAX),
            conflicts: u32::try_from(plan.summary.conflicts).unwrap_or(u32::MAX),
            skipped: u32::try_from(plan.summary.skipped).unwrap_or(u32::MAX),
            missing_hash: u32::try_from(plan.summary.missing_hash).unwrap_or(u32::MAX),
            invalid_name: u32::try_from(plan.summary.invalid_name).unwrap_or(u32::MAX),
            no_permission: u32::try_from(plan.summary.no_permission).unwrap_or(u32::MAX),
            unsupported: u32::try_from(plan.summary.unsupported).unwrap_or(u32::MAX),
            errors: u32::try_from(plan.summary.errors).unwrap_or(u32::MAX),
        },
    })
}

/// Import a report file and return its canonical JSON.
pub fn import_report_request(request: &ImportReportRequest) -> Result<String, ApiError> {
    let bytes = std::fs::read(&request.path)
        .map_err(|error| ApiError::new("io", format!("cannot read the report: {error}")))?;
    let name = std::path::Path::new(&request.path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let format = detect_report_format(&name, &bytes)
        .ok_or_else(|| ApiError::new("io", "cannot detect the report format"))?;
    let report = import_report(&bytes, format)
        .map_err(|error| ApiError::new("invalid_report", error.to_string()))?;
    export_report(&report, ReportFormat::Json)
        .map_err(|error| ApiError::new("export", error.to_string()))
}

fn diff_dto(diff: &batchhash_types::DiffReport) -> DiffResultDto {
    let summary = DiffSummaryDto {
        total: diff.summary.total,
        unchanged: diff.summary.unchanged,
        changed: diff.summary.changed,
        added: diff.summary.added,
        removed: diff.summary.removed,
        unknown: diff.summary.unknown,
        ambiguous: diff.summary.ambiguous,
        duplicate_key: diff.summary.duplicate_key,
        out_of_root: diff.summary.out_of_root,
        corrupt: diff.summary.corrupt,
        unstable: diff.summary.unstable,
        incomplete: diff.summary.incomplete,
    };
    let rows = diff
        .rows
        .iter()
        .map(|row| DiffRowDto {
            path: row.key.clone(),
            status: row.status.as_str().to_string(),
            reason: row.reason.clone(),
            changed_algorithms: row
                .changed_algorithms
                .iter()
                .map(|algorithm| algorithm.stable_id().to_string())
                .collect(),
            left: row.left.as_ref().map(entry_dto_diff),
            right: row.right.as_ref().map(entry_dto_diff),
        })
        .collect();
    let diff_json = batchhash_report::format::export_diff(diff, ReportFormat::Json)
        .unwrap_or_else(|_| "{}".to_string());
    DiffResultDto {
        summary,
        rows,
        diff_json,
    }
}

fn entry_dto_diff(entry: &batchhash_types::DiffEntry) -> EntryDto {
    EntryDto {
        path: entry.path.clone(),
        display_path: entry.path.clone(),
        size: entry.size,
        mtime: entry.mtime.clone(),
        status: status_str(entry.status).to_string(),
        error_kind: entry.error_kind.map(|kind| kind.as_str().to_string()),
        error_message: entry.error_message.clone(),
        hashes: entry
            .hashes
            .iter()
            .map(|(algorithm, hex)| HashDto {
                algorithm: algorithm.stable_id().to_string(),
                hex: hex.clone(),
            })
            .collect(),
    }
}

fn diff_options(case_sensitive: Option<bool>) -> batchhash_report::diff::DiffOptions {
    let case_sensitive = case_sensitive.unwrap_or_else(batchhash_core::default_case_sensitive);
    batchhash_report::diff::DiffOptions::new()
        .with_case_sensitive(case_sensitive)
        .with_generated_at(batchhash_core::now_rfc3339())
}

fn parse_report_json(json: &str) -> Result<Report, ApiError> {
    serde_json::from_str(json).map_err(|error| ApiError::new("invalid_report", error.to_string()))
}

/// Compare two stored reports.
pub fn diff_reports_request(request: &DiffReportsRequest) -> Result<DiffResultDto, ApiError> {
    let left = parse_report_json(&request.left_json)?;
    let right = parse_report_json(&request.right_json)?;
    let diff = batchhash_report::diff::diff_reports(&left, &right, &diff_options(request.case_sensitive))
        .map_err(|error| ApiError::new("diff", error.to_string()))?;
    Ok(diff_dto(&diff))
}

/// Compare a stored report against a live directory scan.
pub fn diff_directory_request(
    request: &DiffDirectoryRequest,
) -> Result<DiffResultDto, ApiError> {
    let report = parse_report_json(&request.report_json)?;
    let scan_options = ScanOptions {
        follow_links: request.follow_links,
        ..ScanOptions::default()
    };
    let scanned = scan(std::path::Path::new(&request.directory), &scan_options)
        .map_err(|error| ApiError::new("io", error.to_string()))?;
    let options = HashEntryOptions {
        hasher: HashOptions::new(report.algorithms.clone()),
        io: FileIo::Path,
    };
    let cancel = batchhash_hash::CancelToken::new();
    let outcome = batchhash_hash::hash_entries(&scanned.entries, &options, &cancel, &|_event| {});
    let mut diff_options = diff_options(request.case_sensitive);
    diff_options.right_kind = batchhash_types::DiffSourceKind::RealtimeDirectory;
    diff_options.left_label = "report".to_string();
    diff_options.right_label = request.directory.clone();
    let right_source = batchhash_types::DiffSource {
        kind: batchhash_types::DiffSourceKind::RealtimeDirectory,
        label: request.directory.clone(),
        generated_at: None,
        scan_time: Some(batchhash_core::now_rfc3339()),
    };
    let diff = batchhash_report::diff::diff_report_vs_entries(
        &report,
        &outcome.entries,
        right_source,
        &diff_options,
    )
    .map_err(|error| ApiError::new("diff", error.to_string()))?;
    Ok(diff_dto(&diff))
}

/// List the batches stored in a rename log, newest first.
pub fn list_batches_request(request: &ListBatchesRequest) -> Result<Vec<BatchInfoDto>, ApiError> {
    let store = LogStore::open(&request.db)
        .map_err(|error| ApiError::new("log", error.to_string()))?;
    let batches = store
        .list_batches()
        .map_err(|error| ApiError::new("log", error.to_string()))?;
    let mut out = Vec::with_capacity(batches.len());
    for batch in batches {
        let operations = store
            .operations_for_batch(&batch.id)
            .map_err(|error| ApiError::new("log", error.to_string()))?;
        let applied = operations
            .iter()
            .filter(|operation| operation.state == OperationState::Applied)
            .count();
        let undone = operations
            .iter()
            .filter(|operation| operation.state == OperationState::Undone)
            .count();
        out.push(BatchInfoDto {
            id: batch.id,
            created_at: batch.created_at,
            root: batch.root,
            algorithms: batch
                .algorithms
                .iter()
                .map(|algorithm| algorithm.stable_id().to_string())
                .collect(),
            status: batch.status,
            file_count: u64::try_from(operations.len()).unwrap_or(u64::MAX),
            applied_count: u64::try_from(applied).unwrap_or(u64::MAX),
            undone_count: u64::try_from(undone).unwrap_or(u64::MAX),
        });
    }
    Ok(out)
}

