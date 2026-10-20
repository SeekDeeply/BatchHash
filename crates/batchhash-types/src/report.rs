//! Report schema v1 (`docs/report-schema-v1.md`).
//!
//! The schema is versioned and stable: JSON field names, algorithm IDs and the
//! canonical algorithm order are part of the contract. Serialization order is
//! defined by the report crate (`batchhash-report`); this module only owns the
//! data model and import-side validation rules.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::algorithm::{AlgorithmId, AlgorithmSpec};
use crate::entry::{validate_relative_path, EntryStatus, ErrorKind, PathError, RootKind};

/// Current report schema version. Importers reject anything newer.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Hard cap on imported report entries (DoS guard for untrusted reports).
pub const MAX_IMPORT_ENTRIES: usize = 2_000_000;

/// Hard cap on imported report bytes (DoS guard for untrusted reports).
pub const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;

/// Letter case used for filename suffixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HexCase {
    Upper,
    Lower,
}

/// Suffix configuration recorded in a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuffixConfig {
    /// Number of hex characters written into the suffix (4..=16).
    pub length: u8,
    #[serde(rename = "case")]
    pub case: HexCase,
    pub enabled: bool,
    /// The single algorithm that produced the suffix (`suffix_algorithm`).
    pub algorithm: AlgorithmId,
}

impl Default for SuffixConfig {
    fn default() -> Self {
        Self {
            length: 5,
            case: HexCase::Upper,
            enabled: false,
            algorithm: crate::algorithm::default_suffix_algorithm(),
        }
    }
}

impl SuffixConfig {
    pub const MIN_LENGTH: u8 = 4;
    pub const MAX_LENGTH: u8 = 16;

    /// Validate suffix length bounds and algorithm usability.
    pub fn validate(&self) -> Result<(), ReportValidationError> {
        if !(Self::MIN_LENGTH..=Self::MAX_LENGTH).contains(&self.length) {
            return Err(ReportValidationError::InvalidSuffixConfig(
                "suffix length must be between 4 and 16",
            ));
        }
        if !self.algorithm.spec().suffix_usable {
            return Err(ReportValidationError::InvalidSuffixConfig(
                "algorithm is not usable for filename suffixes",
            ));
        }
        Ok(())
    }
}

/// Backend selection policy for CPU hash acceleration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BackendPolicy {
    #[default]
    Auto,
    ForcedSoft,
}

/// Hardware acceleration observability block. Values are `reported` or
/// `unknown` — never a claim of what the CPU will actually execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareReport {
    /// CPU architecture, e.g. `x86_64` / `aarch64` / `unknown`.
    pub arch: String,
    /// Detected instruction set features, e.g. `["SSE4.1", "AVX2", "SHA-NI"]`.
    pub detected_features: Vec<String>,
    pub backend_policy: BackendPolicy,
    /// Library-reported BLAKE3 backend: avx2 / avx512 / neon / portable / unknown.
    pub blake3_backend_reported: String,
    /// Library-reported SHA-256 backend: sha-ni / aarch64-sha2 / soft / unknown.
    pub sha256_backend_reported: String,
    /// Where the observation came from: crate-runtime / cpufeatures / inferred.
    pub source: String,
    /// Worker thread count used for the batch.
    pub multithread: usize,
}

impl Default for HardwareReport {
    fn default() -> Self {
        Self {
            arch: "unknown".to_string(),
            detected_features: Vec::new(),
            backend_policy: BackendPolicy::Auto,
            blake3_backend_reported: "unknown".to_string(),
            sha256_backend_reported: "unknown".to_string(),
            source: "inferred".to_string(),
            multithread: 1,
        }
    }
}

/// Aggregate counts for a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ReportSummary {
    pub total: u64,
    pub ok: u64,
    pub failed: u64,
    pub skipped: u64,
    pub total_bytes: u64,
    /// True when the producing export intentionally truncated the entry list
    /// (only Markdown/TXT exports do this; JSON/CSV never truncate).
    pub truncated: bool,
}

/// One report entry (hash result for one file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportEntry {
    /// Path relative to the report root, always `/` separated.
    pub path: String,
    pub size: u64,
    #[serde(default)]
    pub mtime: Option<String>,
    #[serde(default)]
    pub hashes: IndexMap<AlgorithmId, String>,
    pub status: EntryStatus,
    #[serde(default)]
    pub error_kind: Option<ErrorKind>,
    #[serde(default)]
    pub error_message: Option<String>,
    /// Truncated suffix written into the filename, if any (never the full hash).
    #[serde(default)]
    pub suffix_applied: Option<String>,
    /// Algorithm that produced `suffix_applied`.
    #[serde(default)]
    pub suffix_algorithm: Option<AlgorithmId>,
    /// Resulting relative path after a rename, if any.
    #[serde(default)]
    pub renamed_to: Option<String>,
}

impl ReportEntry {
    pub fn is_ok(&self) -> bool {
        self.status.is_ok()
    }
}

/// A complete BatchHash report (schema v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub tool: String,
    pub tool_version: String,
    pub generated_at: String,
    /// Absolute root (filesystem) or SAF tree display path.
    pub root: String,
    pub root_kind: RootKind,
    /// Platform-suggested case sensitivity at generation time. Diff uses the
    /// user-chosen policy, not this value.
    pub case_sensitive: bool,
    /// Algorithms present in this report, canonical registry order.
    pub algorithms: Vec<AlgorithmId>,
    pub suffix_config: SuffixConfig,
    pub hardware: HardwareReport,
    pub summary: ReportSummary,
    pub entries: Vec<ReportEntry>,
}

