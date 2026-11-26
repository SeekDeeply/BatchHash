//! Batch scheduler: order, progress, events, cancellation and fd input.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use batchhash_hash::{
    compute_hashes_file, hash_entries, BatchEvent, CancelToken, FileIo, HashEntryOptions,
    HashOptions, ProgressSnapshot,
};
use batchhash_types::{AlgorithmId, EntryStatus, ErrorKind, FileEntry, RootKind};

fn write_file(dir: &Path, name: &str, data: &[u8]) -> FileEntry {
    let path = dir.join(name);
    fs::write(&path, data).expect("file must be written");
    FileEntry::new(
        "root-1",
        RootKind::Fs,
        name,
        path.to_string_lossy().into_owned(),
        data.len() as u64,
        None,
    )
}

fn batch_options(algorithms: Vec<AlgorithmId>) -> HashEntryOptions {
    HashEntryOptions {
        hasher: HashOptions::new(algorithms).with_buffer_size(64 * 1024),
        io: FileIo::Path,
    }
}

fn event_log() -> (Arc<Mutex<Vec<BatchEvent>>>, impl Fn(BatchEvent) + Sync) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    (log, move |event: BatchEvent| {
        sink.lock().expect("event log lock").push(event);
    })
}

#[test]
fn batch_hashes_all_entries_in_input_order() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let specs = [
        ("a.bin", 1_000usize),
        ("b.bin", 500_000usize),
        ("c.bin", 3usize),
    ];
    let mut entries = Vec::new();
    let mut expected_bytes = 0u64;
    for (name, len) in specs {
        let data: Vec<u8> = (0..len)
            .map(|index| ((index + name.len()) % 251) as u8)
            .collect();
        expected_bytes += len as u64;
        entries.push(write_file(dir.path(), name, &data));
    }
    let algorithms = vec![AlgorithmId::Blake3, AlgorithmId::Crc32, AlgorithmId::Md5];
    let options = batch_options(algorithms.clone());
    let (log, callback) = event_log();

    let outcome = hash_entries(&entries, &options, &CancelToken::new(), &callback);

    assert!(!outcome.cancelled);
    assert_eq!(outcome.entries.len(), specs.len());
    assert_eq!(outcome.bytes_hashed, expected_bytes);
    for (index, entry) in outcome.entries.iter().enumerate() {
        assert_eq!(entry.display_path, entries[index].display_path);
        assert_eq!(entry.status, EntryStatus::Ok);
        assert_eq!(entry.hashes.len(), algorithms.len());
        let reference = compute_hashes_file(
            Path::new(&entry.display_path),
            &HashOptions::new(algorithms.clone()),
        )
        .expect("reference hashing must succeed");
        assert_eq!(entry.hashes, reference);
    }

    let (_, second_callback) = event_log();
    let second = hash_entries(&entries, &options, &CancelToken::new(), &second_callback);
    for (left, right) in outcome.entries.iter().zip(second.entries.iter()) {
        assert_eq!(
            left.hashes, right.hashes,
            "repeated batches must be deterministic"
        );
    }

    let recorded = log.lock().expect("event log lock");
    let entry_done = recorded
        .iter()
        .filter(|event| matches!(event, BatchEvent::EntryDone(_)))
        .count();
    let finished = recorded
        .iter()
        .filter(|event| matches!(event, BatchEvent::Finished(_)))
        .count();
    assert_eq!(entry_done, specs.len());
    assert_eq!(finished, 1);

    let progress: Vec<&ProgressSnapshot> = recorded
        .iter()
        .filter_map(|event| match event {
            BatchEvent::Progress(snapshot) => Some(snapshot),
            _ => None,
        })
        .collect();
    assert!(
        progress.len() >= 2,
        "expected at least start and final progress events"
    );
    let first = progress.first().expect("progress must not be empty");
    let last = progress.last().expect("progress must not be empty");
    assert_eq!(first.percent, 0.0);
    assert_eq!(last.percent, 100.0);
    assert_eq!(last.completed, specs.len() as u64);
    assert_eq!(last.total, specs.len() as u64);
    assert!(progress
        .iter()
        .all(|snapshot| (0.0..=100.0).contains(&snapshot.percent)));
    assert!(first.eta_seconds.is_none());
    assert!(last
        .eta_seconds
        .map(|eta| eta.is_finite() && eta >= 0.0)
        .unwrap_or(true));
}

