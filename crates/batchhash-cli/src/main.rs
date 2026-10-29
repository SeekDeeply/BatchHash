//! BatchHash developer CLI.
//!
//! This binary exists for CI smoke coverage and for debugging through CI-run
//! jobs. It is not a distribution artifact (see CONTRIBUTING.md) and no release
//! workflow packages it.
//!
//! The CLI is deliberately thin: scanning, hashing, suffix planning/execution,
//! report building and the SQLite rename log all live in batchhash-core, which is
//! also the entry point of the Flutter bridge. The CLI only parses arguments,
//! renders tables and writes files.
//!
//! Commands:
//!
//! * hash - hash files or directories, optionally writing filename suffixes.
//! * report - detect, import and re-export a report (format conversion).
//! * diff - compare two report files.
//! * scan-diff - compare a stored report against a live directory scan.
//! * suffix plan/apply/undo - suffix rename dry-run, execution and undo.
//! * extract - parse suffixes out of filenames without hashing anything.
//! * probe - print the hardware capability report as JSON.
//! * algorithms - list the 16 registered algorithms.
//! * version / help.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use batchhash_core::pipeline::{
    default_case_sensitive, now_rfc3339, run_batch, BatchRequest, BatchRun, PipelineEvent,
};
use batchhash_core::scan::{display_path, scan, ScanOptions};
use batchhash_rename::log_store::{apply_undo, LogStore, UndoStatus};
use batchhash_rename::planner::{
    RenamePlan, RenameResultStatus, RenameRowStatus, StdFsBackend, StdRenameFs,
};
use batchhash_rename::suffix::{parse_filename, SuffixFormat, SuffixParseResult};
use batchhash_report::diff::{diff_report_vs_entries, diff_reports, DiffOptions};
use batchhash_report::format::{
    detect_report_format, export_diff, export_report, import_report, ReportFormat,
};
use batchhash_types::{
    AlgorithmId, DiffReport, DiffSource, DiffSourceKind, DiffStatus, HexCase, Report,
};

type CliResult<T> = Result<T, String>;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&argv) {
        eprintln!("batchhash: {error}");
        std::process::exit(1);
    }
}

fn run(argv: &[String]) -> CliResult<()> {
    let Some(command) = argv.first() else {
        print_help();
        return Ok(());
    };
    let rest = &argv[1..];
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!(
                "{} {}",
                batchhash_types::TOOL_NAME,
                batchhash_types::TOOL_VERSION
            );
            Ok(())
        }
        "algorithms" => {
            print_algorithms();
            Ok(())
        }
        "probe" => cmd_probe(rest),
        "hash" => cmd_hash(rest),
        "report" => cmd_report(rest),
        "diff" => cmd_diff(rest),
        "scan-diff" => cmd_scan_diff(rest),
        "suffix" => cmd_suffix(rest),
        "extract" => cmd_extract(rest),
        other => Err(format!("unknown command '{other}' (run 'batchhash help')")),
    }
}

// ---------------------------------------------------------------------------
// argument parsing
// ---------------------------------------------------------------------------

/// Minimal --flag value / --switch parser with an explicit flag vocabulary, so
/// a typo is an error instead of a silently ignored argument.
struct Parsed {
    positional: Vec<String>,
    values: BTreeMap<String, String>,
    switches: BTreeSet<String>,
}

