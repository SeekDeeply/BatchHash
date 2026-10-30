//! Golden diff fixtures for diff schema v1 (project plan §3.7).
//!
//! Every fixture pins ~generated_at~ through [~DiffOptions~] so the JSON is
//! byte-stable. Run ~BLESS_GOLDEN=1 cargo test -p batchhash-report --test
//! diff_golden~ to intentionally regenerate the committed fixtures.

use std::fs;
use std::path::PathBuf;

use batchhash_report::diff::{diff_reports, DiffOptions};
use batchhash_report::format::{export_diff, ReportFormat};
use batchhash_types::{
    AlgorithmId, DiffMode, DiffReport, EntryStatus, HardwareReport, Report, ReportEntry,
    RootAlignment, RootKind,
};
use indexmap::IndexMap;

const LEFT_AT: &str = "2026-01-01T00:00:00Z";
const RIGHT_AT: &str = "2026-01-02T00:00:00Z";
const GENERATED_AT: &str = "2026-10-20T00:00:00Z";

fn digest(algorithm: AlgorithmId, fill: char) -> String {
    fill.to_string().repeat(algorithm.hex_len())
}

fn entry(path: &str, size: u64, hashes: Vec<(AlgorithmId, String)>) -> ReportEntry {
    let mut map = IndexMap::new();
    for (algorithm, value) in hashes {
        map.insert(algorithm, value);
    }
    ReportEntry {
        path: path.to_string(),
        size,
        mtime: None,
        hashes: map,
        status: EntryStatus::Ok,
        error_kind: None,
        error_message: None,
        suffix_applied: None,
        suffix_algorithm: None,
        renamed_to: None,
    }
}

fn report(generated_at: &str, algorithms: Vec<AlgorithmId>, entries: Vec<ReportEntry>) -> Report {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        algorithms,
        HardwareReport::default(),
    );
    report.generated_at = generated_at.to_string();
    report.entries = entries;
    report.recompute_summary();
    report
}

fn options() -> DiffOptions {
    DiffOptions::default()
        .with_generated_at(GENERATED_AT)
        .with_left_label("left.json")
        .with_right_label("right.json")
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

/// Replace the concrete tool version with a placeholder.
///
/// The fixtures pin every deterministic field, but the tool version changes with
/// every release; substituting it keeps the goldens byte-exact for the schema
/// while still failing on any real layout change.
fn normalize_version(text: &str) -> String {
    text.replace(batchhash_types::TOOL_VERSION, "<tool_version>")
}

fn assert_golden(name: &str, diff: &DiffReport) {
    let actual =
        normalize_version(&export_diff(diff, ReportFormat::Json).expect("diff JSON export"));
    let path = golden_path(name);
    if std::env::var("BLESS_GOLDEN").is_ok_and(|value| value == "1") {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create golden directory");
        }
        fs::write(&path, actual.as_bytes()).expect("write golden fixture");
        return;
    }
    let expected = normalize_version(&fs::read_to_string(&path).expect("read golden fixture"));
    assert_eq!(
        actual, expected,
        "golden fixture mismatch for {name}; regenerate with BLESS_GOLDEN=1"
    );
}

#[test]
fn identical_matches_golden_fixture() {
    let left = report(
        LEFT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "a.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
        )],
    );
    let right = report(
        RIGHT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "a.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
        )],
    );
    let diff = diff_reports(&left, &right, &options()).expect("diff");
    assert_golden("identical.json", &diff);
}

#[test]
fn changed_matches_golden_fixture() {
    let left = report(
        LEFT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "a.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
        )],
    );
    let right = report(
        RIGHT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "a.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '1'))],
        )],
    );
    let diff = diff_reports(&left, &right, &options()).expect("diff");
    assert_golden("changed.json", &diff);
}

#[test]
fn added_matches_golden_fixture() {
    let left = report(LEFT_AT, vec![AlgorithmId::Crc32], Vec::new());
    let right = report(
        RIGHT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "new.mkv",
            42,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '2'))],
        )],
    );
    let diff = diff_reports(&left, &right, &options()).expect("diff");
    assert_golden("added.json", &diff);
}

#[test]
fn removed_matches_golden_fixture() {
    let left = report(
        LEFT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "gone.mkv",
            7,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '3'))],
        )],
    );
    let right = report(RIGHT_AT, vec![AlgorithmId::Crc32], Vec::new());
    let diff = diff_reports(&left, &right, &options()).expect("diff");
    assert_golden("removed.json", &diff);
}

#[test]
fn no_algorithm_intersection_matches_golden_fixture() {
    let left = report(
        LEFT_AT,
        vec![AlgorithmId::Md5],
        vec![entry(
            "a.mkv",
            10,
            vec![(AlgorithmId::Md5, digest(AlgorithmId::Md5, 'a'))],
        )],
    );
    let right = report(
        RIGHT_AT,
        vec![AlgorithmId::Sha256],
        vec![entry(
            "a.mkv",
            10,
            vec![(AlgorithmId::Sha256, digest(AlgorithmId::Sha256, 'b'))],
        )],
    );
    let diff = diff_reports(&left, &right, &options()).expect("diff");
    assert_golden("no_algorithm_intersection.json", &diff);
}

#[test]
fn case_difference_matches_golden_fixture() {
    let left = report(
        LEFT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "Movie.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '4'))],
        )],
    );
    let right = report(
        RIGHT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "movie.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '4'))],
        )],
    );
    let diff = diff_reports(&left, &right, &options()).expect("diff");
    assert_golden("case_difference.json", &diff);
}

#[test]
fn basename_ambiguity_matches_golden_fixture() {
    let left = report(
        LEFT_AT,
        vec![AlgorithmId::Crc32],
        vec![
            entry(
                "a/x.mkv",
                10,
                vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
            ),
            entry(
                "b/x.mkv",
                10,
                vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
            ),
        ],
    );
    let right = report(
        RIGHT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "c/x.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
        )],
    );
    let options = options()
        .with_mode(DiffMode::Basename)
        .with_root_alignment(RootAlignment::BasenameOnly);
    let diff = diff_reports(&left, &right, &options).expect("diff");
    assert_golden("basename_ambiguity.json", &diff);
}

#[test]
fn duplicate_key_matches_golden_fixture() {
    let left = report(
        LEFT_AT,
        vec![AlgorithmId::Crc32],
        vec![
            entry(
                "A.mkv",
                10,
                vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
            ),
            entry(
                "a.mkv",
                10,
                vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '1'))],
            ),
        ],
    );
    let right = report(
        RIGHT_AT,
        vec![AlgorithmId::Crc32],
        vec![entry(
            "a.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
        )],
    );
    let diff = diff_reports(&left, &right, &options()).expect("diff");
    assert_golden("duplicate_key.json", &diff);
}

#[test]
fn every_golden_fixture_is_present() {
    for name in [
        "identical.json",
        "changed.json",
        "added.json",
        "removed.json",
        "no_algorithm_intersection.json",
        "case_difference.json",
        "basename_ambiguity.json",
        "duplicate_key.json",
    ] {
        let path = golden_path(name);
        assert!(path.is_file(), "missing golden fixture: {}", path.display());
    }
}
