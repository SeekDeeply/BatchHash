//! Diff schema v1 (`docs/diff-schema-v1.md`).
//!
//! Diff rows are produced by the pure full-outer-join engine in
//! `batchhash-report` and exported as this versioned document. Strategy
//! metadata is part of the schema: a diff without its comparison policy is not
//! reproducible.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::algorithm::AlgorithmId;
use crate::entry::{EntryStatus, ErrorKind};

/// Current diff schema version.
pub const DIFF_SCHEMA_VERSION: u32 = 1;

/// Comparison key strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffMode {
    /// Compare paths relative to a common root (default).
    RootRelative,
    /// Degraded mode: compare by basename only (ambiguous collisions are flagged).
    Basename,
    /// Compare by absolute path; only meaningful on one machine.
    Absolute,
}

/// How the two roots were aligned before comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootAlignment {
    SameRoot,
    UserCommonRoot,
    BasenameOnly,
    Absolute,
}

/// What kind of side feeds a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffSourceKind {
    Report,
    RealtimeDirectory,
    ChecksumFile,
}

/// One side of a comparison, with the provenance needed to reproduce it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSource {
    pub kind: DiffSourceKind,
    /// File name or directory path shown to the user.
    pub label: String,
    /// Report generation time (`generated_at`) for report sides.
    #[serde(default)]
    pub generated_at: Option<String>,
    /// Scan start/end time for realtime directory sides.
    #[serde(default)]
    pub scan_time: Option<String>,
}

/// Complete comparison policy for one diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStrategy {
    pub mode: DiffMode,
    pub case_sensitive: bool,
    pub root_alignment: RootAlignment,
    /// Algorithms declared by the sides (canonical order).
    pub algorithms: Vec<AlgorithmId>,
    /// Algorithms actually compared (the intersection), canonical order.
    pub algorithms_compared: Vec<AlgorithmId>,
    pub left: DiffSource,
    pub right: DiffSource,
}

/// Row status for a full outer join of two entry sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    /// Both sides present; every compared algorithm hash is equal.
    Unchanged,
    /// Both sides present; at least one compared hash differs.
    Changed,
    /// Only the right side has this key.
    Added,
    /// Only the left side has this key (never fabricated after a cancellation).
    Removed,
    /// One or both sides are not `ok`.
    Unknown,
    /// Basename mode collision: same basename in multiple directories.
    Ambiguous,
    /// Duplicate normalized key inside a single side.
    DuplicateKey,
    /// Path falls outside the user-chosen common root.
    OutOfRoot,
    /// Same hash but different size — data anomaly, highlighted in the UI.
    Corrupt,
    /// File changed between the pre-hash and post-hash `(size, mtime)` reads.
    Unstable,
}

impl DiffStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            DiffStatus::Unchanged => "unchanged",
            DiffStatus::Changed => "changed",
            DiffStatus::Added => "added",
            DiffStatus::Removed => "removed",
            DiffStatus::Unknown => "unknown",
            DiffStatus::Ambiguous => "ambiguous",
            DiffStatus::DuplicateKey => "duplicate_key",
            DiffStatus::OutOfRoot => "out_of_root",
            DiffStatus::Corrupt => "corrupt",
            DiffStatus::Unstable => "unstable",
        }
    }

    /// Whether this status represents an actionable difference (for "仅差异" filters).
    pub const fn is_difference(self) -> bool {
        matches!(
            self,
            DiffStatus::Changed
                | DiffStatus::Added
                | DiffStatus::Removed
                | DiffStatus::Corrupt
                | DiffStatus::Unstable
        )
    }

    /// Whether this status needs an explicit warning affordance in the UI.
    pub const fn is_warning(self) -> bool {
        matches!(
            self,
            DiffStatus::Ambiguous
                | DiffStatus::DuplicateKey
                | DiffStatus::OutOfRoot
                | DiffStatus::Unknown
                | DiffStatus::Unstable
        )
    }
}

/// One side's entry snapshot inside a diff row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffEntry {
    pub path: String,
    pub size: u64,
    #[serde(default)]
    pub mtime: Option<String>,
    pub status: EntryStatus,
    #[serde(default)]
    pub hashes: IndexMap<AlgorithmId, String>,
    #[serde(default)]
    pub error_kind: Option<ErrorKind>,
    #[serde(default)]
    pub error_message: Option<String>,
}

/// One row of the joined result. Sorted by normalized key (Unicode code point
/// order), tie-broken by raw path, for byte-reproducible golden tests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffRow {
    /// Normalized comparison key (may be a basename in degraded mode).
    pub key: String,
    pub status: DiffStatus,
    /// Human-readable explanation (policy id, mismatch summary, error reason).
    pub reason: String,
    #[serde(default)]
    pub algorithms_compared: Vec<AlgorithmId>,
    #[serde(default)]
    pub changed_algorithms: Vec<AlgorithmId>,
    #[serde(default)]
    pub left_status: Option<EntryStatus>,
    #[serde(default)]
    pub right_status: Option<EntryStatus>,
    #[serde(default)]
    pub left: Option<DiffEntry>,
    #[serde(default)]
    pub right: Option<DiffEntry>,
}

