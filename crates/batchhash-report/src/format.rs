//! Report export/import formats (report schema v1, project plan §3.6).
//!
//! Exports: canonical JSON, RFC 4180 CSV, Markdown, aligned TXT, md5sum, BSD
//! checksum listings and the human-oriented file tree. Imports: JSON, CSV,
//! md5sum and BSD — all untrusted input, so sizes, entry counts, schema
//! versions and relative paths are validated before a [`Report`] is handed
//! back to the application.
//!
//! # 语言化边界
//!
//! Markdown、TXT 和文件树是**给人看**的，接受 [`ExportLocale`]；JSON、CSV、
//! md5sum、BSD 是**机器契约**，键名与列名恒定英文，不参与语言化。理由：
//! 这些格式的输出会被 `md5sum -c`、CI 脚本和 FFI/Dart 解析器按固定键名读取，
//! 翻译任何一个键都是对下游的破坏性变更。
//!
//! JSON and CSV exports are canonical: entries are sorted by path, algorithm
//! columns follow the registry order and JSON hashes follow the report's
//! declared algorithm order. Exporting an imported JSON/CSV report therefore
//! reproduces the input bytes exactly (see the round-trip tests).

use std::fmt;
use std::path::Path;

use batchhash_types::{
    is_hex, validate_relative_path, AlgorithmId, EntryStatus, ErrorKind, HardwareReport, Report,
    ReportEntry, ReportValidationError, RootKind, MAX_IMPORT_BYTES, MAX_IMPORT_ENTRIES,
    REPORT_SCHEMA_VERSION,
};
use indexmap::IndexMap;

use crate::labels::{
    DiffMetaField, DiffSummaryField, EntryField, EnvironmentField, ExportLocale, LabelStyle,
    Labels, MetaField, SummaryField, TraceField,
};

/// Maximum number of entries rendered by Markdown and TXT table exports.
///
/// JSON and CSV never truncate; Markdown/TXT cap the table and append an
/// explicit `truncated=true` note so the omission is never silent.
pub const MAX_TABLE_ROWS: usize = 500;

/// Supported report formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReportFormat {
    /// Canonical pretty JSON (report schema v1).
    Json,
    /// RFC 4180 CSV with a fixed header plus one column per algorithm.
    Csv,
    /// Human-readable Markdown tables.
    Markdown,
    /// Human-readable aligned plain text.
    Txt,
    /// Coreutils `md5sum` style listing (hex + two spaces + path).
    Md5sum,
    /// BSD `ALGO (path) = hex` checksum listing.
    Bsd,
    /// 给人看的缩进文件树；只能导出，不能导入，也不代表对比文档。
    ///
    /// 它存在的理由是可读性：Checksum 列表丢掉了目录结构，表格在深目录下
    /// 也难以扫读。注意它**不是**可再导入的格式，导入端会直接拒绝。
    Tree,
}

impl ReportFormat {
    /// Every supported format, in menu order.
    ///
    /// Tree 排在最后：它是最"人类"的一项，前面的顺序是历史菜单顺序，
    /// 改动会让设置里保存的下标错位。
    pub const ALL: [ReportFormat; 7] = [
        ReportFormat::Json,
        ReportFormat::Csv,
        ReportFormat::Markdown,
        ReportFormat::Txt,
        ReportFormat::Md5sum,
        ReportFormat::Bsd,
        ReportFormat::Tree,
    ];

    /// Stable identifier used in diagnostics and settings.
    pub const fn as_str(self) -> &'static str {
        match self {
            ReportFormat::Json => "json",
            ReportFormat::Csv => "csv",
            ReportFormat::Markdown => "markdown",
            ReportFormat::Txt => "txt",
            ReportFormat::Md5sum => "md5sum",
            ReportFormat::Bsd => "bsd",
            ReportFormat::Tree => "tree",
        }
    }

    /// Conventional lowercase file extension (without the dot).
    ///
    /// Tree 复用 `txt`：它同样是人读纯文本，给它单独的扩展名只会让文件管理器
    /// 无法双击打开。检测时靠内容（树线）而非扩展名区分。
    pub const fn extension(self) -> &'static str {
        match self {
            ReportFormat::Json => "json",
            ReportFormat::Csv => "csv",
            ReportFormat::Markdown => "md",
            ReportFormat::Txt => "txt",
            ReportFormat::Md5sum => "md5",
            ReportFormat::Bsd => "bsd",
            ReportFormat::Tree => "txt",
        }
    }

    /// Whether [`import_report`] accepts this format.
    ///
    /// Tree 不可导入：它是渲染结果而非数据载体，反解析只会引入歧义
    /// （文件名里含 `|- ` 怎么办、被截断的树怎么区分）。需要可导入的
    /// 结构化输出请用 JSON 或 CSV。
    pub const fn can_import(self) -> bool {
        matches!(
            self,
            ReportFormat::Json | ReportFormat::Csv | ReportFormat::Md5sum | ReportFormat::Bsd
        )
    }

    /// Whether [`export_diff`] can represent a diff document in this format.
    ///
    /// Tree 不在此列：树表达的是"一份报告的目录结构"，而对比是两份报告的
    /// 逐行连接结果，强行塞进树里只会制造没人能解析的混合格式。
    pub const fn can_diff(self) -> bool {
        matches!(
            self,
            ReportFormat::Json | ReportFormat::Csv | ReportFormat::Markdown | ReportFormat::Txt
        )
    }

    /// Whether this format is localized for human readers.
    ///
    /// 机器格式（JSON / CSV / md5sum / BSD）恒为 `false`，这是契约。
    pub const fn is_localized(self) -> bool {
        matches!(
            self,
            ReportFormat::Markdown | ReportFormat::Txt | ReportFormat::Tree
        )
    }
}

impl fmt::Display for ReportFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Failures while serializing a report or diff document.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The requested format cannot represent the requested document.
    #[error("format {0} cannot represent a report")]
    UnsupportedFormat(ReportFormat),
    /// The requested format cannot represent a diff document.
    #[error("format {0} cannot represent a diff")]
    UnsupportedDiffFormat(ReportFormat),
    /// The report itself is invalid and must not be exported.
    #[error("report is invalid: {0}")]
    InvalidReport(#[from] ReportValidationError),
    /// JSON serialization failed.
    #[error("failed to serialize JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// CSV serialization failed.
    #[error("failed to write CSV: {0}")]
    Csv(String),
    /// This format has no safe representation for the path.
    #[error("path cannot be represented in this format: {0}")]
    UnencodablePath(String),
    /// Checksum formats hold exactly one algorithm per file.
    #[error("format requires exactly one algorithm, report declares {count}")]
    RequiresSingleAlgorithm {
        /// Number of algorithms the report declares.
        count: usize,
    },
}

/// Failures while parsing an untrusted report file.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The format has no importer.
    #[error("format {0} cannot be imported")]
    UnsupportedFormat(ReportFormat),
    /// The byte cap was exceeded.
    #[error("input is {size} bytes, maximum is {max}")]
    TooLarge {
        /// Input size in bytes.
        size: u64,
        /// Configured maximum.
        max: u64,
    },
    /// The entry cap was exceeded.
    #[error("report has {count} entries, maximum is {max}")]
    TooManyEntries {
        /// Number of entries found.
        count: usize,
        /// Configured maximum.
        max: usize,
    },
    /// Schema version zero is reserved and never valid.
    #[error("unsupported report schema_version {0}")]
    UnsupportedSchemaVersion(u32),
    /// The file was produced by a newer BatchHash.
    #[error("report schema_version {found} is newer than supported {supported}")]
    FutureSchema {
        /// Version found in the file.
        found: u32,
        /// Highest version this crate supports.
        supported: u32,
    },
    /// JSON parsing failed.
    #[error("invalid JSON: {0}")]
    Json(String),
    /// CSV parsing failed.
    #[error("invalid CSV: {0}")]
    Csv(String),
    /// The input is not valid UTF-8 text where text was expected.
    #[error("input is not valid UTF-8: {0}")]
    Utf8(String),
    /// An imported entry would escape the report root.
    #[error("invalid entry path {path}: {source}")]
    Path {
        /// Offending path.
        path: String,
        /// Why the path is rejected.
        source: batchhash_types::PathError,
    },
    /// An entry status is not a known value.
    #[error("invalid entry status: {0}")]
    InvalidStatus(String),
    /// An error kind is not a known value.
    #[error("invalid error kind: {0}")]
    InvalidErrorKind(String),
    /// A size column is not an unsigned integer.
    #[error("invalid size: {0}")]
    InvalidSize(String),
    /// An algorithm ID is not registered.
    #[error("unknown algorithm id: {0}")]
    InvalidAlgorithm(String),
    /// A digest is not hex or has the wrong length for its algorithm.
    #[error("invalid digest for {path} ({algorithm})")]
    InvalidDigest {
        /// Entry path.
        path: String,
        /// Algorithm whose digest is malformed.
        algorithm: AlgorithmId,
    },
    /// A checksum listing line is malformed.
    #[error("malformed line {line}: {reason}")]
    MalformedLine {
        /// 1-based line number.
        line: usize,
        /// What was wrong.
        reason: String,
    },
    /// A digest length maps to more than one registered algorithm.
    #[error("hex digest length {hex_len} is ambiguous: {candidates:?}")]
    AmbiguousAlgorithm {
        /// Digest length in hex characters.
        hex_len: usize,
        /// Registered algorithms with that length.
        candidates: Vec<AlgorithmId>,
    },
    /// A digest length maps to no registered algorithm.
    #[error("hex digest length {hex_len} does not match any registered algorithm")]
    UnknownAlgorithm {
        /// Digest length in hex characters.
        hex_len: usize,
    },
    /// A checksum listing mixes algorithms, which one report cannot represent.
    #[error("file mixes algorithms {first} and {other}")]
    MixedAlgorithms {
        /// First algorithm seen.
        first: AlgorithmId,
        /// Conflicting algorithm.
        other: AlgorithmId,
    },
    /// A CSV file has no algorithm columns at all.
    #[error("report lists no algorithms")]
    NoAlgorithms,
    /// A CSV header column is neither known metadata nor a registered algorithm.
    ///
    /// Silently ignoring such a column would drop data the importer did not
    /// understand, so the import is rejected instead.
    #[error("unknown CSV column: {column}")]
    UnknownColumn {
        /// Header that matched neither metadata nor a registered algorithm.
        column: String,
    },
    /// Imported report data failed schema validation.
    #[error("report is invalid: {0}")]
    InvalidReport(#[from] ReportValidationError),
}

