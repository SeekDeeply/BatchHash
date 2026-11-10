//! Bridge tests: the serialisable API and the C ABI envelope.
//!
//! These run in CI only (local builds are forbidden in this repository).

use std::ffi::{c_char, CStr, CString};
use std::fs;

use batchhash_ffi::api::{
    algorithms, probe, run_batch_request, scan_request, undo, BatchRequestDto, ProbeRequest,
    ScanRequest, SuffixDto, UndoRequestDto,
};
use batchhash_ffi::c_api;

fn write(path: &std::path::Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write fixture");
}

fn batch_request(root: &std::path::Path, algorithms: Vec<&str>) -> BatchRequestDto {
    BatchRequestDto {
        root: root.to_string_lossy().into_owned(),
        root_label: None,
        algorithms: algorithms.into_iter().map(str::to_string).collect(),
        follow_links: false,
        // The same defaults a JSON caller gets, so these tests exercise the
        // same path the app does: a whole-tree scan with no restriction.
        recursive: true,
        restrict_paths: Vec::new(),
        case_sensitive: None,
        suffix: None,
        apply_renames: false,
        log_db: None,
        overwrite: false,
    }
}

#[test]
fn algorithms_cover_the_registry() {
    let list = algorithms();
    assert_eq!(list.len(), batchhash_types::REGISTRY.len());
    let sha256 = list
        .iter()
        .find(|item| item.id == "sha256")
        .expect("sha256");
    assert_eq!(sha256.hex_len, 64);
    assert_eq!(sha256.digest_len, 32);
    assert!(sha256.suffix_usable);
    assert_eq!(sha256.category, "crypto");
    assert!(!sha256.default_suffix_algorithm);
    let crc32 = list.iter().find(|item| item.id == "crc32").expect("crc32");
    assert!(!crc32.cryptographic);
    assert_eq!(crc32.category, "checksum");
}

#[test]
fn algorithms_expose_the_whole_catalogue_with_categories() {
    let list = algorithms();
    // The UI groups by this field, so it must always be one of the four known
    // values and never an empty string left over from a missing registry entry.
    for item in &list {
        assert!(
            matches!(
                item.category.as_str(),
                "crypto" | "checksum" | "fast" | "legacy"
            ),
            "{} has an unknown category {:?}",
            item.id,
            item.category
        );
        assert!(!item.id.is_empty());
        assert!(!item.display_name.is_empty());
        assert_eq!(item.hex_len, item.digest_len * 2, "{}", item.id);
    }
    // Categories are a real partition, not a single bucket.
    let mut categories: Vec<&str> = list.iter().map(|item| item.category.as_str()).collect();
    categories.sort_unstable();
    categories.dedup();
    assert!(categories.len() >= 3, "categories: {categories:?}");
    // Exactly one algorithm is marked as the suffix default, and it is BLAKE3.
    let defaults: Vec<&str> = list
        .iter()
        .filter(|item| item.default_suffix_algorithm)
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(defaults, vec!["blake3"]);
    // Broken digests carry a warning and sit under legacy, never under crypto.
    for id in ["md5", "sha1", "md2", "md4"] {
        let item = list.iter().find(|item| item.id == id).expect(id);
        assert_eq!(item.category, "legacy", "{id}");
        assert!(item.suffix_warning.is_some(), "{id} needs a warning");
    }
    // Every registry entry appears exactly once, in canonical order.
    let ids: Vec<&str> = list.iter().map(|item| item.id.as_str()).collect();
    let expected: Vec<&str> = batchhash_types::REGISTRY
        .iter()
        .map(|spec| spec.stable_id)
        .collect();
    assert_eq!(ids, expected);
}

#[test]
fn probe_is_honest_about_backends() {
    let hardware = probe(&ProbeRequest::default());
    assert!(!hardware.arch.is_empty());
    assert!(hardware.multithread >= 1);
    // Backends stay "unknown" unless the hash library exposes them.
    assert!(
        hardware.blake3_backend_reported == "unknown"
            || !hardware.blake3_backend_reported.is_empty()
    );
    let forced = probe(&ProbeRequest { forced_soft: true });
    assert_eq!(forced.backend_policy, "forced-soft");
}

