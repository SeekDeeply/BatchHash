//! Report/diff export-import round trips and untrusted-input guards.
//!
//! These tests are the executable form of `docs/report-schema-v1.md`:
//! JSON and CSV export -> import -> export must be byte identical, md5sum and
//! BSD listings must round-trip in both directions, Markdown/TXT must truncate
//! at 500 rows with an explicit note, and imports must reject traversal, future
//! schema versions and files without algorithms.

use batchhash_report::diff::{diff_reports, DiffOptions};
use batchhash_report::format::{
    detect_report_format, export_diff, export_diff_with, export_report, export_report_with,
    import_report, ExportError, ImportError, ReportFormat, MAX_TABLE_ROWS,
};
use batchhash_report::labels::ExportLocale;
use batchhash_types::{
    AlgorithmId, BackendPolicy, DiffStatus, EntryStatus, ErrorKind, HardwareReport, Report,
    ReportEntry, ReportValidationError, RootKind, SuffixConfig,
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
    // Tiger is the only registered algorithm whose hex digest is 48 characters,
    // so a bare checksum of that width is unambiguous. Widths shared by several
    // algorithms (32, 40, 64, ...) are covered by the ambiguity tests instead.
    let report = checksum_report(AlgorithmId::Tiger, &[("a.mkv", 'a'), ("b.mkv", 'b')]);
    let exported = export_report(&report, ReportFormat::Md5sum).expect("md5sum export");
    assert!(exported.ends_with('\n'));
    let imported = import_report(exported.as_bytes(), ReportFormat::Md5sum).expect("md5sum import");
    assert_eq!(imported.algorithms, vec![AlgorithmId::Tiger]);
    assert_eq!(imported.entries.len(), 2);
    assert_eq!(imported.entries[0].path, "a.mkv");
    assert_eq!(
        imported.entries[0].hashes.get(&AlgorithmId::Tiger),
        Some(&digest(AlgorithmId::Tiger, 'a'))
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
    // Tree 也在此列：它是渲染结果，反解析要处理"文件名含树线"和"树被截断"
    // 两类歧义，因此和 Markdown/TXT 一样只导出不导入。
    for format in [
        ReportFormat::Markdown,
        ReportFormat::Txt,
        ReportFormat::Tree,
    ] {
        let error = import_report(b"", format).unwrap_err();
        assert!(
            matches!(error, ImportError::UnsupportedFormat(_)),
            "{format}: {error}"
        );
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

#[test]
fn csv_import_rejects_unknown_columns() {
    // A header that is neither metadata nor a registered algorithm would be
    // dropped silently, so the import must fail instead of losing data.
    let unknown = "path,size,status,note,crc32\nx.mkv,10,ok,hello,00000000\n";
    let error = import_report(unknown.as_bytes(), ReportFormat::Csv).unwrap_err();
    assert!(matches!(
        error,
        ImportError::UnknownColumn { ref column } if column == "note"
    ));

    // Metadata columns are matched case-insensitively; algorithm columns must
    // use the registered stable id, which is lowercase by contract.
    let known = "SIZE,Path,crc32,Generated_At\n10,x.mkv,00000000,2026-10-20T00:00:00Z\n";
    let report = import_report(known.as_bytes(), ReportFormat::Csv).expect("known columns only");
    assert_eq!(report.algorithms, vec![AlgorithmId::Crc32]);
    assert_eq!(report.entries.len(), 1);
}

#[test]
fn imports_reject_rows_a_later_export_would_refuse() {
    // Ok entries must carry a digest: an export of such a report would be
    // rejected by validate(), so the importer rejects it first.
    let no_digest = "path,size,status,crc32\nx.mkv,10,ok,\n";
    let error = import_report(no_digest.as_bytes(), ReportFormat::Csv).unwrap_err();
    assert!(matches!(
        error,
        ImportError::InvalidReport(ReportValidationError::OkEntryWithoutHashes(_))
    ));

    // Duplicate paths cannot round-trip through the canonical sorted export.
    let duplicate = "path,size,status,crc32\nx.mkv,10,ok,00000000\nx.mkv,10,ok,11111111\n";
    let error = import_report(duplicate.as_bytes(), ReportFormat::Csv).unwrap_err();
    assert!(matches!(
        error,
        ImportError::InvalidReport(ReportValidationError::DuplicatePath(_))
    ));

    // Checksum imports go through the same final validation step. 48 hex chars
    // are unique to Tiger, so only the duplicate path can fail here.
    let checksum_duplicate = format!("{}  x.mkv\n{}  x.mkv\n", "a".repeat(48), "b".repeat(48));
    let error = import_report(checksum_duplicate.as_bytes(), ReportFormat::Md5sum).unwrap_err();
    assert!(matches!(
        error,
        ImportError::InvalidReport(ReportValidationError::DuplicatePath(_))
    ));
}

#[test]
fn json_import_rejects_a_forged_summary() {
    let report = sample_report();
    let json = export_report(&report, ReportFormat::Json).expect("JSON export");
    let mut value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    value["summary"]["total"] = serde_json::json!(99);
    let forged = serde_json::to_string(&value).expect("serialize");
    let error = import_report(forged.as_bytes(), ReportFormat::Json).unwrap_err();
    assert!(matches!(
        error,
        ImportError::InvalidReport(ReportValidationError::SummaryMismatch {
            field: "total",
            actual: 99,
            ..
        })
    ));
}

fn partial_hash_report(missing_blake3: bool) -> Report {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        vec![AlgorithmId::Sha256, AlgorithmId::Blake3],
        HardwareReport::default(),
    );
    report.generated_at = "2026-10-20T00:00:00Z".to_string();
    report.entries = vec![record(
        "a.mkv",
        10,
        vec![(AlgorithmId::Sha256, digest(AlgorithmId::Sha256, 'a'))],
    )];
    if !missing_blake3 {
        report.entries[0]
            .hashes
            .insert(AlgorithmId::Blake3, digest(AlgorithmId::Blake3, 'b'));
    }
    report.recompute_summary();
    report
}

#[test]
fn diff_reports_incomplete_hashes_as_unknown() {
    let options = DiffOptions::default().with_generated_at("2026-10-20T00:00:00Z");

    // Only one side carries blake3: the blake3 comparison never happened, so
    // the row must not claim to be unchanged.
    let left = partial_hash_report(false);
    let right = partial_hash_report(true);
    let diff = diff_reports(&left, &right, &options).expect("valid reports");
    assert_eq!(diff.rows.len(), 1);
    assert_eq!(diff.rows[0].status, DiffStatus::Unknown);
    assert!(
        diff.rows[0].reason.contains("blake3"),
        "reason names the missing algorithm: {}",
        diff.rows[0].reason
    );
    assert_eq!(diff.rows[0].algorithms_compared, vec![AlgorithmId::Sha256]);
    assert_eq!(diff.summary.unknown, 1);
    assert_eq!(diff.summary.unchanged, 0);

    // Both sides declare sha256 + blake3 but only carry sha256: same verdict.
    let left = partial_hash_report(true);
    let right = partial_hash_report(true);
    let diff = diff_reports(&left, &right, &options).expect("valid reports");
    assert_eq!(diff.rows[0].status, DiffStatus::Unknown);
    assert_eq!(diff.summary.unchanged, 0);

    // With both digests present and matching, the row is genuinely unchanged.
    let left = partial_hash_report(false);
    let right = partial_hash_report(false);
    let diff = diff_reports(&left, &right, &options).expect("valid reports");
    assert_eq!(diff.rows[0].status, DiffStatus::Unchanged);
    assert_eq!(
        diff.rows[0].algorithms_compared,
        vec![AlgorithmId::Sha256, AlgorithmId::Blake3]
    );
    assert_eq!(diff.summary.unchanged, 1);
}

// ---------------------------------------------------------------------------
// 语言化与文件树（task-24）
// ---------------------------------------------------------------------------

const ZH_MARKDOWN_BITS: [&str; 5] = ["# BatchHash 报告", "## 汇总", "## 条目", "生成时间", "路径"];

#[test]
fn markdown_export_is_localized_by_locale() {
    let report = sample_report();

    let en = export_report_with(&report, ReportFormat::Markdown, ExportLocale::En)
        .expect("English Markdown");
    let zh = export_report_with(&report, ReportFormat::Markdown, ExportLocale::Zh)
        .expect("Chinese Markdown");

    // 英文输出必须与历史 export_report 完全一致：默认语言不能悄悄改变输出。
    let historical = export_report(&report, ReportFormat::Markdown).expect("Markdown export");
    assert_eq!(en, historical);

    assert!(en.starts_with("# BatchHash Report\n"));
    assert!(en.contains("## Summary"));
    assert!(en.contains("## Entries"));
    assert!(en.contains("Generated at"));
    for bit in ZH_MARKDOWN_BITS {
        assert!(zh.contains(bit), "missing {bit} in:\n{zh}");
    }
    // 列名确实不同，而不是只换了标题。
    assert!(en.contains("| Path |") || en.contains(" Path | Size |"));
    assert!(
        !zh.contains("## Summary"),
        "zh must not keep the English section"
    );
    assert_ne!(en, zh);
}

#[test]
fn txt_export_is_localized_and_keeps_machine_formats_english() {
    let report = sample_report();

    let en = export_report_with(&report, ReportFormat::Txt, ExportLocale::En).expect("English TXT");
    let zh = export_report_with(&report, ReportFormat::Txt, ExportLocale::Zh).expect("Chinese TXT");
    assert_eq!(
        en,
        export_report(&report, ReportFormat::Txt).expect("TXT export")
    );
    assert!(en.starts_with("BatchHash Report\n"));
    assert!(en.contains("\nSummary\n"));
    assert!(zh.contains("BatchHash 报告"));
    assert!(zh.contains("汇总"));

    // JSON / CSV 是机器契约：无论 locale 是什么，输出都必须逐字节相同。
    for format in [ReportFormat::Json, ReportFormat::Csv] {
        let with_en = export_report_with(&report, format, ExportLocale::En).expect("en");
        let with_zh = export_report_with(&report, format, ExportLocale::Zh).expect("zh");
        assert_eq!(
            with_en, with_zh,
            "{format} must stay byte-identical across locales"
        );
    }
}

#[test]
fn diff_export_is_localized_by_locale() {
    let left = sample_report();
    let right = sample_report();
    let options = DiffOptions::default().with_generated_at("2026-10-20T00:00:00Z");
    let diff = diff_reports(&left, &right, &options).expect("diff");

    let en = export_diff_with(&diff, ReportFormat::Markdown, ExportLocale::En).expect("en diff");
    let zh = export_diff_with(&diff, ReportFormat::Markdown, ExportLocale::Zh).expect("zh diff");
    assert_eq!(
        en,
        export_diff(&diff, ReportFormat::Markdown).expect("diff")
    );
    assert!(en.starts_with("# BatchHash Diff\n"));
    assert!(zh.contains("BatchHash 对比"));
    assert!(zh.contains("对比明细"));

    // Tree 不能表达对比文档。
    let error = export_diff_with(&diff, ReportFormat::Tree, ExportLocale::Zh).unwrap_err();
    assert!(matches!(
        error,
        ExportError::UnsupportedDiffFormat(ReportFormat::Tree)
    ));
}

#[test]
fn tree_renders_structure_with_ascii_branches() {
    let mut report = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        vec![AlgorithmId::Crc32],
        HardwareReport::default(),
    );
    report.generated_at = "2026-10-20T00:00:00Z".to_string();
    report.entries = vec![
        record(
            "b.mkv",
            30,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '3'))],
        ),
        record(
            "sub/z.mkv",
            10,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '1'))],
        ),
        record(
            "sub/a.mkv",
            20,
            vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '2'))],
        ),
    ];
    report.recompute_summary();

    let tree =
        export_report_with(&report, ReportFormat::Tree, ExportLocale::En).expect("tree export");
    let lines: Vec<&str> = tree.lines().collect();

    // 目录在前、同级按名称升序：b.mkv（文件）排在 sub/（目录）之后。
    assert_eq!(lines[0], "BatchHash Report");
    assert_eq!(lines[1], "|- sub");
    assert_eq!(lines[2], "|  |- a.mkv  crc32:22222222");
    assert_eq!(lines[3], "|  \\- z.mkv  crc32:11111111");
    assert_eq!(lines[4], "\\- b.mkv  crc32:33333333");

    // 树后用同一套标签渲染汇总。
    assert!(tree.contains("\nSummary:\n"));
    assert!(tree.contains("  Total: 3"));
    assert!(tree.contains("  Total bytes: 60"));

    // 多算法时按报告顺序输出 algo:hash，两个空格分隔。
    let mut multi = Report::new(
        "/data/media",
        RootKind::Fs,
        false,
        vec![AlgorithmId::Sha256, AlgorithmId::Blake3],
        HardwareReport::default(),
    );
    multi.generated_at = "2026-10-20T00:00:00Z".to_string();
    multi.entries = vec![record(
        "a.mkv",
        10,
        vec![
            (AlgorithmId::Sha256, digest(AlgorithmId::Sha256, 'a')),
            (AlgorithmId::Blake3, digest(AlgorithmId::Blake3, 'b')),
        ],
    )];
    multi.recompute_summary();
    let tree = export_report_with(&multi, ReportFormat::Tree, ExportLocale::Zh).expect("tree");
    assert!(tree.contains("BatchHash 报告"));
    let expected = format!(
        "sha256:{}  blake3:{}",
        digest(AlgorithmId::Sha256, 'a'),
        digest(AlgorithmId::Blake3, 'b')
    );
    assert!(tree.contains(&expected), "tree:\n{tree}");
    assert!(tree.contains("汇总:"));
}