const CSV_FIXED_HEADER: [&str; 8] = [
    "path",
    "size",
    "mtime",
    "status",
    "error_kind",
    "suffix_applied",
    "suffix_algorithm",
    "renamed_to",
];

const DIFF_CSV_HEADER: [&str; 13] = [
    "key",
    "status",
    "reason",
    "algorithms_compared",
    "changed_algorithms",
    "left_path",
    "left_size",
    "left_status",
    "left_error_kind",
    "right_path",
    "right_size",
    "right_status",
    "right_error_kind",
];

/// Export a report in the requested format.
///
/// The report is validated first; JSON and CSV output is canonical (sorted
/// entries, registry algorithm order), so repeated export of the same report is
/// byte-identical.
pub fn export_report(report: &Report, format: ReportFormat) -> Result<String, ExportError> {
    export_report_with(report, format, ExportLocale::En)
}

/// Export a report with an explicit display locale.
///
/// JSON / CSV / md5sum / BSD 忽略 `locale`：它们的键名是与外部工具和 FFI
/// 之间的契约，翻译会直接破坏 `md5sum -c`、CI 脚本与 Dart 侧解析器。
/// 语言化只作用于 Markdown、TXT 与文件树。
pub fn export_report_with(
    report: &Report,
    format: ReportFormat,
    locale: ExportLocale,
) -> Result<String, ExportError> {
    let report = canonical_report(report)?;
    let labels = locale.labels();
    match format {
        ReportFormat::Json => {
            let mut text = serde_json::to_string_pretty(&report)?;
            text.push('\n');
            Ok(text)
        }
        ReportFormat::Csv => export_csv(&report),
        ReportFormat::Markdown => Ok(export_markdown(&report, labels)),
        ReportFormat::Txt => Ok(export_txt(&report, labels)),
        ReportFormat::Tree => Ok(export_tree(&report, labels)),
        ReportFormat::Md5sum => export_md5sum(&report),
        ReportFormat::Bsd => export_bsd(&report),
    }
}

/// Import an untrusted report file.
///
/// Enforces [`MAX_IMPORT_BYTES`], [`MAX_IMPORT_ENTRIES`], rejects future schema
/// versions and rejects path traversal. Checksum formats infer their single
/// algorithm from the digest length.
pub fn import_report(bytes: &[u8], format: ReportFormat) -> Result<Report, ImportError> {
    if !format.can_import() {
        return Err(ImportError::UnsupportedFormat(format));
    }
    check_import_size(bytes.len() as u64)?;
    match format {
        ReportFormat::Json => import_json(bytes),
        ReportFormat::Csv => import_csv(bytes),
        ReportFormat::Md5sum => import_checksum(bytes, ReportFormat::Md5sum),
        ReportFormat::Bsd => import_checksum(bytes, ReportFormat::Bsd),
        // Tree 是渲染结果，不是数据载体：反解析必须处理"文件名里含树线"
        // 和"树被截断"两类歧义，代价高于收益。要可导入的结构化输出请用 JSON/CSV。
        ReportFormat::Markdown | ReportFormat::Txt | ReportFormat::Tree => {
            Err(ImportError::UnsupportedFormat(format))
        }
    }
}

/// Detect the report format of a file from its name and content.
///
/// Content sniffing wins over the extension: `.txt` files may be TXT reports,
/// md5sum listings or BSD checksum files. Returns `None` when neither the
/// extension nor the content identifies a supported format.
pub fn detect_report_format(filename: &str, bytes: &[u8]) -> Option<ReportFormat> {
    if let Some(format) = sniff_report_format(bytes) {
        return Some(format);
    }
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())?;
    format_from_extension(&extension.to_ascii_lowercase())
}

/// Export a diff document in the requested format.
///
/// Only the four document formats are supported; checksum listings have no
/// representation for added/removed/unknown semantics.
pub fn export_diff(
    diff: &batchhash_types::DiffReport,
    format: ReportFormat,
) -> Result<String, ExportError> {
    export_diff_with(diff, format, ExportLocale::En)
}

/// Export a diff document with an explicit display locale.
///
/// 与 [`export_report_with`] 同样的边界：只有 Markdown / TXT 会用到 `locale`，
/// JSON / CSV 保持机器契约不翻译。
pub fn export_diff_with(
    diff: &batchhash_types::DiffReport,
    format: ReportFormat,
    locale: ExportLocale,
) -> Result<String, ExportError> {
    let labels = locale.labels();
    match format {
        ReportFormat::Json => {
            let mut text = serde_json::to_string_pretty(diff)?;
            text.push('\n');
            Ok(text)
        }
        ReportFormat::Csv => export_diff_csv(diff),
        ReportFormat::Markdown => Ok(export_diff_markdown(diff, labels)),
        ReportFormat::Txt => Ok(export_diff_txt(diff, labels)),
        ReportFormat::Tree | ReportFormat::Md5sum | ReportFormat::Bsd => {
            Err(ExportError::UnsupportedDiffFormat(format))
        }
    }
}

fn canonical_report(report: &Report) -> Result<Report, ExportError> {
    report.validate()?;
    let mut report = report.clone();
    for entry in &mut report.entries {
        let mut ordered: IndexMap<AlgorithmId, String> =
            IndexMap::with_capacity(entry.hashes.len());
        for algorithm in &report.algorithms {
            if let Some(digest) = entry.hashes.get(algorithm) {
                ordered.insert(*algorithm, digest.clone());
            }
        }
        entry.hashes = ordered;
    }
    report.sort_entries();
    Ok(report)
}

fn export_csv(report: &Report) -> Result<String, ExportError> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());

    let mut header: Vec<String> = CSV_FIXED_HEADER
        .iter()
        .map(|column| (*column).to_string())
        .collect();
    for algorithm in &report.algorithms {
        header.push(algorithm.stable_id().to_string());
    }
    writer
        .write_record(header.iter().map(String::as_str))
        .map_err(|error| ExportError::Csv(error.to_string()))?;

    for entry in &report.entries {
        let mut record: Vec<String> =
            Vec::with_capacity(CSV_FIXED_HEADER.len() + report.algorithms.len());
        record.push(entry.path.clone());
        record.push(entry.size.to_string());
        record.push(entry.mtime.clone().unwrap_or_default());
        record.push(entry_status_str(entry.status).to_string());
        record.push(
            entry
                .error_kind
                .map(|kind| kind.as_str().to_string())
                .unwrap_or_default(),
        );
        record.push(entry.suffix_applied.clone().unwrap_or_default());
        record.push(
            entry
                .suffix_algorithm
                .map(|algorithm| algorithm.stable_id().to_string())
                .unwrap_or_default(),
        );
        record.push(entry.renamed_to.clone().unwrap_or_default());
        for algorithm in &report.algorithms {
            record.push(entry.hashes.get(algorithm).cloned().unwrap_or_default());
        }
        writer
            .write_record(record.iter().map(String::as_str))
            .map_err(|error| ExportError::Csv(error.to_string()))?;
    }

    let bytes = writer
        .into_inner()
        .map_err(|error| ExportError::Csv(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ExportError::Csv(error.to_string()))
}

