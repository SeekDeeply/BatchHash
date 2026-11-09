//! Integration tests for the scan and pipeline facade.
//!
//! These run in CI only (local builds are forbidden in this repository).

use std::fs;
use std::path::Path;

use batchhash_core::pipeline::{
    reconcile_log, run_batch, BatchPhase, BatchRequest, PipelineError, PipelineEvent,
};
use batchhash_core::scan::{scan, ScanOptions};
use batchhash_hash::CancelToken;
use batchhash_rename::log_store::{
    apply_undo, reconcile_pending_operations, BatchRecord, LogStore, OperationState,
    RenameOperation, UndoResultStatus, UndoStatus,
};
use batchhash_rename::planner::{StdFsBackend, StdRenameFs};
use batchhash_rename::suffix::{SuffixFormat, SuffixParseResult};
use batchhash_types::{AlgorithmId, EntryStatus, ErrorKind, HexCase, RootKind};

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write fixture");
}

fn md5_format() -> SuffixFormat {
    SuffixFormat {
        length: 8,
        case: HexCase::Upper,
        algorithm: AlgorithmId::Md5,
        prefix_enabled: false,
    }
}

fn no_events(_event: PipelineEvent) {}

#[test]
fn scan_orders_entries_and_normalises_relative_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("b.txt"), b"b");
    write(&temp.path().join("a.txt"), b"a");
    write(&temp.path().join("nested/deep/c.txt"), b"c");

    let outcome = scan(temp.path(), &ScanOptions::default()).expect("scan");
    assert_eq!(outcome.root_kind, RootKind::Fs);
    let paths: Vec<&str> = outcome
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    assert_eq!(paths, vec!["a.txt", "b.txt", "nested/deep/c.txt"]);
    assert!(outcome.skipped.is_empty());
    for entry in &outcome.entries {
        assert!(entry.status.is_ok());
        assert_eq!(entry.root_kind, RootKind::Fs);
        assert!(entry.display_path.contains(
            &entry
                .relative_path
                .replace('/', std::path::MAIN_SEPARATOR_STR)
        ));
    }
}

#[test]
fn scan_of_a_single_file_uses_the_file_name() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("solo.bin");
    write(&file, b"12345");

    let outcome = scan(&file, &ScanOptions::default()).expect("scan");
    assert_eq!(outcome.entries.len(), 1);
    assert_eq!(outcome.entries[0].relative_path, "solo.bin");
    assert_eq!(outcome.entries[0].size, 5);
}

#[test]
fn scan_reports_a_missing_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let error = scan(&temp.path().join("nope"), &ScanOptions::default()).expect_err("must fail");
    let message = error.to_string();
    assert!(
        message.contains("cannot read batch root"),
        "message: {message}"
    );
}

#[cfg(unix)]
#[test]
fn scan_detects_symlink_loops_when_following_links() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("dir/file.txt"), b"x");
    std::os::unix::fs::symlink(temp.path(), temp.path().join("dir/loop")).expect("symlink");

    let skipped_links = scan(temp.path(), &ScanOptions::default()).expect("scan");
    let names: Vec<&str> = skipped_links
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    assert_eq!(names, vec!["dir/file.txt"]);

    let followed =
        scan(temp.path(), &ScanOptions::default().with_follow_links(true)).expect("scan");
    // The loop terminates and no entry appears twice.
    let mut paths: Vec<&str> = followed
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    let count = paths.len();
    paths.sort_unstable();
    paths.dedup();
    assert_eq!(paths.len(), count, "duplicate entries after loop detection");
    assert!(count >= 1);
}

#[test]
fn pipeline_hashes_and_builds_a_valid_report() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");
    write(&temp.path().join("nested/b.txt"), b"bravo");

    let mut request = BatchRequest::new(temp.path(), vec![AlgorithmId::Md5, AlgorithmId::Sha256]);
    request.generated_at = Some("2026-01-01T00:00:00Z".to_string());
    let phases = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = std::sync::Arc::clone(&phases);
    let events = move |event: PipelineEvent| {
        if let PipelineEvent::Phase(phase) = event {
            recorder.lock().expect("lock").push(phase);
        }
    };

    let run = run_batch(&request, &CancelToken::new(), &events).expect("run");
    run.report.validate().expect("valid report");
    assert_eq!(run.report.generated_at, "2026-01-01T00:00:00Z");
    assert_eq!(run.report.summary.total, 2);
    assert_eq!(run.report.summary.ok, 2);
    assert_eq!(run.report.entries.len(), 2);
    assert!(run
        .report
        .entries
        .iter()
        .all(|entry| entry.hashes.len() == 2));
    assert!(run.plan.is_none());
    assert!(run.batch_id.is_none());
    assert!(!run.cancelled);

    let phases = phases.lock().expect("phases").clone();
    assert_eq!(
        phases,
        vec![
            BatchPhase::Scanning,
            BatchPhase::Hashing,
            BatchPhase::Completed
        ]
    );
}