#[test]
fn scan_returns_unhashed_entries() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");
    write(&temp.path().join("nested/b.txt"), b"bravo");

    let result = scan_request(&ScanRequest {
        root: temp.path().to_string_lossy().into_owned(),
        follow_links: false,
        max_depth: None,
    })
    .expect("scan");
    assert_eq!(result.entries.len(), 2);
    assert_eq!(result.total_bytes, 10);
    assert!(result.entries.iter().all(|entry| entry.hashes.is_empty()));
    let paths: Vec<&str> = result.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["a.txt", "nested/b.txt"]);
    assert!(!result.single_file);
}

#[test]
fn run_batch_hashes_and_returns_a_report() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");

    let run = run_batch_request(&batch_request(temp.path(), vec!["md5", "sha256"])).expect("run");
    assert_eq!(run.total, 1);
    assert_eq!(run.ok, 1);
    assert_eq!(run.entries.len(), 1);
    assert_eq!(run.entries[0].hashes.len(), 2);
    assert_eq!(run.entries[0].status, "ok");
    assert!(!run.report_json.is_empty());
    // The canonical JSON must round-trip into the report type.
    let report: batchhash_types::Report =
        serde_json::from_str(&run.report_json).expect("report json");
    assert_eq!(report.summary.ok, 1);
    assert!(run.plan.is_none());
}

#[test]
fn run_batch_rejects_empty_algorithm_list() {
    let temp = tempfile::tempdir().expect("tempdir");
    let error = run_batch_request(&batch_request(temp.path(), Vec::new())).expect_err("must fail");
    assert_eq!(error.kind, "invalid_request");
}

#[test]
fn suffix_dry_run_plans_but_does_not_rename() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write(&file, b"movie");

    let mut request = batch_request(temp.path(), vec!["md5"]);
    request.suffix = Some(SuffixDto {
        algorithm: "md5".to_string(),
        length: 8,
        case: None,
        prefix: false,
    });
    let run = run_batch_request(&request).expect("run");
    assert_eq!(run.renamed, 0);
    assert!(run.batch_id.is_none());
    let summary = run.plan_summary.expect("plan summary");
    assert_eq!(summary.ready, 1);
    assert!(file.exists(), "dry run must not rename");
}

#[test]
fn suffix_apply_then_undo_round_trips_through_the_log() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write(&file, b"movie");
    let db = temp.path().join("log/rename.db");

    let mut request = batch_request(temp.path(), vec!["md5"]);
    request.suffix = Some(SuffixDto {
        algorithm: "md5".to_string(),
        length: 8,
        case: Some("lower".to_string()),
        prefix: false,
    });
    request.apply_renames = true;
    request.log_db = Some(db.to_string_lossy().into_owned());
    let run = run_batch_request(&request).expect("run");
    assert_eq!(run.renamed, 1);
    let batch_id = run.batch_id.clone().expect("batch id");
    assert!(!file.exists());
    let renamed = run
        .entries
        .iter()
        .find(|entry| entry.path.ends_with(".mkv"))
        .expect("entry");
    assert_eq!(renamed.status, "ok");

    let planned = undo(&UndoRequestDto {
        db: db.to_string_lossy().into_owned(),
        batch_id: Some(batch_id.clone()),
        apply: false,
    })
    .expect("undo plan");
    assert_eq!(planned.rows.len(), 1);
    assert_eq!(planned.rows[0].status, "ready");
    assert!(!planned.applied);

    let applied = undo(&UndoRequestDto {
        db: db.to_string_lossy().into_owned(),
        batch_id: Some(batch_id),
        apply: true,
    })
    .expect("undo apply");
    assert!(applied.applied);
    assert_eq!(applied.undone, 1);
    assert!(file.exists(), "undo restores the original name");
}

fn take_string(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null(), "null return from the C ABI");
    // SAFETY: the pointer was returned by the crate and freed exactly once.
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { c_api::bh_free(pointer) };
    text
}

fn call_ok(pointer: *mut c_char) -> serde_json::Value {
    let text = take_string(pointer);
    let value: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(value["ok"], true, "envelope: {text}");
    assert_eq!(value["api_version"], c_api::C_ABI_VERSION);
    value["data"].clone()
}

