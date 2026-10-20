//! Rayon cross-file batch scheduler with progress, ETA and cancellation.
//!
//! hash_entries hashes a batch of FileEntry values in parallel, filling each
//! entry's hashes map in place. Results keep the input order. Progress is
//! aggregated with atomics and reported at most about 20 times per second as
//! BatchEvent::Progress; each finished entry emits BatchEvent::EntryDone and the
//! batch always ends with BatchEvent::Finished.
//!
//! Cancellation is cooperative: the CancelToken is checked before an entry starts
//! and at every read-chunk boundary inside the hash engine. Entries that were not
//! hashed are marked Skipped with ErrorKind::Cancelled and BatchOutcome::cancelled
//! is true.
//!
//! The scheduler never panics on I/O errors; failures are recorded on the entry
//! with a machine-readable ErrorKind.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use batchhash_types::{EntryStatus, ErrorKind, FileEntry};
use rayon::prelude::*;

use crate::hasher::{self, HashError, HashOptions, HashSink};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// Cooperative cancellation flag shared by the scheduler and its workers.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Create a token that is not cancelled yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Workers observe it at the next chunk boundary.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub(crate) fn as_atomic(&self) -> &AtomicBool {
        &self.0
    }
}

/// How each entry's bytes should be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileIo {
    /// Read the file at FileEntry::display_path (desktop and plain filesystem roots).
    #[default]
    Path,
    /// Read from an already-open Unix file descriptor (Android SAF bridge).
    ///
    /// The descriptor is only valid on Unix targets; on other platforms entries
    /// using this variant fail with ErrorKind::Unsupported.
    Fd {
        /// Unix file descriptor owned by the caller.
        fd: i32,
        /// Byte offset to start reading from.
        offset: u64,
        /// Optional maximum number of bytes to read.
        length: Option<u64>,
    },
}

/// Options for a batch hashing call.
#[derive(Debug, Clone, Default)]
pub struct HashEntryOptions {
    /// Per-file hash options.
    pub hasher: HashOptions,
    /// Input mode shared by all entries in the batch.
    pub io: FileIo,
}

/// One progress sample emitted during a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressSnapshot {
    /// Entries finished so far (hashed, failed or skipped).
    pub completed: u64,
    /// Total number of entries in the batch.
    pub total: u64,
    /// Bytes read so far, counted at chunk granularity.
    pub bytes_done: u64,
    /// Sum of the input entries' declared sizes.
    pub bytes_total: u64,
    /// Completion percentage in the inclusive range 0.0..=100.0.
    pub percent: f64,
    /// Estimated seconds remaining from the smoothed byte rate; None while unknown.
    pub eta_seconds: Option<f64>,
    /// Display path of the file most recently started or finished.
    pub current_path: String,
    /// Milliseconds elapsed since the batch started.
    pub elapsed_ms: u64,
}

/// Event stream produced by hash_entries.
#[derive(Debug, Clone)]
pub enum BatchEvent {
    /// Periodic aggregate progress sample (throttled to about 20 per second).
    Progress(ProgressSnapshot),
    /// One entry reached a terminal state; carries a snapshot of that entry.
    EntryDone(FileEntry),
    /// The batch is complete; carries the final outcome.
    Finished(BatchOutcome),
}

/// Result of one batch hashing run.
#[derive(Debug, Clone)]
pub struct BatchOutcome {
    /// One entry per input entry, in input order, with hashes or an error status.
    pub entries: Vec<FileEntry>,
    /// Whether cancellation was requested before the batch finished.
    pub cancelled: bool,
    /// Wall-clock duration of the batch in milliseconds.
    pub elapsed_ms: u128,
    /// Total bytes actually read from inputs.
    pub bytes_hashed: u64,
}

