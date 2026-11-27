//! Storage Access Framework bridge tests.
//!
//! Android cannot hand a folder to the core as a path: its picker grants a
//! content:// tree that no file API can open. These tests pin the two entry
//! points that work around it, and the rules that keep them honest.

use std::ffi::{CStr, CString};
use std::fs;
use std::os::unix::io::AsRawFd;

use batchhash_ffi::api::{
    hash_fd_entries_request, scan_saf_request, FdEntryDto, HashFdEntriesRequest, SafNodeDto,
    ScanSafRequest,
};
use batchhash_ffi::c_api;

/// Send [request] through the scan entry point and return the whole envelope.
fn envelope(request: &impl serde::Serialize) -> serde_json::Value {
    let json = serde_json::to_string(request).expect("serialise request");
    let text = CString::new(json).expect("c string");
    // SAFETY: text is a valid NUL-terminated string for the duration of the
    // call.
    let pointer = unsafe { c_api::bh_scan_saf(text.as_ptr()) };
    assert!(!pointer.is_null(), "null return from the C ABI");
    // SAFETY: pointer came from the crate and is freed exactly once here.
    let body = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { c_api::bh_free(pointer) };
    serde_json::from_str(&body).expect("json envelope")
}

/// Send [request] through the fd-hashing entry point and return the envelope.
fn fd_envelope(request: &impl serde::Serialize) -> serde_json::Value {
    let json = serde_json::to_string(request).expect("serialise request");
    let text = CString::new(json).expect("c string");
    // SAFETY: text is a valid NUL-terminated string for the duration of the
    // call.
    let pointer = unsafe { c_api::bh_hash_fd_entries(text.as_ptr()) };
    assert!(!pointer.is_null(), "null return from the C ABI");
    // SAFETY: pointer came from the crate and is freed exactly once here.
    let body = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { c_api::bh_free(pointer) };
    serde_json::from_str(&body).expect("json envelope")
}

fn scan_request_json(nodes: Vec<SafNodeDto>) -> serde_json::Value {
    envelope(&ScanSafRequest {
        tree_uri: "content://tree/primary%3AMovies".to_string(),
        nodes,
    })
}

/// SHA-256 of [bytes] through the ordinary reader path.
///
/// Used as the reference a descriptor hash must agree with, so a SAF hash is
/// provably the same hash rather than a parallel implementation.
fn sha256_of(bytes: &[u8]) -> String {
    use std::io::Read;
    let mut reader = std::io::Cursor::new(bytes);
    batchhash_hash::compute_hashes(
        &mut reader as &mut dyn Read,
        &batchhash_hash::HashOptions::new(vec![batchhash_types::AlgorithmId::Sha256]),
    )
    .expect("hash bytes")
    .get(&batchhash_types::AlgorithmId::Sha256)
    .cloned()
    .expect("sha256")
}

fn node(path: &str, size: u64) -> SafNodeDto {
    SafNodeDto {
        path: path.to_string(),
        size,
        mtime: Some("2026-11-14T19:03:23Z".to_string()),
    }
}

#[test]
fn scan_saf_builds_entries_from_the_walked_nodes() {
    let reply = scan_request_json(vec![node("a.bin", 3), node("nested/b.bin", 4)]);
    assert_eq!(reply["ok"], true);
    let data = &reply["data"];
    // The tree URI is echoed back as the root: there is no absolute path to
    // report for a granted tree.
    assert_eq!(data["root"], "content://tree/primary%3AMovies");
    let entries = data["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["path"], "a.bin");
    assert_eq!(entries[0]["size"], 3);
    assert_eq!(entries[0]["mtime"], "2026-11-14T19:03:23Z");
    // A walked tree is never a single file: the user picked a folder.
    assert_eq!(data["single_file"], false);
    // Sizes are summed so a report can state the total without re-reading.
    assert_eq!(data["total_bytes"], 7);
}

#[test]
fn scan_saf_refuses_an_empty_node_list() {
    // An empty list means the walk failed, not that the folder is empty.
    // Reporting success would make the two indistinguishable.
    let reply = scan_request_json(Vec::new());
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["kind"], "invalid_request");
}