impl Parsed {
    fn parse(args: &[String], value_flags: &[&str], switch_flags: &[&str]) -> CliResult<Self> {
        let mut parsed = Parsed {
            positional: Vec::new(),
            values: BTreeMap::new(),
            switches: BTreeSet::new(),
        };
        let mut index = 0;
        let mut literal = false;
        while index < args.len() {
            let arg = &args[index];
            if literal {
                parsed.positional.push(arg.clone());
                index += 1;
                continue;
            }
            if arg == "--" {
                literal = true;
                index += 1;
                continue;
            }
            if let Some(name) = arg.strip_prefix("--") {
                let (name, inline) = match name.split_once('=') {
                    Some((name, value)) => (name, Some(value.to_string())),
                    None => (name, None),
                };
                if value_flags.contains(&name) {
                    let value = match inline {
                        Some(value) => value,
                        None => {
                            index += 1;
                            args.get(index)
                                .cloned()
                                .ok_or_else(|| format!("--{name} requires a value"))?
                        }
                    };
                    parsed.values.insert(name.to_string(), value);
                } else if switch_flags.contains(&name) {
                    if inline.is_some() {
                        return Err(format!("--{name} does not take a value"));
                    }
                    parsed.switches.insert(name.to_string());
                } else {
                    return Err(format!("unknown option --{name}"));
                }
                index += 1;
                continue;
            }
            if arg.starts_with('-') && arg.len() > 1 {
                return Err(format!("unknown option {arg}"));
            }
            parsed.positional.push(arg.clone());
            index += 1;
        }
        Ok(parsed)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(|value| value.as_str())
    }

    fn switch(&self, name: &str) -> bool {
        self.switches.contains(name)
    }
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

fn parse_algorithms(list: &str) -> CliResult<Vec<AlgorithmId>> {
    let mut algorithms = Vec::new();
    for raw in list.split(',') {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let id = AlgorithmId::from_stable_id(name)
            .ok_or_else(|| format!("unknown algorithm '{name}' (run 'batchhash algorithms')"))?;
        if !algorithms.contains(&id) {
            algorithms.push(id);
        }
    }
    if algorithms.is_empty() {
        return Err("no algorithms selected".to_string());
    }
    Ok(algorithms)
}

fn parse_format(name: &str) -> CliResult<ReportFormat> {
    ReportFormat::ALL
        .into_iter()
        .find(|format| {
            format.as_str() == name
                || format.extension() == name
                || (*format == ReportFormat::Markdown && name == "md")
        })
        .ok_or_else(|| format!("unknown format '{name}' (json, csv, markdown, txt, md5sum, bsd)"))
}

fn emit(text: &str, out: Option<&Path>) -> CliResult<()> {
    match out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
                }
            }
            fs::write(path, text)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))
        }
        None => {
            let mut stdout = std::io::stdout();
            stdout
                .write_all(text.as_bytes())
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("cannot write stdout: {error}"))
        }
    }
}

