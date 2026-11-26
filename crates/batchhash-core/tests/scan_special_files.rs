//! A scan must never hand a FIFO or device node to the hasher.
//!
//! Reading a named pipe has no end: it blocks until a writer appears. Before
//! the filter these tests cover, a recursive scan pushed every non-directory
//! entry into the hash scheduler, so one FIFO in the tree hung the whole batch
//! instead of being reported.

use std::path::Path;

use batchhash_core::scan::{scan, ScanOptions};
use batchhash_types::RootKind;

/// Create a named pipe at [path].
#[cfg(unix)]
fn make_fifo(path: &Path) {
    use std::ffi::CString;
    let text = path.to_str().expect("utf-8 path");
    let c_path = CString::new(text).expect("path without NUL");
    // SAFETY: `c_path` is a valid NUL-terminated string and the mode is a
    // plain permission mask; mkfifo only reads them.
    let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) };
    assert_eq!(rc, 0, "mkfifo must succeed for {text}");
}

#[cfg(unix)]
#[test]
fn scan_skips_a_named_pipe_instead_of_blocking() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("real.txt"), b"data").expect("regular file");
    make_fifo(&temp.path().join("pipe"));

    let started = std::time::Instant::now();
    let outcome = scan(temp.path(), &ScanOptions::default()).expect("scan");
    let elapsed = started.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "scan must not block on a FIFO, took {elapsed:?}"
    );
    let names: Vec<&str> = outcome
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    assert_eq!(names, vec!["real.txt"], "only the regular file is scanned");
    assert_eq!(outcome.skipped.len(), 1, "the pipe is reported as skipped");
    assert_eq!(
        outcome.skipped[0].path,
        temp.path().join("pipe").display().to_string()
    );
    assert!(
        outcome.skipped[0].reason.contains("named pipe"),
        "reason names the object: {}",
        outcome.skipped[0].reason
    );
}

#[cfg(unix)]
#[test]
fn scan_skips_a_socket_instead_of_blocking() {
    use std::os::unix::net::UnixListener;

    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("real.txt"), b"data").expect("regular file");
    let socket_path = temp.path().join("sock");
    let _listener = UnixListener::bind(&socket_path).expect("bind socket");

    let outcome = scan(temp.path(), &ScanOptions::default()).expect("scan");
    let names: Vec<&str> = outcome
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    assert_eq!(names, vec!["real.txt"]);
    assert_eq!(outcome.skipped.len(), 1);
    assert!(outcome.skipped[0].reason.contains("socket"));
}

#[test]
fn scan_keeps_regular_files_and_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(temp.path().join("nested")).expect("nested dir");
    std::fs::write(temp.path().join("nested/inner.txt"), b"x").expect("inner file");

    let options = ScanOptions::default().with_recursive(true);
    let outcome = scan(temp.path(), &options).expect("scan");
    assert_eq!(outcome.root_kind, RootKind::Fs);
    assert_eq!(outcome.skipped.len(), 0, "regular files are never skipped");
    let names: Vec<&str> = outcome
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    assert_eq!(names, vec!["nested/inner.txt"]);
}