#[test]
fn tree_truncates_at_five_hundred_rows() {
    let report = big_report(MAX_TABLE_ROWS + 1);
    let tree = export_report(&report, ReportFormat::Tree).expect("tree export");
    assert!(tree.contains("Truncated: showing 500 of 501 entries (truncated=true)."));
    // 平铺目录下最多渲染 500 个文件行。
    let file_lines = tree.lines().filter(|line| line.contains("crc32:")).count();
    assert_eq!(file_lines, MAX_TABLE_ROWS);
}

#[test]
fn zz_print_tree_samples() {
    for locale in [ExportLocale::En, ExportLocale::Zh] {
        let mut report = Report::new(
            "/data/media",
            RootKind::Fs,
            false,
            vec![AlgorithmId::Crc32],
            HardwareReport::default(),
        );
        report.generated_at = "2026-10-20T00:00:00Z".to_string();
        let mut failed = record("sub/nested/bad.mkv", 5, vec![]);
        failed.status = EntryStatus::Failed;
        failed.error_kind = Some(ErrorKind::PermissionDenied);
        report.entries = vec![
            record("b.mkv", 30, vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '3'))]),
            record("sub/z.mkv", 10, vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '1'))]),
            record("sub/nested/a.mkv", 20, vec![(AlgorithmId::Crc32, digest(AlgorithmId::Crc32, '2'))]),
            failed,
        ];
        report.recompute_summary();
        println!("=== {} ===", locale.as_str());
        println!("{}", export_report_with(&report, ReportFormat::Tree, locale).unwrap());
    }
}
