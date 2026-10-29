//! Integration tests for the scan and pipeline facade.
//!
//! These run in CI only (local builds are forbidden in this repository).

use std::fs;
use std::path::Path;

use batchhash_core::pipeline::{run_batch, BatchPhase, BatchRequest, PipelineError, PipelineEvent};
use batchhash_core::scan::{scan, ScanOptions};
use batchhash_hash::CancelToken;
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
