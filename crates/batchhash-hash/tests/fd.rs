//! Unix fd entry point: offset, length cap, zero length and descriptor ownership.
#![cfg(unix)]

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::os::unix::io::AsRawFd;

use batchhash_hash::{compute_hashes, compute_hashes_from_fd, HashError, HashOptions};
use batchhash_types::AlgorithmId;

fn content(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index * 7 % 256) as u8).collect()
}

fn options() -> HashOptions {
    HashOptions::new(vec![
        AlgorithmId::Md5,
        AlgorithmId::Sha256,
        AlgorithmId::Blake3,
        AlgorithmId::Crc32,
    ])
}

fn reference(data: &[u8], options: &HashOptions) -> indexmap::IndexMap<AlgorithmId, String> {
    let mut reader = Cursor::new(data.to_vec());
    compute_hashes(&mut reader, options).expect("reference hashing must succeed")
}

#[test]
fn fd_offset_and_length_are_respected() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let path = dir.path().join("data.bin");
    let data = content(1000);
    std::fs::write(&path, &data).expect("file must be written");
    let mut file = File::open(&path).expect("file must open");
    let fd = file.as_raw_fd();
    let options = options();

    assert_eq!(
        compute_hashes_from_fd(fd, 100, None, &options).expect("offset read must succeed"),
        reference(&data[100..], &options)
    );
    assert_eq!(
        compute_hashes_from_fd(fd, 250, Some(300), &options).expect("limited read must succeed"),
        reference(&data[250..550], &options)
    );
    assert_eq!(
        compute_hashes_from_fd(fd, 250, Some(0), &options).expect("zero length must succeed"),
        reference(&[], &options)
    );
    assert_eq!(
        compute_hashes_from_fd(fd, 5000, None, &options).expect("past EOF must succeed"),
        reference(&[], &options)
    );

    file.seek(SeekFrom::Start(0)).expect("rewind must succeed");
    let mut reread = Vec::new();
    file.read_to_end(&mut reread).expect("reread must succeed");
    assert_eq!(reread, data, "caller descriptor must stay open and readable");
}

#[test]
fn original_descriptor_is_never_closed() {
    let dir = tempfile::tempdir().expect("tempdir must be created");
    let path = dir.path().join("data.bin");
    let data = content(64);
    std::fs::write(&path, &data).expect("file must be written");
    let mut file = File::open(&path).expect("file must open");
    let fd = file.as_raw_fd();
    let options = options();

    for offset in 0..3u64 {
        compute_hashes_from_fd(fd, offset, Some(16), &options).expect("repeated reads must succeed");
    }

    file.seek(SeekFrom::Start(0)).expect("rewind must succeed");
    let mut reread = Vec::new();
    file.read_to_end(&mut reread).expect("reread must succeed");
    assert_eq!(reread, data);
}

#[test]
fn invalid_descriptor_is_an_io_error() {
    let result = compute_hashes_from_fd(-1, 0, None, &options());
    assert!(
        matches!(result, Err(HashError::Io(_))),
        "invalid fd must be reported as an io error"
    );
}
