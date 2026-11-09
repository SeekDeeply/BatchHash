//! Stable C ABI over [crate::api].
//!
//! Every function takes a UTF-8 JSON request (a NUL-terminated C string) and
//! returns a heap allocated, NUL-terminated JSON envelope. The caller must free
//! the result with [bh_free]; passing a pointer that this crate did not return is
//! undefined behaviour.
//!
//! Envelopes:
//!
//! * success: {"ok": true, "api_version": 1, "data": ...}
//! * failure: {"ok": false, "api_version": 1, "kind": "...", "error": "..."}
//!
//! No Rust panic may cross this boundary: every entry point catches unwinding and
//! converts it into an "internal" error envelope.

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use serde::Serialize;
use serde_json::json;

use crate::api::{self, ApiError, BatchRequestDto, ProbeRequest, ScanRequest, UndoRequestDto};

/// Version of the C ABI (bumped when the envelope shape changes).
pub const C_ABI_VERSION: u32 = 1;

/// Return the bridge contract version as JSON.
///
/// # Safety
/// The returned pointer must be released with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_api_version() -> *mut c_char {
    reply(|| {
        Ok(json!({
            "contract_version": api::CONTRACT_VERSION,
            "c_abi_version": C_ABI_VERSION,
            "tool": batchhash_types::TOOL_NAME,
            "tool_version": batchhash_types::TOOL_VERSION,
            // Capabilities let the Dart side detect optional features without
            // inferring them from the ABI version: the envelope shape did not
            // change when async jobs were added, so the version did not either.
            "features": ["async_jobs", "progress"],
        }))
    })
}

/// List the registered algorithms.
///
/// # Safety
/// The returned pointer must be released with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_algorithms() -> *mut c_char {
    reply(|| {
        let algorithms = api::algorithms();
        serde_json::to_value(algorithms).map_err(internal_error)
    })
}

/// Report hardware capabilities. Request: {"forced_soft": bool}.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_probe(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: ProbeRequest = parse_request(request, true)?;
        let hardware = api::probe(&parsed);
        serde_json::to_value(hardware).map_err(internal_error)
    })
}

/// Scan a root. Request: {"root": "...", "follow_links": bool}.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_scan(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: ScanRequest = parse_request(request, false)?;
        let scanned = api::scan_request(&parsed)?;
        serde_json::to_value(scanned).map_err(internal_error)
    })
}

/// Run the pipeline. Request: see [crate::api::BatchRequestDto].
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_run_batch(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: BatchRequestDto = parse_request(request, false)?;
        let run = api::run_batch_request(&parsed)?;
        serde_json::to_value(run).map_err(internal_error)
    })
}

/// Plan or execute an undo. Request: see [crate::api::UndoRequestDto].
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_undo(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: UndoRequestDto = parse_request(request, false)?;
        let result = api::undo(&parsed)?;
        serde_json::to_value(result).map_err(internal_error)
    })
}

/// Re-export report JSON. Request: {"report_json": "...", "format": "csv"}.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_export_report(request: *const c_char) -> *mut c_char {
    reply(|| {
        #[derive(serde::Deserialize)]
        struct ExportRequest {
            report_json: String,
            format: String,
        }
        let parsed: ExportRequest = parse_request(request, false)?;
        let text = api::export(&parsed.report_json, &parsed.format)?;
        Ok(json!({ "text": text }))
    })
}

/// Start a native job. Request: {"action": "batch" | "hash_entries" |
/// "diff_directory", ...payload fields for that action}.
///
/// Returns {"job_id": "..."} immediately; the work runs on its own thread so
/// the calling isolate stays responsive. Poll with [bh_job_poll].
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_job_start(request: *const c_char) -> *mut c_char {
    reply(|| {
        #[derive(serde::Deserialize)]
        struct StartRequest {
            action: String,
            #[serde(default)]
            payload: Option<serde_json::Value>,
        }
        let parsed: StartRequest = parse_request(request, false)?;
        let payload = parsed.payload.unwrap_or(serde_json::Value::Null);
        let job_id = match parsed.action.as_str() {
            "batch" => {
                let value: BatchRequestDto = serde_json::from_value(payload)
                    .map_err(|error| ApiError::invalid(format!("batch payload: {error}")))?;
                crate::jobs::start_batch(value)?
            }
            "hash_entries" => {
                let value: api::HashEntriesRequest = serde_json::from_value(payload)
                    .map_err(|error| ApiError::invalid(format!("hash_entries payload: {error}")))?;
                crate::jobs::start_hash_entries(value)?
            }
            "diff_directory" => {
                let value: api::DiffDirectoryRequest =
                    serde_json::from_value(payload).map_err(|error| {
                        ApiError::invalid(format!("diff_directory payload: {error}"))
                    })?;
                crate::jobs::start_diff_directory(value)?
            }
            other => {
                return Err(ApiError::invalid(format!(
                    "unknown job action: {other}; expected batch, hash_entries or diff_directory",
                )));
            }
        };
        Ok(json!({ "job_id": job_id }))
    })
}

/// Poll a job. Request: {"job_id": "..."}.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_job_poll(request: *const c_char) -> *mut c_char {
    reply(|| {
        #[derive(serde::Deserialize)]
        struct PollRequest {
            job_id: String,
        }
        let parsed: PollRequest = parse_request(request, false)?;
        let status = crate::jobs::poll(&parsed.job_id)?;
        serde_json::to_value(status).map_err(internal_error)
    })
}