fn call_err(pointer: *mut c_char) -> serde_json::Value {
    let text = take_string(pointer);
    let value: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(value["ok"], false, "envelope: {text}");
    value
}

#[test]
fn c_abi_jobs_run_poll_and_release() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");
    write(&temp.path().join("b.txt"), b"bravo");

    let start = serde_json::json!({
        "action": "hash_entries",
        "payload": {
            "root": temp.path().to_string_lossy(),
            "paths": ["a.txt", "b.txt"],
            "algorithms": ["sha256"],
        },
    });
    let start_text = CString::new(serde_json::to_string(&start).expect("json")).expect("cstr");
    let started = call_ok(unsafe { c_api::bh_job_start(start_text.as_ptr()) });
    let job_id = started["job_id"].as_str().expect("job id").to_string();
    assert!(!job_id.is_empty());

    // Poll until the worker finishes; the start call returns immediately, which
    // is the whole point of the job API.
    let mut final_state = String::new();
    let mut payload: Option<String> = None;
    for _ in 0..600 {
        let poll = serde_json::json!({ "job_id": job_id });
        let poll_text = CString::new(serde_json::to_string(&poll).expect("json")).expect("cstr");
        let status = call_ok(unsafe { c_api::bh_job_poll(poll_text.as_ptr()) });
        final_state = status["state"].as_str().unwrap_or_default().to_string();
        if final_state != "running" {
            payload = status["result_json"].as_str().map(str::to_string);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(final_state, "completed", "the job must finish");
    assert!(
        payload.expect("payload").contains("report_json"),
        "the completed job carries the batch result"
    );

    let release = serde_json::json!({ "job_id": job_id });
    let release_text = CString::new(serde_json::to_string(&release).expect("json")).expect("cstr");
    let released = call_ok(unsafe { c_api::bh_job_release(release_text.as_ptr()) });
    assert_eq!(released["released"], true);

    // A released job is unknown, and an unknown job is a caller error.
    let poll_again = CString::new(serde_json::to_string(&release).expect("json")).expect("cstr");
    let error = call_err(unsafe { c_api::bh_job_poll(poll_again.as_ptr()) });
    assert_eq!(error["kind"], "invalid_request");
}

#[test]
fn c_abi_jobs_reject_unknown_actions_and_ids() {
    let bad_action = serde_json::json!({ "action": "nope", "payload": {} });
    let text = CString::new(serde_json::to_string(&bad_action).expect("json")).expect("cstr");
    let error = call_err(unsafe { c_api::bh_job_start(text.as_ptr()) });
    assert_eq!(error["kind"], "invalid_request");

    let cancel = serde_json::json!({ "job_id": "job-missing" });
    let cancel_text = CString::new(serde_json::to_string(&cancel).expect("json")).expect("cstr");
    let cancel_error = call_err(unsafe { c_api::bh_job_cancel(cancel_text.as_ptr()) });
    assert_eq!(cancel_error["kind"], "invalid_request");
}

#[test]
fn c_abi_version_advertises_async_jobs() {
    let version = call_ok(unsafe { c_api::bh_api_version() });
    let features = version["features"].as_array().expect("features array");
    assert!(
        features.iter().any(|value| value == "async_jobs"),
        "the Dart side detects job support from this capability"
    );
}
#[test]
fn c_abi_version_and_algorithms() {
    let version = call_ok(unsafe { c_api::bh_api_version() });
    assert_eq!(version["contract_version"], 1);
    assert_eq!(version["tool"], "BatchHash");

    let list = call_ok(unsafe { c_api::bh_algorithms() });
    let entries = list.as_array().expect("array");
    assert_eq!(entries.len(), batchhash_types::REGISTRY.len());
    // The JSON contract the Dart decoder reads: every entry carries the
    // category string and the suffix metadata, not just the digest length.
    for entry in entries {
        assert!(entry["id"].is_string());
        assert!(entry["display_name"].is_string());
        assert!(entry["category"].is_string());
        assert!(entry["hex_len"].is_u64());
        assert!(entry["digest_len"].is_u64());
        assert!(entry["cryptographic"].is_boolean());
        assert!(entry["suffix_usable"].is_boolean());
        assert!(entry["default_suffix_algorithm"].is_boolean());
        // null or a string, never absent: `suffix_warning` is Optional.
        assert!(entry["suffix_warning"].is_string() || entry["suffix_warning"].is_null());
    }
    let blake3 = entries
        .iter()
        .find(|entry| entry["id"] == "blake3")
        .expect("blake3");
    assert_eq!(blake3["category"], "crypto");
    assert_eq!(blake3["default_suffix_algorithm"], true);
}

#[test]
fn c_abi_scan_and_batch() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");

    let scan_request = serde_json::json!({
        "root": temp.path().to_string_lossy(),
        "follow_links": false,
    });
    let scan_text =
        CString::new(serde_json::to_string(&scan_request).expect("json")).expect("cstr");
    let scanned = call_ok(unsafe { c_api::bh_scan(scan_text.as_ptr()) });
    assert_eq!(scanned["entries"].as_array().expect("entries").len(), 1);

    let batch = serde_json::json!({
        "root": temp.path().to_string_lossy(),
        "algorithms": ["md5"],
    });
    let batch_text = CString::new(serde_json::to_string(&batch).expect("json")).expect("cstr");
    let run = call_ok(unsafe { c_api::bh_run_batch(batch_text.as_ptr()) });
    assert_eq!(
        run["ok"]
            .as_u64()
            .unwrap_or(run["total"].as_u64().unwrap_or(1)),
        1
    );
    assert_eq!(run["entries"].as_array().expect("entries").len(), 1);
}

