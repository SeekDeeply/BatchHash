//! BatchHash report/diff engine.
//!
//! This crate owns the report layer of BatchHash: path normalization for
//! comparison keys, the pure full-outer-join diff engine (including streaming
//! report-vs-directory comparison) and the six report export / four report
//! import formats of report schema v1.
//!
//! # Modules
//!
//! * [`pathnorm`] — [`PathNormalizer`]: the nine-step normalization pipeline
//!   (dot segments, separators, NFC, case policy, NFC again; no NFKC, no
//!   symlink resolution) shared by reports and realtime scans.
//! * [`diff`] — [`diff_reports`], [`diff_report_vs_entries`] and
//!   [`StreamingDiff`]: a deterministic full outer join with explicit
//!   `unknown`/`unstable`/`out_of_root`/`duplicate_key` semantics so a
//!   cancelled scan can never be presented as a set of removals.
//! * [`format`] — [`ReportFormat`], [`export_report`], [`export_diff`],
//!   [`import_report`] and [`detect_report_format`]: canonical JSON, RFC 4180
//!   CSV, Markdown, aligned TXT, md5sum and BSD.
//!
//! # Schema documentation
//!
//! * `docs/report-schema-v1.md` — report JSON schema, CSV columns and the
//!   md5sum/BSD conventions.
//! * `docs/diff-schema-v1.md` — diff JSON schema and the status semantics.
//! * `docs/path-normalization.md` — the comparison-key pipeline and its table
//!   of examples.
//!
//! Imported files are untrusted: size and entry caps are enforced, future
//! schema versions are rejected and entry paths are checked with
//! [`batchhash_types::validate_relative_path`] before they reach the diff
//! engine.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod diff;
pub mod format;
pub mod pathnorm;

pub use diff::{diff_report_vs_entries, diff_reports, DiffOptions, StreamingDiff};
pub use format::{
    detect_report_format, export_diff, export_report, import_report, ExportError, ImportError,
    ReportFormat,
};
pub use pathnorm::{NormalizeError, NormalizeMode, PathNormalizer};
