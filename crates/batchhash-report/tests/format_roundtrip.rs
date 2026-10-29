//! Report/diff export-import round trips and untrusted-input guards.
//!
//! These tests are the executable form of `docs/report-schema-v1.md`:
//! JSON and CSV export -> import -> export must be byte identical, md5sum and
//! BSD listings must round-trip in both directions, Markdown/TXT must truncate
//! at 500 rows with an explicit note, and imports must reject traversal, future
//! schema versions and files without algorithms.

use batchhash_report::diff::{diff_reports, DiffOptions};
use batchhash_report::format::{
    detect_report_format, export_diff, export_report, import_report, ExportError, ImportError,
    ReportFormat,
};
use batchhash_types::{
    AlgorithmId, BackendPolicy, EntryStatus, ErrorKind, HardwareReport, Report, ReportEntry,
    RootKind, SuffixConfig,
};
use indexmap::IndexMap;

fn digest(algorithm: AlgorithmId, fill: char) -> String {
    fill.to_string().repeat(algorithm.hex_len())
}

fn record(path: impl Into<String>, size: u64, hashes: Vec<(AlgorithmId, String)>) -> ReportEntry {
    let mut map = IndexMap::new();
    for (algorithm, value) in hashes {
        map.insert(algorithm, value);
    }
    ReportEntry {
        path: path.into(),
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

fn sample_hardware() -> HardwareReport {
    HardwareReport {
        arch: "x86_64".to_string(),
        detected_features: vec!["SSE4.1".to_string(), "AVX2".to_string()],
        backend_policy: BackendPolicy::ForcedSoft,
        blake3_backend_reported: "portable".to_string(),
        sha256_backend_reported: "soft".to_string(),
        source: "cpufeatures".to_string(),
        multithread: 4,
    }
}

fn sample_report() -> Report {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        vec![AlgorithmId::Sha256, AlgorithmId::Blake3],
        sample_hardware(),
    );
    report.generated_at = "2026-10-20T00:00:00Z".to_string();

    let mut first = record(
        "a.mkv",
        10,
        vec![
            (AlgorithmId::Sha256, digest(AlgorithmId::Sha256, 'a')),
            (AlgorithmId::Blake3, digest(AlgorithmId::Blake3, 'b')),
        ],
    );
    first.mtime = Some("2026-01-01T00:00:00Z".to_string());

    let mut second = record(
        "b.mkv",
        20,
        vec![
            (AlgorithmId::Sha256, digest(AlgorithmId::Sha256, 'c')),
            (AlgorithmId::Blake3, digest(AlgorithmId::Blake3, 'd')),
        ],
    );
    second.suffix_applied = Some("ABCDE".to_string());
    second.suffix_algorithm = Some(AlgorithmId::Blake3);
    second.renamed_to = Some("b[ABCDE].mkv".to_string());

    report.entries = vec![first, second];
    report.recompute_summary();
    report
}

fn checksum_report(algorithm: AlgorithmId, files: &[(&str, char)]) -> Report {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        vec![algorithm],
        HardwareReport::default(),
    );
    report.generated_at = "2026-10-20T00:00:00Z".to_string();
    report.entries = files
        .iter()
        .map(|(path, fill)| record(*path, 0, vec![(algorithm, digest(algorithm, *fill))]))
        .collect();
    report.recompute_summary();
    report
}

fn big_report(count: usize) -> Report {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        vec![AlgorithmId::Crc32],
        HardwareReport::default(),
    );
    report.generated_at = "2026-10-20T00:00:00Z".to_string();
    report.entries = (0..count)
        .map(|index| {
            record(
                format!("f{:04}.mkv", index),
                index as u64,
                vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '0'))],
            )
        })
        .collect();
    report.recompute_summary();
    report
}