#[test]
fn scan_saf_skips_a_path_it_cannot_represent() {
    let reply = scan_request_json(vec![
        node("ok.bin", 1),
        // Absolute and backslash paths are rejected by the shared validator,
        // the same rule the filesystem scanner applies.
        node("/escape.bin", 1),
        node("back\\slash.bin", 1),
    ]);
    assert_eq!(reply["ok"], true);
    let data = &reply["data"];
    assert_eq!(data["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(data["skipped"].as_array().expect("skipped").len(), 2);
}

#[test]
fn hash_fd_entries_hashes_a_real_descriptor() {
    let dir = std::env::temp_dir().join("batchhash-saf-fd-test");
    fs::create_dir_all(&dir).expect("create dir");
    let path = dir.join("payload.bin");
    fs::write(&path, b"batchhash saf payload").expect("write fixture");

    let file = fs::File::open(&path).expect("open fixture");
    let size = file.metadata().expect("stat").len();
    let request = HashFdEntriesRequest {
        root: "content://tree/primary%3AMovies".to_string(),
        entries: vec![FdEntryDto {
            path: "payload.bin".to_string(),
            fd: file.as_raw_fd(),
            offset: 0,
            length: Some(size),
        }],
        algorithms: vec!["sha256".to_string()],
    };

    let reply = fd_envelope(&request);
    assert_eq!(reply["ok"], true);
    let data = &reply["data"];
    assert_eq!(data["total"], 1);
    assert_eq!(data["ok"], 1);
    let entries = data["entries"].as_array().expect("entries");
    let digest = entries[0]["hashes"]
        .as_array()
        .expect("hashes")
        .iter()
        .find(|item| item["algorithm"] == "sha256")
        .expect("sha256 row")["hex"]
        .as_str()
        .expect("digest string")
        .to_owned();

    // The descriptor is duped, never adopted: the original must still be
    // readable afterwards.
    use std::io::{Read, Seek, SeekFrom};
    let mut reopened = file;
    reopened.seek(SeekFrom::Start(0)).expect("rewind");
    let mut buf = Vec::new();
    reopened.read_to_end(&mut buf).expect("read original");
    assert_eq!(buf, b"batchhash saf payload");

    // And the digest must match hashing the same bytes the ordinary way, so a
    // SAF hash is not a different hash.
    let expected = sha256_of(b"batchhash saf payload");
    assert_eq!(digest, expected);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hash_fd_entries_honours_a_non_zero_offset() {
    // The offset is honoured rather than assumed to be zero: a provider may
    // hand back a descriptor positioned anywhere, and hashing from the start
    // would silently hash the wrong bytes.
    let dir = std::env::temp_dir().join("batchhash-saf-offset-test");
    fs::create_dir_all(&dir).expect("create dir");
    let path = dir.join("payload.bin");
    fs::write(&path, b"XXbatchhash tail").expect("write fixture");

    let file = fs::File::open(&path).expect("open fixture");
    let request = HashFdEntriesRequest {
        root: "content://tree/primary%3AMovies".to_string(),
        entries: vec![FdEntryDto {
            path: "payload.bin".to_string(),
            fd: file.as_raw_fd(),
            offset: 2,
            length: Some(9),
        }],
        algorithms: vec!["sha256".to_string()],
    };

    let reply = fd_envelope(&request);
    assert_eq!(reply["ok"], true);
    let entries = reply["data"]["entries"].as_array().expect("entries");
    let digest = entries[0]["hashes"]
        .as_array()
        .expect("hashes")
        .iter()
        .find(|item| item["algorithm"] == "sha256")
        .expect("sha256 row")["hex"]
        .as_str()
        .expect("digest")
        .to_owned();

    // Only the nine bytes after the two skipped ones were hashed.
    let expected = sha256_of(b"batchhash");
    assert_eq!(digest, expected);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hash_fd_entries_refuses_an_empty_selection() {
    // Same rule as the rename plan: an empty list is a caller mistake, never a
    // successful no-op.
    let request = HashFdEntriesRequest {
        root: "content://tree/primary%3AMovies".to_string(),
        entries: Vec::new(),
        algorithms: vec!["sha256".to_string()],
    };
    let reply = fd_envelope(&request);
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["kind"], "invalid_request");
}

#[test]
fn hash_fd_entries_reports_a_missing_descriptor_on_the_entry() {
    // An entry with no descriptor fails on that entry and the batch keeps
    // going; it must not abort the whole run.
    let dir = std::env::temp_dir().join("batchhash-saf-missing-test");
    fs::create_dir_all(&dir).expect("create dir");
    let good = dir.join("good.bin");
    fs::write(&good, b"good").expect("write fixture");

    let file = fs::File::open(&good).expect("open fixture");
    let request = HashFdEntriesRequest {
        root: "content://tree/primary%3AMovies".to_string(),
        entries: vec![
            FdEntryDto {
                path: "good.bin".to_string(),
                fd: file.as_raw_fd(),
                offset: 0,
                length: Some(4),
            },
            // Never opened: the resolver finds nothing for it.
            FdEntryDto {
                path: "absent.bin".to_string(),
                fd: -1,
                offset: 0,
                length: Some(0),
            },
        ],
        algorithms: vec!["sha256".to_string()],
    };

    let reply = fd_envelope(&request);
    assert_eq!(reply["ok"], true);
    let data = &reply["data"];
    assert_eq!(data["total"], 2);
    assert_eq!(data["ok"], 1);
    assert_eq!(data["failed"], 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scan_saf_survives_the_c_abi_round_trip() {
    // The app reaches these through the C ABI, so the envelope has to carry
    // the same payload the serialisable API produces.
    let json = serde_json::json!({
        "tree_uri": "content://tree/primary%3AMovies",
        "nodes": [
            {"path": "a.bin", "size": 3, "mtime": "2026-11-14T19:03:23Z"},
        ],
    })
    .to_string();
    let c_request = CString::new(json).expect("c string");
    // SAFETY: c_request is a valid NUL-terminated string.
    let raw = unsafe { c_api::bh_scan_saf(c_request.as_ptr()) };
    assert!(!raw.is_null());
    // SAFETY: raw came from bh_scan_saf and has not been freed yet.
    let text = unsafe { CStr::from_ptr(raw) }.to_str().expect("utf8");
    let reply: serde_json::Value = serde_json::from_str(text).expect("json");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["entries"][0]["path"], "a.bin");
    // SAFETY: raw is the pointer bh_scan_saf returned.
    unsafe { c_api::bh_free(raw) };
}

#[test]
fn scan_saf_request_deserialises_without_mtime() {
    // mtime is optional: a provider that reports no modification time must
    // not become a fabricated epoch.
    let parsed: ScanSafRequest = serde_json::from_value(serde_json::json!({
        "tree_uri": "content://tree/primary%3AMovies",
        "nodes": [{"path": "a.bin", "size": 3}],
    }))
    .expect("deserialise");
    let result = scan_saf_request(&parsed).expect("scan");
    assert_eq!(result.root, "content://tree/primary%3AMovies");
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].path, "a.bin");
    assert_eq!(result.entries[0].mtime, None);
}

#[test]
fn the_dart_bridge_payload_deserialises_verbatim() {
    // The literal JSON the Flutter bridge sends, copied from
    // app/lib/src/bridge/native_bridge.dart.
    //
    // Every other test here builds the request struct in Rust, which is exactly
    // why a missing field could ship: the struct always had one. This one starts
    // from the wire shape, so a DTO that demands a field the bridge does not
    // send fails here instead of on a user's phone.
    let dart_payload = r#"{
        "root": "content://tree/primary%3AMovies",
        "entries": [
            {"path": "clip.mkv", "fd": 42, "offset": 0, "length": 1024}
        ],
        "algorithms": ["sha256", "blake3"]
    }"#;
    let parsed: HashFdEntriesRequest =
        serde_json::from_str(dart_payload).expect("the bridge payload must deserialise");
    assert_eq!(parsed.root, "content://tree/primary%3AMovies");
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].path, "clip.mkv");
    assert_eq!(parsed.entries[0].fd, 42);
    assert_eq!(parsed.entries[0].length, Some(1024));
    assert_eq!(parsed.algorithms, vec!["sha256", "blake3"]);
}

