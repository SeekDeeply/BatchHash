//! Asynchronous job registry over [crate::api].
//!
//! The C ABI is synchronous: one call returns one JSON envelope. A batch over a
//! multi-gigabyte file, or a diff against a large directory, blocks for as long
//! as it runs, which froze the Flutter UI (the whole app shares one isolate
//! with the FFI call). This module runs those jobs on their own thread and
//! exposes a start/poll/cancel/release protocol so the UI can show real
//! progress and stay responsive.
//!
//! Contract:
//!
//! * start validates and queues, returning a job id immediately,
//! * poll returns the current state, a progress sample and, once finished, the
//!   result payload as a JSON string,
//! * cancel asks the worker to stop; a cancelled job still reports its
//!   terminal state through poll,
//! * release drops the job; it must not be polled afterwards.
//!
//! Every entry point catches unwinding, like [crate::c_api].

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::api::{
    self, ApiError, BatchRequestDto, DiffDirectoryRequest, HashEntriesRequest, ProgressDto,
};

/// Lifecycle state of one native job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Accepted and running.
    Running,
    /// Finished successfully; the payload is available.
    Completed,
    /// Finished with an error; the error is available.
    Failed,
    /// Cancellation was requested and the worker stopped.
    Cancelled,
}

impl JobState {
    /// Stable identifier used in the wire format.
    pub const fn as_str(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    /// Whether the job reached a terminal state.
    pub const fn is_terminal(self) -> bool {
        matches!(self, JobState::Completed | JobState::Failed | JobState::Cancelled)
    }
}

/// Result of one poll.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusDto {
    /// Job identifier.
    pub job_id: String,
    /// Lifecycle state.
    pub state: String,
    /// Latest progress sample, or null before the first one.
    pub progress: Option<ProgressDto>,
    /// Result payload when the state is completed.
    pub result_json: Option<String>,
    /// Error kind when the state is failed.
    pub error_kind: Option<String>,
    /// Error message when the state is failed.
    pub error: Option<String>,
}

struct Job {
    state: JobState,
    progress: Option<ProgressDto>,
    result_json: Option<String>,
    error: Option<ApiError>,
    cancel: batchhash_hash::CancelToken,
}

impl Job {
    fn status(&self, job_id: &str) -> JobStatusDto {
        // A job stays Running after a cancel request until its worker actually
        // stops; reporting Cancelled here would let the UI drop a result the
        // worker is still writing.
        JobStatusDto {
            job_id: job_id.to_string(),
            state: self.state.as_str().to_string(),
            progress: self.progress.clone(),
            result_json: self.result_json.clone(),
            error_kind: self.error.as_ref().map(|error| error.kind.clone()),
            error: self.error.as_ref().map(|error| error.message.clone()),
        }
    }
}

fn registry() -> &'static Mutex<BTreeMap<String, Job>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<String, Job>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn next_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    format!("job-{}", COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// Work executed on a job thread; returns the serialized result payload.
///
/// The progress sink is a plain closure reference so a job body can forward
/// samples without knowing how they are stored.
type ProgressSink<'a> = &'a (dyn Fn(ProgressDto) + Sync);

