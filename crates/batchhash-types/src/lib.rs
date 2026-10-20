//! BatchHash shared types: the algorithm registry, the unified file-entry/root
//! model, report schema v1 and diff schema v1.
//!
//! This crate is intentionally dependency-light and I/O-free. It is the
//! contract every other BatchHash crate (hash engine, report/diff, rename
//! planner, Flutter bridge) compiles against.
//!
//! Schema documentation:
//! * `docs/algorithm-registry.md`
//! * `docs/file-entry-root-spec.md`
//! * `docs/report-schema-v1.md`
//! * `docs/diff-schema-v1.md`

pub mod algorithm;
pub mod diff;
pub mod entry;
pub mod report;

pub use algorithm::{
    default_suffix_algorithm, is_hex, is_hex_digest_of_len, normalize_hex, spec_by_stable_id,
    spec_by_suffix_prefix, AlgorithmId, AlgorithmParseError, AlgorithmSpec, REGISTRY,
};
pub use diff::{
    DiffEntry, DiffMode, DiffReport, DiffRow, DiffSource, DiffSourceKind, DiffStatus,
    DiffStrategy, DiffSummary, DiffValidationError, DIFF_SCHEMA_VERSION,
};
pub use entry::{
    join_display_path, validate_relative_path, EntryStatus, ErrorKind, FileEntry, PathError,
    PlatformMeta, RootKind, RootSpec,
};
pub use report::{
    algorithm_spec, canonical_algorithm_order, BackendPolicy, HardwareReport, HexCase, Report,
    ReportEntry, ReportSummary, ReportValidationError, SuffixConfig, MAX_IMPORT_BYTES,
    MAX_IMPORT_ENTRIES, REPORT_SCHEMA_VERSION,
};

/// Product name used in reports and exported documents.
pub const TOOL_NAME: &str = "BatchHash";

/// Version stamped into reports and diffs.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