impl Report {
    /// Build an empty report with a default suffix configuration disabled.
    pub fn new(
        root: impl Into<String>,
        root_kind: RootKind,
        case_sensitive: bool,
        algorithms: Vec<AlgorithmId>,
        hardware: HardwareReport,
    ) -> Self {
        let mut algorithms = algorithms;
        algorithms.sort_unstable();
        algorithms.dedup();
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            tool: crate::TOOL_NAME.to_string(),
            tool_version: crate::TOOL_VERSION.to_string(),
            generated_at: String::new(),
            root: root.into(),
            root_kind,
            case_sensitive,
            suffix_config: SuffixConfig {
                algorithm: crate::algorithm::default_suffix_algorithm(),
                ..SuffixConfig::default()
            },
            algorithms,
            hardware,
            summary: ReportSummary::default(),
            entries: Vec::new(),
        }
    }

    /// Recompute the summary from `entries`.
    pub fn recompute_summary(&mut self) {
        let mut summary = ReportSummary::default();
        for entry in &self.entries {
            summary.total += 1;
            summary.total_bytes += entry.size;
            match entry.status {
                EntryStatus::Ok => summary.ok += 1,
                EntryStatus::Failed => summary.failed += 1,
                EntryStatus::Skipped => summary.skipped += 1,
            }
        }
        summary.truncated = self.summary.truncated;
        self.summary = summary;
    }

    /// Sort entries by their stored (relative) path using Unicode code point
    /// order, so export is deterministic.
    pub fn sort_entries(&mut self) {
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
    }

    /// Import-side and export-side validation. Rejects future schema versions,
    /// path traversal, malformed hashes and duplicate entries.
    pub fn validate(&self) -> Result<(), ReportValidationError> {
        if self.schema_version > REPORT_SCHEMA_VERSION {
            return Err(ReportValidationError::UnsupportedFutureSchema {
                found: self.schema_version,
                supported: REPORT_SCHEMA_VERSION,
            });
        }
        if self.schema_version == 0 {
            return Err(ReportValidationError::UnsupportedSchemaVersion);
        }
        self.suffix_config.validate()?;
        if self.entries.len() > MAX_IMPORT_ENTRIES {
            return Err(ReportValidationError::TooManyEntries {
                count: self.entries.len(),
                max: MAX_IMPORT_ENTRIES,
            });
        }
        if self.algorithms.is_empty() {
            return Err(ReportValidationError::NoAlgorithms);
        }
        let mut previous: Option<AlgorithmId> = None;
        for algorithm in &self.algorithms {
            if let Some(prev) = previous {
                if prev >= *algorithm {
                    return Err(ReportValidationError::AlgorithmsNotCanonical);
                }
            }
            previous = Some(*algorithm);
        }
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for entry in &self.entries {
            validate_relative_path(&entry.path).map_err(|source| ReportValidationError::Path {
                path: entry.path.clone(),
                source,
            })?;
            if !seen.insert(entry.path.as_str()) {
                return Err(ReportValidationError::DuplicatePath(entry.path.clone()));
            }
            for (algorithm, digest) in &entry.hashes {
                if !self.algorithms.contains(algorithm) {
                    return Err(ReportValidationError::HashNotDeclared {
                        path: entry.path.clone(),
                        algorithm: *algorithm,
                    });
                }
                let expected = algorithm.spec().hex_len;
                if digest.len() != expected {
                    return Err(ReportValidationError::HashLengthMismatch {
                        path: entry.path.clone(),
                        algorithm: *algorithm,
                        expected,
                        actual: digest.len(),
                    });
                }
            }
            if entry.status.is_ok() && entry.hashes.is_empty() {
                return Err(ReportValidationError::OkEntryWithoutHashes(entry.path.clone()));
            }
            if let Some(suffix_algorithm) = entry.suffix_algorithm {
                if !self.algorithms.contains(&suffix_algorithm) {
                    return Err(ReportValidationError::HashNotDeclared {
                        path: entry.path.clone(),
                        algorithm: suffix_algorithm,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Validation failures for report import/export.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReportValidationError {
    #[error("unsupported report schema_version")]
    UnsupportedSchemaVersion,
    #[error("report schema_version {found} is newer than supported {supported}")]
    UnsupportedFutureSchema { found: u32, supported: u32 },
    #[error("report lists no algorithms")]
    NoAlgorithms,
    #[error("report algorithms are not in canonical registry order")]
    AlgorithmsNotCanonical,
    #[error("duplicate report entry path: {0}")]
    DuplicatePath(String),
    #[error("invalid entry path {path}: {source}")]
    Path { path: String, source: PathError },
    #[error("hash for {path} uses undeclared algorithm {algorithm}")]
    HashNotDeclared { path: String, algorithm: AlgorithmId },
    #[error("hash for {path} ({algorithm}) has length {actual}, expected {expected}")]
    HashLengthMismatch {
        path: String,
        algorithm: AlgorithmId,
        expected: usize,
        actual: usize,
    },
    #[error("ok entry {0} has no hashes")]
    OkEntryWithoutHashes(String),
    #[error("invalid suffix config: {0}")]
    InvalidSuffixConfig(&'static str),
    #[error("report has {count} entries, maximum is {max}")]
    TooManyEntries { count: usize, max: usize },
}

/// Helper: canonical algorithm column order for a set of algorithms.
pub fn canonical_algorithm_order(algorithms: &[AlgorithmId]) -> Vec<AlgorithmId> {
    let mut present: Vec<AlgorithmId> = algorithms
        .iter()
        .copied()
        .filter(|id| AlgorithmId::ALL.contains(id))
        .collect();
    present.sort_unstable();
    present.dedup();
    present
}

/// Helper: spec for an algorithm (re-exported convenience).
pub fn algorithm_spec(id: AlgorithmId) -> &'static AlgorithmSpec {
    id.spec()
}
