//! HardwareProbe shape tests. No test asserts a specific accelerator.

use batchhash_hash::{probe, probe_auto, HardwareInfo};
use batchhash_types::{BackendPolicy, HardwareReport};

const ALLOWED_BLAKE3_BACKENDS: [&str; 5] = ["avx512", "avx2", "neon", "portable", "unknown"];
const ALLOWED_SHA256_BACKENDS: [&str; 4] = ["sha-ni", "aarch64-sha2", "soft", "unknown"];
const ALLOWED_FEATURES: [&str; 7] = [
    "SSE2",
    "SSE4.1",
    "AVX2",
    "AVX-512F",
    "SHA-NI",
    "NEON",
    "SHA2",
];

#[test]
fn hardware_info_is_complete_and_honest() {
    let info = probe_auto();
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert_eq!(info.backend_policy, BackendPolicy::Auto);
    assert!(info.multithread >= 1);
    assert!(
        info.source == "cpufeatures" || info.source == "inferred",
        "unexpected source: {}",
        info.source
    );
    assert!(
        ALLOWED_BLAKE3_BACKENDS.contains(&info.blake3_backend_reported.as_str()),
        "unexpected blake3 backend: {}",
        info.blake3_backend_reported
    );
    assert!(
        ALLOWED_SHA256_BACKENDS.contains(&info.sha256_backend_reported.as_str()),
        "unexpected sha256 backend: {}",
        info.sha256_backend_reported
    );
    for feature in &info.detected_features {
        assert!(
            ALLOWED_FEATURES.contains(&feature.as_str()),
            "unexpected feature: {feature}"
        );
    }
    let mut sorted = info.detected_features.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        info.detected_features.len(),
        "features must be reported once"
    );
}

#[test]
fn forced_soft_reports_real_features_and_policy() {
    let auto = probe_auto();
    let forced = probe(BackendPolicy::ForcedSoft);
    assert_eq!(forced.backend_policy, BackendPolicy::ForcedSoft);
    assert_eq!(forced.arch, auto.arch);
    assert_eq!(forced.detected_features, auto.detected_features);
    assert_eq!(forced.source, auto.source);
    assert_eq!(forced.blake3_backend_reported, auto.blake3_backend_reported);
    assert_eq!(forced.sha256_backend_reported, auto.sha256_backend_reported);
    assert_eq!(forced.multithread, auto.multithread);
}

#[test]
fn explicit_auto_probe_matches_probe_auto() {
    assert_eq!(probe(BackendPolicy::Auto), probe_auto());
}

#[test]
fn hardware_info_converts_to_report_block() {
    let info = probe_auto();
    let expected = info.clone();
    let report: HardwareReport = info.into();
    assert_eq!(report.arch, expected.arch);
    assert_eq!(report.detected_features, expected.detected_features);
    assert_eq!(report.backend_policy, expected.backend_policy);
    assert_eq!(report.blake3_backend_reported, expected.blake3_backend_reported);
    assert_eq!(report.sha256_backend_reported, expected.sha256_backend_reported);
    assert_eq!(report.source, expected.source);
    assert_eq!(report.multithread, expected.multithread);
}