#[test]
fn pipeline_suffix_dry_run_never_touches_the_filesystem() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write(&file, b"movie");

    let request =
        BatchRequest::new(temp.path(), vec![AlgorithmId::Md5]).with_suffix_dry_run(md5_format());
    let run = run_batch(&request, &CancelToken::new(), &no_events).expect("run");
    let plan = run.plan.expect("plan");
    assert_eq!(plan.summary.ready, 1);
    assert!(run.batch_id.is_none());
    assert!(file.exists(), "dry run must not rename");
    assert!(run.report.suffix_config.enabled);
    assert_eq!(run.report.suffix_config.algorithm, AlgorithmId::Md5);
}

#[test]
fn pipeline_suffix_apply_writes_log_and_annotates_the_report() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write(&file, b"movie");
    let db = temp.path().join("logs/rename.db");

    let request =
        BatchRequest::new(temp.path(), vec![AlgorithmId::Md5]).with_suffix(md5_format(), &db);
    let run = run_batch(&request, &CancelToken::new(), &no_events).expect("run");
    assert!(db.exists(), "rename log must be created");
    let batch_id = run.batch_id.clone().expect("batch id");
    assert_eq!(run.rename_results.len(), 1);
    assert_eq!(run.report.entries.len(), 1);
    let entry = &run.report.entries[0];
    assert_eq!(entry.suffix_algorithm, Some(AlgorithmId::Md5));

    let renamed_to = entry.renamed_to.clone().expect("renamed_to");
    let token = entry.suffix_applied.clone().expect("suffix applied");
    assert_eq!(token.len(), 8);
    assert!(renamed_to.starts_with("movie[") && renamed_to.ends_with("].mkv"));
    assert!(!file.exists(), "the original name must be gone");
    assert!(temp.path().join(&renamed_to).exists());

    // The renamed file parses back to the recorded token.
    match batchhash_rename::suffix::parse_filename(&renamed_to) {
        SuffixParseResult::Parsed(parsed) => assert_eq!(parsed.token, token),
        other => panic!("expected a parsed suffix, got {other:?}"),
    }

    // Undo restores the original name through the log store.
    let mut store = batchhash_rename::log_store::LogStore::open(&db).expect("open log");
    let plan = store
        .plan_undo(&batch_id, &batchhash_rename::planner::StdRenameFs)
        .expect("plan undo");
    assert_eq!(plan.rows.len(), 1);
    let results = batchhash_rename::log_store::apply_undo(
        &plan,
        &mut batchhash_rename::planner::StdFsBackend,
        &mut store,
    )
    .expect("apply undo");
    assert_eq!(results.len(), 1);
    assert!(file.exists(), "undo restores the original name");
}

#[test]
fn cancelled_pipeline_marks_entries_and_never_renames() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write(&file, b"movie");

    let cancel = CancelToken::new();
    cancel.cancel();
    let request = BatchRequest::new(temp.path(), vec![AlgorithmId::Md5])
        .with_suffix(md5_format(), temp.path().join("rename.db"));
    let run = run_batch(&request, &cancel, &no_events).expect("run");
    assert!(run.cancelled);
    assert_eq!(run.report.entries.len(), 1);
    let entry = &run.report.entries[0];
    assert_eq!(entry.status, EntryStatus::Skipped);
    assert_eq!(entry.error_kind, Some(ErrorKind::Cancelled));
    assert!(file.exists(), "a cancelled run must not rename");
    assert!(!temp.path().join("rename.db").exists());
}

#[test]
fn empty_algorithm_list_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = BatchRequest::new(temp.path(), Vec::new());
    let error = run_batch(&request, &CancelToken::new(), &no_events).expect_err("must fail");
    assert!(matches!(error, PipelineError::InvalidRequest(_)));
}

