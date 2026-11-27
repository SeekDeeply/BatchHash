//! The rename log lives inside the tree being scanned, so it must not be hashed
//! or renamed. Excluding it is a name match, and a name match is easy to get
//! wrong in a way that silently drops real files.

use std::fs;

use batchhash_core::hash::CancelToken;
use batchhash_core::pipeline::{run_batch, BatchRequest};
use batchhash_types::AlgorithmId;
use tempfile::tempdir;

fn request_for(root: &std::path::Path, log_db: &std::path::Path) -> BatchRequest {
    let mut request = BatchRequest::new(root.to_path_buf(), vec![AlgorithmId::Blake3]);
    request.log_db = Some(log_db.to_path_buf());
    request
}

#[test]
fn the_rename_log_is_excluded_from_the_report() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    fs::write(root.join("a.txt"), b"a").expect("file");
    let log_db = root.join(".batchhash-rename.db");
    fs::write(&log_db, b"not really a database").expect("log file");

    let run = run_batch(
        &request_for(root, &log_db),
        &CancelToken::new(),
        &|_event| {},
    )
    .expect("run");
    let paths: Vec<&str> = run
        .report
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec!["a.txt"],
        "the log must not be hashed: {paths:?}"
    );
}

#[test]
fn a_file_that_merely_shares_the_log_prefix_is_kept() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let log_db = root.join(".batchhash-rename.db");
    fs::write(&log_db, b"not really a database").expect("log file");
    // A backup the user made of the log is a real file with real content. A
    // prefix test would drop it from the report without saying so.
    fs::write(root.join(".batchhash-rename.db.bak"), b"backup").expect("backup");
    fs::write(root.join(".batchhash-rename.db2"), b"other").expect("other");

    let run = run_batch(
        &request_for(root, &log_db),
        &CancelToken::new(),
        &|_event| {},
    )
    .expect("run");
    let mut paths: Vec<&str> = run
        .report
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        vec![".batchhash-rename.db.bak", ".batchhash-rename.db2"],
        "neighbours of the log are real files"
    );
}

#[test]
fn sqlite_companion_files_are_excluded_too() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let log_db = root.join(".batchhash-rename.db");
    fs::write(&log_db, b"db").expect("log file");
    // In WAL mode SQLite keeps the newest committed pages in `-wal`; hashing it
    // would put the log's own bookkeeping into the report.
    fs::write(root.join(".batchhash-rename.db-wal"), b"wal").expect("wal");
    fs::write(root.join(".batchhash-rename.db-shm"), b"shm").expect("shm");
    fs::write(root.join("keep.txt"), b"keep").expect("keep");

    let run = run_batch(
        &request_for(root, &log_db),
        &CancelToken::new(),
        &|_event| {},
    )
    .expect("run");
    let paths: Vec<&str> = run
        .report
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(paths, vec!["keep.txt"]);
}

#[test]
fn a_suffix_algorithm_that_is_not_hashed_is_rejected() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    fs::write(root.join("a.txt"), b"a").expect("file");
    let log_db = root.join(".batchhash-rename.db");
    let mut request = request_for(root, &log_db);
    // Hashes blake3 only, but asks for an md5 suffix: the digest would be
    // missing at rename time, so the request is refused before any work.
    request.suffix = Some(batchhash_core::rename::suffix::MultiSuffixFormat::single(
        batchhash_core::rename::suffix::SuffixFormat {
            algorithm: AlgorithmId::Md5,
            ..Default::default()
        },
    ));

    let error = run_batch(&request, &CancelToken::new(), &|_event| {}).expect_err("must fail");
    assert!(
        error
            .to_string()
            .contains("not among the hashing algorithms"),
        "error: {error}"
    );
}
