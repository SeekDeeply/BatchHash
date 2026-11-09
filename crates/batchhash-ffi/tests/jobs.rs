//! Tests for the asynchronous job registry.

use batchhash_ffi::api::{BatchRequestDto, DiffDirectoryRequest, HashEntriesRequest};
use batchhash_ffi::jobs::{self, JobState};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "batchhash-jobs-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn wait_for(job_id: &str) -> batchhash_ffi::jobs::JobStatusDto {
    for _ in 0..600 {
        let status = jobs::poll(job_id).expect("poll a live job");
        if status.state != JobState::Running.as_str() {
            return status;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("job {job_id} never reached a terminal state");
}

fn hash_request(root: &std::path::Path, files: &[&str]) -> HashEntriesRequest {
    HashEntriesRequest {
        root: root.to_string_lossy().into_owned(),
        paths: files.iter().map(|name| (*name).to_string()).collect(),
        algorithms: vec!["sha256".to_string(), "md5".to_string()],
        follow_links: false,
    }
}

#[test]
fn hashing_job_completes_and_reports_a_payload() {
    let dir = temp_dir("hash");
    std::fs::write(dir.join("a.txt"), b"alpha").expect("write fixture");
    std::fs::write(dir.join("b.txt"), b"bravo").expect("write fixture");

    let job_id =
        jobs::start_hash_entries(hash_request(&dir, &["a.txt", "b.txt"])).expect("start the job");
    let status = wait_for(&job_id);
    assert_eq!(status.state, "completed", "{status:?}");
    let payload = status.result_json.expect("a completed job has a payload");
    assert!(payload.contains("report_json"), "payload was {payload}");
    assert_eq!(status.error, None);

    assert!(jobs::release(&job_id).expect("release the job"));
    assert!(jobs::poll(&job_id).is_err(), "a released job is gone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hashing_job_forwards_progress_samples() {
    let dir = temp_dir("progress");
    // Enough bytes that at least one intermediate sample is produced from the
    // chunk-level reporter rather than only the terminal one.
    let payload = vec![7u8; 3 * 1024 * 1024];
    std::fs::write(dir.join("big.bin"), &payload).expect("write fixture");

    let job_id = jobs::start_hash_entries(hash_request(&dir, &["big.bin"])).expect("start the job");
    let mut seen_bytes = 0u64;
    let mut saw_sample = false;
    for _ in 0..600 {
        let status = jobs::poll(&job_id).expect("poll");
        if let Some(progress) = status.progress.as_ref() {
            saw_sample = true;
            assert_eq!(progress.phase, "hashing");
            seen_bytes = seen_bytes.max(progress.bytes_done);
            assert!(progress.percent >= 0.0 && progress.percent <= 100.0);
        }
        if status.state != "running" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(saw_sample, "the job must report progress while it runs");
    assert!(seen_bytes > 0, "byte progress must advance");
    let _ = jobs::release(&job_id);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelling_a_job_wins_over_a_successful_return() {
    let dir = temp_dir("cancel");
    // 64 MiB keeps the job running long enough that the cancel below is
    // observed by the workers instead of racing a job that already finished;
    // the point of this test is the precedence rule in the worker, which a
    // tolerant assertion would stop protecting.
    let payload = vec![5u8; 64 * 1024 * 1024];
    std::fs::write(dir.join("big.bin"), &payload).expect("write fixture");

    let job_id = jobs::start_hash_entries(hash_request(&dir, &["big.bin"])).expect("start the job");
    jobs::cancel(&job_id).expect("cancel the job");
    let status = wait_for(&job_id);
    assert_eq!(
        status.state, "cancelled",
        "cancellation wins over a successful return: {status:?}"
    );
    assert_eq!(
        status.result_json, None,
        "a cancelled job must never expose a payload"
    );
    let _ = jobs::release(&job_id);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_jobs_are_rejected() {
    assert!(jobs::poll("job-does-not-exist").is_err());
    assert!(jobs::cancel("job-does-not-exist").is_err());
    assert!(!jobs::release("job-does-not-exist").expect("release is infallible"));
}

#[test]
fn an_invalid_batch_request_fails_at_start() {
    let dir = temp_dir("invalid");
    let request = BatchRequestDto {
        root: dir.to_string_lossy().into_owned(),
        root_label: None,
        algorithms: vec!["not-an-algorithm".to_string()],
        follow_links: false,
        recursive: true,
        restrict_paths: Vec::new(),
        case_sensitive: None,
        suffix: None,
        apply_renames: false,
        log_db: None,
        overwrite: false,
    };
    let error = jobs::start_batch(request).expect_err("an unknown algorithm must fail fast");
    assert_eq!(error.kind, "invalid_request");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn directory_diff_job_completes_after_a_report() {
    let dir = temp_dir("diff");
    std::fs::write(dir.join("a.txt"), b"alpha").expect("write fixture");

    let job_id = jobs::start_hash_entries(hash_request(&dir, &["a.txt"])).expect("start");
    let status = wait_for(&job_id);
    let payload = status.result_json.expect("payload");
    let value: serde_json::Value = serde_json::from_str(&payload).expect("payload is JSON");
    let report_json = value["report_json"]
        .as_str()
        .expect("the batch result carries report JSON")
        .to_string();
    let _ = jobs::release(&job_id);

    let diff_id = jobs::start_diff_directory(DiffDirectoryRequest {
        report_json,
        directory: dir.to_string_lossy().into_owned(),
        case_sensitive: None,
        follow_links: false,
    })
    .expect("start the diff job");
    let diff_status = wait_for(&diff_id);
    assert_eq!(diff_status.state, "completed", "{diff_status:?}");
    let diff_payload = diff_status.result_json.expect("diff payload");
    assert!(diff_payload.contains("rows"), "payload was {diff_payload}");
    let _ = jobs::release(&diff_id);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn releasing_a_terminal_job_removes_only_that_entry() {
    // The registry is process wide and these tests run in parallel, so this
    // asserts on the specific job rather than on a global count: another test
    // starting a job between two reads must not make this flaky.
    let dir = temp_dir("cleanup");
    std::fs::write(dir.join("a.txt"), b"alpha").expect("write fixture");

    let job_id = jobs::start_hash_entries(hash_request(&dir, &["a.txt"])).expect("start");
    let _ = wait_for(&job_id);
    assert!(
        jobs::poll(&job_id).is_ok(),
        "the finished job is still tracked"
    );
    assert!(jobs::release(&job_id).expect("release"));
    assert!(
        jobs::poll(&job_id).is_err(),
        "a released job must be gone, so its result cannot leak"
    );
    assert!(
        !jobs::release(&job_id).expect("releasing twice is not an error"),
        "the second release reports that nothing was tracked"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
