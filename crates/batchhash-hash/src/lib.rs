//! BatchHash hash engine.
//!
//! This crate owns the CPU-side hashing pipeline:
//!
//! * [`hasher`]: single-pass multi-hash computation over any byte stream,
//! * [`fd`]: Unix file-descriptor input (dup + seek + optional length cap),
//! * [`scheduler`]: Rayon cross-file batch execution with progress and cancel,
//! * [`probe`]: honest hardware capability reporting.
//!
//! The public surface follows docs/internal-api-contracts.md. Every hash is
//! returned as lowercase hex in canonical AlgorithmId registry order.
#![deny(missing_docs)]

#[cfg(unix)]
pub mod fd;
pub mod hasher;
pub mod probe;
pub mod scheduler;

#[cfg(unix)]
pub use fd::compute_hashes_from_fd;
pub use hasher::{
    compute_hashes, compute_hashes_file, compute_hashes_with_sink, hash_reader, Blake3Policy,
    HashError, HashOptions, HashSink,
};
pub use probe::{probe, probe_auto, HardwareInfo};
pub use scheduler::{
    hash_entries, BatchEvent, BatchOutcome, CancelToken, FileIo, HashEntryOptions,
    ProgressSnapshot,
};

pub use batchhash_types::BackendPolicy;
