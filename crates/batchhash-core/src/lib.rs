//! BatchHash core facade.
//!
//! This crate is the single Rust entry point for the Flutter bridge (P0.5,
//! flutter_rust_bridge 2.x) and the developer CLI. It re-exports the engine
//! crates so callers depend on one stable crate instead of three, and it owns the
//! two pieces of orchestration both surfaces need:
//!
//! * [scan] - deterministic, platform-neutral directory scanning into [FileEntry],
//! * [pipeline] - scan -> hash -> optional suffix write -> report -> rename log.
//!
//! Engines re-exported:
//!
//! * [hash] - single-pass multi-hash engine, fd input, scheduler, hardware probe
//! * [report] - PathNormalizer, Diff Engine, report export/import
//! * [rename] - suffix rules, rename planner/executor, SQLite log store
//! * [types] - AlgorithmRegistry, FileEntry, Report schema v1, Diff schema v1

#![deny(missing_docs)]

pub mod pipeline;
pub mod scan;

pub use batchhash_hash as hash;
pub use batchhash_rename as rename;
pub use batchhash_report as report;
pub use batchhash_types as types;

pub use pipeline::{
    build_report, default_case_sensitive, now_rfc3339, report_entry_from, run_batch, BatchPhase,
    BatchProgress, BatchRequest, BatchRun, PipelineError, PipelineEvent,
};
pub use scan::{scan, ScanError, ScanOptions, ScanOutcome, ScanSkip};

/// Product name stamped into reports and diffs.
pub const TOOL_NAME: &str = batchhash_types::TOOL_NAME;

/// Core crate version.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version string exposed to the Flutter bridge and CLI.
pub fn version() -> &'static str {
    TOOL_VERSION
}
