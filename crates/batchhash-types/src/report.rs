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
    ///
    /// Byte totals saturate instead of overflowing, so a hostile or corrupt report
    /// can never panic the importer or wrap to a misleadingly small total.
    pub fn recompute_summary(&mut self) {
        let summary = self.summary_from_entries();
        self.summary = summary;
    }

    /// Summary implied by `entries` alone.
    ///
    /// `truncated` is carried over from the stored summary because truncation is
    /// an export decision (Markdown/TXT cap the table), not a property of the
    /// entry list.
    fn summary_from_entries(&self) -> ReportSummary {
        let mut summary = ReportSummary {
            truncated: self.summary.truncated,
            ..ReportSummary::default()
        };
        for entry in &self.entries {
            summary.total = summary.total.saturating_add(1);
            summary.total_bytes = summary.total_bytes.saturating_add(entry.size);
            match entry.status {
                EntryStatus::Ok => summary.ok = summary.ok.saturating_add(1),
                EntryStatus::Failed => summary.failed = summary.failed.saturating_add(1),
                EntryStatus::Skipped => summary.skipped = summary.skipped.saturating_add(1),
            }
        }
        summary
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
                if !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(ReportValidationError::HashNotHex {
                        path: entry.path.clone(),
                        algorithm: *algorithm,
                    });
                }
            }
            if entry.status.is_ok() && entry.hashes.is_empty() {
                return Err(ReportValidationError::OkEntryWithoutHashes(
                    entry.path.clone(),
                ));
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
        self.validate_summary()?;
        Ok(())
    }

    /// Require the stored summary to agree with the entry list.
    ///
    /// Summaries travel independently of entries in JSON and CSV, so a forged or
    /// stale summary would otherwise let a report claim totals its entries do not
    /// support. `truncated` is exempt: it records an export decision, not a
    /// property derived from `entries`.
    fn validate_summary(&self) -> Result<(), ReportValidationError> {
        let implied = self.summary_from_entries();
        let counted = [
            ("total", self.summary.total, implied.total),
            ("ok", self.summary.ok, implied.ok),
            ("failed", self.summary.failed, implied.failed),
            ("skipped", self.summary.skipped, implied.skipped),
            ("total_bytes", self.summary.total_bytes, implied.total_bytes),
        ];
        for (field, actual, expected) in counted {
            if actual != expected {
                return Err(ReportValidationError::SummaryMismatch {
                    field,
                    actual,
                    expected,
                });
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
    HashNotDeclared {
        path: String,
        algorithm: AlgorithmId,
    },
    #[error("hash for {path} ({algorithm}) has length {actual}, expected {expected}")]
    HashLengthMismatch {
        path: String,
        algorithm: AlgorithmId,
        expected: usize,
        actual: usize,
    },
    #[error("hash for {path} ({algorithm}) is not a hexadecimal digest")]
    HashNotHex {
        path: String,
        algorithm: AlgorithmId,
    },
    #[error("ok entry {0} has no hashes")]
    OkEntryWithoutHashes(String),
    #[error("summary {field} is {actual}, but entries imply {expected}")]
    SummaryMismatch {
        field: &'static str,
        actual: u64,
        expected: u64,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, size: u64, status: EntryStatus, fill: char) -> ReportEntry {
        let mut hashes = IndexMap::new();
        if status.is_ok() {
            hashes.insert(
                AlgorithmId::Sha256,
                fill.to_string().repeat(AlgorithmId::Sha256.hex_len()),
            );
        }
        ReportEntry {
            path: path.to_string(),
            size,
            mtime: None,
            hashes,
            status,
            error_kind: if status.is_ok() {
                None
            } else {
                Some(ErrorKind::PermissionDenied)
            },
            error_message: None,
            suffix_applied: None,
            suffix_algorithm: None,
            renamed_to: None,
        }
    }

    fn make_report(entries: Vec<ReportEntry>) -> Report {
        let mut report = Report::new(
            "/data/media",
            RootKind::Fs,
            false,
            vec![AlgorithmId::Sha256],
            HardwareReport::default(),
        );
        report.entries = entries;
        report.recompute_summary();
        report
    }

    #[test]
    fn recomputed_report_is_valid() {
        let report = make_report(vec![
            entry("a.mkv", 10, EntryStatus::Ok, 'a'),
            entry("b.mkv", 0, EntryStatus::Failed, 'b'),
        ]);
        assert_eq!(
            report.summary,
            ReportSummary {
                total: 2,
                ok: 1,
                failed: 1,
                skipped: 0,
                total_bytes: 10,
                truncated: false,
            }
        );
        assert!(report.validate().is_ok());
    }

    #[test]
    fn forged_summary_is_rejected() {
        let mut report = make_report(vec![entry("a.mkv", 10, EntryStatus::Ok, 'a')]);
        report.summary.total = 99;
        assert!(matches!(
            report.validate(),
            Err(ReportValidationError::SummaryMismatch {
                field: "total",
                actual: 99,
                expected: 1,
            })
        ));

        let mut report = make_report(vec![entry("a.mkv", 10, EntryStatus::Ok, 'a')]);
        report.summary.total_bytes = 1 << 40;
        assert!(matches!(
            report.validate(),
            Err(ReportValidationError::SummaryMismatch {
                field: "total_bytes",
                actual,
                ..
            }) if actual == 1 << 40
        ));

        let mut report = make_report(vec![entry("a.mkv", 10, EntryStatus::Ok, 'a')]);
        report.summary.ok = 0;
        assert!(matches!(
            report.validate(),
            Err(ReportValidationError::SummaryMismatch { field: "ok", .. })
        ));
    }

    #[test]
    fn truncated_flag_is_not_part_of_the_summary_invariant() {
        let mut report = make_report(vec![entry("a.mkv", 10, EntryStatus::Ok, 'a')]);
        report.summary.truncated = true;
        assert!(report.validate().is_ok());

        report.recompute_summary();
        assert!(
            report.summary.truncated,
            "recompute keeps the export decision"
        );
        assert!(report.validate().is_ok());
    }

    #[test]
    fn non_hex_digest_is_rejected() {
        let mut report = make_report(vec![entry("a.mkv", 10, EntryStatus::Ok, 'a')]);
        report.entries[0]
            .hashes
            .insert(AlgorithmId::Sha256, "z".repeat(64));
        assert!(matches!(
            report.validate(),
            Err(ReportValidationError::HashNotHex { .. })
        ));
    }

    #[test]
    fn summary_counters_saturate_instead_of_panicking() {
        let mut report = make_report(vec![entry("a.mkv", u64::MAX, EntryStatus::Ok, 'a')]);
        report
            .entries
            .push(entry("b.mkv", u64::MAX, EntryStatus::Ok, 'b'));
        report.recompute_summary();
        assert_eq!(report.summary.total, 2);
        assert_eq!(report.summary.total_bytes, u64::MAX);
    }
}
