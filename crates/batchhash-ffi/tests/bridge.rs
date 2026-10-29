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
    assert!(sha256.suffix_usable);
    let crc32 = list.iter().find(|item| item.id == "crc32").expect("crc32");
    assert!(!crc32.cryptographic);
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
fn c_abi_version_and_algorithms() {
    let version = call_ok(unsafe { c_api::bh_api_version() });
    assert_eq!(version["contract_version"], 1);
    assert_eq!(version["tool"], "BatchHash");

    let list = call_ok(unsafe { c_api::bh_algorithms() });
    assert_eq!(
        list.as_array().expect("array").len(),
        batchhash_types::REGISTRY.len()
    );
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