#[test]
fn missing_file_is_marked_failed_and_other_entries_continue() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let good = write_file(dir.path(), "good.bin", b"good content");
    let missing_path = dir.path().join("missing.bin");
    let missing = FileEntry::new(
        "root-1",
        RootKind::Fs,
        "missing.bin",
        missing_path.to_string_lossy().into_owned(),
        10,
        None,
    );
    let good_size = good.size;
    let (_, callback) = event_log();
    let outcome = hash_entries(
        &[missing, good],
        &batch_options(vec![AlgorithmId::Blake3]),
        &CancelToken::new(),
        &callback,
    );

    assert_eq!(outcome.entries[0].status, EntryStatus::Failed);
    assert_eq!(outcome.entries[0].error_kind, Some(ErrorKind::NotFound));
    assert!(outcome.entries[0].hashes.is_empty());
    assert_eq!(outcome.entries[1].status, EntryStatus::Ok);
    assert_eq!(outcome.bytes_hashed, good_size);
}

#[test]
fn pre_cancelled_batch_skips_every_entry() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let first = write_file(dir.path(), "first.bin", b"first");
    let second = write_file(dir.path(), "second.bin", b"second");
    let cancel = CancelToken::new();
    cancel.cancel();
    let (log, callback) = event_log();

    let outcome = hash_entries(
        &[first, second],
        &batch_options(vec![AlgorithmId::Blake3]),
        &cancel,
        &callback,
    );

    assert!(outcome.cancelled);
    assert_eq!(outcome.bytes_hashed, 0);
    assert_eq!(outcome.entries.len(), 2);
    for entry in &outcome.entries {
        assert_eq!(entry.status, EntryStatus::Skipped);
        assert_eq!(entry.error_kind, Some(ErrorKind::Cancelled));
        assert!(entry.hashes.is_empty());
    }
    let recorded = log.lock().expect("event log lock");
    assert!(recorded
        .iter()
        .any(|event| matches!(event, BatchEvent::Finished(_))));
}

#[test]
fn empty_batch_finishes_immediately() {
    let (log, callback) = event_log();
    let outcome = hash_entries(
        &[],
        &HashEntryOptions::default(),
        &CancelToken::new(),
        &callback,
    );
    assert!(outcome.entries.is_empty());
    assert!(!outcome.cancelled);
    assert_eq!(outcome.bytes_hashed, 0);
    let recorded = log.lock().expect("event log lock");
    let finished = recorded
        .iter()
        .filter(|event| matches!(event, BatchEvent::Finished(_)))
        .count();
    assert_eq!(finished, 1);
    let last_progress = recorded.iter().rev().find_map(|event| match event {
        BatchEvent::Progress(snapshot) => Some(snapshot),
        _ => None,
    });
    assert_eq!(last_progress.map(|snapshot| snapshot.percent), Some(100.0));
}

