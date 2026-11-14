//! SQLite persistence for rename batches, operations and cross-session undo.
//!
//! The store owns the durable rename log: one `batch` row per planned rename
//! batch and one `rename_operation` row per file. The operation state machine is
//! `pending -> applied -> undone | conflict`; every write is an explicit
//! [`LogStore`] call returning [`LogError`] and schema changes are versioned in
//! the `schema_migration` table.
//!
//! A row is written as `pending` before the file is touched and as `applied`
//! after the backend confirms the rename. If the process dies in between, the
//! log alone cannot say whether the file moved, so
//! [`reconcile_pending_operations`] re-inspects every pending row against the
//! filesystem and records the verdict (`applied`, or `conflict` with a
//! `not_applied`/`ambiguous` reason). It only touches rows that are still
//! pending, so it is safe to run on every store open.
//!
//! Undo is planned, not assumed: [`LogStore::plan_undo`] walks the batch
//! newest-first and marks each operation [`UndoStatus::Ready`],
//! [`UndoStatus::Conflict`] or [`UndoStatus::AlreadyUndone`] by checking that the
//! renamed file still exists and the original name is free. [`apply_undo`]
//! re-checks the filesystem again immediately before each reverse rename, uses
//! [`RenameBackend::rename_no_replace`] so a name that was claimed in the
//! meantime is never overwritten, and renames only ready rows.

use std::fmt;
use std::path::Path;

use batchhash_types::{AlgorithmId, HexCase, RootKind};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::planner::{RenameBackend, RenameFs, StdRenameFs};
use crate::suffix::SuffixFormat;

const SCHEMA_VERSION: u32 = 1;

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS batch (
    id TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    root TEXT NOT NULL,
    root_kind TEXT NOT NULL,
    algorithms TEXT NOT NULL,
    suffix_length INTEGER NOT NULL,
    suffix_case TEXT NOT NULL,
    suffix_algorithm TEXT NOT NULL,
    status TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS rename_operation (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id TEXT NOT NULL REFERENCES batch(id),
    original_path TEXT NOT NULL,
    new_path TEXT NOT NULL,
    algorithm TEXT NOT NULL,
    full_hash TEXT NOT NULL,
    suffix TEXT NOT NULL,
    op_type TEXT NOT NULL,
    result TEXT,
    error_kind TEXT,
    created_at TEXT NOT NULL,
    undone_at TEXT,
    state TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_rename_operation_batch_id
    ON rename_operation(batch_id);
CREATE INDEX IF NOT EXISTS idx_rename_operation_state
    ON rename_operation(state);
"#;

const MIGRATIONS: &[&str] = &[MIGRATION_V1];

/// Lifecycle state of a recorded rename operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Recorded locally but not yet confirmed by the platform backend.
    #[default]
    Pending,
    /// The backend confirmed the rename.
    Applied,
    /// The rename was undone and the original name restored.
    Undone,
    /// The rename could not be applied, or its result conflicts with the
    /// current filesystem state.
    Conflict,
}

impl OperationState {
    /// Database representation stored in the `state` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            OperationState::Pending => "pending",
            OperationState::Applied => "applied",
            OperationState::Undone => "undone",
            OperationState::Conflict => "conflict",
        }
    }

    /// Parse the database representation stored in the `state` column.
    ///
    /// Returns [`None`] for an unknown state string.
    pub fn from_str_name(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(OperationState::Pending),
            "applied" => Some(OperationState::Applied),
            "undone" => Some(OperationState::Undone),
            "conflict" => Some(OperationState::Conflict),
            _ => None,
        }
    }
}

impl fmt::Display for OperationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One persisted rename batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRecord {
    /// Batch identity (UUID string).
    pub id: String,
    /// Creation timestamp in RFC 3339 form.
    pub created_at: String,
    /// Batch root display path or SAF tree display name.
    pub root: String,
    /// How the batch root is addressed.
    pub root_kind: RootKind,
    /// Algorithms hashed for this batch, canonical registry order.
    pub algorithms: Vec<AlgorithmId>,
    /// Suffix length in hex characters.
    pub suffix_length: u8,
    /// Letter case of the suffix token.
    pub suffix_case: HexCase,
    /// Algorithm that produced the suffix token.
    pub suffix_algorithm: AlgorithmId,
    /// Free-form batch status (for example `active` or `completed`).
    pub status: String,
}

impl BatchRecord {
    /// Batch status written by [`BatchRecord::new`].
    pub const STATUS_ACTIVE: &str = "active";

    /// Create a new batch record with a generated UUID and current timestamp.
    pub fn new(
        root: impl Into<String>,
        root_kind: RootKind,
        mut algorithms: Vec<AlgorithmId>,
        format: SuffixFormat,
    ) -> Self {
        algorithms.sort_unstable();
        algorithms.dedup();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: now_rfc3339(),
            root: root.into(),
            root_kind,
            algorithms,
            suffix_length: format.length,
            suffix_case: format.case,
            suffix_algorithm: format.algorithm,
            status: Self::STATUS_ACTIVE.to_string(),
        }
    }
}

/// One persisted rename operation (one file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameOperation {
    /// Database row id; zero before [`LogStore::insert_operation`] assigns it.
    pub id: i64,
    /// Owning batch id.
    pub batch_id: String,
    /// Original absolute or root-relative path.
    pub original_path: String,
    /// Path after the rename.
    pub new_path: String,
    /// Algorithm whose digest produced the suffix.
    pub algorithm: AlgorithmId,
    /// Full lowercase hex digest recorded for verification.
    pub full_hash: String,
    /// Truncated suffix token that was written (without brackets).
    pub suffix: String,
    /// Operation type (for example `rename`).
    pub op_type: String,
    /// Backend result text, once known.
    pub result: Option<String>,
    /// Machine-readable error kind, once known.
    pub error_kind: Option<String>,
    /// Creation timestamp in RFC 3339 form.
    pub created_at: String,
    /// Timestamp of the undo, once undone.
    pub undone_at: Option<String>,
    /// Current lifecycle state.
    pub state: OperationState,
}

impl RenameOperation {
    /// Operation type written by [`RenameOperation::new`].
    pub const TYPE_RENAME: &str = "rename";

    /// Create a pending rename operation with a current timestamp.
    pub fn new(
        batch_id: impl Into<String>,
        original_path: impl Into<String>,
        new_path: impl Into<String>,
        algorithm: AlgorithmId,
        full_hash: impl Into<String>,
        suffix: impl Into<String>,
    ) -> Self {
        Self {
            id: 0,
            batch_id: batch_id.into(),
            original_path: original_path.into(),
            new_path: new_path.into(),
            algorithm,
            full_hash: full_hash.into(),
            suffix: suffix.into(),
            op_type: Self::TYPE_RENAME.to_string(),
            result: None,
            error_kind: None,
            created_at: now_rfc3339(),
            undone_at: None,
            state: OperationState::Pending,
        }
    }
}

/// Version of the portable rename-log archive format.
///
/// Bumped only when the document shape changes incompatibly; importers accept
/// every version they know and reject anything newer than [`ARCHIVE_VERSION`], so
/// a newer export never silently loses fields on an older build.
pub const ARCHIVE_VERSION: u32 = 1;

/// One archived rename operation.
///
/// The database row id is deliberately absent: it is a local autoincrement value
/// that means nothing on another machine, and an archive a human might read or
/// diff should not carry it. Operations reference their batch by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedOperation {
    /// Owning batch id (the portable reference, not a numeric foreign key).
    pub batch_id: String,
    /// Original path recorded by the rename.
    pub original_path: String,
    /// Path after the rename.
    pub new_path: String,
    /// Algorithm whose digest produced the suffix.
    pub algorithm: AlgorithmId,
    /// Full hex digest recorded for verification.
    pub full_hash: String,
    /// Truncated suffix token written into the filename, without brackets.
    pub suffix: String,
    /// Operation type, for example `rename`.
    pub op_type: String,
    /// Backend result text, once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Machine-readable error kind, once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    /// Creation timestamp in RFC 3339 form.
    pub created_at: String,
    /// Timestamp of the undo, once undone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_at: Option<String>,
    /// Lifecycle state at export time.
    pub state: String,
}