/// Hash a batch of entries with Rayon cross-file parallelism.
///
/// The event callback is called from worker threads and from the calling thread;
/// it must be Sync and should only push into a channel or lock a short-lived
/// buffer. It must not call hash_entries re-entrantly.
pub fn hash_entries(
    entries: &[FileEntry],
    options: &HashEntryOptions,
    cancel: &CancelToken,
    events: &(dyn Fn(BatchEvent) + Sync),
) -> BatchOutcome {
    let started = Instant::now();
    let total = entries.len() as u64;
    let bytes_total: u64 = entries.iter().map(|entry| entry.size).sum();
    let reporter = ProgressReporter::new(total, bytes_total, events);
    let mut results = entries.to_vec();
    let first_path = results
        .first()
        .map(|entry| entry.display_path.as_str())
        .unwrap_or("");
    reporter.emit(first_path, true);

    let threads = worker_count();
    match rayon::ThreadPoolBuilder::new().num_threads(threads).build() {
        Ok(pool) => pool.install(|| {
            results
                .par_iter_mut()
                .for_each(|entry| process_entry(entry, options, cancel, &reporter));
        }),
        Err(_) => {
            for entry in results.iter_mut() {
                process_entry(entry, options, cancel, &reporter);
            }
        }
    }

    let cancelled = cancel.is_cancelled();
    let elapsed_ms = started.elapsed().as_millis();
    let bytes_hashed = reporter.bytes_done.load(Ordering::Relaxed);
    let last_path = results
        .last()
        .map(|entry| entry.display_path.as_str())
        .unwrap_or("");
    reporter.emit(last_path, true);
    let outcome = BatchOutcome {
        entries: results,
        cancelled,
        elapsed_ms,
        bytes_hashed,
    };
    reporter.finished(&outcome);
    outcome
}

fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .max(1)
}

fn process_entry(
    entry: &mut FileEntry,
    options: &HashEntryOptions,
    cancel: &CancelToken,
    reporter: &ProgressReporter<'_>,
) {
    if cancel.is_cancelled() {
        entry.hashes.clear();
        entry.skip(ErrorKind::Cancelled, "batch cancelled");
        reporter.entry_done(entry);
        return;
    }

    let mut progress_sink = ProgressSink {
        reporter,
        current_path: entry.display_path.clone(),
    };
    let sink: &mut dyn HashSink = &mut progress_sink;
    let result = match options.io {
        FileIo::Path => hasher::compute_hashes_path(
            Path::new(&entry.display_path),
            &options.hasher,
            Some(sink),
            Some(cancel.as_atomic()),
        ),
        #[cfg(unix)]
        FileIo::Fd { fd, offset, length } => crate::fd::compute_hashes_from_fd_streaming(
            fd,
            offset,
            length,
            &options.hasher,
            Some(sink),
            Some(cancel.as_atomic()),
        ),
        #[cfg(not(unix))]
        FileIo::Fd { .. } => Err(HashError::Unsupported(
            "file descriptor input is only supported on unix targets".to_string(),
        )),
    };

    match result {
        Ok(hashes) => {
            entry.hashes = hashes;
            entry.status = EntryStatus::Ok;
            entry.error_kind = None;
            entry.error_message = None;
        }
        Err(HashError::Cancelled) => {
            entry.hashes.clear();
            entry.skip(ErrorKind::Cancelled, "cancelled while hashing");
        }
        Err(error) => {
            let (kind, message) = classify_hash_error(&error);
            entry.fail(kind, message);
        }
    }
    reporter.entry_done(entry);
}

fn classify_hash_error(error: &HashError) -> (ErrorKind, String) {
    match error {
        HashError::Cancelled => (ErrorKind::Cancelled, error.to_string()),
        HashError::PermissionDenied(message) => (ErrorKind::PermissionDenied, message.clone()),
        HashError::Unsupported(message) => (ErrorKind::Unsupported, message.clone()),
        HashError::NoAlgorithms => (ErrorKind::Other, error.to_string()),
        HashError::Io(source) => match source.kind() {
            io::ErrorKind::PermissionDenied => (ErrorKind::PermissionDenied, source.to_string()),
            io::ErrorKind::NotFound => (ErrorKind::NotFound, source.to_string()),
            _ => (ErrorKind::IoError, source.to_string()),
        },
    }
}

struct ProgressState {
    last_emit: Option<Instant>,
    last_sample: Instant,
    last_bytes: u64,
    smoothed_rate: f64,
}

struct ProgressReporter<'a> {
    total: u64,
    bytes_total: u64,
    bytes_done: AtomicU64,
    completed: AtomicU64,
    started: Instant,
    state: Mutex<ProgressState>,
    events: &'a (dyn Fn(BatchEvent) + Sync),
}