#[test]
fn c_abi_reports_errors_as_envelopes() {
    let bad = CString::new("{not json").expect("cstr");
    let envelope = call_err(unsafe { c_api::bh_run_batch(bad.as_ptr()) });
    assert_eq!(envelope["kind"], "invalid_request");

    let null_envelope = call_err(unsafe { c_api::bh_scan(std::ptr::null()) });
    assert_eq!(null_envelope["kind"], "invalid_request");
}

#[test]
fn c_abi_probe_accepts_an_empty_request() {
    let profile = call_ok(unsafe { c_api::bh_probe(std::ptr::null()) });
    assert!(profile["arch"].is_string());
}

#[test]
fn hash_entries_request_hashes_explicit_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");
    write(&temp.path().join("nested/b.txt"), b"bravo");

    let run = batchhash_ffi::api::hash_entries_request(&batchhash_ffi::api::HashEntriesRequest {
        root: temp.path().to_string_lossy().into_owned(),
        paths: vec!["a.txt".to_string(), "nested/b.txt".to_string()],
        algorithms: vec!["md5".to_string()],
        follow_links: false,
    })
    .expect("hash entries");
    assert_eq!(run.total, 2);
    assert_eq!(run.ok, 2);
    assert_eq!(run.entries.len(), 2);
    assert!(run.entries.iter().all(|entry| entry.hashes.len() == 1));
    // Path traversal is rejected, not silently dropped.
    let rejected =
        batchhash_ffi::api::hash_entries_request(&batchhash_ffi::api::HashEntriesRequest {
            root: temp.path().to_string_lossy().into_owned(),
            paths: vec!["../escape.txt".to_string()],
            algorithms: vec!["md5".to_string()],
            follow_links: false,
        })
        .expect("hash entries");
    assert_eq!(rejected.total, 0);
    assert_eq!(rejected.skipped.len(), 1);
}

#[test]
fn plan_renames_request_uses_the_supplied_hashes() {
    let entry = batchhash_ffi::api::EntryInputDto {
        path: "movie.mkv".to_string(),
        display_path: None,
        size: 5,
        mtime: None,
        hashes: vec![batchhash_ffi::api::HashDto {
            algorithm: "md5".to_string(),
            hex: "0123456789abcdef0123456789abcdef".to_string(),
        }],
    };
    let plan = batchhash_ffi::api::plan_renames_request(&batchhash_ffi::api::PlanRenamesRequest {
        root: None,
        entries: vec![entry],
        algorithm: "md5".to_string(),
        length: 8,
        case: Some("upper".to_string()),
        prefix: false,
        overwrite: false,
    })
    .expect("plan");
    assert_eq!(plan.summary.ready, 1);
    assert_eq!(plan.rows.len(), 1);
    assert!(plan.rows[0].proposed_name.starts_with("movie["));
}