#[test]
fn json_round_trip_is_byte_identical() {
    let report = sample_report();
    let exported = export_report(&report, ReportFormat::Json).expect("JSON export");
    assert!(exported.ends_with('\n'));
    let imported = import_report(exported.as_bytes(), ReportFormat::Json).expect("JSON import");
    assert_eq!(imported, report);
    let again = export_report(&imported, ReportFormat::Json).expect("JSON re-export");
    assert_eq!(exported, again);
}

#[test]
fn csv_round_trip_is_byte_identical() {
    let report = sample_report();
    let exported = export_report(&report, ReportFormat::Csv).expect("CSV export");
    assert!(
        exported.starts_with(
            "path,size,mtime,status,error_kind,suffix_applied,suffix_algorithm,renamed_to,sha256,blake3\n"
        ),
        "canonical CSV header: {exported:?}"
    );
    let imported = import_report(exported.as_bytes(), ReportFormat::Csv).expect("CSV import");
    assert_eq!(imported.root, "");
    assert_eq!(imported.root_kind, RootKind::Fs);
    assert!(!imported.case_sensitive);
    assert_eq!(imported.generated_at, "");
    assert_eq!(imported.suffix_config, SuffixConfig::default());
    assert_eq!(
        imported.algorithms,
        vec![AlgorithmId::Sha256, AlgorithmId::Blake3]
    );
    assert_eq!(imported.entries, report.entries);
    let again = export_report(&imported, ReportFormat::Csv).expect("CSV re-export");
    assert_eq!(exported, again);
}

#[test]
fn csv_preserves_status_and_suffix_metadata() {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        vec![AlgorithmId::Crc32],
        HardwareReport::default(),
    );
    report.generated_at = "2026-10-20T00:00:00Z".to_string();
    let mut failed = record("bad.mkv", 0, Vec::new());
    failed.status = EntryStatus::Failed;
    failed.error_kind = Some(ErrorKind::PermissionDenied);
    let mut ok = record(
        "good.mkv",
        5,
        vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, 'a'))],
    );
    ok.suffix_applied = Some("ABCDE".to_string());
    ok.suffix_algorithm = Some(AlgorithmId::Crc32);
    report.entries = vec![failed, ok];
    report.recompute_summary();

    let exported = export_report(&report, ReportFormat::Csv).expect("CSV export");
    let imported = import_report(exported.as_bytes(), ReportFormat::Csv).expect("CSV import");
    assert_eq!(imported.entries[0].status, EntryStatus::Failed);
    assert_eq!(
        imported.entries[0].error_kind,
        Some(ErrorKind::PermissionDenied)
    );
    assert_eq!(imported.entries[1].suffix_applied.as_deref(), Some("ABCDE"));
    assert_eq!(
        imported.entries[1].suffix_algorithm,
        Some(AlgorithmId::Crc32)
    );
    let again = export_report(&imported, ReportFormat::Csv).expect("CSV re-export");
    assert_eq!(exported, again);
}

#[test]
fn md5sum_round_trips_in_both_directions() {
    let report = checksum_report(AlgorithmId::Sha1, &[("a.mkv", 'a'), ("b.mkv", 'b')]);
    let exported = export_report(&report, ReportFormat::Md5sum).expect("md5sum export");
    assert!(exported.ends_with('\n'));
    let imported = import_report(exported.as_bytes(), ReportFormat::Md5sum).expect("md5sum import");
    assert_eq!(imported.algorithms, vec![AlgorithmId::Sha1]);
    assert_eq!(imported.entries.len(), 2);
    assert_eq!(imported.entries[0].path, "a.mkv");
    assert_eq!(
        imported.entries[0].hashes.get(&AlgorithmId::Sha1),
        Some(&digest(AlgorithmId::Sha1, 'a'))
    );
    let again = export_report(&imported, ReportFormat::Md5sum).expect("md5sum re-export");
    assert_eq!(exported, again);
}