impl ArchivedOperation {
    /// Convert a stored operation into its portable form.
    pub fn from_operation(operation: &RenameOperation) -> Self {
        Self {
            batch_id: operation.batch_id.clone(),
            original_path: operation.original_path.clone(),
            new_path: operation.new_path.clone(),
            algorithm: operation.algorithm,
            full_hash: operation.full_hash.clone(),
            suffix: operation.suffix.clone(),
            op_type: operation.op_type.clone(),
            result: operation.result.clone(),
            error_kind: operation.error_kind.clone(),
            created_at: operation.created_at.clone(),
            undone_at: operation.undone_at.clone(),
            state: operation.state.as_str().to_string(),
        }
    }

    /// Rebuild a storable operation; the row id is assigned by the store.
    ///
    /// Returns [`LogError::InvalidStoredValue`] when the archived state is not a
    /// known lifecycle state, so a hand-edited archive fails loudly instead of
    /// being imported as `pending`.
    pub fn to_operation(&self) -> Result<RenameOperation, LogError> {
        let state = OperationState::from_str_name(&self.state).ok_or_else(|| {
            LogError::InvalidStoredValue(format!("unknown state: {}", self.state))
        })?;
        Ok(RenameOperation {
            id: 0,
            batch_id: self.batch_id.clone(),
            original_path: self.original_path.clone(),
            new_path: self.new_path.clone(),
            algorithm: self.algorithm,
            full_hash: self.full_hash.clone(),
            suffix: self.suffix.clone(),
            op_type: self.op_type.clone(),
            result: self.result.clone(),
            error_kind: self.error_kind.clone(),
            created_at: self.created_at.clone(),
            undone_at: self.undone_at.clone(),
            state,
        })
    }
}

/// One archived batch with its operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedBatch {
    /// The batch record, unchanged.
    pub batch: BatchRecord,
    /// Operations belonging to this batch, in insertion order.
    #[serde(default)]
    pub operations: Vec<ArchivedOperation>,
}

/// A whole rename log in one portable JSON document.
///
/// The document is meant to be human readable and diffable: keys are stable,
/// batches are self-contained and nothing refers to a local row id. Import it with
/// [`import_archive`] to restore the log, or open it in a text editor to see
/// exactly which files a batch renamed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameLogArchive {
    /// Archive format version ([`ARCHIVE_VERSION`]).
    pub version: u32,
    /// Tool that produced the archive, for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Tool version, for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    /// Export timestamp in RFC 3339 form.
    pub exported_at: String,
    /// Every exported batch, oldest first.
    pub batches: Vec<ArchivedBatch>,
}

impl RenameLogArchive {
    /// Total number of operations across every batch.
    pub fn operation_count(&self) -> usize {
        self.batches
            .iter()
            .map(|batch| batch.operations.len())
            .sum()
    }
}

/// Result of one import pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ImportSummary {
    /// Batches written to the log.
    pub batches: u64,
    /// Operations written to the log.
    pub operations: u64,
    /// Batches skipped because the log already had that batch id.
    pub skipped: u64,
}

/// Status of one row in an [`UndoPlan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoStatus {
    /// The reverse rename can be executed.
    Ready,
    /// The filesystem no longer matches the recorded rename; the file was left
    /// untouched.
    Conflict,
    /// The operation was already undone, so a repeated undo must not rename.
    AlreadyUndone,
}

/// One row of an undo plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoRow {
    /// Id of the operation this row would undo.
    pub operation_id: i64,
    /// Current path of the renamed file (the rename source for undo).
    pub from: String,
    /// Original path to restore (the rename target for undo).
    pub to: String,
    /// Plan status for this row.
    pub status: UndoStatus,
    /// Human-readable explanation for non-ready rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Newest-first undo plan for one batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoPlan {
    /// Batch the plan belongs to.
    pub batch_id: String,
    /// Rows ordered newest operation first.
    pub rows: Vec<UndoRow>,
}

/// Outcome of one attempted undo rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoResultStatus {
    /// The rename was reversed and the operation marked undone.
    Undone,
    /// The row was skipped without touching the filesystem.
    Skipped,
    /// The backend failed to reverse the rename.
    Failed,
}

/// Result of one row processed by [`apply_undo`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoResult {
    /// Operation id from the plan row.
    pub operation_id: i64,
    /// Rename source used for the undo.
    pub from: String,
    /// Original path the undo targeted.
    pub to: String,
    /// Outcome for this row.
    pub status: UndoResultStatus,
    /// Backend error or skip reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Failures returned by the rename log store.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// SQLite reported an error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// JSON (de)serialization of the algorithms column failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// A stored column value is not understood by this version.
    #[error("invalid stored value: {0}")]
    InvalidStoredValue(String),
    /// The requested batch does not exist.
    #[error("batch not found: {0}")]
    BatchNotFound(String),
    /// The requested operation does not exist.
    #[error("operation not found: {0}")]
    OperationNotFound(i64),
    /// The requested operation state change is not allowed.
    #[error("invalid operation state transition from {from} to {to}")]
    InvalidStateTransition {
        /// Current state.
        from: OperationState,
        /// Requested state.
        to: OperationState,
    },
}

/// SQLite rename log store.
pub struct LogStore {
    conn: Connection,
}