fn default_log_path(root: &Path) -> PathBuf {
    let base = if root.is_dir() {
        root.to_path_buf()
    } else {
        root.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    base.join(".batchhash-rename.db")
}

fn build_suffix_format(parsed: &Parsed, algorithm_flag: &str) -> CliResult<SuffixFormat> {
    let algorithm = match parsed.value(algorithm_flag) {
        Some(name) => AlgorithmId::from_stable_id(name)
            .ok_or_else(|| format!("unknown algorithm '{name}'"))?,
        None => batchhash_types::default_suffix_algorithm(),
    };
    let length = match parsed.value("length") {
        Some(raw) => raw
            .parse::<u8>()
            .map_err(|_| format!("invalid --length '{raw}'"))?,
        None => 5,
    };
    let case = match parsed.value("case") {
        Some("upper") | None => HexCase::Upper,
        Some("lower") => HexCase::Lower,
        Some(other) => return Err(format!("invalid --case '{other}' (upper or lower)")),
    };
    let format = SuffixFormat {
        length,
        case,
        algorithm,
        prefix_enabled: parsed.switch("prefix"),
    };
    format
        .validate()
        .map_err(|error| format!("invalid suffix configuration: {error}"))?;
    Ok(format)
}

fn terminal_progress(parsed: &Parsed) -> bool {
    !parsed.switch("quiet") && std::io::IsTerminal::is_terminal(&std::io::stderr())
}

/// Run the shared pipeline and print a coarse progress meter on stderr.
fn execute_pipeline(request: &BatchRequest, progress: bool) -> CliResult<BatchRun> {
    let cancel = batchhash_hash::CancelToken::new();
    let last_percent = AtomicU64::new(0);
    let events = |event: PipelineEvent| {
        if let PipelineEvent::Progress(sample) = event {
            if !progress {
                return;
            }
            let rounded = sample.percent as u64;
            let previous = last_percent.swap(rounded, Ordering::Relaxed);
            if rounded >= previous + 10 || sample.percent >= 100.0 {
                eprint!(
                    "\r  {:>3}%  {}/{}  {}   ",
                    rounded, sample.completed, sample.total, sample.current_path
                );
            }
        }
    };
    let run = run_batch(request, &cancel, &events).map_err(|error| error.to_string())?;
    if progress {
        eprintln!();
    }
    Ok(run)
}

// ---------------------------------------------------------------------------
// hash
// ---------------------------------------------------------------------------

fn cmd_hash(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(
        args,
        &[
            "algo",
            "format",
            "out",
            "root",
            "suffix-algo",
            "length",
            "case",
            "db",
        ],
        &["follow-links", "quiet", "write-suffix", "dry-run", "prefix"],
    )?;
    if parsed.positional.len() != 1 {
        return Err("hash requires exactly one root path (a file or a directory)".to_string());
    }
    let algorithms = match parsed.value("algo") {
        Some(list) => parse_algorithms(list)?,
        None => vec![AlgorithmId::Sha256, AlgorithmId::Md5, AlgorithmId::Blake3],
    };
    let format = match parsed.value("format") {
        Some(name) => parse_format(name)?,
        None => ReportFormat::Json,
    };
    let root_path = PathBuf::from(&parsed.positional[0]);
    let mut request = BatchRequest::new(&root_path, algorithms);
    request.scan = ScanOptions {
        follow_links: parsed.switch("follow-links"),
        ..ScanOptions::default()
    };
    if let Some(root) = parsed.value("root") {
        request.root = root.to_string();
    }
    if parsed.switch("write-suffix") {
        let suffix = build_suffix_format(&parsed, "suffix-algo")?;
        let db_path = match parsed.value("db") {
            Some(path) => PathBuf::from(path),
            None => default_log_path(&root_path),
        };
        request.suffix = Some(suffix);
        request.apply_renames = !parsed.switch("dry-run");
        request.log_db = Some(db_path);
    }

    eprintln!(
        "hashing {} with {} algorithm(s): {}",
        display_path(&root_path),
        request.algorithms.len(),
        request
            .algorithms
            .iter()
            .map(|algorithm| algorithm.stable_id())
            .collect::<Vec<_>>()
            .join(",")
    );
    let run = execute_pipeline(&request, terminal_progress(&parsed))?;
    if run.cancelled {
        return Err("the batch was cancelled".to_string());
    }
    let text = export_report(&run.report, format).map_err(|error| error.to_string())?;
    emit(&text, parsed.value("out").map(Path::new))?;
    eprintln!(
        "done: {} ok, {} failed, {} skipped in {} ms",
        run.report.summary.ok,
        run.report.summary.failed,
        run.report.summary.skipped,
        run.elapsed_ms
    );
    if let Some(plan) = &run.plan {
        eprintln!(
            "suffix: {} ready, {} conflict(s), {} skipped, {} missing hash",
            plan.summary.ready,
            plan.summary.conflicts,
            plan.summary.skipped,
            plan.summary.missing_hash
        );
        if let Some(batch_id) = &run.batch_id {
            eprintln!(
                "renamed {} file(s); batch {batch_id}",
                run.rename_results
                    .iter()
                    .filter(|result| result.status == RenameResultStatus::Renamed)
                    .count()
            );
        } else {
            eprintln!("dry run: no file was renamed");
        }
    }
    for skip in &run.skipped_paths {
        eprintln!("skipped {}: {}", skip.path, skip.reason);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// report / diff / scan-diff
// ---------------------------------------------------------------------------

fn read_report(path: &Path, forced: Option<ReportFormat>) -> CliResult<Report> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let format = match forced.or_else(|| detect_report_format(&name, &bytes)) {
        Some(format) => format,
        None => {
            return Err(format!(
                "cannot detect the report format of {} (pass --format)",
                path.display()
            ))
        }
    };
    import_report(&bytes, format)
        .map_err(|error| format!("cannot import {}: {error}", path.display()))
}

fn cmd_report(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(args, &["format", "to", "out"], &[])?;
    let Some(input) = parsed.positional.first() else {
        return Err("report requires an input file".to_string());
    };
    let from = match parsed.value("format") {
        Some(name) => Some(parse_format(name)?),
        None => None,
    };
    let report = read_report(Path::new(input), from)?;
    let to = match parsed.value("to") {
        Some(name) => parse_format(name)?,
        None => ReportFormat::Json,
    };
    let text = export_report(&report, to).map_err(|error| error.to_string())?;
    emit(&text, parsed.value("out").map(Path::new))?;
    eprintln!(
        "{} entrie(s): {} ok, {} failed, {} skipped, root {}",
        report.summary.total,
        report.summary.ok,
        report.summary.failed,
        report.summary.skipped,
        report.root
    );
    Ok(())
}

fn parse_case_sensitivity(parsed: &Parsed) -> bool {
    if parsed.switch("case-sensitive") {
        return true;
    }
    if parsed.switch("case-insensitive") {
        return false;
    }
    default_case_sensitive()
}

fn cmd_diff(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(
        args,
        &["format", "to", "out"],
        &["case-sensitive", "case-insensitive"],
    )?;
    if parsed.positional.len() != 2 {
        return Err("diff requires exactly two report files".to_string());
    }
    let forced = match parsed.value("format") {
        Some(name) => Some(parse_format(name)?),
        None => None,
    };
    let left_path = Path::new(&parsed.positional[0]);
    let right_path = Path::new(&parsed.positional[1]);
    let left = read_report(left_path, forced)?;
    let right = read_report(right_path, forced)?;
    let options = DiffOptions::new()
        .with_case_sensitive(parse_case_sensitivity(&parsed))
        .with_generated_at(now_rfc3339())
        .with_left_label(display_path(left_path))
        .with_right_label(display_path(right_path));
    let diff = diff_reports(&left, &right, &options).map_err(|error| error.to_string())?;
    let to = match parsed.value("to") {
        Some(name) => parse_format(name)?,
        None => ReportFormat::Markdown,
    };
    let text = export_diff(&diff, to).map_err(|error| error.to_string())?;
    emit(&text, parsed.value("out").map(Path::new))?;
    print_diff_summary(&diff);
    Ok(())
}

fn cmd_scan_diff(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(
        args,
        &["format", "to", "out"],
        &[
            "case-sensitive",
            "case-insensitive",
            "follow-links",
            "quiet",
        ],
    )?;
    if parsed.positional.len() != 2 {
        return Err("scan-diff requires a report file and a directory".to_string());
    }
    let forced = match parsed.value("format") {
        Some(name) => Some(parse_format(name)?),
        None => None,
    };
    let report_path = PathBuf::from(&parsed.positional[0]);
    let report = read_report(&report_path, forced)?;
    let directory = PathBuf::from(&parsed.positional[1]);
    let mut request = BatchRequest::new(&directory, report.algorithms.clone());
    request.scan = ScanOptions {
        follow_links: parsed.switch("follow-links"),
        ..ScanOptions::default()
    };
    let run = execute_pipeline(&request, terminal_progress(&parsed))?;
    let options = DiffOptions::new()
        .with_case_sensitive(parse_case_sensitivity(&parsed))
        .with_generated_at(now_rfc3339())
        .with_left_label(display_path(&report_path))
        .with_right_label(display_path(&directory))
        .with_left_kind(DiffSourceKind::Report)
        .with_right_kind(DiffSourceKind::RealtimeDirectory);
    let right_source = DiffSource {
        kind: DiffSourceKind::RealtimeDirectory,
        label: display_path(&directory),
        generated_at: None,
        scan_time: Some(now_rfc3339()),
    };
    let diff = diff_report_vs_entries(&report, &run.entries, right_source, &options)
        .map_err(|error| error.to_string())?;
    let to = match parsed.value("to") {
        Some(name) => parse_format(name)?,
        None => ReportFormat::Markdown,
    };
    let text = export_diff(&diff, to).map_err(|error| error.to_string())?;
    emit(&text, parsed.value("out").map(Path::new))?;
    print_diff_summary(&diff);
    Ok(())
}

fn print_diff_summary(diff: &DiffReport) {
    let actionable = diff
        .rows
        .iter()
        .filter(|row| row.status != DiffStatus::Unchanged)
        .count();
    eprintln!(
        "diff: {} row(s), {} non-identical, unchanged {}, incomplete {}",
        diff.summary.total, actionable, diff.summary.unchanged, diff.summary.incomplete
    );
}

// ---------------------------------------------------------------------------
// suffix plan / apply / undo
// ---------------------------------------------------------------------------

fn cmd_suffix(args: &[String]) -> CliResult<()> {
    let Some(sub) = args.first() else {
        return Err("suffix requires a subcommand: plan, apply or undo".to_string());
    };
    let rest = &args[1..];
    match sub.as_str() {
        "plan" => cmd_suffix_plan(rest, false),
        "apply" => cmd_suffix_plan(rest, true),
        "undo" => cmd_suffix_undo(rest),
        other => Err(format!("unknown suffix subcommand '{other}'")),
    }
}

fn cmd_suffix_plan(args: &[String], apply: bool) -> CliResult<()> {
    let parsed = Parsed::parse(
        args,
        &["algo", "length", "case", "db", "out"],
        &["prefix", "dry-run", "quiet", "follow-links"],
    )?;
    let Some(target) = parsed.positional.first() else {
        return Err("suffix plan/apply requires a directory".to_string());
    };
    let directory = PathBuf::from(target);
    let format = build_suffix_format(&parsed, "algo")?;
    let execute = apply && !parsed.switch("dry-run");
    let db_path = match parsed.value("db") {
        Some(path) => PathBuf::from(path),
        None => default_log_path(&directory),
    };

    let mut request = BatchRequest::new(&directory, vec![format.algorithm]);
    request.scan = ScanOptions {
        follow_links: parsed.switch("follow-links"),
        ..ScanOptions::default()
    };
    request.suffix = Some(format);
    request.apply_renames = execute;
    request.log_db = Some(db_path.clone());

    let run = execute_pipeline(&request, terminal_progress(&parsed))?;
    let plan = run
        .plan
        .as_ref()
        .ok_or_else(|| "no plan produced".to_string())?;
    let text = render_plan(plan, format);
    emit(&text, parsed.value("out").map(Path::new))?;
    if execute {
        let renamed = run
            .rename_results
            .iter()
            .filter(|result| result.status == RenameResultStatus::Renamed)
            .count();
        eprintln!(
            "renamed {renamed} file(s) of {} ready; batch {} logged to {}",
            plan.summary.ready,
            run.batch_id.as_deref().unwrap_or("<none>"),
            db_path.display()
        );
    } else {
        eprintln!(
            "dry run: {} ready, {} conflict(s), {} skipped, {} missing hash, {} invalid name, {} no permission",
            plan.summary.ready,
            plan.summary.conflicts,
            plan.summary.skipped,
            plan.summary.missing_hash,
            plan.summary.invalid_name,
            plan.summary.no_permission
        );
    }
    Ok(())
}

/// Human-readable dry-run table; the UI renders the same plan data.
fn render_plan(plan: &RenamePlan, format: SuffixFormat) -> String {
    let mut text = String::new();
    text.push_str("# BatchHash rename plan (dry run)\n\n");
    text.push_str(&format!(
        "Suffix format: [{}] - algorithm {}, {} hex chars, {} case\n\n",
        if format.prefix_enabled {
            format!("{}:", format.algorithm.stable_id())
        } else {
            String::new()
        },
        format.algorithm.stable_id(),
        format.length,
        match format.case {
            HexCase::Upper => "upper",
            HexCase::Lower => "lower",
        }
    ));
    text.push_str("| # | Original | Proposed | Status | Reason |\n");
    text.push_str("|---:|---|---|---|---|\n");
    for (index, row) in plan.rows.iter().enumerate() {
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            index + 1,
            row.original_name,
            row.proposed_name,
            row_status_label(row.status),
            row.reason.as_deref().unwrap_or("")
        ));
    }
    text
}