#[test]
fn bsd_round_trips_legacy_and_registry_tags() {
    for algorithm in [
        AlgorithmId::Md5,
        AlgorithmId::Sha1,
        AlgorithmId::Sha256,
        AlgorithmId::Sha512,
        AlgorithmId::Blake3,
    ] {
        let report = checksum_report(algorithm, &[("a.mkv", 'a')]);
        let exported = export_report(&report, ReportFormat::Bsd).expect("BSD export");
        assert!(
            exported.contains(" ("),
            "BSD output must carry an algorithm tag: {exported:?}"
        );
        let imported = import_report(exported.as_bytes(), ReportFormat::Bsd).expect("BSD import");
        assert_eq!(imported.algorithms, vec![algorithm]);
        let again = export_report(&imported, ReportFormat::Bsd).expect("BSD re-export");
        assert_eq!(exported, again);
    }
}

#[test]
fn ambiguous_digest_lengths_are_rejected() {
    let line = format!("{}  a.mkv\n", "a".repeat(32));
    let error = import_report(line.as_bytes(), ReportFormat::Md5sum).unwrap_err();
    assert!(matches!(
        error,
        ImportError::AmbiguousAlgorithm { hex_len: 32, .. }
    ));

    let bsd = format!("UNKNOWN (a.mkv) = {}\n", "a".repeat(32));
    let error = import_report(bsd.as_bytes(), ReportFormat::Bsd).unwrap_err();
    assert!(matches!(
        error,
        ImportError::AmbiguousAlgorithm { hex_len: 32, .. }
    ));

    let unknown = format!("{}  a.mkv\n", "a".repeat(63));
    let error = import_report(unknown.as_bytes(), ReportFormat::Md5sum).unwrap_err();
    assert!(matches!(
        error,
        ImportError::UnknownAlgorithm { hex_len: 63 }
    ));
}

#[test]
fn checksum_formats_require_a_single_algorithm() {
    let report = sample_report();
    assert!(matches!(
        export_report(&report, ReportFormat::Md5sum),
        Err(ExportError::RequiresSingleAlgorithm { count: 2 })
    ));
    assert!(matches!(
        export_report(&report, ReportFormat::Bsd),
        Err(ExportError::RequiresSingleAlgorithm { count: 2 })
    ));
}

#[test]
fn markdown_and_txt_truncate_at_five_hundred_rows() {
    let report = big_report(501);
    let markdown = export_report(&report, ReportFormat::Markdown).expect("Markdown export");
    assert!(markdown.contains("Truncated: showing 500 of 501 entries (truncated=true)."));
    assert_eq!(
        markdown
            .lines()
            .filter(|line| line.starts_with("| f"))
            .count(),
        500
    );

    let txt = export_report(&report, ReportFormat::Txt).expect("TXT export");
    assert!(txt.contains("Truncated: showing 500 of 501 entries (truncated=true)"));
    assert_eq!(
        txt.lines().filter(|line| line.starts_with("path")).count(),
        500
    );

    let json = export_report(&report, ReportFormat::Json).expect("JSON export");
    assert_eq!(json.matches("\"path\": \"").count(), 501);

    let csv = export_report(&report, ReportFormat::Csv).expect("CSV export");
    assert_eq!(csv.lines().count(), 502);
}

#[test]
fn import_rejects_path_traversal_and_future_schema() {
    let report = sample_report();
    let json = export_report(&report, ReportFormat::Json).expect("JSON export");

    let traversal = json.replace("\"a.mkv\"", "\"../a.mkv\"");
    assert_ne!(traversal, json);
    let error = import_report(traversal.as_bytes(), ReportFormat::Json).unwrap_err();
    assert!(matches!(error, ImportError::Path { .. }));

    let future = json.replace("\"schema_version\": 1,", "\"schema_version\": 2,");
    assert!(future.contains("\"schema_version\": 2"));
    let error = import_report(future.as_bytes(), ReportFormat::Json).unwrap_err();
    assert!(matches!(
        error,
        ImportError::FutureSchema {
            found: 2,
            supported: 1
        }
    ));

    let csv = "path,size,status,crc32\n../escape.mkv,10,ok,00000000\n";
    let error = import_report(csv.as_bytes(), ReportFormat::Csv).unwrap_err();
    assert!(matches!(error, ImportError::Path { .. }));

    let no_algorithms = "path,size\nx.mkv,10\n";
    let error = import_report(no_algorithms.as_bytes(), ReportFormat::Csv).unwrap_err();
    assert!(matches!(error, ImportError::NoAlgorithms));
}