impl LogStore {
    /// Open (or create) a database file and apply pending schema migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LogError> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// Open an in-memory database and apply pending schema migrations.
    pub fn open_in_memory() -> Result<Self, LogError> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, LogError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Apply pending schema migrations. This is idempotent.
    pub fn migrate(&mut self) -> Result<(), LogError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migration (
                version INTEGER PRIMARY KEY NOT NULL,
                applied_at TEXT NOT NULL
            );",
        )?;
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migration",
            params![],
            |row| row.get(0),
        )?;
        if applied > i64::from(SCHEMA_VERSION) {
            return Err(LogError::InvalidStoredValue(format!(
                "database schema version {applied} is newer than supported {SCHEMA_VERSION}"
            )));
        }
        for (index, migration) in MIGRATIONS.iter().copied().enumerate() {
            let version = (index as i64) + 1;
            if version <= applied {
                continue;
            }
            let transaction = self.conn.transaction()?;
            transaction.execute_batch(migration)?;
            transaction.execute(
                "INSERT INTO schema_migration (version, applied_at) VALUES (?1, ?2)",
                params![version, now_rfc3339()],
            )?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// Highest applied schema version (zero for an unmigrated database).
    pub fn schema_version(&self) -> Result<u32, LogError> {
        let version: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migration",
            params![],
            |row| row.get(0),
        )?;
        u32::try_from(version).map_err(|_| {
            LogError::InvalidStoredValue(format!("schema version out of range: {version}"))
        })
    }

    /// Insert one batch record.
    pub fn create_batch(&mut self, batch: &BatchRecord) -> Result<(), LogError> {
        let algorithms = serde_json::to_string(&batch.algorithms)?;
        self.conn.execute(
            "INSERT INTO batch (
                id, created_at, root, root_kind, algorithms,
                suffix_length, suffix_case, suffix_algorithm, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                batch.id,
                batch.created_at,
                batch.root,
                batch.root_kind.as_str(),
                algorithms,
                i64::from(batch.suffix_length),
                hex_case_to_db(batch.suffix_case),
                batch.suffix_algorithm.stable_id(),
                batch.status,
            ],
        )?;
        Ok(())
    }

    /// Insert one pending operation and return its assigned row id.
    pub fn insert_operation(&mut self, operation: &RenameOperation) -> Result<i64, LogError> {
        self.conn.execute(
            "INSERT INTO rename_operation (
                batch_id, original_path, new_path, algorithm, full_hash, suffix,
                op_type, result, error_kind, created_at, undone_at, state
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                operation.batch_id,
                operation.original_path,
                operation.new_path,
                operation.algorithm.stable_id(),
                operation.full_hash,
                operation.suffix,
                operation.op_type,
                operation.result,
                operation.error_kind,
                operation.created_at,
                operation.undone_at,
                operation.state.as_str(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Record the outcome of an operation and transition its state.
    ///
    /// Allowed transitions are `pending -> applied | conflict` and
    /// `applied -> undone | conflict`; re-recording the current state only
    /// updates `result` and `error_kind`. Any other transition is rejected with
    /// [`LogError::InvalidStateTransition`].
    pub fn record_result(
        &mut self,
        operation_id: i64,
        result: &str,
        error_kind: Option<&str>,
        state: OperationState,
    ) -> Result<(), LogError> {
        let current = self
            .operation_state(operation_id)?
            .ok_or_else(|| LogError::OperationNotFound(operation_id))?;
        if current != state && !can_transition(current, state) {
            return Err(LogError::InvalidStateTransition {
                from: current,
                to: state,
            });
        }
        let undone_at = if state == OperationState::Undone {
            Some(now_rfc3339())
        } else {
            None
        };
        self.conn.execute(
            "UPDATE rename_operation
             SET result = ?1, error_kind = ?2, state = ?3, undone_at = COALESCE(?4, undone_at)
             WHERE id = ?5",
            params![result, error_kind, state.as_str(), undone_at, operation_id],
        )?;
        Ok(())
    }

    /// List all batches, oldest first.
    pub fn list_batches(&self) -> Result<Vec<BatchRecord>, LogError> {
        let mut statement = self.conn.prepare(
            "SELECT id, created_at, root, root_kind, algorithms,
                    suffix_length, suffix_case, suffix_algorithm, status
             FROM batch ORDER BY created_at ASC, id ASC",
        )?;
        let raw = statement
            .query_map(params![], |row| {
                Ok(RawBatch {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    root: row.get(2)?,
                    root_kind: row.get(3)?,
                    algorithms: row.get(4)?,
                    suffix_length: row.get(5)?,
                    suffix_case: row.get(6)?,
                    suffix_algorithm: row.get(7)?,
                    status: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        raw.into_iter().map(RawBatch::into_record).collect()
    }

    /// List the operations of one batch, oldest first.
    pub fn operations_for_batch(&self, batch_id: &str) -> Result<Vec<RenameOperation>, LogError> {
        self.load_operations(Some(batch_id))
    }

    /// Current state of one operation, or [`None`] when the id is unknown.
    pub fn operation_state(&self, operation_id: i64) -> Result<Option<OperationState>, LogError> {
        let state: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM rename_operation WHERE id = ?1",
                params![operation_id],
                |row| row.get(0),
            )
            .optional()?;
        match state {
            None => Ok(None),
            Some(value) => OperationState::from_str_name(&value)
                .map(Some)
                .ok_or_else(|| LogError::InvalidStoredValue(format!("unknown state: {value}"))),
        }
    }

    /// Mark an applied operation undone.
    ///
    /// Undoing an already undone operation is a no-op; undoing a pending or
    /// conflicted operation is rejected.
    pub fn mark_undone(&mut self, operation_id: i64) -> Result<(), LogError> {
        match self.operation_state(operation_id)? {
            None => Err(LogError::OperationNotFound(operation_id)),
            Some(OperationState::Undone) => Ok(()),
            Some(OperationState::Applied) => {
                self.conn.execute(
                    "UPDATE rename_operation
                     SET state = ?1, undone_at = ?2
                     WHERE id = ?3 AND state = ?4",
                    params![
                        OperationState::Undone.as_str(),
                        now_rfc3339(),
                        operation_id,
                        OperationState::Applied.as_str(),
                    ],
                )?;
                Ok(())
            }
            Some(from) => Err(LogError::InvalidStateTransition {
                from,
                to: OperationState::Undone,
            }),
        }
    }

    /// Build a newest-first undo plan for one batch.
    ///
    /// Only applied operations are candidates. A row is [`UndoStatus::Ready`]
    /// when the renamed file still exists and the original name is free;
    /// otherwise it is [`UndoStatus::Conflict`] and must not be executed.
    /// Already-undone operations are reported as [`UndoStatus::AlreadyUndone`]
    /// so a repeated undo is visibly a no-op.
    ///
    /// Pending operations are **not** dropped silently: the log cannot say on
    /// its own whether their rename happened, so they surface as
    /// [`UndoStatus::Conflict`] with an explicit reason.
    /// [`reconcile_pending_operations`] resolves them against the filesystem
    /// first (to `applied` or `conflict`), so run it before planning. Operations
    /// recorded as conflicted are not listed: their failure is already recorded
    /// on the row itself.
    pub fn plan_undo(&self, batch_id: &str, fs: &dyn RenameFs) -> Result<UndoPlan, LogError> {
        if !self.batch_exists(batch_id)? {
            return Err(LogError::BatchNotFound(batch_id.to_string()));
        }
        let mut operations = self.load_operations(Some(batch_id))?;
        operations.reverse();
        let mut rows = Vec::new();
        for operation in operations {
            match operation.state {
                OperationState::Applied => {
                    let current_missing = !fs.exists(Path::new(&operation.new_path));
                    let original_occupied = fs.exists(Path::new(&operation.original_path));
                    let (status, reason) = if current_missing {
                        (
                            UndoStatus::Conflict,
                            Some("renamed file no longer exists".to_string()),
                        )
                    } else if original_occupied {
                        (
                            UndoStatus::Conflict,
                            Some("original name is already occupied".to_string()),
                        )
                    } else {
                        (UndoStatus::Ready, None)
                    };
                    rows.push(UndoRow {
                        operation_id: operation.id,
                        from: operation.new_path,
                        to: operation.original_path,
                        status,
                        reason,
                    });
                }
                OperationState::Undone => rows.push(UndoRow {
                    operation_id: operation.id,
                    from: operation.new_path,
                    to: operation.original_path,
                    status: UndoStatus::AlreadyUndone,
                    reason: Some("operation was already undone".to_string()),
                }),
                OperationState::Conflict => {}
                // A pending row means the log never recorded whether the rename
                // happened. It is not dropped: it is reported as a conflict
                // because it cannot be proven safe to reverse.
                OperationState::Pending => rows.push(UndoRow {
                    operation_id: operation.id,
                    from: operation.new_path,
                    to: operation.original_path,
                    status: UndoStatus::Conflict,
                    reason: Some(
                        "operation is still pending; its rename was never confirmed".to_string(),
                    ),
                }),
            }
        }
        Ok(UndoPlan {
            batch_id: batch_id.to_string(),
            rows,
        })
    }

    /// Every operation whose state is still `pending`.
    ///
    /// A pending row means the pipeline died (or failed to write) between the
    /// filesystem rename and `record_result`, so the log alone cannot say
    /// whether the file was renamed. [`reconcile_pending_operations`] resolves
    /// those rows against the filesystem.
    pub fn pending_operations(
        &self,
        batch_id: Option<&str>,
    ) -> Result<Vec<RenameOperation>, LogError> {
        let mut pending: Vec<RenameOperation> = self
            .load_operations(batch_id)?
            .into_iter()
            .filter(|operation| operation.state == OperationState::Pending)
            .collect();
        pending.reverse();
        Ok(pending)
    }

    /// Record a reconciled outcome for a pending operation.
    ///
    /// [`reconcile_pending_operations`] calls this after inspecting the
    /// filesystem; it is exposed so a backend that reconciles on its own (the
    /// Android SAF bridge) can persist the same verdict. The transition is the
    /// ordinary `pending -> applied | conflict` edge of the state machine, so
    /// recording it twice fails with [`LogError::InvalidStateTransition`].
    pub fn record_reconciled_state(
        &mut self,
        operation_id: i64,
        state: OperationState,
        result: &str,
        error_kind: Option<&str>,
    ) -> Result<(), LogError> {
        self.record_result(operation_id, result, error_kind, state)
    }

    /// Export the whole log, or one batch, as a portable archive.
    ///
    /// `batch_id` of [`None`] exports every batch, oldest first. The result carries
    /// no local row ids, so the document is stable across machines and readable by
    /// a human: see [`RenameLogArchive`].
    pub fn export_archive(&self, batch_id: Option<&str>) -> Result<RenameLogArchive, LogError> {
        let batches = match batch_id {
            Some(id) => {
                if !self.batch_exists(id)? {
                    return Err(LogError::BatchNotFound(id.to_string()));
                }
                vec![id.to_string()]
            }
            None => self
                .list_batches()?
                .into_iter()
                .map(|batch| batch.id)
                .collect(),
        };
        let mut archived: Vec<ArchivedBatch> = Vec::with_capacity(batches.len());
        for id in batches {
            let batch = self.load_batch(&id)?;
            let operations = self.load_operations(Some(&id))?;
            archived.push(ArchivedBatch {
                batch,
                operations: operations
                    .iter()
                    .map(ArchivedOperation::from_operation)
                    .collect(),
            });
        }
        Ok(RenameLogArchive {
            version: ARCHIVE_VERSION,
            tool: Some(batchhash_types::TOOL_NAME.to_string()),
            tool_version: Some(batchhash_types::TOOL_VERSION.to_string()),
            exported_at: now_rfc3339(),
            batches: archived,
        })
    }

    /// Export the log as canonical pretty-printed JSON.
    ///
    /// Pretty-printed on purpose: the archive is meant to be opened, read and
    /// diffed by a person, and a single-line document defeats that.
    pub fn export_archive_json(&self, batch_id: Option<&str>) -> Result<String, LogError> {
        let archive = self.export_archive(batch_id)?;
        serde_json::to_string_pretty(&archive).map_err(|error| {
            LogError::InvalidStoredValue(format!("cannot serialize archive: {error}"))
        })
    }

    /// Import a portable archive into this log.
    ///
    /// Import is idempotent per batch id: a batch whose id is already present is
    /// skipped together with its operations, so importing the same document twice
    /// cannot duplicate records or overwrite newer local state. Nothing here
    /// touches the filesystem; restoring filenames is a separate, explicit
    /// [`plan_undo`](LogStore::plan_undo) plus [`apply_undo`] call.
    pub fn import_archive(
        &mut self,
        archive: &RenameLogArchive,
    ) -> Result<ImportSummary, LogError> {
        if archive.version > ARCHIVE_VERSION {
            return Err(LogError::InvalidStoredValue(format!(
                "archive version {} is newer than the supported version {ARCHIVE_VERSION}",
                archive.version
            )));
        }
        let mut summary = ImportSummary::default();
        for entry in &archive.batches {
            if self.batch_exists(&entry.batch.id)? {
                summary.skipped += 1;
                continue;
            }
            let batch = entry.batch.clone();
            self.create_batch(&batch)?;
            let mut written = 0u64;
            for operation in &entry.operations {
                let operation = operation.to_operation()?;
                // A mismatched batch reference would be rejected by the foreign
                // key anyway; reporting it as a malformed archive is clearer.
                if operation.batch_id != batch.id {
                    return Err(LogError::InvalidStoredValue(format!(
                        "operation for batch {} claims batch {}",
                        batch.id, operation.batch_id
                    )));
                }
                self.insert_operation(&operation)?;
                written += 1;
            }
            summary.batches += 1;
            summary.operations += written;
        }
        Ok(summary)
    }

    /// Import an archive from JSON text.
    ///
    /// Parsing failures are reported as [`LogError::InvalidStoredValue`] so a
    /// malformed document (a truncated download, a hand edit) is distinguishable
    /// from a valid archive that conflicts with local state.
    pub fn import_archive_json(&mut self, text: &str) -> Result<ImportSummary, LogError> {
        let archive: RenameLogArchive = serde_json::from_str(text).map_err(|error| {
            LogError::InvalidStoredValue(format!("cannot parse archive: {error}"))
        })?;
        self.import_archive(&archive)
    }

    /// Load one batch by id.
    fn load_batch(&self, batch_id: &str) -> Result<BatchRecord, LogError> {
        let raw = self
            .conn
            .query_row(
                "SELECT id, created_at, root, root_kind, algorithms,
                        suffix_length, suffix_case, suffix_algorithm, status
                 FROM batch WHERE id = ?1",
                params![batch_id],
                |row| {
                    Ok(RawBatch {
                        id: row.get(0)?,
                        created_at: row.get(1)?,
                        root: row.get(2)?,
                        root_kind: row.get(3)?,
                        algorithms: row.get(4)?,
                        suffix_length: row.get(5)?,
                        suffix_case: row.get(6)?,
                        suffix_algorithm: row.get(7)?,
                        status: row.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| LogError::BatchNotFound(batch_id.to_string()))?;
        raw.into_record()
    }

    fn batch_exists(&self, batch_id: &str) -> Result<bool, LogError> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM batch WHERE id = ?1",
                params![batch_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    fn load_operations(&self, batch_id: Option<&str>) -> Result<Vec<RenameOperation>, LogError> {
        let mut statement = self.conn.prepare(
            "SELECT id, batch_id, original_path, new_path, algorithm, full_hash,
                    suffix, op_type, result, error_kind, created_at, undone_at, state
             FROM rename_operation
             WHERE (?1 IS NULL OR batch_id = ?1)
             ORDER BY id ASC",
        )?;
        let raw = statement
            .query_map(params![batch_id], |row| {
                Ok(RawOperation {
                    id: row.get(0)?,
                    batch_id: row.get(1)?,
                    original_path: row.get(2)?,
                    new_path: row.get(3)?,
                    algorithm: row.get(4)?,
                    full_hash: row.get(5)?,
                    suffix: row.get(6)?,
                    op_type: row.get(7)?,
                    result: row.get(8)?,
                    error_kind: row.get(9)?,
                    created_at: row.get(10)?,
                    undone_at: row.get(11)?,
                    state: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        raw.into_iter().map(RawOperation::into_record).collect()
    }
}

/// Execute a previously planned undo.
///
/// This is [`apply_undo_with_cancel`] against the standard filesystem and
/// with nothing cancelled.
pub fn apply_undo(
    plan: &UndoPlan,
    backend: &mut dyn RenameBackend,
    store: &mut LogStore,
) -> Result<Vec<UndoResult>, LogError> {
    apply_undo_with_cancel(plan, backend, store, &StdRenameFs, &|| false)
}

/// Execute a previously planned undo, stopping when [`should_stop`] reports
/// cancellation.
///
/// Only [`UndoStatus::Ready`] rows are attempted. Three checks guard every row,
/// because a plan is a snapshot and the filesystem keeps moving:
///
/// 1. the operation must still be `applied` in the store, so a stale plan (or a
///    double apply of the same plan) can never rename twice;
/// 2. the renamed file must still exist and the original path must still be
///    free, re-read from the backend's filesystem rather than trusted from the
///    plan; an occupied original path or a vanished rename source yields
///    [`UndoResultStatus::Skipped`] with a conflict reason and **no** rename;
/// 3. the reverse rename goes through [`RenameBackend::rename_no_replace`], so
///    even a target created between check 2 and the rename cannot be destroyed.
///
/// The checks read the filesystem through [`fs`], so a backend with its own path
/// model (Android SAF) can pass its document probe instead of the standard one.
///
/// Successful rows are marked undone in one transaction. A backend failure
/// leaves the operation applied so the undo can be retried, and a cancellation
/// leaves later rows untouched and reported as [`UndoResultStatus::Skipped`] with
/// a cancellation reason.
pub fn apply_undo_with_cancel(
    plan: &UndoPlan,
    backend: &mut dyn RenameBackend,
    store: &mut LogStore,
    fs: &dyn RenameFs,
    should_stop: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<UndoResult>, LogError> {
    let mut results = Vec::with_capacity(plan.rows.len());
    let mut undone = Vec::new();
    for row in &plan.rows {
        if should_stop() {
            results.push(UndoResult {
                operation_id: row.operation_id,
                from: row.from.clone(),
                to: row.to.clone(),
                status: UndoResultStatus::Skipped,
                error: Some("undo cancelled before this row".to_string()),
            });
            continue;
        }
        match row.status {
            UndoStatus::Ready => {
                let current = store.operation_state(row.operation_id)?;
                if current != Some(OperationState::Applied) {
                    results.push(UndoResult {
                        operation_id: row.operation_id,
                        from: row.from.clone(),
                        to: row.to.clone(),
                        status: UndoResultStatus::Skipped,
                        error: Some("operation is no longer applied".to_string()),
                    });
                    continue;
                }
                if let Some(reason) = undo_filesystem_conflict(row, fs) {
                    results.push(UndoResult {
                        operation_id: row.operation_id,
                        from: row.from.clone(),
                        to: row.to.clone(),
                        status: UndoResultStatus::Skipped,
                        error: Some(reason),
                    });
                    continue;
                }
                match backend.rename_no_replace(Path::new(&row.from), Path::new(&row.to)) {
                    Ok(()) => {
                        undone.push(row.operation_id);
                        results.push(UndoResult {
                            operation_id: row.operation_id,
                            from: row.from.clone(),
                            to: row.to.clone(),
                            status: UndoResultStatus::Undone,
                            error: None,
                        });
                    }
                    Err(error) => results.push(UndoResult {
                        operation_id: row.operation_id,
                        from: row.from.clone(),
                        to: row.to.clone(),
                        status: UndoResultStatus::Failed,
                        error: Some(error.to_string()),
                    }),
                }
            }
            UndoStatus::AlreadyUndone => results.push(UndoResult {
                operation_id: row.operation_id,
                from: row.from.clone(),
                to: row.to.clone(),
                status: UndoResultStatus::Skipped,
                error: Some("operation was already undone".to_string()),
            }),
            UndoStatus::Conflict => results.push(UndoResult {
                operation_id: row.operation_id,
                from: row.from.clone(),
                to: row.to.clone(),
                status: UndoResultStatus::Skipped,
                error: Some(
                    row.reason
                        .clone()
                        .unwrap_or_else(|| "undo conflict".to_string()),
                ),
            }),
        }
    }
    if !undone.is_empty() {
        let transaction = store.conn.transaction()?;
        let undone_at = now_rfc3339();
        for operation_id in undone {
            transaction.execute(
                "UPDATE rename_operation
                 SET state = ?1, undone_at = ?2
                 WHERE id = ?3 AND state = ?4",
                params![
                    OperationState::Undone.as_str(),
                    undone_at,
                    operation_id,
                    OperationState::Applied.as_str(),
                ],
            )?;
        }
        transaction.commit()?;
    }
    Ok(results)
}

/// Re-check a ready undo row against the filesystem immediately before the
/// reverse rename.
///
/// Returns the conflict reason when the undo must not be attempted. The checks
/// mirror [`LogStore::plan_undo`] but are evaluated fresh: the plan may be stale,
/// and a file may have appeared at the original path since it was built.
fn undo_filesystem_conflict(row: &UndoRow, fs: &dyn RenameFs) -> Option<String> {
    if !fs.exists(Path::new(&row.from)) {
        return Some(format!("renamed file no longer exists: {}", row.from));
    }
    if fs.exists(Path::new(&row.to)) {
        return Some(format!("original name is already occupied: {}", row.to));
    }
    None
}

/// Filesystem verdict for one pending operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// The rename happened: the new path exists and the original path is gone.
    /// The operation is recorded `applied`.
    Applied,
    /// The rename never happened: the original path still exists and the new
    /// path does not. The operation is recorded `conflict` so it is never
    /// reported as undoable.
    NotApplied,
    /// Both paths exist, or neither does. The filesystem cannot tell which side
    /// the file is on, so the operation is recorded `conflict` and left for a
    /// human.
    Ambiguous,
}

impl ReconcileOutcome {
    /// Stable machine-readable identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReconcileOutcome::Applied => "applied",
            ReconcileOutcome::NotApplied => "not_applied",
            ReconcileOutcome::Ambiguous => "ambiguous",
        }
    }
}

/// One resolved pending operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciledOperation {
    /// Operation id in the rename log.
    pub operation_id: i64,
    /// Batch the operation belongs to.
    pub batch_id: String,
    /// Original path recorded by the rename.
    pub original_path: String,
    /// Renamed path recorded by the rename.
    pub new_path: String,
    /// What the filesystem said.
    pub outcome: ReconcileOutcome,
    /// The state written back to the log.
    pub state: OperationState,
    /// Human-readable reason stored in `error_kind`.
    pub reason: String,
}

/// Summary of one reconciliation pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReconcileReport {
    /// Operations resolved because the rename did happen.
    pub applied: u64,
    /// Operations resolved because the rename did not happen.
    pub not_applied: u64,
    /// Operations that could not be resolved and were marked conflicted.
    pub ambiguous: u64,
    /// Per-operation detail, newest operation first (the undo order).
    pub operations: Vec<ReconciledOperation>,
}

impl ReconcileReport {
    /// How many pending operations this pass resolved into a terminal state.
    pub fn resolved(&self) -> u64 {
        self.applied + self.not_applied + self.ambiguous
    }
}

/// Resolve every `pending` operation against the filesystem.
///
/// The pipeline records a rename as `pending` before touching the file and as
/// `applied` after the backend confirms it. A crash or a log write failure
/// between those two steps leaves a row that looks unapplied even though the
/// file may already be renamed. This pass inspects each pending row and writes
/// the verdict back:
///
/// * new path exists and original path does not -> `applied` (undoable),
/// * original path exists and new path does not -> `conflict` +
///   `not_applied` (the batch simply never renamed it),
/// * both or neither exist -> `conflict` + `ambiguous` (never guessed).
///
/// Only pending rows are touched, so the pass is safe to run on every store
/// open: a row that already records applied, undone or conflict is never
/// reclassified and history is never rewritten. Use
/// [`reconcile_pending_operations_with_cancel`] when the caller is cancelable.
pub fn reconcile_pending_operations(
    store: &mut LogStore,
    fs: &dyn RenameFs,
) -> Result<ReconcileReport, LogError> {
    reconcile_pending_operations_with_cancel(store, fs, &|| false)
}

/// [`reconcile_pending_operations`] with a cancellation check before every row.
///
/// A cancelled pass commits the rows it already resolved and returns; the
/// remaining rows stay pending and the next open finishes the job, because
/// reconciliation is idempotent.
pub fn reconcile_pending_operations_with_cancel(
    store: &mut LogStore,
    fs: &dyn RenameFs,
    should_stop: &(dyn Fn() -> bool + Sync),
) -> Result<ReconcileReport, LogError> {
    let pending = store.pending_operations(None)?;
    let mut report = ReconcileReport::default();
    for operation in pending {
        if should_stop() {
            break;
        }
        let (outcome, state, reason) = if fs.exists(Path::new(&operation.new_path))
            && !fs.exists(Path::new(&operation.original_path))
        {
            (
                ReconcileOutcome::Applied,
                OperationState::Applied,
                "renamed file exists and the original path is free",
            )
        } else if fs.exists(Path::new(&operation.original_path))
            && !fs.exists(Path::new(&operation.new_path))
        {
            (
                ReconcileOutcome::NotApplied,
                OperationState::Conflict,
                "rename never reached the filesystem; the original file is still in place",
            )
        } else {
            (
                ReconcileOutcome::Ambiguous,
                OperationState::Conflict,
                "both the original and the renamed path exist, or neither does",
            )
        };
        store.record_reconciled_state(operation.id, state, outcome.as_str(), Some(reason))?;
        match outcome {
            ReconcileOutcome::Applied => report.applied += 1,
            ReconcileOutcome::NotApplied => report.not_applied += 1,
            ReconcileOutcome::Ambiguous => report.ambiguous += 1,
        }
        report.operations.push(ReconciledOperation {
            operation_id: operation.id,
            batch_id: operation.batch_id,
            original_path: operation.original_path,
            new_path: operation.new_path,
            outcome,
            state,
            reason: reason.to_string(),
        });
    }
    Ok(report)
}

struct RawBatch {
    id: String,
    created_at: String,
    root: String,
    root_kind: String,
    algorithms: String,
    suffix_length: i64,
    suffix_case: String,
    suffix_algorithm: String,
    status: String,
}

impl RawBatch {
    fn into_record(self) -> Result<BatchRecord, LogError> {
        let algorithms: Vec<AlgorithmId> = serde_json::from_str(&self.algorithms)?;
        let root_kind = root_kind_from_db(&self.root_kind)?;
        let suffix_case = hex_case_from_db(&self.suffix_case)?;
        let suffix_algorithm =
            AlgorithmId::from_stable_id(&self.suffix_algorithm).ok_or_else(|| {
                LogError::InvalidStoredValue(format!(
                    "unknown algorithm id: {}",
                    self.suffix_algorithm
                ))
            })?;
        let suffix_length = u8::try_from(self.suffix_length).map_err(|_| {
            LogError::InvalidStoredValue(format!(
                "suffix length out of range: {}",
                self.suffix_length
            ))
        })?;
        Ok(BatchRecord {
            id: self.id,
            created_at: self.created_at,
            root: self.root,
            root_kind,
            algorithms,
            suffix_length,
            suffix_case,
            suffix_algorithm,
            status: self.status,
        })
    }
}

struct RawOperation {
    id: i64,
    batch_id: String,
    original_path: String,
    new_path: String,
    algorithm: String,
    full_hash: String,
    suffix: String,
    op_type: String,
    result: Option<String>,
    error_kind: Option<String>,
    created_at: String,
    undone_at: Option<String>,
    state: String,
}

impl RawOperation {
    fn into_record(self) -> Result<RenameOperation, LogError> {
        let algorithm = AlgorithmId::from_stable_id(&self.algorithm).ok_or_else(|| {
            LogError::InvalidStoredValue(format!("unknown algorithm id: {}", self.algorithm))
        })?;
        let state = OperationState::from_str_name(&self.state).ok_or_else(|| {
            LogError::InvalidStoredValue(format!("unknown state: {}", self.state))
        })?;
        Ok(RenameOperation {
            id: self.id,
            batch_id: self.batch_id,
            original_path: self.original_path,
            new_path: self.new_path,
            algorithm,
            full_hash: self.full_hash,
            suffix: self.suffix,
            op_type: self.op_type,
            result: self.result,
            error_kind: self.error_kind,
            created_at: self.created_at,
            undone_at: self.undone_at,
            state,
        })
    }
}

fn can_transition(from: OperationState, to: OperationState) -> bool {
    matches!(
        (from, to),
        (OperationState::Pending, OperationState::Applied)
            | (OperationState::Pending, OperationState::Conflict)
            | (OperationState::Applied, OperationState::Undone)
            | (OperationState::Applied, OperationState::Conflict)
    )
}

fn hex_case_to_db(case: HexCase) -> &'static str {
    match case {
        HexCase::Upper => "upper",
        HexCase::Lower => "lower",
    }
}

fn hex_case_from_db(value: &str) -> Result<HexCase, LogError> {
    match value {
        "upper" => Ok(HexCase::Upper),
        "lower" => Ok(HexCase::Lower),
        other => Err(LogError::InvalidStoredValue(format!(
            "unknown hex case: {other}"
        ))),
    }
}

fn root_kind_from_db(value: &str) -> Result<RootKind, LogError> {
    match value {
        "fs" => Ok(RootKind::Fs),
        "saf" => Ok(RootKind::Saf),
        other => Err(LogError::InvalidStoredValue(format!(
            "unknown root kind: {other}"
        ))),
    }
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{
        execute_plan, plan_renames, RenamePlanOptions, RenameResultStatus, StdFsBackend,
        StdRenameFs,
    };
    use crate::suffix::SuffixFormat;
    use batchhash_types::{AlgorithmId, FileEntry, RootKind};
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn digest() -> String {
        "abcdef0123456789".repeat(4)
    }

    fn test_batch() -> BatchRecord {
        BatchRecord::new(
            "/tmp/root",
            RootKind::Fs,
            vec![AlgorithmId::Blake3],
            SuffixFormat::default(),
        )
    }

    fn test_operation(batch_id: &str, original: &str, new: &str) -> RenameOperation {
        RenameOperation::new(
            batch_id,
            original,
            new,
            AlgorithmId::Blake3,
            digest(),
            "ABCDE",
        )
    }

    fn insert_applied(store: &mut LogStore, batch_id: &str, original: &str, new: &str) -> i64 {
        let operation = test_operation(batch_id, original, new);
        let id = store
            .insert_operation(&operation)
            .expect("insert operation");
        store
            .record_result(id, "renamed", None, OperationState::Applied)
            .expect("record applied");
        id
    }

    /// Insert an operation and leave it `pending`, like a crashed run would.
    fn insert_pending(store: &mut LogStore, batch_id: &str, original: &str, new: &str) -> i64 {
        let operation = test_operation(batch_id, original, new);
        store
            .insert_operation(&operation)
            .expect("insert operation")
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        assert_eq!(store.schema_version().expect("schema version"), 1);
        store.migrate().expect("second migrate");
        store.migrate().expect("third migrate");
        let migrations: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM schema_migration", params![], |row| {
                row.get(0)
            })
            .expect("count migrations");
        assert_eq!(migrations, 1);
    }

    #[test]
    fn batch_and_operations_round_trip() {
        let temp = tempdir().expect("tempdir");
        let db = temp.path().join("rename.sqlite");
        let batch = test_batch();
        {
            let mut store = LogStore::open(&db).expect("open store");
            store.create_batch(&batch).expect("create batch");
            let operation = test_operation(&batch.id, "/tmp/a.txt", "/tmp/a[ABCDE].txt");
            let id = store
                .insert_operation(&operation)
                .expect("insert operation");
            assert!(id > 0);
            let operations = store
                .operations_for_batch(&batch.id)
                .expect("operations for batch");
            assert_eq!(operations.len(), 1);
            assert_eq!(operations[0].id, id);
            assert_eq!(operations[0].state, OperationState::Pending);
            assert_eq!(operations[0].algorithm, AlgorithmId::Blake3);
            assert_eq!(operations[0].full_hash, digest());
            assert_eq!(operations[0].suffix, "ABCDE");
            assert_eq!(operations[0].op_type, "rename");
        }
        let store = LogStore::open(&db).expect("reopen store");
        let batches = store.list_batches().expect("list batches");
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], batch);

        let operations = store
            .operations_for_batch(&batch.id)
            .expect("reopened operations");
        assert_eq!(operations.len(), 1);
        let operation = &operations[0];
        assert_eq!(operation.id, 1);
        assert_eq!(operation.batch_id, batch.id);
        assert_eq!(operation.original_path, "/tmp/a.txt");
        assert_eq!(operation.new_path, "/tmp/a[ABCDE].txt");
        assert_eq!(operation.algorithm, AlgorithmId::Blake3);
        assert_eq!(operation.full_hash, digest());
        assert_eq!(operation.suffix, "ABCDE");
        assert_eq!(operation.op_type, "rename");
        assert_eq!(operation.result, None);
        assert_eq!(operation.error_kind, None);
        assert_eq!(operation.undone_at, None);
        assert_eq!(operation.state, OperationState::Pending);
        assert!(!operation.created_at.is_empty());
    }

    #[test]
    fn list_batches_orders_by_creation_time() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let mut newer = test_batch();
        newer.id = "b-newer".to_string();
        newer.created_at = "2026-01-02T00:00:00.000Z".to_string();
        let mut older = test_batch();
        older.id = "b-older".to_string();
        older.created_at = "2026-01-01T00:00:00.000Z".to_string();
        store.create_batch(&newer).expect("create newer");
        store.create_batch(&older).expect("create older");
        let batches = store.list_batches().expect("list batches");
        assert_eq!(
            batches
                .iter()
                .map(|batch| batch.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b-older", "b-newer"]
        );
    }

    #[test]
    fn operation_state_machine_is_enforced() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");
        let operation = test_operation(&batch.id, "/tmp/a.txt", "/tmp/a[ABCDE].txt");
        let id = store
            .insert_operation(&operation)
            .expect("insert operation");

        assert_eq!(
            store.operation_state(id).expect("state"),
            Some(OperationState::Pending)
        );
        let error = store
            .mark_undone(id)
            .expect_err("pending operation cannot be undone");
        assert!(matches!(error, LogError::InvalidStateTransition { .. }));

        store
            .record_result(id, "renamed", None, OperationState::Applied)
            .expect("pending to applied");
        assert_eq!(
            store.operation_state(id).expect("state"),
            Some(OperationState::Applied)
        );
        store
            .record_result(id, "renamed", None, OperationState::Applied)
            .expect("idempotent applied");

        store.mark_undone(id).expect("applied to undone");
        assert_eq!(
            store.operation_state(id).expect("state"),
            Some(OperationState::Undone)
        );
        store.mark_undone(id).expect("idempotent undo");

        let error = store
            .record_result(id, "renamed", None, OperationState::Pending)
            .expect_err("undone cannot become pending");
        assert!(matches!(error, LogError::InvalidStateTransition { .. }));
        let error = store
            .record_result(id, "conflict", Some("race"), OperationState::Conflict)
            .expect_err("undone cannot become conflict");
        assert!(matches!(error, LogError::InvalidStateTransition { .. }));

        let operation = store
            .operations_for_batch(&batch.id)
            .expect("operations")
            .into_iter()
            .next()
            .expect("operation");
        assert_eq!(operation.state, OperationState::Undone);
        assert!(operation.undone_at.is_some());
    }

    #[test]
    fn conflict_transitions_are_recorded() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");
        let operation = test_operation(&batch.id, "/tmp/a.txt", "/tmp/a[ABCDE].txt");
        let id = store
            .insert_operation(&operation)
            .expect("insert operation");
        store
            .record_result(
                id,
                "failed",
                Some("already_exists"),
                OperationState::Conflict,
            )
            .expect("pending to conflict");
        assert_eq!(
            store.operation_state(id).expect("state"),
            Some(OperationState::Conflict)
        );
        let error = store
            .record_result(id, "renamed", None, OperationState::Applied)
            .expect_err("conflict is terminal");
        assert!(matches!(error, LogError::InvalidStateTransition { .. }));
    }

    #[test]
    fn foreign_keys_require_an_existing_batch() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let operation = test_operation("missing-batch", "/tmp/a.txt", "/tmp/a[ABCDE].txt");
        let error = store
            .insert_operation(&operation)
            .expect_err("foreign key must reject an unknown batch");
        assert!(matches!(error, LogError::Sqlite(_)));
    }

    #[test]
    fn plan_undo_marks_conflicts_newest_first() {
        let temp = tempdir().expect("tempdir");
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");

        let original_one = temp.path().join("one.txt");
        let renamed_one = temp.path().join("one[ABCDE].txt");
        fs::write(&original_one, b"one").expect("original one");
        fs::write(&renamed_one, b"renamed one").expect("renamed one");

        let original_two = temp.path().join("two.txt");
        let renamed_two = temp.path().join("two[ABCDE].txt");

        let original_three = temp.path().join("three.txt");
        let renamed_three = temp.path().join("three[ABCDE].txt");
        fs::write(&renamed_three, b"renamed three").expect("renamed three");

        let id_one = insert_applied(
            &mut store,
            &batch.id,
            &original_one.to_string_lossy(),
            &renamed_one.to_string_lossy(),
        );
        let id_two = insert_applied(
            &mut store,
            &batch.id,
            &original_two.to_string_lossy(),
            &renamed_two.to_string_lossy(),
        );
        let id_three = insert_applied(
            &mut store,
            &batch.id,
            &original_three.to_string_lossy(),
            &renamed_three.to_string_lossy(),
        );

        let plan = store.plan_undo(&batch.id, &StdRenameFs).expect("plan undo");
        assert_eq!(plan.batch_id, batch.id);
        assert_eq!(plan.rows.len(), 3);
        assert_eq!(plan.rows[0].operation_id, id_three);
        assert_eq!(plan.rows[0].status, UndoStatus::Ready);
        assert_eq!(plan.rows[1].operation_id, id_two);
        assert_eq!(plan.rows[1].status, UndoStatus::Conflict);
        assert_eq!(plan.rows[2].operation_id, id_one);
        assert_eq!(plan.rows[2].status, UndoStatus::Conflict);
        assert_eq!(
            plan.rows[1].from,
            renamed_two.to_string_lossy().into_owned()
        );
        assert_eq!(plan.rows[1].to, original_two.to_string_lossy().into_owned());

        let error = store
            .plan_undo("missing-batch", &StdRenameFs)
            .expect_err("unknown batch");
        assert!(matches!(error, LogError::BatchNotFound(_)));
    }

    #[test]
    fn plan_undo_reports_already_undone_and_leaves_pending_to_its_own_case() {
        let temp = tempdir().expect("tempdir");
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");

        let original = temp.path().join("done.txt");
        let renamed = temp.path().join("done[ABCDE].txt");
        fs::write(&renamed, b"done").expect("renamed file");
        let id = insert_applied(
            &mut store,
            &batch.id,
            &original.to_string_lossy(),
            &renamed.to_string_lossy(),
        );
        store.mark_undone(id).expect("mark undone");

        let plan = store.plan_undo(&batch.id, &StdRenameFs).expect("plan undo");
        // Pending rows are covered by plan_undo_reports_pending_operations_instead_of_dropping_them;
        // this case only pins the already-undone verdict.
        assert_eq!(plan.rows.len(), 1);
        assert_eq!(plan.rows[0].operation_id, id);
        assert_eq!(plan.rows[0].status, UndoStatus::AlreadyUndone);
    }

    #[test]
    fn e2e_rename_log_and_undo() {
        let temp = tempdir().expect("tempdir");
        let original = temp.path().join("a.txt");
        fs::write(&original, b"hello").expect("original file");
        let original_display = original.to_string_lossy().into_owned();
        let mut entry = FileEntry::new(
            "root",
            RootKind::Fs,
            "a.txt",
            original_display.clone(),
            5,
            None,
        );
        entry.hashes.insert(AlgorithmId::Blake3, digest());

        let plan = plan_renames(&[entry], &RenamePlanOptions::default(), &StdRenameFs);
        let results = execute_plan(&plan, &mut StdFsBackend::new());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, RenameResultStatus::Renamed);
        let renamed = results[0].new_path.clone();
        assert!(fs::metadata(&renamed).is_ok());
        assert!(!original.exists());

        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = BatchRecord::new(
            temp.path().to_string_lossy().into_owned(),
            RootKind::Fs,
            vec![AlgorithmId::Blake3],
            SuffixFormat::default(),
        );
        store.create_batch(&batch).expect("create batch");
        let operation = RenameOperation::new(
            batch.id.clone(),
            original_display.clone(),
            renamed.clone(),
            AlgorithmId::Blake3,
            digest(),
            "ABCDE",
        );
        let operation_id = store.insert_operation(&operation).expect("insert");
        store
            .record_result(operation_id, "renamed", None, OperationState::Applied)
            .expect("record applied");

        let undo_plan = store.plan_undo(&batch.id, &StdRenameFs).expect("plan undo");
        assert_eq!(undo_plan.rows.len(), 1);
        assert_eq!(undo_plan.rows[0].status, UndoStatus::Ready);
        assert_eq!(undo_plan.rows[0].from, renamed);
        assert_eq!(undo_plan.rows[0].to, original_display);

        let undo_results =
            apply_undo(&undo_plan, &mut StdFsBackend::new(), &mut store).expect("apply undo");
        assert_eq!(undo_results.len(), 1);
        assert_eq!(undo_results[0].status, UndoResultStatus::Undone);
        assert_eq!(
            fs::read(&original).expect("restored content"),
            b"hello".to_vec()
        );
        assert!(!Path::new(&renamed).exists());
        assert_eq!(
            store.operation_state(operation_id).expect("state"),
            Some(OperationState::Undone)
        );

        let second_plan = store
            .plan_undo(&batch.id, &StdRenameFs)
            .expect("second plan");
        assert_eq!(second_plan.rows[0].status, UndoStatus::AlreadyUndone);
        let second_results =
            apply_undo(&second_plan, &mut StdFsBackend::new(), &mut store).expect("second apply");
        assert_eq!(second_results[0].status, UndoResultStatus::Skipped);
        assert!(original.exists());

        let stale_results = apply_undo(&undo_plan, &mut StdFsBackend::new(), &mut store)
            .expect("stale plan must not rename twice");
        assert_eq!(stale_results[0].status, UndoResultStatus::Skipped);
        assert!(original.exists());
        assert!(!Path::new(&renamed).exists());
    }

    #[test]
    fn archive_round_trips_batches_and_operations() {
        let temp = tempdir().expect("tempdir");
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");
        let first = insert_applied(&mut store, &batch.id, "/tmp/a.txt", "/tmp/a[ABCDEF].txt");
        let second = insert_pending(&mut store, &batch.id, "/tmp/b.txt", "/tmp/b[ABCDEF].txt");
        store.mark_undone(first).expect("mark undone");

        let archive = store.export_archive(None).expect("export");
        assert_eq!(archive.version, ARCHIVE_VERSION);
        assert_eq!(archive.batches.len(), 1);
        assert_eq!(archive.operation_count(), 2);
        let exported = &archive.batches[0];
        assert_eq!(exported.batch.id, batch.id);
        assert_eq!(exported.operations.len(), 2);
        assert_eq!(exported.operations[0].state, "undone");
        assert_eq!(exported.operations[1].state, "pending");
        assert!(exported.operations[0].undone_at.is_some());

        // The document must not leak local row ids: they mean nothing elsewhere.
        let json = store.export_archive_json(None).expect("export json");
        assert!(
            !json.contains("\"id\": 1"),
            "row ids must not be archived: {json}"
        );
        assert!(json.contains("\"batch_id\""));

        // Import into a fresh store and compare the visible state.
        let mut restored = LogStore::open_in_memory().expect("second store");
        let summary = restored.import_archive_json(&json).expect("import");
        assert_eq!(summary.batches, 1);
        assert_eq!(summary.operations, 2);
        assert_eq!(summary.skipped, 0);
        let batches = restored.list_batches().expect("list");
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].id, batch.id);
        let operations = restored
            .operations_for_batch(&batch.id)
            .expect("operations");
        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0].state, OperationState::Undone);
        assert_eq!(operations[1].state, OperationState::Pending);
        assert_eq!(operations[1].original_path, "/tmp/b.txt");
        let _ = second;
        let _ = temp;
    }

    #[test]
    fn importing_the_same_archive_twice_skips_every_batch() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");
        insert_applied(&mut store, &batch.id, "/tmp/a.txt", "/tmp/a[ABCDEF].txt");
        let json = store.export_archive_json(None).expect("export json");

        let mut target = LogStore::open_in_memory().expect("second store");
        let first = target.import_archive_json(&json).expect("first import");
        assert_eq!(first.batches, 1);
        assert_eq!(first.operations, 1);

        // Importing the same document again must be a no-op, not a duplicate.
        let second = target.import_archive_json(&json).expect("second import");
        assert_eq!(second.batches, 0);
        assert_eq!(second.operations, 0);
        assert_eq!(second.skipped, 1);
        assert_eq!(
            target
                .operations_for_batch(&batch.id)
                .expect("operations")
                .len(),
            1,
            "an idempotent import must not duplicate operations"
        );
    }

    #[test]
    fn import_rejects_a_newer_archive_version_and_unknown_states() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let mut archive = store.export_archive(None).expect("export");
        archive.version = ARCHIVE_VERSION + 1;
        let error = store.import_archive(&archive).expect_err("newer version");
        assert!(matches!(error, LogError::InvalidStoredValue(_)));

        let batch = test_batch();
        let mut archive = RenameLogArchive {
            version: ARCHIVE_VERSION,
            tool: None,
            tool_version: None,
            exported_at: now_rfc3339(),
            batches: vec![ArchivedBatch {
                batch: batch.clone(),
                operations: vec![ArchivedOperation {
                    batch_id: batch.id.clone(),
                    original_path: "/tmp/a.txt".to_string(),
                    new_path: "/tmp/a[ABCDEF].txt".to_string(),
                    algorithm: AlgorithmId::Blake3,
                    full_hash: digest(),
                    suffix: "ABCDEF".to_string(),
                    op_type: RenameOperation::TYPE_RENAME.to_string(),
                    result: None,
                    error_kind: None,
                    created_at: now_rfc3339(),
                    undone_at: None,
                    state: "nonsense".to_string(),
                }],
            }],
        };
        let error = store.import_archive(&archive).expect_err("bad state");
        assert!(matches!(error, LogError::InvalidStoredValue(_)));

        // A malformed document is reported as invalid rather than panicking.
        let error = store
            .import_archive_json("{\"version\":1")
            .expect_err("truncated json");
        assert!(matches!(error, LogError::InvalidStoredValue(_)));
        archive.version = ARCHIVE_VERSION;
    }

    #[test]
    fn export_of_one_batch_only_contains_that_batch() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let first = test_batch();
        let mut second = test_batch();
        second.id = "other-batch".to_string();
        store.create_batch(&first).expect("first batch");
        store.create_batch(&second).expect("second batch");
        insert_applied(&mut store, &first.id, "/tmp/a.txt", "/tmp/a[ABCDEF].txt");

        let archive = store.export_archive(Some(&first.id)).expect("export");
        assert_eq!(archive.batches.len(), 1);
        assert_eq!(archive.batches[0].batch.id, first.id);

        let error = store
            .export_archive(Some("missing"))
            .expect_err("unknown batch");
        assert!(matches!(error, LogError::BatchNotFound(_)));
    }

    #[test]
    fn plan_undo_reports_pending_operations_instead_of_dropping_them() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");
        // Never recorded as applied: the crash window left it pending.
        let id = store
            .insert_operation(&test_operation(
                &batch.id,
                "/tmp/a.txt",
                "/tmp/a[ABCDE].txt",
            ))
            .expect("insert operation");

        let plan = store.plan_undo(&batch.id, &StdRenameFs).expect("plan undo");
        assert_eq!(plan.rows.len(), 1, "a pending row must not vanish");
        assert_eq!(plan.rows[0].operation_id, id);
        assert_eq!(plan.rows[0].status, UndoStatus::Conflict);
        assert!(
            plan.rows[0]
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("pending")),
            "reason: {:?}",
            plan.rows[0].reason
        );

        // Executing that row must not rename anything.
        let results = apply_undo(&plan, &mut StdFsBackend::new(), &mut store).expect("apply undo");
        assert_eq!(results[0].status, UndoResultStatus::Skipped);
    }

    #[test]
    fn reconcile_marks_an_ambiguous_pending_operation_as_conflict() {
        let temp = tempdir().expect("tempdir");
        let original = temp.path().join("a.txt");
        let renamed = temp.path().join("a[ABCDE].txt");
        fs::write(&original, b"original").expect("original file");
        fs::write(&renamed, b"renamed").expect("renamed file");

        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");
        let id = insert_pending(
            &mut store,
            &batch.id,
            &original.to_string_lossy(),
            &renamed.to_string_lossy(),
        );

        let report = reconcile_pending_operations(&mut store, &StdRenameFs).expect("reconcile");
        assert_eq!(report.ambiguous, 1, "report: {report:?}");
        assert_eq!(report.operations[0].operation_id, id);
        assert_eq!(
            report.operations[0].outcome,
            ReconcileOutcome::Ambiguous,
            "both paths present must never be guessed"
        );
        assert_eq!(
            store.operation_state(id).expect("state"),
            Some(OperationState::Conflict)
        );
        let stored = store
            .operations_for_batch(&batch.id)
            .expect("operations")
            .into_iter()
            .next()
            .expect("operation");
        assert_eq!(stored.result.as_deref(), Some("ambiguous"));
        assert!(stored.error_kind.is_some(), "the reason must be stored");

        // Both files survive: reconciliation never moves anything.
        assert!(original.exists());
        assert!(renamed.exists());
    }
}