#[test]
fn import_diff_and_history_round_trip() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");
    let log_db = temp.path().join("log/rename.db");

    let mut request = batch_request(temp.path(), vec!["md5"]);
    request.suffix = Some(SuffixDto {
        algorithm: "md5".to_string(),
        length: 8,
        case: None,
        prefix: false,
    });
    request.apply_renames = true;
    request.log_db = Some(log_db.to_string_lossy().into_owned());
    let applied = run_batch_request(&request).expect("run");

    // Import the exported JSON through the bridge.
    let report_path = temp.path().join("report.json");
    write(&report_path, applied.report_json.as_bytes());
    let imported =
        batchhash_ffi::api::import_report_request(&batchhash_ffi::api::ImportReportRequest {
            path: report_path.to_string_lossy().into_owned(),
        })
        .expect("import");
    assert!(imported.contains("\"schema_version\": 1"));

    // A report compared with itself is unchanged.
    let diff = batchhash_ffi::api::diff_reports_request(&batchhash_ffi::api::DiffReportsRequest {
        left_json: imported.clone(),
        right_json: imported.clone(),
        case_sensitive: None,
    })
    .expect("diff");
    assert_eq!(diff.summary.changed, 0);
    assert_eq!(diff.summary.removed, 0);

    // Comparing against the live directory sees the renamed file.
    let directory_diff =
        batchhash_ffi::api::diff_directory_request(&batchhash_ffi::api::DiffDirectoryRequest {
            report_json: imported,
            directory: temp.path().to_string_lossy().into_owned(),
            case_sensitive: None,
            follow_links: false,
            recursive: Some(true),
            restrict_paths: Vec::new(),
            max_entries: None,
        })
        .expect("directory diff");
    assert_eq!(
        directory_diff.summary.total,
        directory_diff.rows.len() as u64
    );

    // The rename log lists the applied batch.
    let batches =
        batchhash_ffi::api::list_batches_request(&batchhash_ffi::api::ListBatchesRequest {
            db: log_db.to_string_lossy().into_owned(),
        })
        .expect("history");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].file_count, 1);
    assert_eq!(batches[0].applied_count, 1);
    assert_eq!(batches[0].undone_count, 0);
}

/// Picking one file must not make the comparison read its whole directory.
/// Regression test for hashing an entire tree from a single-file report.
#[test]
fn directory_diff_honours_the_report_scope() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("picked.txt"), b"alpha");
    // A sibling and a nested file that the report never mentions.
    write(&temp.path().join("sibling.txt"), b"bravo");
    write(&temp.path().join("nested/deep.txt"), b"charlie");

    let scan_request = serde_json::json!({
        "root": temp.path().to_string_lossy(),
        "follow_links": false,
    });
    let scan_text =
        CString::new(serde_json::to_string(&scan_request).expect("json")).expect("cstr");
    let scanned = call_ok(unsafe { c_api::bh_scan(scan_text.as_ptr()) });
    let all: Vec<serde_json::Value> = scanned["entries"].as_array().expect("entries").clone();
    let picked = all
        .iter()
        .filter(|entry| entry["path"] == "picked.txt")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(picked.len(), 1, "the fixture has one picked file");

    let hash_request = serde_json::json!({
        "root": temp.path().to_string_lossy(),
        "paths": ["picked.txt"],
        "algorithms": ["sha256"],
    });
    let hash_text =
        CString::new(serde_json::to_string(&hash_request).expect("json")).expect("cstr");
    let hashed = call_ok(unsafe { c_api::bh_hash_entries(hash_text.as_ptr()) });
    let report_json = hashed["report_json"]
        .as_str()
        .expect("report json")
        .to_string();

    // Unscoped would see all three files; restricted sees only the reported one.
    let unscoped =
        batchhash_ffi::api::diff_directory_request(&batchhash_ffi::api::DiffDirectoryRequest {
            report_json: report_json.clone(),
            directory: temp.path().to_string_lossy().into_owned(),
            case_sensitive: None,
            follow_links: false,
            recursive: Some(true),
            restrict_paths: Vec::new(),
            max_entries: None,
        })
        .expect("unscoped diff");

    let scoped =
        batchhash_ffi::api::diff_directory_request(&batchhash_ffi::api::DiffDirectoryRequest {
            report_json,
            directory: temp.path().to_string_lossy().into_owned(),
            case_sensitive: None,
            follow_links: false,
            recursive: Some(true),
            restrict_paths: vec!["picked.txt".to_string()],
            max_entries: None,
        })
        .expect("scoped diff");

    assert!(
        scoped.summary.total < unscoped.summary.total,
        "scoping must reduce what gets compared: scoped={} unscoped={}",
        scoped.summary.total,
        unscoped.summary.total
    );
    assert_eq!(
        scoped.summary.added, 0,
        "files outside the scope must not appear as added"
    );
}