type JobBody = Box<
    dyn FnOnce(&batchhash_hash::CancelToken, ProgressSink<'_>) -> Result<String, ApiError> + Send,
>;

fn start_job(body: JobBody) -> String {
    let job_id = next_id();
    let cancel = batchhash_hash::CancelToken::new();
    let worker_cancel = cancel.clone();
    {
        let mut jobs = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        jobs.insert(
            job_id.clone(),
            Job {
                state: JobState::Running,
                progress: None,
                result_json: None,
                error: None,
                cancel,
            },
        );
    }
    let thread_id = job_id.clone();
    let spawn = std::thread::Builder::new()
        .name(format!("batchhash-{job_id}"))
        .spawn(move || {
            // The reporter needs its own handle: a closure that captured
            // `thread_id` by reference would borrow it for its whole lifetime
            // and block the terminal update below.
            let progress_id = thread_id.clone();
            let reporter = move |sample: ProgressDto| {
                let mut jobs =
                    registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(job) = jobs.get_mut(progress_id.as_str()) {
                    job.progress = Some(sample);
                }
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                body(&worker_cancel, &reporter)
            }));
            let mut jobs = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(job) = jobs.get_mut(&thread_id) {
                if job.cancel.is_cancelled() {
                    // Cancellation wins over a successful return: a run that
                    // stopped early must never be reported as a complete result.
                    job.state = JobState::Cancelled;
                    job.result_json = None;
                    job.error = None;
                } else {
                    match outcome {
                        Ok(Ok(payload)) => {
                            job.state = JobState::Completed;
                            job.result_json = Some(payload);
                        }
                        Ok(Err(error)) => {
                            job.state = JobState::Failed;
                            job.error = Some(error);
                        }
                        Err(_) => {
                            job.state = JobState::Failed;
                            job.error =
                                Some(ApiError::new("internal", "the native job panicked"));
                        }
                    }
                }
            }
        });
    if let Err(error) = spawn {
        let mut jobs = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(job) = jobs.get_mut(&job_id) {
            job.state = JobState::Failed;
            job.error = Some(ApiError::new(
                "internal",
                format!("cannot start the worker thread: {error}"),
            ));
        }
    }
    job_id
}

fn serialize<T: Serialize>(value: &T) -> Result<String, ApiError> {
    serde_json::to_string(value).map_err(|error| ApiError::new("internal", error.to_string()))
}

/// Start a pipeline batch (scan, hash, optional suffix, report) as a job.
pub fn start_batch(request: BatchRequestDto) -> Result<String, ApiError> {
    // Validate eagerly so a bad request fails at start instead of asynchronously.
    api::validate_batch_request(&request)?;
    Ok(start_job(Box::new(move |cancel, progress| {
        let result = api::run_batch_request_with_progress(&request, cancel, progress)?;
        serialize(&result)
    })))
}

/// Start hashing explicit root-relative paths as a job.
pub fn start_hash_entries(request: HashEntriesRequest) -> Result<String, ApiError> {
    if request.algorithms.is_empty() {
        return Err(ApiError::invalid("at least one algorithm is required"));
    }
    Ok(start_job(Box::new(move |cancel, progress| {
        let result = api::hash_entries_request_with_progress(&request, cancel, progress)?;
        serialize(&result)
    })))
}

/// Start a report-versus-directory comparison as a job.
pub fn start_diff_directory(request: DiffDirectoryRequest) -> Result<String, ApiError> {
    Ok(start_job(Box::new(move |cancel, progress| {
        let result = api::diff_directory_request_with_progress(&request, cancel, progress)?;
        serialize(&result)
    })))
}

/// Poll one job. Returns an `invalid_request` error for an unknown id.
pub fn poll(job_id: &str) -> Result<JobStatusDto, ApiError> {
    let jobs = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = jobs
        .get(job_id)
        .ok_or_else(|| ApiError::invalid(format!("unknown job id: {job_id}")))?;
    Ok(job.status(job_id))
}

/// Ask a running job to stop. Unknown ids are rejected; finished jobs are a no-op.
pub fn cancel(job_id: &str) -> Result<JobStatusDto, ApiError> {
    let jobs = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = jobs
        .get(job_id)
        .ok_or_else(|| ApiError::invalid(format!("unknown job id: {job_id}")))?;
    if !job.state.is_terminal() {
        job.cancel.cancel();
    }
    Ok(job.status(job_id))
}

/// Drop a job. A still-running job is cancelled first so its thread stops.
pub fn release(job_id: &str) -> Result<bool, ApiError> {
    let removed = {
        let mut jobs = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        jobs.remove(job_id)
    };
    match removed {
        Some(job) => {
            if !job.state.is_terminal() {
                job.cancel.cancel();
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Number of jobs currently tracked; used by tests to prove cleanup.
pub fn live_job_count() -> usize {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .len()
}