/// 报告用的环境与追溯取值。
///
/// 三个渲染器共用同一份取值，避免 Markdown 说「未知」而 TXT 留空这种不一致。
/// Wire name of a [BackendPolicy].
///
/// Mirrors the serde `kebab-case` rename so the human report and the JSON report
/// name the same thing identically; writing a second hand-rolled spelling here
/// is how the two drift apart.
fn backend_policy_str(policy: batchhash_types::BackendPolicy) -> &'static str {
    match policy {
        batchhash_types::BackendPolicy::Auto => "auto",
        batchhash_types::BackendPolicy::ForcedSoft => "forced-soft",
    }
}
fn environment_value(report: &Report, field: EnvironmentField, labels: &Labels) -> String {
    let hardware = &report.hardware;
    let unknown = labels.unknown_value();
    let text = match field {
        EnvironmentField::Arch => hardware.arch.clone(),
        EnvironmentField::Features => {
            if hardware.detected_features.is_empty() {
                unknown.to_string()
            } else {
                hardware.detected_features.join(", ")
            }
        }
        EnvironmentField::BackendPolicy => backend_policy_str(hardware.backend_policy).to_string(),
        EnvironmentField::Blake3Backend => hardware.blake3_backend_reported.clone(),
        EnvironmentField::Sha256Backend => hardware.sha256_backend_reported.clone(),
        EnvironmentField::ProbeSource => hardware.source.clone(),
        EnvironmentField::Threads => hardware.multithread.to_string(),
    };
    if text.is_empty() {
        unknown.to_string()
    } else {
        text
    }
}

/// 追溯取值，直接来自报告自身的 summary。
fn trace_value(report: &Report, field: TraceField, labels: &Labels) -> String {
    let summary = &report.summary;
    match field {
        TraceField::Entries => summary.total.to_string(),
        TraceField::OkEntries => summary.ok.to_string(),
        TraceField::FailedEntries => summary.failed.to_string(),
        TraceField::SkippedEntries => summary.skipped.to_string(),
        TraceField::TotalBytes => summary.total_bytes.to_string(),
        TraceField::Truncated => labels.boolean(summary.truncated).to_string(),
    }
}

/// 所有运行环境字段，按渲染顺序。
const ENVIRONMENT_FIELDS: [EnvironmentField; 7] = [
    EnvironmentField::Arch,
    EnvironmentField::Features,
    EnvironmentField::BackendPolicy,
    EnvironmentField::Blake3Backend,
    EnvironmentField::Sha256Backend,
    EnvironmentField::ProbeSource,
    EnvironmentField::Threads,
];

/// 所有追溯字段，按渲染顺序。
const TRACE_FIELDS: [TraceField; 6] = [
    TraceField::Entries,
    TraceField::OkEntries,
    TraceField::FailedEntries,
    TraceField::SkippedEntries,
    TraceField::TotalBytes,
    TraceField::Truncated,
];
fn export_markdown(report: &Report, labels: &Labels) -> String {
    let table = LabelStyle::Table;
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(labels.report_title);
    out.push_str("\n\n");

    out.push_str("| Field | Value |\n| --- | --- |\n");
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::GeneratedAt, table),
        &report.generated_at,
    );
    push_markdown_row(&mut out, labels.meta(MetaField::Tool, table), &report.tool);
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::ToolVersion, table),
        &report.tool_version,
    );
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::SchemaVersion, table),
        &report.schema_version.to_string(),
    );
    push_markdown_row(&mut out, labels.meta(MetaField::Root, table), &report.root);
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::RootKind, table),
        report.root_kind.as_str(),
    );
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::CaseSensitive, table),
        labels.boolean(report.case_sensitive),
    );
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::Algorithms, table),
        &algorithm_list(&report.algorithms),
    );
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::SuffixAlgorithm, table),
        report.suffix_config.algorithm.stable_id(),
    );
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::SuffixLength, table),
        &report.suffix_config.length.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.meta(MetaField::SuffixCase, table),
        labels.hex_case(report.suffix_config.case),
    );

    out.push_str("\n## ");
    out.push_str(labels.summary_section);
    out.push_str("\n\n");
    out.push_str("| Metric | Value |\n| --- | --- |\n");
    push_markdown_row(
        &mut out,
        labels.summary(SummaryField::Total, table),
        &report.summary.total.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.summary(SummaryField::Ok, table),
        &report.summary.ok.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.summary(SummaryField::Failed, table),
        &report.summary.failed.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.summary(SummaryField::Skipped, table),
        &report.summary.skipped.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.summary(SummaryField::TotalBytes, table),
        &report.summary.total_bytes.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.summary(SummaryField::Truncated, table),
        labels.boolean(report.summary.truncated),
    );

    // Traceability block: the counts the summary is based on, plus the machine
    // that produced them. Placed before the entry table so a reader knows the
    // conditions first, and after the totals so counts stay next to what they
    // qualify. "unknown" is reported as unknown on purpose, matching the
    // hardware panel: guessing here would make the report lie about itself.
    out.push_str("\n## ");
    out.push_str(labels.environment_section());
    out.push_str("\n\n");
    out.push_str("| Field | Value |\n| --- | --- |\n");
    for field in TRACE_FIELDS {
        push_markdown_row(
            &mut out,
            labels.trace(field, table),
            &trace_value(report, field, labels),
        );
    }
    for field in ENVIRONMENT_FIELDS {
        push_markdown_row(
            &mut out,
            labels.environment(field, table),
            &environment_value(report, field, labels),
        );
    }

    out.push_str("\n## ");
    out.push_str(labels.entries_section);
    out.push_str("\n\n");
    out.push('|');
    for field in [
        EntryField::Path,
        EntryField::Size,
        EntryField::Mtime,
        EntryField::Status,
        EntryField::ErrorKind,
        EntryField::SuffixApplied,
        EntryField::SuffixAlgorithm,
        EntryField::RenamedTo,
    ] {
        out.push(' ');
        out.push_str(labels.entry(field, table));
        out.push_str(" |");
    }
    for algorithm in &report.algorithms {
        out.push(' ');
        out.push_str(algorithm.stable_id());
        out.push_str(" |");
    }
    out.push('\n');
    out.push('|');
    out.push_str(" --- | --- | --- | --- | --- | --- | --- | --- |");
    for _ in &report.algorithms {
        out.push_str(" --- |");
    }
    out.push('\n');

    let rendered = report.entries.len().min(MAX_TABLE_ROWS);
    for entry in report.entries.iter().take(rendered) {
        out.push('|');
        out.push(' ');
        out.push_str(&markdown_cell(&entry.path));
        out.push_str(" | ");
        out.push_str(&entry.size.to_string());
        out.push_str(" | ");
        out.push_str(&markdown_cell(entry.mtime.as_deref().unwrap_or("")));
        out.push_str(" | ");
        out.push_str(labels.status(entry.status));
        out.push_str(" | ");
        out.push_str(&markdown_cell(
            entry.error_kind.map(ErrorKind::as_str).unwrap_or(""),
        ));
        out.push_str(" | ");
        out.push_str(&markdown_cell(
            entry.suffix_applied.as_deref().unwrap_or(""),
        ));
        out.push_str(" | ");
        out.push_str(
            entry
                .suffix_algorithm
                .map(AlgorithmId::stable_id)
                .unwrap_or(""),
        );
        out.push_str(" | ");
        out.push_str(&markdown_cell(entry.renamed_to.as_deref().unwrap_or("")));
        out.push_str(" |");
        for algorithm in &report.algorithms {
            out.push(' ');
            out.push_str(
                entry
                    .hashes
                    .get(algorithm)
                    .map(String::as_str)
                    .unwrap_or(""),
            );
            out.push_str(" |");
        }
        out.push('\n');
    }

    if let Some(note) = truncation_note(labels, report.entries.len()) {
        out.push('\n');
        out.push_str("> ");
        out.push_str(&note);
        out.push('\n');
    }
    out
}

