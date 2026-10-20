//! BatchHash filename suffixes, rename planning/execution and the SQLite
//! rename log store with cross-session undo.
//!
//! The crate is split into three layers:
//!
//! * [`suffix`] formats and parses the `name[ABCDE].ext` filename suffix.
//! * [`planner`] builds a pure dry-run plan and executes it through a
//!   [`planner::RenameBackend`], so Android can substitute a Kotlin/SAF backend.
//! * [`log_store`] persists batches and rename operations in SQLite and plans
//!   crash-safe, newest-first undo.
//!
//! Planning never writes to the filesystem; every rename goes through an
//! explicit backend call and every log mutation returns a [`log_store::LogError`].

#![deny(missing_docs)]

pub mod log_store;
pub mod planner;
pub mod suffix;

pub use log_store::{BatchRecord, LogStore, OperationState, RenameOperation, UndoPlan, UndoStatus};
pub use planner::{
    execute_plan, plan_renames, RenameBackend, RenameFs, RenamePlan, RenamePlanOptions,
    RenamePlanRow, RenameResult, RenameRowStatus, StdFsBackend,
};
pub use suffix::{
    collision_probability, format_filename, parse_filename, recommended_length, ParsedSuffix,
    SuffixError, SuffixFormat, SuffixParseResult,
};
