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

use crate::api::{
    self, ApiError, BatchRequestDto, ProbeRequest, ScanRequest, UndoRequestDto,
};

/// Version of the C ABI (bumped when the envelope shape changes).
pub const C_ABI_VERSION: u32 = 1;

/// Return the bridge contract version as JSON.
///
/// # Safety
/// The returned pointer must be released with [bh_free].
#[no_mangle]
pub unsafe extern "C" fn bh_api_version() -> *mut c_char {
    reply_ok(json!({
        "contract_version": api::CONTRACT_VERSION,
        "c_abi_version": C_ABI_VERSION,
        "tool": batchhash_types::TOOL_NAME,
        "tool_version": batchhash_types::TOOL_VERSION,
    }))
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
