//! A checksum import records no size, so a diff must not treat zero as real.
//!
//! Before `size_known`, importing an md5sum file and comparing it against the
//! same directory reported every non-empty unchanged file as `Corrupt`: the
//! digests matched, but the imported size was a placeholder zero and the
//! comparison concluded the files disagreed.

use batchhash_report::diff::{diff_reports, DiffOptions};
use batchhash_types::{
    AlgorithmId, DiffStatus, EntryStatus, HardwareReport, Report, ReportEntry, RootKind,
};
use indexmap::IndexMap;

fn entry(path: &str, size: u64, digest: &str) -> ReportEntry {
    let mut hashes = IndexMap::new();
    hashes.insert(AlgorithmId::Md5, digest.to_string());
    ReportEntry {
        path: path.to_string(),
        size,
        size_known: true,
        mtime: None,
        hashes,
        status: EntryStatus::Ok,
        error_kind: None,
        error_message: None,
        suffix_applied: None,
        suffix_algorithm: None,
        renamed_to: None,
    }
}

fn report(entries: Vec<ReportEntry>) -> Report {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        true,
        vec![AlgorithmId::Md5],
        HardwareReport::default(),
    );
    report.entries = entries;
    report.recompute_summary();
    report
}

#[test]
fn an_unknown_size_does_not_make_matching_hashes_look_corrupt() {
    let mut imported = entry("a.txt", 0, "abc123");
    // What a checksum import produces: the digest is known, the size is not.
    imported.size_known = false;
    let real = entry("a.txt", 4096, "abc123");

    let diff = diff_reports(
        &report(vec![imported]),
        &report(vec![real]),
        &DiffOptions::default(),
    )
    .expect("diff");
    assert_eq!(diff.rows.len(), 1);
    assert_eq!(
        diff.rows[0].status,
        DiffStatus::Unchanged,
        "matching digests must win over an unknown size: {}",
        diff.rows[0].reason
    );
}

#[test]
fn a_known_size_mismatch_is_still_reported_as_corrupt() {
    let diff = diff_reports(
        &report(vec![entry("a.txt", 10, "abc123")]),
        &report(vec![entry("a.txt", 20, "abc123")]),
        &DiffOptions::default(),
    )
    .expect("diff");
    assert_eq!(diff.rows[0].status, DiffStatus::Corrupt);
}

#[test]
fn two_unknown_sizes_are_not_claimed_as_unchanged() {
    // With no shared algorithm and no size, there is no evidence at all;
    // "unchanged" would be a guess dressed up as a verdict.
    let mut left = entry("a.txt", 0, "abc123");
    left.size_known = false;
    left.hashes.clear();
    let mut right = entry("a.txt", 0, "abc123");
    right.size_known = false;
    right.hashes.clear();

    let diff = diff_reports(
        &report(vec![left]),
        &report(vec![right]),
        &DiffOptions::default(),
    )
    .expect("diff");
    assert_ne!(diff.rows[0].status, DiffStatus::Unchanged);
}