fn export_txt(report: &Report, labels: &Labels) -> String {
    let text = LabelStyle::Text;
    let mut out = String::new();
    let title = labels.report_title;
    out.push_str(title);
    out.push('\n');
    // 下划线长度按标题的**显示宽度**算：中文标题按字符数会明显偏短，
    // 用 `chars().count()` 得到的宽字符数在等宽字体下约为实际宽度的一半，
    // 所以按字符数补足后至少和英文观感一致，不会短于标题本身。
    out.push_str(&"=".repeat(title.chars().count()));
    out.push('\n');
    push_txt_line(&mut out, labels.meta(MetaField::Tool, text), &report.tool);
    push_txt_line(
        &mut out,
        labels.meta(MetaField::ToolVersion, text),
        &report.tool_version,
    );
    push_txt_line(
        &mut out,
        labels.meta(MetaField::SchemaVersion, text),
        &report.schema_version.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.meta(MetaField::GeneratedAt, text),
        &report.generated_at,
    );
    push_txt_line(&mut out, labels.meta(MetaField::Root, text), &report.root);
    push_txt_line(
        &mut out,
        labels.meta(MetaField::RootKind, text),
        report.root_kind.as_str(),
    );
    push_txt_line(
        &mut out,
        labels.meta(MetaField::CaseSensitive, text),
        labels.boolean(report.case_sensitive),
    );
    push_txt_line(
        &mut out,
        labels.meta(MetaField::Algorithms, text),
        &algorithm_list(&report.algorithms),
    );
    push_txt_line(
        &mut out,
        labels.meta(MetaField::SuffixAlgorithm, text),
        report.suffix_config.algorithm.stable_id(),
    );
    push_txt_line(
        &mut out,
        labels.meta(MetaField::SuffixLength, text),
        &report.suffix_config.length.to_string(),
    );

    out.push('\n');
    let summary_section = labels.summary_section;
    out.push_str(summary_section);
    out.push('\n');
    out.push_str(&"-".repeat(summary_section.chars().count()));
    out.push('\n');
    push_txt_line(
        &mut out,
        labels.summary(SummaryField::Total, text),
        &report.summary.total.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.summary(SummaryField::Ok, text),
        &report.summary.ok.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.summary(SummaryField::Failed, text),
        &report.summary.failed.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.summary(SummaryField::Skipped, text),
        &report.summary.skipped.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.summary(SummaryField::TotalBytes, text),
        &report.summary.total_bytes.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.summary(SummaryField::Truncated, text),
        labels.boolean(report.summary.truncated),
    );

    // Same traceability block as Markdown, in the aligned Text style so the
    // labels stay machine-friendly (`arch`, not `CPU architecture`).
    out.push('\n');
    let env_section = labels.environment_section();
    out.push_str(env_section);
    out.push('\n');
    out.push_str(&"-".repeat(env_section.chars().count()));
    out.push('\n');
    for field in TRACE_FIELDS {
        push_txt_line(
            &mut out,
            labels.trace(field, text),
            &trace_value(report, field, labels),
        );
    }
    for field in ENVIRONMENT_FIELDS {
        push_txt_line(
            &mut out,
            labels.environment(field, text),
            &environment_value(report, field, labels),
        );
    }

    out.push('\n');
    let entries_section = labels.entries_section;
    out.push_str(entries_section);
    out.push('\n');
    out.push_str(&"-".repeat(entries_section.chars().count()));
    out.push('\n');

    let entry_fields = [
        EntryField::Path,
        EntryField::Size,
        EntryField::Mtime,
        EntryField::Status,
        EntryField::ErrorKind,
        EntryField::SuffixApplied,
        EntryField::SuffixAlgorithm,
        EntryField::RenamedTo,
    ];
    let mut width = entry_fields
        .iter()
        .map(|field| labels.entry(*field, text).chars().count())
        .max()
        .unwrap_or(0);
    width = width.max(
        report
            .algorithms
            .iter()
            .map(|algorithm| algorithm.stable_id().chars().count())
            .max()
            .unwrap_or(0),
    );

    let rendered = report.entries.len().min(MAX_TABLE_ROWS);
    for entry in report.entries.iter().take(rendered) {
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::Path, text),
            &entry.path,
            width,
        );
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::Size, text),
            &entry.size.to_string(),
            width,
        );
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::Mtime, text),
            entry.mtime.as_deref().unwrap_or(""),
            width,
        );
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::Status, text),
            labels.status(entry.status),
            width,
        );
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::ErrorKind, text),
            entry.error_kind.map(ErrorKind::as_str).unwrap_or(""),
            width,
        );
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::SuffixApplied, text),
            entry.suffix_applied.as_deref().unwrap_or(""),
            width,
        );
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::SuffixAlgorithm, text),
            entry
                .suffix_algorithm
                .map(AlgorithmId::stable_id)
                .unwrap_or(""),
            width,
        );
        push_txt_aligned(
            &mut out,
            labels.entry(EntryField::RenamedTo, text),
            entry.renamed_to.as_deref().unwrap_or(""),
            width,
        );
        for algorithm in &report.algorithms {
            push_txt_aligned(
                &mut out,
                algorithm.stable_id(),
                entry
                    .hashes
                    .get(algorithm)
                    .map(String::as_str)
                    .unwrap_or(""),
                width,
            );
        }
        out.push('\n');
    }

    if let Some(note) = truncation_note(labels, report.entries.len()) {
        out.push_str(&note);
        out.push('\n');
    }
    out
}

fn export_md5sum(report: &Report) -> Result<String, ExportError> {
    let algorithm = single_algorithm(report)?;
    let mut out = String::new();
    for entry in &report.entries {
        if !entry.status.is_ok() {
            continue;
        }
        let Some(digest) = entry.hashes.get(&algorithm) else {
            continue;
        };
        if entry.path.contains('\n') || entry.path.contains('\r') {
            return Err(ExportError::UnencodablePath(entry.path.clone()));
        }
        out.push_str(&digest.to_ascii_lowercase());
        out.push_str("  ");
        out.push_str(&entry.path);
        out.push('\n');
    }
    Ok(out)
}

fn export_bsd(report: &Report) -> Result<String, ExportError> {
    let algorithm = single_algorithm(report)?;
    let tag = bsd_tag(algorithm);
    let mut out = String::new();
    for entry in &report.entries {
        if !entry.status.is_ok() {
            continue;
        }
        let Some(digest) = entry.hashes.get(&algorithm) else {
            continue;
        };
        if entry.path.contains('\n')
            || entry.path.contains('\r')
            || entry.path.contains('(')
            || entry.path.contains(')')
        {
            return Err(ExportError::UnencodablePath(entry.path.clone()));
        }
        out.push_str(&tag);
        out.push_str(" (");
        out.push_str(&entry.path);
        out.push_str(") = ");
        out.push_str(&digest.to_ascii_lowercase());
        out.push('\n');
    }
    Ok(out)
}

fn single_algorithm(report: &Report) -> Result<AlgorithmId, ExportError> {
    if report.algorithms.len() != 1 {
        return Err(ExportError::RequiresSingleAlgorithm {
            count: report.algorithms.len(),
        });
    }
    report
        .algorithms
        .first()
        .copied()
        .ok_or(ExportError::RequiresSingleAlgorithm { count: 0 })
}

fn bsd_tag(algorithm: AlgorithmId) -> String {
    match algorithm {
        AlgorithmId::Md5 => "MD5".to_string(),
        AlgorithmId::Sha1 => "SHA1".to_string(),
        AlgorithmId::Sha256 => "SHA256".to_string(),
        AlgorithmId::Sha512 => "SHA512".to_string(),
        other => other.stable_id().to_ascii_uppercase(),
    }
}

fn import_json(bytes: &[u8]) -> Result<Report, ImportError> {
    let report: Report =
        serde_json::from_slice(bytes).map_err(|error| ImportError::Json(error.to_string()))?;
    if report.schema_version > REPORT_SCHEMA_VERSION {
        return Err(ImportError::FutureSchema {
            found: report.schema_version,
            supported: REPORT_SCHEMA_VERSION,
        });
    }
    if report.schema_version == 0 {
        return Err(ImportError::UnsupportedSchemaVersion(0));
    }
    check_entry_cap(report.entries.len())?;
    for entry in &report.entries {
        validate_relative_path(&entry.path).map_err(|source| ImportError::Path {
            path: entry.path.clone(),
            source,
        })?;
    }
    finish_import(report)
}

