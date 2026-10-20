//! Honest hardware capability reporting.
//!
//! The probe reports what can actually be observed:
//!
//! * arch comes from std::env::consts::ARCH,
//! * detected_features comes from the platform runtime feature-detection macros
//!   (CPUID on x86/x86_64, HWCAP on aarch64),
//! * blake3_backend_reported and sha256_backend_reported stay "unknown" because
//!   neither the blake3 nor the sha2 crate exposes the backend it selected at
//!   runtime. Unknown is never replaced by a guess: tests must not assert a
//!   specific accelerator, and CI runners differ.
//!
//! source is "cpufeatures" when runtime CPU feature detection was available and
//! "inferred" when only the architecture string could be obtained.

pub use batchhash_types::BackendPolicy;

use batchhash_types::HardwareReport;

/// Observed hardware capabilities relevant to hashing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareInfo {
    /// Target architecture reported by the Rust standard library (for example x86_64).
    pub arch: String,
    /// Detected instruction-set features in a stable order (for example SSE4.1, AVX2).
    pub detected_features: Vec<String>,
    /// Acceleration policy this probe was run with.
    pub backend_policy: BackendPolicy,
    /// Reported BLAKE3 backend: one of avx512, avx2, neon, portable, unknown.
    pub blake3_backend_reported: String,
    /// Reported SHA-256 backend: one of sha-ni, aarch64-sha2, soft, unknown.
    pub sha256_backend_reported: String,
    /// Where the observation came from: cpufeatures or inferred.
    pub source: String,
    /// Number of worker threads the batch scheduler will use (always at least 1).
    pub multithread: usize,
}

impl From<HardwareInfo> for HardwareReport {
    fn from(info: HardwareInfo) -> Self {
        Self {
            arch: info.arch,
            detected_features: info.detected_features,
            backend_policy: info.backend_policy,
            blake3_backend_reported: info.blake3_backend_reported,
            sha256_backend_reported: info.sha256_backend_reported,
            source: info.source,
            multithread: info.multithread,
        }
    }
}

/// Probe hardware capabilities with an explicit acceleration policy.
pub fn probe(policy: BackendPolicy) -> HardwareInfo {
    HardwareInfo {
        arch: std::env::consts::ARCH.to_string(),
        detected_features: detected_features(),
        backend_policy: policy,
        blake3_backend_reported: reported_unknown(),
        sha256_backend_reported: reported_unknown(),
        source: detection_source().to_string(),
        multithread: worker_count(),
    }
}

/// Probe hardware capabilities with the default BackendPolicy::Auto policy.
pub fn probe_auto() -> HardwareInfo {
    probe(BackendPolicy::Auto)
}

fn reported_unknown() -> String {
    "unknown".to_string()
}

fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .max(1)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn detected_features() -> Vec<String> {
    let mut features = Vec::new();
    if std::arch::is_x86_feature_detected!("sse2") {
        features.push("SSE2".to_string());
    }
    if std::arch::is_x86_feature_detected!("sse4.1") {
        features.push("SSE4.1".to_string());
    }
    if std::arch::is_x86_feature_detected!("avx2") {
        features.push("AVX2".to_string());
    }
    if std::arch::is_x86_feature_detected!("avx512f") {
        features.push("AVX-512F".to_string());
    }
    if std::arch::is_x86_feature_detected!("sha") {
        features.push("SHA-NI".to_string());
    }
    features
}

#[cfg(target_arch = "aarch64")]
fn detected_features() -> Vec<String> {
    let mut features = Vec::new();
    if std::arch::is_aarch64_feature_detected!("neon") {
        features.push("NEON".to_string());
    }
    if std::arch::is_aarch64_feature_detected!("sha2") {
        features.push("SHA2".to_string());
    }
    features
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
fn detected_features() -> Vec<String> {
    Vec::new()
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
fn detection_source() -> &'static str {
    "cpufeatures"
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
fn detection_source() -> &'static str {
    "inferred"
}