/// Request cancellation of a job. Request: {"job_id": "..."}.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_job_cancel(request: *const c_char) -> *mut c_char {
    reply(|| {
        #[derive(serde::Deserialize)]
        struct CancelRequest {
            job_id: String,
        }
        let parsed: CancelRequest = parse_request(request, false)?;
        let status = crate::jobs::cancel(&parsed.job_id)?;
        serde_json::to_value(status).map_err(internal_error)
    })
}

/// Release a job and its result. Request: {"job_id": "..."}.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_job_release(request: *const c_char) -> *mut c_char {
    reply(|| {
        #[derive(serde::Deserialize)]
        struct ReleaseRequest {
            job_id: String,
        }
        let parsed: ReleaseRequest = parse_request(request, false)?;
        let released = crate::jobs::release(&parsed.job_id)?;
        Ok(json!({ "released": released }))
    })
}

/// Free a string returned by this crate.
///
/// # Safety
/// ptr must be null or a pointer previously returned by a bh_* function that has
/// not been freed yet.
#[no_mangle]
pub unsafe extern "C" fn bh_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    drop(CString::from_raw(ptr));
}

fn parse_request<T: serde::de::DeserializeOwned>(
    request: *const c_char,
    allow_null: bool,
) -> Result<T, ApiError> {
    if request.is_null() {
        if allow_null {
            return serde_json::from_str::<T>("{}")
                .map_err(|error| ApiError::invalid(error.to_string()));
        }
        return Err(ApiError::invalid("request pointer is null"));
    }
    // SAFETY: the caller guarantees a NUL-terminated string that stays alive for
    // the duration of this call.
    let text = unsafe { CStr::from_ptr(request) }
        .to_str()
        .map_err(|error| ApiError::invalid(format!("request is not valid UTF-8: {error}")))?;
    serde_json::from_str::<T>(text)
        .map_err(|error| ApiError::invalid(format!("cannot parse request: {error}")))
}

fn internal_error(error: serde_json::Error) -> ApiError {
    ApiError::new("internal", error.to_string())
}

/// Run a closure and convert its result into an envelope string.
fn reply<F>(body: F) -> *mut c_char
where
    F: FnOnce() -> Result<serde_json::Value, ApiError>,
{
    let outcome = catch_unwind(AssertUnwindSafe(body));
    let envelope = match outcome {
        Ok(Ok(data)) => json!({ "ok": true, "api_version": C_ABI_VERSION, "data": data }),
        Ok(Err(error)) => json!({
            "ok": false,
            "api_version": C_ABI_VERSION,
            "kind": error.kind,
            "error": error.message,
        }),
        Err(_) => json!({
            "ok": false,
            "api_version": C_ABI_VERSION,
            "kind": "internal",
            "error": "the native library panicked",
        }),
    };
    reply_ok(envelope)
}

fn reply_ok(value: serde_json::Value) -> *mut c_char {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| {
        let fallback = json!({
            "ok": false,
            "api_version": C_ABI_VERSION,
            "kind": "internal",
            "error": "cannot serialise the response",
        });
        fallback.to_string()
    });
    // A JSON document never contains a NUL byte, so CString::new is safe here.
    match CString::new(text) {
        Ok(cstring) => cstring.into_raw(),
        Err(_) => CString::new("{\"ok\":false,\"kind\":\"internal\",\"error\":\"nul byte\"}")
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
    }
}

/// Serialise a value into a JSON envelope, used by tests and bindings that must
/// not go through the raw pointers.
pub fn call_json<T: Serialize>(value: T) -> String {
    serde_json::to_string(&json!({
        "ok": true,
        "api_version": C_ABI_VERSION,
        "data": value,
    }))
    .unwrap_or_else(|_| "{\"ok\":false,\"kind\":\"internal\",\"error\":\"serialise\"}".to_string())
}

/// Hash explicit root-relative paths. Request: see api::HashEntriesRequest.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_hash_entries(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: api::HashEntriesRequest = parse_request(request, false)?;
        let run = api::hash_entries_request(&parsed)?;
        serde_json::to_value(run).map_err(internal_error)
    })
}

/// Plan suffix renames from entries that already carry their hashes.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_plan_renames(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: api::PlanRenamesRequest = parse_request(request, false)?;
        let plan = api::plan_renames_request(&parsed)?;
        serde_json::to_value(plan).map_err(internal_error)
    })
}

/// Import a report file and return its canonical JSON.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_import_report(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: api::ImportReportRequest = parse_request(request, false)?;
        let json = api::import_report_request(&parsed)?;
        Ok(json!({ "report_json": json }))
    })
}

/// Compare two stored reports.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_diff_reports(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: api::DiffReportsRequest = parse_request(request, false)?;
        let diff = api::diff_reports_request(&parsed)?;
        serde_json::to_value(diff).map_err(internal_error)
    })
}

/// Compare a stored report against a live directory scan.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_diff_directory(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: api::DiffDirectoryRequest = parse_request(request, false)?;
        let diff = api::diff_directory_request(&parsed)?;
        serde_json::to_value(diff).map_err(internal_error)
    })
}

/// List the batches stored in a rename log.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_list_batches(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: api::ListBatchesRequest = parse_request(request, false)?;
        let batches = api::list_batches_request(&parsed)?;
        serde_json::to_value(batches).map_err(internal_error)
    })
}

/// Build canonical report JSON from entries the UI already holds.
///
/// # Safety
/// request must be a valid NUL-terminated UTF-8 string; the result must be freed
/// with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_build_report(request: *const c_char) -> *mut c_char {
    reply(|| {
        let parsed: api::BuildReportRequest = parse_request(request, false)?;
        let report_json = api::build_report_request(&parsed)?;
        Ok(json!({ "report_json": report_json }))
    })
}