#[test]
fn unsupported_import_formats_are_rejected() {
    for format in [ReportFormat::Markdown, ReportFormat::Txt] {
        let error = import_report(b"", format).unwrap_err();
        assert!(matches!(
            error,
            ImportError::UnsupportedFormat(ReportFormat::Markdown | ReportFormat::Txt)
        ));
    }
}

#[test]
fn diff_export_formats_are_stable() {
    let left = sample_report();
    let right = sample_report();
    let options = DiffOptions::default().with_generated_at("2026-10-20T00:00:00Z");
    let diff = diff_reports(&left, &right, &options).expect("diff");
    assert_eq!(diff.summary.unchanged, 2);

    let json = export_diff(&diff, ReportFormat::Json).expect("diff JSON");
    assert!(json.ends_with('\n'));
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid diff JSON");
    assert_eq!(value["rows"][0]["status"].as_str(), Some("unchanged"));

    let csv = export_diff(&diff, ReportFormat::Csv).expect("diff CSV");
    assert!(csv.starts_with("key,status,reason,algorithms_compared,changed_algorithms,"));
    assert!(csv.contains("unchanged"));

    let markdown = export_diff(&diff, ReportFormat::Markdown).expect("diff Markdown");
    assert!(markdown.starts_with("# BatchHash Diff"));
    assert!(markdown.contains("unchanged"));

    let txt = export_diff(&diff, ReportFormat::Txt).expect("diff TXT");
    assert!(txt.starts_with("BatchHash Diff"));
    assert!(txt.contains("unchanged"));

    let error = export_diff(&diff, ReportFormat::Md5sum).unwrap_err();
    assert!(matches!(
        error,
        ExportError::UnsupportedDiffFormat(ReportFormat::Md5sum)
    ));
}

#[test]
fn format_detection_prefers_content_then_extension() {
    let report = sample_report();
    let json = export_report(&report, ReportFormat::Json).expect("JSON export");
    assert_eq!(
        detect_report_format("upload.bin", json.as_bytes()),
        Some(ReportFormat::Json)
    );

    let csv = export_report(&report, ReportFormat::Csv).expect("CSV export");
    assert_eq!(
        detect_report_format("report.csv", csv.as_bytes()),
        Some(ReportFormat::Csv)
    );

    let markdown = export_report(&report, ReportFormat::Markdown).expect("Markdown export");
    assert_eq!(
        detect_report_format("report.txt", markdown.as_bytes()),
        Some(ReportFormat::Markdown)
    );

    let txt = export_report(&report, ReportFormat::Txt).expect("TXT export");
    assert_eq!(
        detect_report_format("report.txt", txt.as_bytes()),
        Some(ReportFormat::Txt)
    );

    let checksum = checksum_report(AlgorithmId::Sha1, &[("a.mkv", 'a')]);
    let md5sum = export_report(&checksum, ReportFormat::Md5sum).expect("md5sum export");
    assert_eq!(
        detect_report_format("checksums.txt", md5sum.as_bytes()),
        Some(ReportFormat::Md5sum)
    );

    let bsd = export_report(&checksum, ReportFormat::Bsd).expect("BSD export");
    assert_eq!(
        detect_report_format("checksums.txt", bsd.as_bytes()),
        Some(ReportFormat::Bsd)
    );

    assert_eq!(
        detect_report_format("report.json", b""),
        Some(ReportFormat::Json)
    );
    assert_eq!(detect_report_format("mystery.bin", b""), None);
}
