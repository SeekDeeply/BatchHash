//! Unix file-descriptor input.
//!
//! Some hosts (notably Android through the Storage Access Framework) already own
//! an open file descriptor instead of a path. This module hashes such a descriptor
//! without taking ownership of it.
//!
//! The public entry point dup(2)s the caller descriptor, wraps the duplicate in a
//! File, seeks to the requested offset and reads at most the requested length.
//! The duplicate is closed when the File is dropped; the caller descriptor is
//! never closed. dup(2) shares the underlying open file description, so seeking
//! on the duplicate also moves the shared offset; callers that need the original
//! offset preserved must rewind the descriptor themselves (the tests do this).
//!
//! Safety contract: the caller guarantees that fd is a valid, open, seekable file
//! descriptor for the duration of the call. The call itself only fails with
//! HashError::Io when the descriptor is invalid or not seekable.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::io::{FromRawFd, RawFd};
use std::sync::atomic::AtomicBool;

use batchhash_types::AlgorithmId;
use indexmap::IndexMap;

use crate::hasher::{compute_hashes_streaming, HashError, HashOptions, HashSink};

/// Compute hashes for a region of an already-open Unix file descriptor.
///
/// Reads from offset for at most length bytes (length None reads to end of file).
/// The caller keeps ownership of fd: this function only works on a dup(2) of it.
///
/// # Safety contract
///
/// The caller guarantees that fd is a valid, open and seekable descriptor for the
/// whole call. The descriptor is not closed, but its shared file offset may move.
pub fn compute_hashes_from_fd(
    fd: RawFd,
    offset: u64,
    length: Option<u64>,
    options: &HashOptions,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    compute_hashes_from_fd_streaming(fd, offset, length, options, None, None)
}

pub(crate) fn compute_hashes_from_fd_streaming(
    fd: RawFd,
    offset: u64,
    length: Option<u64>,
    options: &HashOptions,
    sink: Option<&mut dyn HashSink>,
    cancel: Option<&AtomicBool>,
) -> Result<IndexMap<AlgorithmId, String>, HashError> {
    // SAFETY: the caller contract guarantees fd is valid for this call. dup(2)
    // never takes ownership of the original descriptor and returns a new one
    // (or -1, handled below).
    let duplicated = unsafe { libc::dup(fd) };
    if duplicated < 0 {
        return Err(HashError::Io(std::io::Error::last_os_error()));
    }
    // SAFETY: duplicated is a fresh descriptor owned by this function (dup returns
    // a new file descriptor), so File may take ownership and close it on drop.
    // The original fd is left untouched and open.
    let mut file = unsafe { File::from_raw_fd(duplicated) };
    file.seek(SeekFrom::Start(offset))?;
    match length {
        Some(limit) => {
            let mut limited = file.take(limit);
            compute_hashes_streaming(&mut limited, options, sink, cancel)
        }
        None => compute_hashes_streaming(&mut file, options, sink, cancel),
    }
}