fn import_csv(bytes: &[u8]) -> Result<Report, ImportError> {
    let bytes = strip_bom(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(bytes);
    let headers: Vec<String> = reader
        .headers()
        .map_err(|error| ImportError::Csv(error.to_string()))?
        .iter()
        .map(|header| header.trim().to_string())
        .collect();
    if headers.is_empty() {
        return Err(ImportError::Csv("missing CSV header".to_string()));
    }

    let lookup = |name: &str| {
        headers
            .iter()
            .position(|header| header.eq_ignore_ascii_case(name))
    };
    let path_index = lookup("path")
        .ok_or_else(|| ImportError::Csv("CSV header has no path column".to_string()))?;
    let size_index = lookup("size");
    let mtime_index = lookup("mtime");
    let status_index = lookup("status");
    let error_kind_index = lookup("error_kind");
    let suffix_applied_index = lookup("suffix_applied");
    let suffix_algorithm_index = lookup("suffix_algorithm");
    let renamed_to_index = lookup("renamed_to");
    let generated_at_index = lookup("generated_at");

    // Reject columns the importer does not understand: a header that is
    // neither report metadata nor a registered algorithm id would otherwise be
    // dropped silently, so the caller would lose data without any diagnostic.
    for header in &headers {
        let is_metadata = CSV_FIXED_HEADER
            .iter()
            .any(|column| header.eq_ignore_ascii_case(column))
            || header.eq_ignore_ascii_case("generated_at");
        if !is_metadata && AlgorithmId::from_stable_id(header).is_none() {
            return Err(ImportError::UnknownColumn {
                column: header.clone(),
            });
        }
    }

    let mut algorithm_columns: Vec<(usize, AlgorithmId)> = headers
        .iter()
        .enumerate()
        .filter_map(|(index, header)| {
            AlgorithmId::from_stable_id(header).map(|algorithm| (index, algorithm))
        })
        .collect();
    algorithm_columns.sort_by_key(|(_, algorithm)| *algorithm);
    algorithm_columns.dedup_by(|a, b| a.1 == b.1);
    if algorithm_columns.is_empty() {
        return Err(ImportError::NoAlgorithms);
    }
    let algorithms: Vec<AlgorithmId> = algorithm_columns
        .iter()
        .map(|(_, algorithm)| *algorithm)
        .collect();

    let mut entries: Vec<ReportEntry> = Vec::new();
    let mut generated_at = String::new();
    for record in reader.records() {
        let record = record.map_err(|error| ImportError::Csv(error.to_string()))?;
        check_entry_cap(entries.len() + 1)?;

        let path = record.get(path_index).unwrap_or("").to_string();
        validate_relative_path(&path).map_err(|source| ImportError::Path {
            path: path.clone(),
            source,
        })?;

        let size = match record_field(&record, size_index) {
            "" => 0,
            value => value
                .trim()
                .parse::<u64>()
                .map_err(|_| ImportError::InvalidSize(value.to_string()))?,
        };
        let status = parse_status(record_field(&record, status_index))?;
        let error_kind = match record_field(&record, error_kind_index).trim() {
            "" => None,
            value => Some(
                error_kind_from_str(value)
                    .ok_or_else(|| ImportError::InvalidErrorKind(value.to_string()))?,
            ),
        };
        let suffix_algorithm = match record_field(&record, suffix_algorithm_index).trim() {
            "" => None,
            value => Some(
                AlgorithmId::from_stable_id(value)
                    .ok_or_else(|| ImportError::InvalidAlgorithm(value.to_string()))?,
            ),
        };
        if generated_at.is_empty() {
            let value = record_field(&record, generated_at_index);
            if !value.is_empty() {
                generated_at = value.to_string();
            }
        }

        let mut hashes: IndexMap<AlgorithmId, String> = IndexMap::new();
        for (index, algorithm) in &algorithm_columns {
            let value = record.get(*index).unwrap_or("");
            if value.is_empty() {
                continue;
            }
            if !is_hex(value) || value.len() != algorithm.hex_len() {
                return Err(ImportError::InvalidDigest {
                    path: path.clone(),
                    algorithm: *algorithm,
                });
            }
            hashes.insert(*algorithm, value.to_string());
        }

        entries.push(ReportEntry {
            path,
            size,
            mtime: non_empty(record_field(&record, mtime_index)),
            hashes,
            status,
            error_kind,
            error_message: None,
            suffix_applied: non_empty(record_field(&record, suffix_applied_index)),
            suffix_algorithm,
            renamed_to: non_empty(record_field(&record, renamed_to_index)),
        });
    }

    finish_import(base_report(algorithms, entries, generated_at))
}

/// Validate an imported report before it leaves the importer.
///
/// An importer must never hand back a [`Report`] that a later export or diff
/// would reject, so JSON, CSV and checksum imports all share this final step.
fn finish_import(report: Report) -> Result<Report, ImportError> {
    report.validate().map_err(ImportError::InvalidReport)?;
    Ok(report)
}

fn import_checksum(bytes: &[u8], format: ReportFormat) -> Result<Report, ImportError> {
    let text = std::str::from_utf8(bytes).map_err(|error| ImportError::Utf8(error.to_string()))?;
    let mut entries: Vec<ReportEntry> = Vec::new();
    let mut algorithm: Option<AlgorithmId> = None;

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parsed = match format {
            ReportFormat::Md5sum => parse_md5sum_line(trimmed, line_number)?,
            ReportFormat::Bsd => parse_bsd_line(trimmed, line_number)?,
            _ => return Err(ImportError::UnsupportedFormat(format)),
        };
        validate_relative_path(&parsed.path).map_err(|source| ImportError::Path {
            path: parsed.path.clone(),
            source,
        })?;
        let resolved = match parsed.algorithm {
            Some(explicit) => explicit,
            None => infer_algorithm(parsed.digest.len())?,
        };
        if resolved.hex_len() != parsed.digest.len() {
            return Err(ImportError::MalformedLine {
                line: line_number,
                reason: format!(
                    "digest length {} does not match algorithm {}",
                    parsed.digest.len(),
                    resolved.stable_id()
                ),
            });
        }
        match algorithm {
            Some(existing) if existing != resolved => {
                return Err(ImportError::MixedAlgorithms {
                    first: existing,
                    other: resolved,
                });
            }
            None => algorithm = Some(resolved),
            _ => {}
        }
        check_entry_cap(entries.len() + 1)?;
        let mut hashes: IndexMap<AlgorithmId, String> = IndexMap::new();
        hashes.insert(resolved, parsed.digest);
        entries.push(ReportEntry {
            path: parsed.path,
            size: 0,
            mtime: None,
            hashes,
            status: EntryStatus::Ok,
            error_kind: None,
            error_message: None,
            suffix_applied: None,
            suffix_algorithm: None,
            renamed_to: None,
        });
    }

    let algorithm = algorithm.ok_or(ImportError::NoAlgorithms)?;
    finish_import(base_report(vec![algorithm], entries, String::new()))
}

struct ChecksumLine {
    path: String,
    digest: String,
    algorithm: Option<AlgorithmId>,
}

fn parse_md5sum_line(line: &str, line_number: usize) -> Result<ChecksumLine, ImportError> {
    let split = line
        .find(char::is_whitespace)
        .ok_or_else(|| ImportError::MalformedLine {
            line: line_number,
            reason: "missing digest/path separator".to_string(),
        })?;
    let digest = &line[..split];
    let remainder = line[split..].trim_start();
    let remainder = remainder.strip_prefix('*').unwrap_or(remainder);
    let path = remainder.trim_end();
    if path.is_empty() {
        return Err(ImportError::MalformedLine {
            line: line_number,
            reason: "missing path".to_string(),
        });
    }
    if !is_hex(digest) {
        return Err(ImportError::MalformedLine {
            line: line_number,
            reason: "digest is not hexadecimal".to_string(),
        });
    }
    Ok(ChecksumLine {
        path: path.to_string(),
        digest: digest.to_ascii_lowercase(),
        algorithm: None,
    })
}

fn parse_bsd_line(line: &str, line_number: usize) -> Result<ChecksumLine, ImportError> {
    let open = line.find(" (").ok_or_else(|| ImportError::MalformedLine {
        line: line_number,
        reason: "expected 'ALGO (path) = hex'".to_string(),
    })?;
    let tag = &line[..open];
    let remainder = &line[open + 2..];
    let close = remainder
        .rfind(") = ")
        .ok_or_else(|| ImportError::MalformedLine {
            line: line_number,
            reason: "expected ') = ' before the digest".to_string(),
        })?;
    let path = &remainder[..close];
    let digest = &remainder[close + 4..];
    if path.is_empty() {
        return Err(ImportError::MalformedLine {
            line: line_number,
            reason: "missing path".to_string(),
        });
    }
    if !is_hex(digest) {
        return Err(ImportError::MalformedLine {
            line: line_number,
            reason: "digest is not hexadecimal".to_string(),
        });
    }
    Ok(ChecksumLine {
        path: path.to_string(),
        digest: digest.to_ascii_lowercase(),
        algorithm: algorithm_from_bsd_tag(tag),
    })
}

fn algorithm_from_bsd_tag(tag: &str) -> Option<AlgorithmId> {
    let normalized = tag.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "MD5" => Some(AlgorithmId::Md5),
        "SHA1" | "SHA-1" => Some(AlgorithmId::Sha1),
        "SHA256" | "SHA-256" => Some(AlgorithmId::Sha256),
        "SHA512" | "SHA-512" => Some(AlgorithmId::Sha512),
        _ => AlgorithmId::from_stable_id(&normalized.to_ascii_lowercase()),
    }
}

fn infer_algorithm(hex_len: usize) -> Result<AlgorithmId, ImportError> {
    let candidates = AlgorithmId::infer_by_hex_len(hex_len);
    if candidates.is_empty() {
        return Err(ImportError::UnknownAlgorithm { hex_len });
    }
    if candidates.len() > 1 {
        return Err(ImportError::AmbiguousAlgorithm {
            hex_len,
            candidates,
        });
    }
    Ok(candidates[0])
}