impl<'a> ProgressReporter<'a> {
    fn new(total: u64, bytes_total: u64, events: &'a (dyn Fn(BatchEvent) + Sync)) -> Self {
        let now = Instant::now();
        Self {
            total,
            bytes_total,
            bytes_done: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            started: now,
            state: Mutex::new(ProgressState {
                last_emit: None,
                last_sample: now,
                last_bytes: 0,
                smoothed_rate: 0.0,
            }),
            events,
        }
    }

    fn emit(&self, current_path: &str, force: bool) {
        let now = Instant::now();
        let snapshot = {
            let mut state = match self.state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let throttled = !force
                && state
                    .last_emit
                    .is_some_and(|last_emit| {
                        now.saturating_duration_since(last_emit) < PROGRESS_INTERVAL
                    });
            if throttled {
                return;
            }
            state.last_emit = Some(now);
            let bytes_done = self.bytes_done.load(Ordering::Relaxed);
            let elapsed = now.saturating_duration_since(state.last_sample);
            let elapsed_secs = elapsed.as_secs_f64();
            if elapsed_secs > 0.0 {
                let delta = bytes_done.saturating_sub(state.last_bytes) as f64;
                let instant_rate = delta / elapsed_secs;
                state.smoothed_rate = if state.smoothed_rate > 0.0 {
                    state.smoothed_rate * 0.7 + instant_rate * 0.3
                } else {
                    instant_rate
                };
            }
            state.last_sample = now;
            state.last_bytes = bytes_done;
            let completed = self.completed.load(Ordering::Relaxed);
            let percent = if self.bytes_total > 0 {
                (bytes_done as f64 / self.bytes_total as f64 * 100.0).clamp(0.0, 100.0)
            } else if self.total > 0 {
                (completed as f64 / self.total as f64 * 100.0).clamp(0.0, 100.0)
            } else {
                100.0
            };
            let remaining = self.bytes_total.saturating_sub(bytes_done) as f64;
            let eta_seconds = if state.smoothed_rate > 0.0 {
                Some(remaining / state.smoothed_rate)
            } else {
                None
            };
            ProgressSnapshot {
                completed,
                total: self.total,
                bytes_done,
                bytes_total: self.bytes_total,
                percent,
                eta_seconds,
                current_path: current_path.to_string(),
                elapsed_ms: self.started.elapsed().as_millis() as u64,
            }
        };
        (self.events)(BatchEvent::Progress(snapshot));
    }

    fn entry_done(&self, entry: &FileEntry) {
        self.completed.fetch_add(1, Ordering::Relaxed);
        (self.events)(BatchEvent::EntryDone(entry.clone()));
        self.emit(&entry.display_path, false);
    }

    fn finished(&self, outcome: &BatchOutcome) {
        (self.events)(BatchEvent::Finished(outcome.clone()));
    }
}

struct ProgressSink<'a, 'b> {
    reporter: &'b ProgressReporter<'a>,
    current_path: String,
}

impl HashSink for ProgressSink<'_, '_> {
    fn on_chunk(&mut self, chunk: &[u8]) {
        self.reporter
            .bytes_done
            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
        self.reporter.emit(&self.current_path, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn cancel_token_is_shared_and_sticky() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!token.is_cancelled());
        clone.cancel();
        assert!(token.is_cancelled());
        assert!(clone.is_cancelled());
    }

    #[test]
    fn worker_count_is_at_least_one() {
        assert!(worker_count() >= 1);
    }

    #[test]
    fn io_error_classification_is_machine_readable() {
        let not_found = HashError::Io(io::Error::new(io::ErrorKind::NotFound, "missing"));
        assert_eq!(classify_hash_error(&not_found).0, ErrorKind::NotFound);
        let denied = HashError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "denied",
        ));
        assert_eq!(classify_hash_error(&denied).0, ErrorKind::PermissionDenied);
        let other = HashError::Io(io::Error::new(io::ErrorKind::Other, "other"));
        assert_eq!(classify_hash_error(&other).0, ErrorKind::IoError);
    }
}