#[test]
fn applying_renames_without_a_log_path_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("movie.mkv"), b"movie");
    let mut request = BatchRequest::new(temp.path(), vec![AlgorithmId::Md5]);
    request.suffix = Some(md5_format());
    request.apply_renames = true;
    request.log_db = None;
    let error = run_batch(&request, &CancelToken::new(), &no_events).expect_err("must fail");
    assert!(matches!(error, PipelineError::InvalidRequest(_)));
}

#[test]
fn invalid_suffix_format_is_rejected_before_scanning() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut format = md5_format();
    format.length = 3;
    let request =
        BatchRequest::new(temp.path(), vec![AlgorithmId::Md5]).with_suffix_dry_run(format);
    let error = run_batch(&request, &CancelToken::new(), &no_events).expect_err("must fail");
    assert!(matches!(error, PipelineError::Suffix(_)));
}

/// Insert one pending operation for a rename that has *not* happened yet.
fn insert_pending(store: &mut LogStore, batch: &BatchRecord, from: &Path, to: &Path) -> i64 {
    let operation = RenameOperation::new(
        batch.id.clone(),
        from.to_string_lossy().into_owned(),
        to.to_string_lossy().into_owned(),
        AlgorithmId::Md5,
        "0123456789abcdef0123456789abcdef".to_string(),
        "ABCDEF12".to_string(),
    );
    store.insert_operation(&operation).expect("insert operation")
}

fn new_batch(root: &Path) -> BatchRecord {
    BatchRecord::new(
        root.to_string_lossy().into_owned(),
        RootKind::Fs,
        vec![AlgorithmId::Md5],
        md5_format(),
    )
}

#[test]
fn undo_never_restores_over_an_occupied_original_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db = temp.path().join("rename.db");
    let original = temp.path().join("movie.mkv");
    write(&original, b"movie");

    // A real batch: plan the rename, then perform it so the log records applied.
    let request =
        BatchRequest::new(temp.path(), vec![AlgorithmId::Md5]).with_suffix(md5_format(), &db);
    let run = run_batch(&request, &CancelToken::new(), &no_events).expect("run");
    let batch_id = run.batch_id.clone().expect("batch id");
    assert!(!original.exists(), "the rename happened");

    let mut store = LogStore::open(&db).expect("open log");
    let plan = store.plan_undo(&batch_id, &StdRenameFs).expect("plan undo");
    assert_eq!(plan.rows.len(), 1);
    assert_eq!(plan.rows[0].status, UndoStatus::Ready);
    let undo_from = plan.rows[0].from.clone();

    // The original name is claimed again after the plan was built: a recreated
    // file there must survive the stale undo untouched.
    write(&original, b"squatter");
    assert!(
        Path::new(&undo_from).exists(),
        "the renamed file still exists"
    );

    let results = apply_undo(&plan, &mut StdFsBackend, &mut store).expect("apply undo");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].status,
        UndoResultStatus::Skipped,
        "an occupied original path must skip the row: {:?}",
        results[0]
    );
    assert!(results[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("already occupied")));
    // Both files are intact: nothing was overwritten and nothing was moved.
    assert_eq!(fs::read(&original).expect("read squatter"), b"squatter".to_vec());
    assert!(Path::new(&undo_from).exists(), "the renamed file must stay");
    assert_eq!(
        store.operation_state(results[0].operation_id).expect("state"),
        Some(OperationState::Applied),
        "a skipped undo must stay applied"
    );
}

#[test]
fn a_pending_operation_whose_rename_happened_is_recovered_as_applied() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db = temp.path().join("rename.db");
    let original = temp.path().join("movie.mkv");
    let renamed = temp.path().join("movie[ABCDEF12].mkv");

    let mut store = LogStore::open(&db).expect("open log");
    let batch = new_batch(temp.path());
    store.create_batch(&batch).expect("create batch");
    let id = insert_pending(&mut store, &batch, &original, &renamed);
    assert_eq!(
        store.operation_state(id).expect("state"),
        Some(OperationState::Pending)
    );

    // The crash window: the file was renamed but the result was never recorded.
    write(&renamed, b"movie");

    let report = reconcile_pending_operations(&mut store, &StdRenameFs).expect("reconcile");
    assert_eq!(report.applied, 1, "report: {report:?}");
    assert_eq!(report.resolved(), 1);
    assert_eq!(
        store.operation_state(id).expect("state"),
        Some(OperationState::Applied),
        "a rename that happened must become undoable"
    );

    // The recovered operation is undoable again, which the old code silently dropped.
    let plan = store.plan_undo(&batch.id, &StdRenameFs).expect("plan undo");
    assert_eq!(plan.rows.len(), 1);
    assert_eq!(plan.rows[0].status, UndoStatus::Ready);
    let results = apply_undo(&plan, &mut StdFsBackend, &mut store).expect("apply undo");
    assert_eq!(results[0].status, UndoResultStatus::Undone);
    assert!(original.exists(), "undo restores the recovered rename");

    // Reconciliation is idempotent: nothing is pending any more.
    let report = reconcile_pending_operations(&mut store, &StdRenameFs).expect("reconcile");
    assert_eq!(report.resolved(), 0);
}