fn parse_status(value: &str) -> Result<EntryStatus, ImportError> {
    match value.trim() {
        "" | "ok" => Ok(EntryStatus::Ok),
        "failed" => Ok(EntryStatus::Failed),
        "skipped" => Ok(EntryStatus::Skipped),
        other => Err(ImportError::InvalidStatus(other.to_string())),
    }
}

fn error_kind_from_str(value: &str) -> Option<ErrorKind> {
    match value.trim() {
        "permission_denied" => Some(ErrorKind::PermissionDenied),
        "not_found" => Some(ErrorKind::NotFound),
        "is_directory" => Some(ErrorKind::IsDirectory),
        "io_error" => Some(ErrorKind::IoError),
        "unsupported" => Some(ErrorKind::Unsupported),
        "cancelled" => Some(ErrorKind::Cancelled),
        "unstable" => Some(ErrorKind::Unstable),
        "out_of_root" => Some(ErrorKind::OutOfRoot),
        "suffix_exists" => Some(ErrorKind::SuffixExists),
        "invalid_name" => Some(ErrorKind::InvalidName),
        "rename_unsupported" => Some(ErrorKind::RenameUnsupported),
        "rename_failed" => Some(ErrorKind::RenameFailed),
        "platform_access_lost" => Some(ErrorKind::PlatformAccessLost),
        "hash_mismatch" => Some(ErrorKind::HashMismatch),
        "parse_error" => Some(ErrorKind::ParseError),
        "other" => Some(ErrorKind::Other),
        _ => None,
    }
}

fn base_report(
    algorithms: Vec<AlgorithmId>,
    entries: Vec<ReportEntry>,
    generated_at: String,
) -> Report {
    let mut report = Report::new(
        String::new(),
        RootKind::Fs,
        false,
        algorithms,
        HardwareReport::default(),
    );
    report.generated_at = generated_at;
    report.entries = entries;
    report.recompute_summary();
    report
}

fn check_import_size(size: u64) -> Result<(), ImportError> {
    if size > MAX_IMPORT_BYTES {
        Err(ImportError::TooLarge {
            size,
            max: MAX_IMPORT_BYTES,
        })
    } else {
        Ok(())
    }
}

fn check_entry_cap(count: usize) -> Result<(), ImportError> {
    if count > MAX_IMPORT_ENTRIES {
        return Err(ImportError::TooManyEntries {
            count,
            max: MAX_IMPORT_ENTRIES,
        });
    }
    Ok(())
}

fn export_diff_csv(diff: &batchhash_types::DiffReport) -> Result<String, ExportError> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    writer
        .write_record(DIFF_CSV_HEADER.iter().copied())
        .map_err(|error| ExportError::Csv(error.to_string()))?;
    for row in &diff.rows {
        let record = [
            row.key.clone(),
            row.status.as_str().to_string(),
            row.reason.clone(),
            algorithm_list(&row.algorithms_compared),
            algorithm_list(&row.changed_algorithms),
            row.left
                .as_ref()
                .map(|entry| entry.path.clone())
                .unwrap_or_default(),
            row.left
                .as_ref()
                .map(|entry| entry.size.to_string())
                .unwrap_or_default(),
            row.left_status
                .map(entry_status_str)
                .unwrap_or("")
                .to_string(),
            row.left
                .as_ref()
                .and_then(|entry| entry.error_kind)
                .map(ErrorKind::as_str)
                .unwrap_or("")
                .to_string(),
            row.right
                .as_ref()
                .map(|entry| entry.path.clone())
                .unwrap_or_default(),
            row.right
                .as_ref()
                .map(|entry| entry.size.to_string())
                .unwrap_or_default(),
            row.right_status
                .map(entry_status_str)
                .unwrap_or("")
                .to_string(),
            row.right
                .as_ref()
                .and_then(|entry| entry.error_kind)
                .map(ErrorKind::as_str)
                .unwrap_or("")
                .to_string(),
        ];
        writer
            .write_record(record.iter().map(String::as_str))
            .map_err(|error| ExportError::Csv(error.to_string()))?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|error| ExportError::Csv(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ExportError::Csv(error.to_string()))
}