#[test]
fn the_dart_scan_payload_deserialises_verbatim() {
    // Same reasoning as above, for the scan entry point.
    let dart_payload = r#"{
        "tree_uri": "content://tree/primary%3AMovies",
        "nodes": [
            {"path": "clip.mkv", "size": 1024, "mtime": "2026-11-15T12:00:00Z"}
        ]
    }"#;
    let parsed: ScanSafRequest =
        serde_json::from_str(dart_payload).expect("the bridge payload must deserialise");
    let result = scan_saf_request(&parsed).expect("scan");
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].size, 1024);
    assert_eq!(
        result.entries[0].mtime.as_deref(),
        Some("2026-11-15T12:00:00Z")
    );
}
#[test]
fn a_missing_length_reads_to_the_end_of_the_file() {
    use std::io::Write;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("unknown.bin");
    let mut writer = fs::File::create(&path).expect("create fixture");
    writer.write_all(b"twelve bytes").expect("write fixture");
    writer.flush().expect("flush");
    let file = fs::File::open(&path).expect("open fixture");

    // The Storage Access Framework reports statSize as -1 for some providers.
    // Carrying that as a length made the reader stop immediately and hash
    // nothing; `None` must mean "read the whole file".
    let request = HashFdEntriesRequest {
        root: "content://tree/primary%3AMovies".to_string(),
        entries: vec![FdEntryDto {
            path: "unknown.bin".to_string(),
            fd: file.as_raw_fd(),
            offset: 0,
            length: None,
        }],
        algorithms: vec!["sha256".to_string()],
    };
    let result = hash_fd_entries_request(&request).expect("hash");
    assert_eq!(result.entries.len(), 1);
    let entry = &result.entries[0];
    assert_eq!(entry.status, "ok", "entry: {entry:?}");
    // `size` mirrors the request, which carried no length, so it is 0 here by
    // design; what proves the read worked is a digest of the whole content.
    let digest = entry
        .hashes
        .iter()
        .find(|hash| hash.algorithm == "sha256")
        .map(|hash| hash.hex.as_str())
        .expect("a sha256 digest");
    // sha256("twelve bytes"), computed independently of this crate.
    assert_eq!(
        digest, "d4ce2c527afe674c7a086bd74e256019e3d5dcdb31eeb6eaadef5ada8c4383b9",
        "the whole file must be hashed, not zero bytes"
    );
    assert!(
        entry.hashes.iter().any(|hash| hash.algorithm == "sha256"),
        "a digest is still produced: {entry:?}"
    );
}