#[test]
fn a_pending_operation_whose_rename_never_happened_is_recorded_as_conflict() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db = temp.path().join("rename.db");
    let original = temp.path().join("movie.mkv");
    let renamed = temp.path().join("movie[ABCDEF12].mkv");

    let mut store = LogStore::open(&db).expect("open log");
    let batch = new_batch(temp.path());
    store.create_batch(&batch).expect("create batch");
    let id = insert_pending(&mut store, &batch, &original, &renamed);

    // The other crash window: the log wrote the row but the rename never ran.
    write(&original, b"movie");

    let report = reconcile_pending_operations(&mut store, &StdRenameFs).expect("reconcile");
    assert_eq!(report.not_applied, 1, "report: {report:?}");
    assert_eq!(
        store.operation_state(id).expect("state"),
        Some(OperationState::Conflict),
        "an unrenamed file must not be reported as undoable"
    );
    let plan = store.plan_undo(&batch.id, &StdRenameFs).expect("plan undo");
    assert!(plan.rows.is_empty(), "conflicted rows are not undoable");
    assert!(original.exists());
    assert!(!renamed.exists());
}

#[test]
fn reconcile_log_resolves_pending_rows_before_an_apply() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db = temp.path().join("rename.db");
    let original = temp.path().join("movie.mkv");
    let renamed = temp.path().join("movie[ABCDEF12].mkv");

    let mut store = LogStore::open(&db).expect("open log");
    let batch = new_batch(temp.path());
    store.create_batch(&batch).expect("create batch");
    let id = insert_pending(&mut store, &batch, &original, &renamed);
    write(&renamed, b"movie");
    drop(store);

    // Opening the log for a later run resolves the stale row before it plans.
    let mut store = LogStore::open(&db).expect("reopen log");
    let report = reconcile_log(&mut store, &CancelToken::new()).expect("reconcile");
    assert_eq!(report.applied, 1);
    assert_eq!(
        store.operation_state(id).expect("state"),
        Some(OperationState::Applied)
    );
}

#[test]
fn cancellation_raised_on_the_apply_event_renames_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write(&file, b"movie");
    let db = temp.path().join("rename.db");

    let cancel = CancelToken::new();
    let request =
        BatchRequest::new(temp.path(), vec![AlgorithmId::Md5]).with_suffix(md5_format(), &db);
    // The UI cancels exactly when it is told renames are about to start: the
    // write must not happen after that event, and no row may stay unrecorded.
    let token = cancel.clone();
    let events = move |event: PipelineEvent| {
        if let PipelineEvent::Phase(BatchPhase::ApplyingSuffix) = event {
            token.cancel();
        }
    };

    let run = run_batch(&request, &cancel, &events).expect("run");
    assert!(run.cancelled, "the cancellation must be reported");
    assert!(file.exists(), "cancelling on the apply event must not rename");
    assert!(
        run.rename_results.is_empty(),
        "no rename may be executed after cancellation: {:?}",
        run.rename_results
    );
    assert!(db.exists(), "the log batch was already written");

    // Every inserted row is resolved: nothing stays pending for the next open.
    let mut store = LogStore::open(&db).expect("open log");
    let batch_id = run.batch_id.clone().expect("batch id");
    let operations = store.operations_for_batch(&batch_id).expect("operations");
    assert_eq!(operations.len(), 1, "the ready row was logged before the check");
    assert_eq!(
        operations[0].state,
        OperationState::Conflict,
        "an unattempted row must be recorded as a conflict, not left pending"
    );
    let report = reconcile_pending_operations(&mut store, &StdRenameFs).expect("reconcile");
    assert_eq!(report.resolved(), 0, "no pending row may survive the run");

    // The report must not advertise a rename that never happened.
    let entry = &run.report.entries[0];
    assert_eq!(entry.renamed_to, None, "no rename was performed");
    assert_eq!(entry.suffix_applied, None);
}