fn export_diff_markdown(diff: &batchhash_types::DiffReport, labels: &Labels) -> String {
    let table = LabelStyle::Table;
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(labels.diff_title);
    out.push_str("\n\n");
    out.push_str("| Field | Value |\n| --- | --- |\n");
    push_markdown_row(
        &mut out,
        labels.diff_meta(DiffMetaField::GeneratedAt, table),
        &diff.generated_at,
    );
    push_markdown_row(
        &mut out,
        labels.diff_meta(DiffMetaField::SchemaVersion, table),
        &diff.schema_version.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.diff_meta(DiffMetaField::Mode, table),
        diff_mode_str(diff.strategy.mode),
    );
    push_markdown_row(
        &mut out,
        labels.diff_meta(DiffMetaField::CaseSensitive, table),
        labels.boolean(diff.strategy.case_sensitive),
    );
    push_markdown_row(
        &mut out,
        labels.diff_meta(DiffMetaField::AlgorithmsCompared, table),
        &algorithm_list(&diff.strategy.algorithms_compared),
    );
    push_markdown_row(
        &mut out,
        labels.diff_meta(DiffMetaField::Left, table),
        &diff.strategy.left.label,
    );
    push_markdown_row(
        &mut out,
        labels.diff_meta(DiffMetaField::Right, table),
        &diff.strategy.right.label,
    );
    push_markdown_row(
        &mut out,
        labels.diff_summary(DiffSummaryField::Total, table),
        &diff.summary.total.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.diff_summary(DiffSummaryField::Changed, table),
        &diff.summary.changed.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.diff_summary(DiffSummaryField::Added, table),
        &diff.summary.added.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.diff_summary(DiffSummaryField::Removed, table),
        &diff.summary.removed.to_string(),
    );
    push_markdown_row(
        &mut out,
        labels.diff_summary(DiffSummaryField::Incomplete, table),
        labels.boolean(diff.summary.incomplete),
    );

    out.push_str("\n## ");
    out.push_str(labels.diff_rows_section);
    out.push_str("\n\n");
    out.push_str("| Key | Status | Reason | Changed algorithms | Left path | Left size | Right path | Right size |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    let rendered = diff.rows.len().min(MAX_TABLE_ROWS);
    for row in diff.rows.iter().take(rendered) {
        out.push_str("| ");
        out.push_str(&markdown_cell(&row.key));
        out.push_str(" | ");
        out.push_str(row.status.as_str());
        out.push_str(" | ");
        out.push_str(&markdown_cell(&row.reason));
        out.push_str(" | ");
        out.push_str(&algorithm_list(&row.changed_algorithms));
        out.push_str(" | ");
        out.push_str(&markdown_cell(
            row.left
                .as_ref()
                .map(|entry| entry.path.as_str())
                .unwrap_or(""),
        ));
        out.push_str(" | ");
        out.push_str(
            &row.left
                .as_ref()
                .map(|entry| entry.size.to_string())
                .unwrap_or_default(),
        );
        out.push_str(" | ");
        out.push_str(&markdown_cell(
            row.right
                .as_ref()
                .map(|entry| entry.path.as_str())
                .unwrap_or(""),
        ));
        out.push_str(" | ");
        out.push_str(
            &row.right
                .as_ref()
                .map(|entry| entry.size.to_string())
                .unwrap_or_default(),
        );
        out.push_str(" |\n");
    }
    if let Some(note) = truncation_note(labels, diff.rows.len()) {
        out.push('\n');
        out.push_str("> ");
        out.push_str(&note);
        out.push('\n');
    }
    out
}

fn export_diff_txt(diff: &batchhash_types::DiffReport, labels: &Labels) -> String {
    let text = LabelStyle::Text;
    let mut out = String::new();
    let title = labels.diff_title;
    out.push_str(title);
    out.push('\n');
    out.push_str(&"=".repeat(title.chars().count()));
    out.push('\n');
    push_txt_line(
        &mut out,
        labels.diff_meta(DiffMetaField::GeneratedAt, text),
        &diff.generated_at,
    );
    push_txt_line(
        &mut out,
        labels.diff_meta(DiffMetaField::SchemaVersion, text),
        &diff.schema_version.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.diff_meta(DiffMetaField::Mode, text),
        diff_mode_str(diff.strategy.mode),
    );
    push_txt_line(
        &mut out,
        labels.diff_meta(DiffMetaField::AlgorithmsCompared, text),
        &algorithm_list(&diff.strategy.algorithms_compared),
    );
    push_txt_line(
        &mut out,
        labels.diff_meta(DiffMetaField::Left, text),
        &diff.strategy.left.label,
    );
    push_txt_line(
        &mut out,
        labels.diff_meta(DiffMetaField::Right, text),
        &diff.strategy.right.label,
    );
    push_txt_line(
        &mut out,
        labels.diff_summary(DiffSummaryField::Total, text),
        &diff.summary.total.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.diff_summary(DiffSummaryField::Changed, text),
        &diff.summary.changed.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.diff_summary(DiffSummaryField::Added, text),
        &diff.summary.added.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.diff_summary(DiffSummaryField::Removed, text),
        &diff.summary.removed.to_string(),
    );
    push_txt_line(
        &mut out,
        labels.diff_summary(DiffSummaryField::Incomplete, text),
        labels.boolean(diff.summary.incomplete),
    );

    out.push('\n');
    let rows_section = labels.diff_rows_section;
    out.push_str(rows_section);
    out.push('\n');
    out.push_str(&"-".repeat(rows_section.chars().count()));
    out.push('\n');
    let width = 11;
    let rendered = diff.rows.len().min(MAX_TABLE_ROWS);
    for row in diff.rows.iter().take(rendered) {
        push_txt_aligned(&mut out, "key", &row.key, width);
        push_txt_aligned(&mut out, "status", row.status.as_str(), width);
        push_txt_aligned(&mut out, "reason", &row.reason, width);
        push_txt_aligned(
            &mut out,
            "changed",
            &algorithm_list(&row.changed_algorithms),
            width,
        );
        push_txt_aligned(
            &mut out,
            "left_path",
            row.left
                .as_ref()
                .map(|entry| entry.path.as_str())
                .unwrap_or(""),
            width,
        );
        push_txt_aligned(
            &mut out,
            "right_path",
            row.right
                .as_ref()
                .map(|entry| entry.path.as_str())
                .unwrap_or(""),
            width,
        );
        out.push('\n');
    }
    if let Some(note) = truncation_note(labels, diff.rows.len()) {
        out.push_str(&note);
        out.push('\n');
    }
    out
}

/// 文件树渲染。
///
/// 树线用 Unicode 制表符（`├─ `、`└─ `、`│  `、`   `），与用户熟悉的 `tree`
/// 命令输出一致。早先用过 ASCII（`|- `、`\- `），理由是终端编码安全；实际
/// 取舍反过来了：这份输出主要给人看，制表符的视觉结构更清晰，而 UTF-8 已经是
/// 本项目的硬前提（报告本身、文件名、界面文案都依赖它）。
///
/// 这套规则与 Dart 侧测试替身 app/lib/src/services/report_rendering.dart 的
/// renderReportTree **严格一致**（缩进两空格、目录先于文件、同级按名称升序、
/// 四种 ASCII 前缀、多摘要用两个空格分隔），**改动必须两边同步**，
/// 否则 widget 测试看到的结构会和真实产物漂移。
fn export_tree(report: &Report, labels: &Labels) -> String {
    // 截断发生在建树阶段而不是渲染阶段：entries 已按 path 排序，取前
    // MAX_TABLE_ROWS 个就是稳定的前缀截断，且不会出现"目录还在、子项被砍光"
    // 的半截子树。总条数超过上限时另行追加截断提示。
    let mut root = TreeNode::default();
    let mut inserted = 0usize;
    for (index, entry) in report.entries.iter().enumerate() {
        if inserted >= MAX_TABLE_ROWS {
            break;
        }
        let relative = relative_to_root(&entry.path, &report.root);
        if relative.is_empty() {
            continue;
        }
        let segments: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
        root.insert(&segments, index);
        inserted += 1;
    }

    let mut out = String::new();
    out.push_str(labels.report_title);
    out.push('\n');
    render_tree_children(&mut out, &root, &[], report, labels);

    out.push('\n');
    render_tree_summary(&mut out, report, labels);

    if let Some(note) = truncation_note(labels, report.entries.len()) {
        out.push_str(&note);
        out.push('\n');
    }
    out
}

/// 路径相对根目录的部分；不在根下时原样返回。
///
/// 报告 root 可能为空（校验和格式导入的结果），也可能带尾部分隔符，
/// 所以这里按“归一化后去掉根前缀”处理，而不是假设某个固定形状。
fn relative_to_root(path: &str, root: &str) -> String {
    let normalized = path.replace('\\', "/");
    if root.is_empty() {
        return normalized;
    }
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    if normalized == root {
        return String::new();
    }
    match normalized.strip_prefix(root) {
        Some(rest) => rest.trim_start_matches('/').to_string(),
        None => normalized,
    }
}

/// 树的一个节点；目录本身不带条目，叶子通过下标引用 Report.entries。
#[derive(Debug, Default)]
struct TreeNode {
    name: String,
    is_directory: bool,
    entry_index: Option<usize>,
    children: std::collections::BTreeMap<String, TreeNode>,
}

impl TreeNode {
    fn insert(&mut self, segments: &[&str], entry_index: usize) {
        let Some((head, rest)) = segments.split_first() else {
            return;
        };
        let child = self.children.entry((*head).to_string()).or_default();
        child.name = (*head).to_string();
        if rest.is_empty() {
            // 只有当该节点还没有子节点时才降级为叶子：同名目录先到的情况
            // （"a" 与 "a/b"）必须保留目录身份，否则子树会被整个丢掉。
            if child.children.is_empty() {
                child.is_directory = false;
            }
            child.entry_index = Some(entry_index);
        } else {
            child.is_directory = true;
            child.insert(rest, entry_index);
        }
    }

    /// 目录在前、文件在后，同类按名称升序。
    fn sorted_children(&self) -> Vec<&TreeNode> {
        let mut children: Vec<&TreeNode> = self.children.values().collect();
        children.sort_by(
            |left, right| match (left.is_directory, right.is_directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => left.name.cmp(&right.name),
            },
        );
        children
    }
}

/// 子节点的分支前缀：末项用 └─，其余用 ├─。
fn tree_branch(is_last: bool) -> &'static str {
    if is_last {
        "└─ "
    } else {
        "├─ "
    }
}

/// 子节点后代要重复的前缀：父级还有兄弟时画垂线，否则留空白。
fn tree_continuation(is_last: bool) -> &'static str {
    if is_last {
        "   "
    } else {
        "│  "
    }
}

/// 递归渲染一层子节点；ancestors_are_last 记录每一级祖先是否为末项。
fn render_tree_children(
    out: &mut String,
    node: &TreeNode,
    ancestors_are_last: &[bool],
    report: &Report,
    labels: &Labels,
) {
    let children = node.sorted_children();
    for (index, child) in children.iter().enumerate() {
        let is_last = index + 1 == children.len();
        for ancestor_last in ancestors_are_last {
            out.push_str(tree_continuation(*ancestor_last));
        }
        out.push_str(tree_branch(is_last));
        if child.is_directory {
            out.push_str(&child.name);
            out.push('\n');
            let mut next = ancestors_are_last.to_vec();
            next.push(is_last);
            render_tree_children(out, child, &next, report, labels);
        } else {
            out.push_str(&child.name);
            if let Some(entry) = child.entry_index.and_then(|i| report.entries.get(i)) {
                let digests = tree_digest_columns(entry, &report.algorithms);
                if !digests.is_empty() {
                    out.push_str("  ");
                    out.push_str(&digests);
                }
                if let Some(note) = labels.status_note(entry.status) {
                    out.push_str("  ");
                    out.push_str(note);
                }
            }
            out.push('\n');
        }
    }
}

/// 按报告声明的算法顺序拼 algo:hash，两空格分隔。
///
/// 顺序取自报告而非 map：同一份报告两次渲染必须逐字节一致。
fn tree_digest_columns(entry: &ReportEntry, algorithms: &[AlgorithmId]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for algorithm in algorithms {
        if let Some(digest) = entry.hashes.get(algorithm) {
            if !digest.is_empty() {
                parts.push(format!("{}:{}", algorithm.stable_id(), digest));
            }
        }
    }
    parts.join("  ")
}