/// Aggregate diff counts. `incomplete` is true when a realtime scan was
/// cancelled, so `removed` must not be read as authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DiffSummary {
    pub total: u64,
    pub unchanged: u64,
    pub changed: u64,
    pub added: u64,
    pub removed: u64,
    pub unknown: u64,
    pub ambiguous: u64,
    pub duplicate_key: u64,
    pub out_of_root: u64,
    pub corrupt: u64,
    pub unstable: u64,
    pub incomplete: bool,
}

impl DiffSummary {
    /// Recompute counts from rows, preserving the `incomplete` flag.
    pub fn from_rows(rows: &[DiffRow], incomplete: bool) -> Self {
        let mut summary = DiffSummary {
            total: rows.len() as u64,
            incomplete,
            ..DiffSummary::default()
        };
        for row in rows {
            match row.status {
                DiffStatus::Unchanged => summary.unchanged += 1,
                DiffStatus::Changed => summary.changed += 1,
                DiffStatus::Added => summary.added += 1,
                DiffStatus::Removed => summary.removed += 1,
                DiffStatus::Unknown => summary.unknown += 1,
                DiffStatus::Ambiguous => summary.ambiguous += 1,
                DiffStatus::DuplicateKey => summary.duplicate_key += 1,
                DiffStatus::OutOfRoot => summary.out_of_root += 1,
                DiffStatus::Corrupt => summary.corrupt += 1,
                DiffStatus::Unstable => summary.unstable += 1,
            }
        }
        summary
    }
}

/// Exported diff document (schema v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffReport {
    pub schema_version: u32,
    pub tool: String,
    pub tool_version: String,
    pub generated_at: String,
    pub strategy: DiffStrategy,
    pub summary: DiffSummary,
    pub rows: Vec<DiffRow>,
}

impl DiffReport {
    /// Validate schema version, key uniqueness and count consistency.
    pub fn validate(&self) -> Result<(), DiffValidationError> {
        if self.schema_version > DIFF_SCHEMA_VERSION {
            return Err(DiffValidationError::UnsupportedFutureSchema {
                found: self.schema_version,
                supported: DIFF_SCHEMA_VERSION,
            });
        }
        if self.schema_version == 0 {
            return Err(DiffValidationError::UnsupportedSchemaVersion);
        }
        if self.rows.len() as u64 != self.summary.total {
            return Err(DiffValidationError::SummaryMismatch {
                rows: self.rows.len() as u64,
                total: self.summary.total,
            });
        }
        let mut seen = std::collections::BTreeSet::new();
        for row in &self.rows {
            if row.key.is_empty() {
                return Err(DiffValidationError::EmptyKey);
            }
            if !seen.insert(row.key.as_str()) {
                return Err(DiffValidationError::DuplicateKey(row.key.clone()));
            }
        }
        Ok(())
    }
}

/// Diff document validation failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiffValidationError {
    #[error("unsupported diff schema_version")]
    UnsupportedSchemaVersion,
    #[error("diff schema_version {found} is newer than supported {supported}")]
    UnsupportedFutureSchema { found: u32, supported: u32 },
    #[error("diff summary total {total} does not match {rows} rows")]
    SummaryMismatch { rows: u64, total: u64 },
    #[error("diff row has an empty key")]
    EmptyKey,
    #[error("diff contains duplicate key: {0}")]
    DuplicateKey(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_are_stable() {
        for (status, expected) in [
            (DiffStatus::Unchanged, "unchanged"),
            (DiffStatus::Changed, "changed"),
            (DiffStatus::Added, "added"),
            (DiffStatus::Removed, "removed"),
            (DiffStatus::Unknown, "unknown"),
            (DiffStatus::Ambiguous, "ambiguous"),
            (DiffStatus::DuplicateKey, "duplicate_key"),
            (DiffStatus::OutOfRoot, "out_of_root"),
            (DiffStatus::Corrupt, "corrupt"),
            (DiffStatus::Unstable, "unstable"),
        ] {
            assert_eq!(status.as_str(), expected);
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
        }
    }

    #[test]
    fn summary_counts_every_status() {
        let rows: Vec<DiffRow> = [
            DiffStatus::Unchanged,
            DiffStatus::Changed,
            DiffStatus::Added,
            DiffStatus::Removed,
            DiffStatus::Unknown,
            DiffStatus::Ambiguous,
            DiffStatus::DuplicateKey,
            DiffStatus::OutOfRoot,
            DiffStatus::Corrupt,
            DiffStatus::Unstable,
        ]
        .into_iter()
        .map(|status| DiffRow {
            key: status.as_str().to_string(),
            status,
            reason: String::new(),
            algorithms_compared: Vec::new(),
            changed_algorithms: Vec::new(),
            left_status: None,
            right_status: None,
            left: None,
            right: None,
        })
        .collect();
        let summary = DiffSummary::from_rows(&rows, true);
        assert_eq!(summary.total, 10);
        assert_eq!(summary.changed, 1);
        assert_eq!(summary.duplicate_key, 1);
        assert!(summary.incomplete);
    }
}