fn row_status_label(status: RenameRowStatus) -> &'static str {
    match status {
        RenameRowStatus::Ready => "ready",
        RenameRowStatus::ConflictTargetExists => "conflict",
        RenameRowStatus::SkippedAlreadySuffixed => "skipped",
        RenameRowStatus::MissingHash => "missing_hash",
        RenameRowStatus::InvalidName => "invalid_name",
        RenameRowStatus::NoPermission => "no_permission",
        RenameRowStatus::Unsupported => "unsupported",
        RenameRowStatus::Error => "error",
    }
}

fn cmd_suffix_undo(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(args, &["db", "batch", "out"], &["apply", "dry-run"])?;
    let Some(db) = parsed.value("db") else {
        return Err("suffix undo requires --db <path>".to_string());
    };
    let mut store =
        LogStore::open(db).map_err(|error| format!("cannot open rename log {db}: {error}"))?;
    let batch_id = match parsed.value("batch") {
        Some(id) => id.to_string(),
        None => {
            let batches = store.list_batches().map_err(|error| error.to_string())?;
            batches
                .last()
                .map(|batch| batch.id.clone())
                .ok_or_else(|| "the rename log is empty".to_string())?
        }
    };
    let plan = store
        .plan_undo(&batch_id, &StdRenameFs)
        .map_err(|error| error.to_string())?;
    let mut text = String::new();
    text.push_str(&format!("# BatchHash undo plan for batch {batch_id}\n\n"));
    text.push_str("| # | From | To | Status | Reason |\n|---:|---|---|---|---|\n");
    let mut ready = 0usize;
    for (index, row) in plan.rows.iter().enumerate() {
        let status = match row.status {
            UndoStatus::Ready => {
                ready += 1;
                "ready"
            }
            UndoStatus::Conflict => "conflict",
            UndoStatus::AlreadyUndone => "already_undone",
        };
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            index + 1,
            row.from,
            row.to,
            status,
            row.reason.as_deref().unwrap_or("")
        ));
    }
    emit(&text, parsed.value("out").map(Path::new))?;
    if parsed.switch("apply") && !parsed.switch("dry-run") {
        let results =
            apply_undo(&plan, &mut StdFsBackend, &mut store).map_err(|error| error.to_string())?;
        eprintln!(
            "undo: {} operation(s) processed, {ready} were ready",
            results.len()
        );
    } else {
        eprintln!("undo (dry run): {ready} row(s) ready; pass --apply to execute");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// extract / probe
// ---------------------------------------------------------------------------

fn cmd_extract(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(args, &["out"], &["follow-links"])?;
    let Some(target) = parsed.positional.first() else {
        return Err("extract requires a directory".to_string());
    };
    let options = ScanOptions {
        follow_links: parsed.switch("follow-links"),
        ..ScanOptions::default()
    };
    let outcome = scan(Path::new(target), &options).map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    let mut parsed_count = 0usize;
    let mut unsuffixed = 0usize;
    let mut unparseable = 0usize;
    for entry in &outcome.entries {
        let name = Path::new(&entry.display_path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let row = match parse_filename(&name) {
            SuffixParseResult::Parsed(parsed) => {
                parsed_count += 1;
                serde_json::json!({
                    "path": entry.relative_path,
                    "filename": name,
                    "status": "parsed",
                    "token": parsed.token,
                    "hex": parsed.hex,
                    "algorithm": parsed.algorithm.map(|algorithm| algorithm.stable_id()),
                    "ambiguous": parsed.ambiguous,
                })
            }
            SuffixParseResult::NotSuffixed => {
                unsuffixed += 1;
                serde_json::json!({
                    "path": entry.relative_path,
                    "filename": name,
                    "status": "not_suffixed",
                })
            }
            SuffixParseResult::Unparseable => {
                unparseable += 1;
                serde_json::json!({
                    "path": entry.relative_path,
                    "filename": name,
                    "status": "unparseable",
                })
            }
        };
        rows.push(row);
    }
    let document = serde_json::json!({
        "root": target,
        "total": outcome.entries.len(),
        "parsed": parsed_count,
        "not_suffixed": unsuffixed,
        "unparseable": unparseable,
        "entries": rows,
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("cannot serialize extract output: {error}"))?;
    emit(&format!("{text}\n"), parsed.value("out").map(Path::new))?;
    eprintln!(
        "extract: {parsed_count} suffixed, {unsuffixed} unsuffixed, {unparseable} unparseable"
    );
    Ok(())
}

fn cmd_probe(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(args, &["out"], &["forced-soft"])?;
    let policy = if parsed.switch("forced-soft") {
        batchhash_types::BackendPolicy::ForcedSoft
    } else {
        batchhash_types::BackendPolicy::Auto
    };
    let hardware: batchhash_types::HardwareReport = batchhash_hash::probe(policy).into();
    let document = serde_json::json!({
        "tool": batchhash_types::TOOL_NAME,
        "tool_version": batchhash_types::TOOL_VERSION,
        "task": "hash",
        "arch": hardware.arch,
        "detected_features": hardware.detected_features,
        "backend_policy": hardware.backend_policy,
        "blake3_backend_reported": hardware.blake3_backend_reported,
        "sha256_backend_reported": hardware.sha256_backend_reported,
        "source": hardware.source,
        "multithread": hardware.multithread,
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("cannot serialize probe output: {error}"))?;
    emit(&format!("{text}\n"), parsed.value("out").map(Path::new))
}

// ---------------------------------------------------------------------------
// help
// ---------------------------------------------------------------------------

fn print_help() {
    println!("BatchHash CLI (developer tool, not a distribution build)");
    println!();
    println!("USAGE:");
    println!("  batchhash <command> [options]");
    println!();
    println!("COMMANDS:");
    println!("  hash <path>               Hash a file or directory and emit a report");
    println!("      --algo <list>         Ids (default sha256,md5,blake3)");
    println!("      --format <fmt>        json | csv | markdown | txt | md5sum | bsd");
    println!("      --out <file>          Write to a file instead of stdout");
    println!("      --root <label>        Override the report root label");
    println!("      --follow-links        Follow symlinks (loops are still detected)");
    println!("      --write-suffix        Also write filename suffixes from a digest");
    println!("      --suffix-algo <id>    Algorithm used for the suffix token");
    println!("      --length <n> --case upper|lower --prefix");
    println!("      --dry-run             Only plan the suffixes, never rename");
    println!("      --db <path>           Rename log path");
    println!("  report <file>             Detect, import and re-export a report");
    println!("  diff <left> <right>       Compare two report files");
    println!("  scan-diff <report> <dir>  Compare a report against a live directory scan");
    println!("  suffix plan <dir>         Dry-run the suffix rename plan");
    println!("  suffix apply <dir>        Execute the plan and write the rename log");
    println!("  suffix undo --db <path> [--batch <id>] [--apply]");
    println!("  extract <dir>             Parse suffixes out of filenames (JSON)");
    println!("  probe                     Print the hardware capability report (JSON)");
    println!("  algorithms                List the 16 registered hash algorithms");
    println!("  version                   Print tool name and version");
    println!("  help                      Show this help");
    println!();
    println!("All builds run in CI; this binary is for developers, never for distribution.");
}

fn print_algorithms() {
    println!(
        "{:<10} {:<14} {:>4} {:>5}  SUFFIX",
        "ID", "DISPLAY", "HEX", "CRYPTO"
    );
    for spec in batchhash_types::REGISTRY.iter() {
        println!(
            "{:<10} {:<14} {:>4} {:>5}  {}",
            spec.stable_id,
            spec.display_name,
            spec.hex_len,
            if spec.cryptographic { "yes" } else { "no" },
            if spec.suffix_usable { "yes" } else { "no" }
        );
    }
}