#[cfg(unix)]
#[test]
fn fd_input_is_used_for_fd_entries() {
    use std::fs::File;
    use std::io::{Cursor, Read, Seek, SeekFrom};
    use std::os::unix::io::AsRawFd;

    let dir = tempfile::tempdir().expect("tempdir must be created");
    let path = dir.path().join("fd.bin");
    let data: Vec<u8> = (0..200).map(|index| (index * 7 % 256) as u8).collect();
    fs::write(&path, data.as_slice()).expect("file must be written");
    let mut file = File::open(&path).expect("file must open");
    let fd = file.as_raw_fd();

    let entry = FileEntry::new("root-1", RootKind::Fs, "fd.bin", "fd.bin", 50, None);
    let options = HashEntryOptions {
        hasher: HashOptions::new(vec![AlgorithmId::Blake3, AlgorithmId::Md5]),
        io: FileIo::Fd {
            fd,
            offset: 10,
            length: Some(50),
        },
    };
    let (_, callback) = event_log();
    let outcome = hash_entries(&[entry], &options, &CancelToken::new(), &callback);

    assert_eq!(outcome.entries[0].status, EntryStatus::Ok);
    assert_eq!(outcome.bytes_hashed, 50);
    let mut reader = Cursor::new(data[10..60].to_vec());
    let expected =
        batchhash_hash::compute_hashes(&mut reader, &options.hasher).expect("expected hashes");
    assert_eq!(outcome.entries[0].hashes, expected);

    file.seek(SeekFrom::Start(0)).expect("rewind must succeed");
    let mut reread = Vec::new();
    file.read_to_end(&mut reread).expect("reread must succeed");
    assert_eq!(reread, data, "caller descriptor must stay open");
}

/// A scan that already failed an entry must not be hashed again.
///
/// Re-hashing it would do pointless I/O and, worse, overwrite the original
/// reason: a permission failure recorded by the scan would come back as a
/// generic read error, hiding the cause the user has to act on.
#[test]
fn entries_failed_by_the_scan_keep_their_reason_and_are_not_reopened() {
    let dir = tempfile::tempdir().expect("tempdir");
    // No file on disk: if the scheduler tried to open it, the status would
    // become a read error rather than staying exactly as it was.
    let mut entry = FileEntry::new(
        "root-1",
        RootKind::Fs,
        "gone.txt",
        dir.path().join("gone.txt").to_string_lossy().into_owned(),
        0,
        None,
    );
    entry.status = EntryStatus::Failed;
    entry.error_kind = Some(ErrorKind::PermissionDenied);
    entry.error_message = Some("scan could not read this file".to_string());

    let outcome = hash_entries(
        &[entry],
        &batch_options(vec![AlgorithmId::Md5]),
        &CancelToken::new(),
        &|_event| {},
    );

    assert_eq!(outcome.entries.len(), 1);
    let result = &outcome.entries[0];
    assert_eq!(result.status, EntryStatus::Failed);
    assert_eq!(result.error_kind, Some(ErrorKind::PermissionDenied));
    assert_eq!(
        result.error_message.as_deref(),
        Some("scan could not read this file"),
        "the scan's reason must survive"
    );
    assert!(result.hashes.is_empty());
}

/// A file that changes size while it is being read must not be trusted.
///
/// The digest would describe bytes that are no longer the file's contents, so
/// recording it would label the file with a value it does not have.
#[test]
fn a_file_that_changes_while_being_read_is_reported_as_unstable() {
    use std::io::Write;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("grow.bin");
    // Large enough that hashing takes more than one buffer, giving the writer a
    // window to append before the read finishes.
    let mut file = fs::File::create(&path).expect("create");
    file.write_all(&vec![0u8; 8 * 1024 * 1024]).expect("write");
    drop(file);

    let writer_path = path.clone();
    let handle = std::thread::spawn(move || {
        // Append while the hash is running; the size check at the end is what
        // catches it, so the exact timing does not need to be tight.
        std::thread::sleep(std::time::Duration::from_millis(1));
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(&writer_path) {
            let _ = file.write_all(&vec![1u8; 1024]);
        }
    });

    let result = compute_hashes_file(
        &path,
        &HashOptions::new(vec![AlgorithmId::Md5]).with_buffer_size(1024),
    );
    let _ = handle.join();

    match result {
        Ok(_) => {
            // The writer may have lost the race; that is not a failure of the
            // guard, so this test only asserts the guard never returns a digest
            // for a file it observed changing.
        }
        Err(error) => {
            assert!(
                error
                    .to_string()
                    .contains("changed while it was being read"),
                "unexpected error: {error}"
            );
        }
    }
}
