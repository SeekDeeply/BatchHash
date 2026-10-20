//! SQLite persistence for rename batches, operations and cross-session undo.
//!
//! The store owns the durable rename log: one `batch` row per planned rename
//! batch and one `rename_operation` row per file. The operation state machine is
//! `pending -> applied -> undone | conflict`; every write is an explicit
//! [`LogStore`] call returning [`LogError`] and schema changes are versioned in
//! the `schema_migration` table.
//!
//! Undo is planned, not assumed: [`LogStore::plan_undo`] walks the batch
//! newest-first and marks each operation [`UndoStatus::Ready`],
//! [`UndoStatus::Conflict`] or [`UndoStatus::AlreadyUndone`] by checking that the
//! renamed file still exists and the original name is free. [`apply_undo`]
//! renames only ready rows and never overwrites an existing original path.

use std::fmt;
use std::path::Path;

use batchhash_types::{AlgorithmId, HexCase, RootKind};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::planner::{RenameBackend, RenameFs};
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
        u32::try_from(version)
            .map_err(|_| LogError::InvalidStoredValue(format!("schema version out of range: {version}")))
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
    /// so a repeated undo is visibly a no-op. Pending and conflicted
    /// operations are skipped.
    pub fn plan_undo(
        &self,
        batch_id: &str,
        fs: &dyn RenameFs,
    ) -> Result<UndoPlan, LogError> {
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
                OperationState::Pending | OperationState::Conflict => {}
            }
        }
        Ok(UndoPlan {
            batch_id: batch_id.to_string(),
            rows,
        })
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
/// Only [`UndoStatus::Ready`] rows are attempted, and an operation is re-checked
/// against the store immediately before its rename so a stale plan (or a double
/// apply of the same plan) can never rename twice. Successful rows are marked
/// undone in one transaction. A backend failure leaves the operation applied so
/// the undo can be retried.
pub fn apply_undo(
    plan: &UndoPlan,
    backend: &mut dyn RenameBackend,
    store: &mut LogStore,
) -> Result<Vec<UndoResult>, LogError> {
    let mut results = Vec::with_capacity(plan.rows.len());
    let mut undone = Vec::new();
    for row in &plan.rows {
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
                match backend.rename(Path::new(&row.from), Path::new(&row.to)) {
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
        let state = OperationState::from_str_name(&self.state)
            .ok_or_else(|| LogError::InvalidStoredValue(format!("unknown state: {}", self.state)))?;
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
        let id = store.insert_operation(&operation).expect("insert operation");
        store
            .record_result(id, "renamed", None, OperationState::Applied)
            .expect("record applied");
        id
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
            let id = store.insert_operation(&operation).expect("insert operation");
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
            batches.iter().map(|batch| batch.id.as_str()).collect::<Vec<_>>(),
            vec!["b-older", "b-newer"]
        );
    }

    #[test]
    fn operation_state_machine_is_enforced() {
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");
        let operation = test_operation(&batch.id, "/tmp/a.txt", "/tmp/a[ABCDE].txt");
        let id = store.insert_operation(&operation).expect("insert operation");

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
        let id = store.insert_operation(&operation).expect("insert operation");
        store
            .record_result(id, "failed", Some("already_exists"), OperationState::Conflict)
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

        let plan = store
            .plan_undo(&batch.id, &StdRenameFs)
            .expect("plan undo");
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
        assert_eq!(
            plan.rows[1].to,
            original_two.to_string_lossy().into_owned()
        );

        let error = store
            .plan_undo("missing-batch", &StdRenameFs)
            .expect_err("unknown batch");
        assert!(matches!(error, LogError::BatchNotFound(_)));
    }

    #[test]
    fn plan_undo_reports_already_undone_and_skips_pending() {
        let temp = tempdir().expect("tempdir");
        let mut store = LogStore::open_in_memory().expect("in-memory store");
        let batch = test_batch();
        store.create_batch(&batch).expect("create batch");

        let pending = test_operation(
            &batch.id,
            &temp.path().join("pending.txt").to_string_lossy(),
            &temp.path().join("pending[ABCDE].txt").to_string_lossy(),
        );
        store.insert_operation(&pending).expect("insert pending");

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

        let plan = store
            .plan_undo(&batch.id, &StdRenameFs)
            .expect("plan undo");
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

        let undo_plan = store
            .plan_undo(&batch.id, &StdRenameFs)
            .expect("plan undo");
        assert_eq!(undo_plan.rows.len(), 1);
        assert_eq!(undo_plan.rows[0].status, UndoStatus::Ready);
        assert_eq!(undo_plan.rows[0].from, renamed);
        assert_eq!(undo_plan.rows[0].to, original_display);

        let undo_results = apply_undo(&undo_plan, &mut StdFsBackend::new(), &mut store)
            .expect("apply undo");
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
        let second_results = apply_undo(&second_plan, &mut StdFsBackend::new(), &mut store)
            .expect("second apply");
        assert_eq!(second_results[0].status, UndoResultStatus::Skipped);
        assert!(original.exists());

        let stale_results = apply_undo(&undo_plan, &mut StdFsBackend::new(), &mut store)
            .expect("stale plan must not rename twice");
        assert_eq!(stale_results[0].status, UndoResultStatus::Skipped);
        assert!(original.exists());
        assert!(!Path::new(&renamed).exists());
    }
}
