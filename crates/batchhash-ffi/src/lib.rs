//! BatchHash native bridge crate.
//!
//! This crate is the only place where the Rust core is exposed to non-Rust code.
//! It has two layers:
//!
//! * [api] - a plain, serialisable Rust API (request/response structs). Every
//!   value crossing the boundary is either a scalar, a String or a Vec, so the
//!   same functions can be wrapped by any binding technology (a C ABI today, a
//!   flutter_rust_bridge generated layer later).
//! * [c_api] - a small stable C ABI (JSON in, JSON out) consumed by Dart through
//!   dart:ffi. Every call returns a JSON envelope:
//!   {"ok": true, "data": ...} or {"ok": false, "error": "...", "kind": "..."}.
//!
//! Rules for this crate:
//!
//! * no panics may cross the ABI boundary (every entry point catches unwind),
//! * the caller owns nothing: returned pointers are freed with bh_free,
//! * request strings stay valid for the duration of the call only.

#![deny(missing_docs)]

pub mod api;
pub mod c_api;

/// Version of the bridge API contract (bumped when request/response shapes change).
pub const API_VERSION: u32 = 1;