/// A comparison must refuse a selection that exceeds the entry cap instead of
/// grinding through it.
#[test]
fn directory_diff_rejects_a_selection_above_the_cap() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");
    write(&temp.path().join("b.txt"), b"bravo");

    let hash_request = serde_json::json!({
        "root": temp.path().to_string_lossy(),
        "paths": ["a.txt"],
        "algorithms": ["sha256"],
    });
    let hash_text =
        CString::new(serde_json::to_string(&hash_request).expect("json")).expect("cstr");
    let hashed = call_ok(unsafe { c_api::bh_hash_entries(hash_text.as_ptr()) });
    let report_json = hashed["report_json"]
        .as_str()
        .expect("report json")
        .to_string();

    let error =
        batchhash_ffi::api::diff_directory_request(&batchhash_ffi::api::DiffDirectoryRequest {
            report_json,
            directory: temp.path().to_string_lossy().into_owned(),
            case_sensitive: None,
            follow_links: false,
            recursive: Some(true),
            restrict_paths: Vec::new(),
            max_entries: Some(1),
        })
        .expect_err("the cap must reject a larger directory");
    assert_eq!(error.kind, "too_large", "{error:?}");
}
#[test]
fn c_abi_plan_and_diff_functions() {
    let temp = tempfile::tempdir().expect("tempdir");
    write(&temp.path().join("a.txt"), b"alpha");

    let plan_request = serde_json::json!({
        "entries": [{
            "path": "a.txt",
            "hashes": [{"algorithm": "md5", "hex": "0123456789abcdef0123456789abcdef"}]
        }],
        "algorithm": "md5",
        "length": 8,
    });
    let text = CString::new(serde_json::to_string(&plan_request).expect("json")).expect("cstr");
    let plan = call_ok(unsafe { c_api::bh_plan_renames(text.as_ptr()) });
    assert_eq!(plan["summary"]["ready"], 1);

    let batch = serde_json::json!({
        "root": temp.path().to_string_lossy(),
        "algorithms": ["md5"],
    });
    let batch_text = CString::new(serde_json::to_string(&batch).expect("json")).expect("cstr");
    let run = call_ok(unsafe { c_api::bh_run_batch(batch_text.as_ptr()) });
    let report_json = run["report_json"]
        .as_str()
        .expect("report json")
        .to_string();

    let diff_request = serde_json::json!({
        "left_json": report_json,
        "right_json": report_json,
    });
    let diff_text =
        CString::new(serde_json::to_string(&diff_request).expect("json")).expect("cstr");
    let diff = call_ok(unsafe { c_api::bh_diff_reports(diff_text.as_ptr()) });
    assert_eq!(diff["summary"]["total"], 1);

    let hash_request = serde_json::json!({
        "root": temp.path().to_string_lossy(),
        "paths": ["a.txt"],
        "algorithms": ["sha256"],
    });
    let hash_text =
        CString::new(serde_json::to_string(&hash_request).expect("json")).expect("cstr");
    let hashed = call_ok(unsafe { c_api::bh_hash_entries(hash_text.as_ptr()) });
    assert_eq!(hashed["ok"], 1);
}