/// 树尾部的汇总块，标签与表格导出共用同一套。
///
/// 计数直接读 summary 而不是重新扫描 entries：报告在导出前已通过 validate()，
/// 两者必然一致，重扫只会让两处逻辑有机会走偏。
fn render_tree_summary(out: &mut String, report: &Report, labels: &Labels) {
    out.push_str(labels.summary_section);
    out.push_str(":\n");
    let rows = [
        (SummaryField::Total, report.summary.total),
        (SummaryField::Ok, report.summary.ok),
        (SummaryField::Failed, report.summary.failed),
        (SummaryField::Skipped, report.summary.skipped),
        (SummaryField::TotalBytes, report.summary.total_bytes),
    ];
    for (field, value) in rows {
        out.push_str("  ");
        out.push_str(labels.tree_summary(field));
        out.push_str(": ");
        out.push_str(&value.to_string());
        out.push('\n');
    }

    // The tree is a report a person reads, so it carries the same provenance as
    // Markdown and TXT: without it a saved tree cannot say which machine or
    // thread count produced the digests beside the names.
    out.push('\n');
    out.push_str(labels.environment_section());
    out.push_str(":\n");
    for field in TRACE_FIELDS {
        out.push_str("  ");
        out.push_str(labels.trace(field, LabelStyle::Text));
        out.push_str(": ");
        out.push_str(&trace_value(report, field, labels));
        out.push('\n');
    }
    for field in ENVIRONMENT_FIELDS {
        out.push_str("  ");
        out.push_str(labels.environment(field, LabelStyle::Text));
        out.push_str(": ");
        out.push_str(&environment_value(report, field, labels));
        out.push('\n');
    }
}
fn sniff_report_format(bytes: &[u8]) -> Option<ReportFormat> {
    let text = std::str::from_utf8(bytes).ok()?;
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{') {
        return Some(ReportFormat::Json);
    }
    let first_line = trimmed.lines().find(|line| !line.trim().is_empty())?;
    let line = first_line.trim_end();
    if line.starts_with("# BatchHash Report") {
        return Some(ReportFormat::Markdown);
    }
    if line.starts_with("BatchHash Report") {
        return Some(ReportFormat::Txt);
    }
    if line.starts_with("path,") && line.contains("size") {
        return Some(ReportFormat::Csv);
    }
    if line.contains(" (") && line.contains(") = ") {
        return Some(ReportFormat::Bsd);
    }
    if let Some(space) = line.find(char::is_whitespace) {
        let digest = &line[..space];
        if is_hex(digest) && is_known_hex_len(digest.len()) {
            return Some(ReportFormat::Md5sum);
        }
    }
    None
}

fn is_known_hex_len(len: usize) -> bool {
    matches!(len, 8 | 16 | 32 | 40 | 56 | 64 | 96 | 128)
}

fn format_from_extension(extension: &str) -> Option<ReportFormat> {
    match extension {
        "json" => Some(ReportFormat::Json),
        "csv" => Some(ReportFormat::Csv),
        "md" | "markdown" => Some(ReportFormat::Markdown),
        "txt" => Some(ReportFormat::Txt),
        "md5" | "md5sum" | "checksum" => Some(ReportFormat::Md5sum),
        "sha1" | "sha224" | "sha256" | "sha384" | "sha512" | "bsd" => Some(ReportFormat::Bsd),
        _ => None,
    }
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_prefix(b"\xEF\xBB\xBF".as_slice())
        .unwrap_or(bytes)
}

fn push_markdown_row(out: &mut String, key: &str, value: &str) {
    out.push_str("| ");
    out.push_str(key);
    out.push_str(" | ");
    out.push_str(&markdown_cell(value));
    out.push_str(" |\n");
}

fn push_txt_line(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn push_txt_aligned(out: &mut String, key: &str, value: &str, width: usize) {
    out.push_str(&format!("{:<width$}: {}\n", key, value, width = width));
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
}

fn truncation_note(labels: &Labels, total: usize) -> Option<String> {
    if total > MAX_TABLE_ROWS {
        Some(labels.truncation(MAX_TABLE_ROWS, total))
    } else {
        None
    }
}

fn algorithm_list(algorithms: &[AlgorithmId]) -> String {
    algorithms
        .iter()
        .map(|algorithm| algorithm.stable_id())
        .collect::<Vec<_>>()
        .join(", ")
}

fn entry_status_str(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Ok => "ok",
        EntryStatus::Failed => "failed",
        EntryStatus::Skipped => "skipped",
    }
}

fn diff_mode_str(mode: batchhash_types::DiffMode) -> &'static str {
    match mode {
        batchhash_types::DiffMode::RootRelative => "root_relative",
        batchhash_types::DiffMode::Basename => "basename",
        batchhash_types::DiffMode::Absolute => "absolute",
    }
}

fn record_field(record: &csv::StringRecord, index: Option<usize>) -> &str {
    index
        .and_then(|position| record.get(position))
        .unwrap_or("")
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_capabilities_are_explicit() {
        for format in ReportFormat::ALL {
            assert_eq!(
                format.can_import(),
                !matches!(
                    format,
                    ReportFormat::Markdown | ReportFormat::Txt | ReportFormat::Tree
                ),
                "{format} import capability"
            );
            assert_eq!(
                format.can_diff(),
                !matches!(
                    format,
                    ReportFormat::Md5sum | ReportFormat::Bsd | ReportFormat::Tree
                ),
                "{format} diff capability"
            );
            assert!(!format.as_str().is_empty());
            assert!(!format.extension().is_empty());
        }
    }

    #[test]
    fn only_human_formats_are_localized() {
        // 机器格式的键名是与 coreutils / CI / FFI 的契约，翻译即破坏性变更。
        for format in ReportFormat::ALL {
            assert_eq!(
                format.is_localized(),
                matches!(
                    format,
                    ReportFormat::Markdown | ReportFormat::Txt | ReportFormat::Tree
                ),
                "{format} localization"
            );
        }
    }

    #[test]
    fn entry_cap_is_enforced() {
        assert!(check_entry_cap(MAX_IMPORT_ENTRIES).is_ok());
        assert!(matches!(
            check_entry_cap(MAX_IMPORT_ENTRIES + 1),
            Err(ImportError::TooManyEntries { .. })
        ));
    }

    #[test]
    fn byte_cap_is_enforced() {
        assert!(check_import_size(MAX_IMPORT_BYTES).is_ok());
        assert!(matches!(
            check_import_size(MAX_IMPORT_BYTES + 1),
            Err(ImportError::TooLarge { .. })
        ));
    }

    #[test]
    fn checksum_inference_reports_ambiguity() {
        // 48 hex characters are unique to Tiger, 80 to RIPEMD-320, so they infer.
        assert_eq!(infer_algorithm(48).unwrap(), AlgorithmId::Tiger);
        assert_eq!(infer_algorithm(80).unwrap(), AlgorithmId::Ripemd320);
        // 40 is now shared by SHA-1 and RIPEMD-160, 32 by MD2/MD4/MD5/XXH3-128 and
        // more: a bare checksum of those widths cannot be attributed to one
        // algorithm and must be reported as ambiguous.
        for ambiguous in [32usize, 40, 56, 64, 128] {
            assert!(
                matches!(
                    infer_algorithm(ambiguous),
                    Err(ImportError::AmbiguousAlgorithm { .. })
                ),
                "{ambiguous} hex characters must be ambiguous"
            );
        }
        assert!(matches!(
            infer_algorithm(63),
            Err(ImportError::UnknownAlgorithm { .. })
        ));
    }

    #[test]
    fn status_and_error_kind_round_trip_strings() {
        assert_eq!(parse_status("ok").unwrap(), EntryStatus::Ok);
        assert_eq!(parse_status("skipped").unwrap(), EntryStatus::Skipped);
        assert!(parse_status("bogus").is_err());
        assert_eq!(error_kind_from_str("cancelled"), Some(ErrorKind::Cancelled));
        assert_eq!(error_kind_from_str("nope"), None);
    }

    #[test]
    fn bsd_legacy_tags_are_recognised() {
        assert_eq!(algorithm_from_bsd_tag("MD5"), Some(AlgorithmId::Md5));
        assert_eq!(algorithm_from_bsd_tag("SHA256"), Some(AlgorithmId::Sha256));
        assert_eq!(algorithm_from_bsd_tag("BLAKE3"), Some(AlgorithmId::Blake3));
        assert_eq!(
            algorithm_from_bsd_tag("sha3-512"),
            Some(AlgorithmId::Sha3_512)
        );
        assert_eq!(algorithm_from_bsd_tag("NOT-A-HASH"), None);
    }

    #[test]
    fn content_sniffing_prefers_strong_signatures() {
        assert_eq!(
            sniff_report_format(b"{\n  \"schema_version\": 1\n}"),
            Some(ReportFormat::Json)
        );
        assert_eq!(
            sniff_report_format(b"# BatchHash Report\n"),
            Some(ReportFormat::Markdown)
        );
        assert_eq!(
            sniff_report_format(b"BatchHash Report\n================\n"),
            Some(ReportFormat::Txt)
        );
        assert_eq!(
            sniff_report_format(b"path,size,mtime,status\n"),
            Some(ReportFormat::Csv)
        );
        let digest = "a".repeat(AlgorithmId::Sha256.hex_len());
        let line = format!("SHA256 (a.mkv) = {digest}\n");
        assert_eq!(
            sniff_report_format(line.as_bytes()),
            Some(ReportFormat::Bsd)
        );
        let md5_line = format!("{}  a.mkv\n", "a".repeat(AlgorithmId::Sha1.hex_len()));
        assert_eq!(
            sniff_report_format(md5_line.as_bytes()),
            Some(ReportFormat::Md5sum)
        );
        assert_eq!(sniff_report_format(b"not a report"), None);
    }

    #[test]
    fn extension_detection_is_case_insensitive() {
        assert_eq!(
            detect_report_format("REPORT.JSON", b""),
            Some(ReportFormat::Json)
        );
        assert_eq!(
            detect_report_format("checksums.sha256", b""),
            Some(ReportFormat::Bsd)
        );
    }
}
