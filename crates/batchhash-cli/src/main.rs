//! BatchHash developer CLI.
//!
//! This binary exists for CI smoke coverage and for debugging through CI-run
//! jobs. It is not a distribution artifact (see CONTRIBUTING.md) and no release
//! workflow packages it.
//!
//! The CLI is a thin adapter over the engine crates: every behaviour lives in
//! batchhash-hash, batchhash-report or batchhash-rename, so the desktop UI and
//! the CLI cannot drift apart.
//!
//! Commands:
//!
//! * hash - hash files or directories and emit a report in any export format.
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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use batchhash_rename::log_store::{
    apply_undo, BatchRecord, LogStore, OperationState, RenameOperation, UndoStatus,
};
use batchhash_rename::planner::{
    execute_plan, plan_renames, RenamePlan, RenamePlanOptions, RenameResultStatus, RenameRowStatus,
    StdFsBackend, StdRenameFs,
};
use batchhash_rename::suffix::{parse_filename, SuffixFormat, SuffixParseResult};
use batchhash_report::diff::{diff_report_vs_entries, diff_reports, DiffOptions};
use batchhash_report::format::{
    detect_report_format, export_diff, export_report, import_report, ReportFormat,
};
use batchhash_types::{
    AlgorithmId, DiffReport, DiffSource, DiffSourceKind, DiffStatus, EntryStatus, ErrorKind,
    FileEntry, HardwareReport, Report, ReportEntry, RootKind,
};

type CliResult<T> = Result<T, String>;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(&argv) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("batchhash: {error}");
            std::process::exit(1);
        }
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
        other => Err(format!(
            "unknown command '{other}' (run 'batchhash help')"
        )),
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
// small helpers
// ---------------------------------------------------------------------------

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn rfc3339_from(time: Option<SystemTime>) -> Option<String> {
    let time = time?;
    let converted: chrono::DateTime<chrono::Utc> = time.into();
    Some(converted.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn default_algorithms() -> Vec<AlgorithmId> {
    vec![
        AlgorithmId::Sha256,
        AlgorithmId::Md5,
        AlgorithmId::Blake3,
    ]
}

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
        .ok_or_else(|| {
            format!("unknown format '{name}' (json, csv, markdown, txt, md5sum, bsd)")
        })
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

/// Convert an OS path into the forward-slash relative form used by reports.
fn relative_slash(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .filter(|part| !part.is_empty() && part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_entry(path: &Path, relative: &str, size: u64, mtime: Option<SystemTime>) -> FileEntry {
    FileEntry::new(
        "root",
        RootKind::Fs,
        relative.to_string(),
        display_path(path),
        size,
        rfc3339_from(mtime),
    )
}

/// Walk a root and build unhashed entries. Directories are recursed in sorted
/// order; symlinks are skipped unless follow_links is set (loops are detected).
fn collect_entries(root: &Path, follow_links: bool) -> CliResult<Vec<FileEntry>> {
    let metadata =
        fs::metadata(root).map_err(|error| format!("cannot stat {}: {error}", root.display()))?;
    let mut entries = Vec::new();
    if metadata.is_file() {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| display_path(root));
        entries.push(file_entry(
            root,
            &name,
            metadata.len(),
            metadata.modified().ok(),
        ));
        return Ok(entries);
    }
    if !metadata.is_dir() {
        return Err(format!(
            "{} is neither a file nor a directory",
            root.display()
        ));
    }
    let mut visited = BTreeSet::new();
    visited.insert(root.to_path_buf());
    walk(root, root, follow_links, 0, &mut visited, &mut entries)?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

fn walk(
    root: &Path,
    current: &Path,
    follow_links: bool,
    depth: usize,
    visited: &mut BTreeSet<PathBuf>,
    entries: &mut Vec<FileEntry>,
) -> CliResult<()> {
    if depth > 64 {
        return Err(format!("directory tree too deep at {}", current.display()));
    }
    let read_dir = match fs::read_dir(current) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            // A single unreadable directory must not abort the batch; it is
            // reported as a failed entry instead.
            let mut relative = current
                .strip_prefix(root)
                .map(relative_slash)
                .unwrap_or_else(|_| relative_slash(current));
            if relative.is_empty() {
                relative = ".".to_string();
            }
            let mut entry = file_entry(current, &relative, 0, None);
            entry.status = EntryStatus::Failed;
            entry.error_kind = Some(ErrorKind::PermissionDenied);
            entry.error_message = Some(error.to_string());
            entries.push(entry);
            return Ok(());
        }
    };
    let mut children: Vec<_> = read_dir.filter_map(|item| item.ok()).collect();
    children.sort_by_key(|item| item.file_name());
    for child in children {
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map(relative_slash)
            .unwrap_or_else(|_| child.file_name().to_string_lossy().into_owned());
        let file_type = match child.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                let mut entry = file_entry(&path, &relative, 0, None);
                entry.status = EntryStatus::Failed;
                entry.error_kind = Some(ErrorKind::IoError);
                entry.error_message = Some(error.to_string());
                entries.push(entry);
                continue;
            }
        };
        if file_type.is_symlink() && !follow_links {
            continue;
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                let mut entry = file_entry(&path, &relative, 0, None);
                entry.status = EntryStatus::Failed;
                entry.error_kind = Some(ErrorKind::NotFound);
                entry.error_message = Some(error.to_string());
                entries.push(entry);
                continue;
            }
        };
        if metadata.is_dir() {
            if !visited.insert(path.clone()) {
                continue;
            }
            walk(root, &path, follow_links, depth + 1, visited, entries)?;
            continue;
        }
        entries.push(file_entry(
            &path,
            &relative,
            metadata.len(),
            metadata.modified().ok(),
        ));
    }
    Ok(())
}

fn report_entry(entry: &FileEntry) -> ReportEntry {
    ReportEntry {
        path: entry.relative_path.clone(),
        size: entry.size,
        mtime: entry.mtime.clone(),
        hashes: entry.hashes.clone(),
        status: entry.status,
        error_kind: entry.error_kind,
        error_message: entry.error_message.clone(),
        suffix_applied: None,
        suffix_algorithm: None,
        renamed_to: None,
    }
}

fn default_case_sensitive() -> bool {
    !(cfg!(windows) || cfg!(target_os = "macos"))
}

fn hardware_report() -> HardwareReport {
    batchhash_hash::probe_auto().into()
}

fn build_report(
    root: &str,
    algorithms: &[AlgorithmId],
    entries: &[FileEntry],
    hardware: HardwareReport,
) -> Report {
    let mut report = Report::new(
        root,
        RootKind::Fs,
        default_case_sensitive(),
        algorithms.to_vec(),
        hardware,
    );
    report.generated_at = now_rfc3339();
    report.entries = entries.iter().map(report_entry).collect();
    report.recompute_summary();
    report.sort_entries();
    report
}

/// Hash entries with the configured algorithms and print a coarse progress
/// meter on stderr when stderr is a terminal.
fn hash_batch(
    entries: &[FileEntry],
    algorithms: &[AlgorithmId],
    progress: bool,
) -> CliResult<batchhash_hash::BatchOutcome> {
    let options = batchhash_hash::HashEntryOptions {
        hasher: batchhash_hash::HashOptions::new(algorithms.to_vec()),
        ..batchhash_hash::HashEntryOptions::default()
    };
    let cancel = batchhash_hash::CancelToken::new();
    let last_emitted = AtomicUsize::new(0);
    let events = |event: batchhash_hash::BatchEvent| match event {
        batchhash_hash::BatchEvent::Progress(snapshot) => {
            if !progress {
                return;
            }
            let percent = snapshot.percent as usize;
            let previous = last_emitted.swap(percent, Ordering::Relaxed);
            if percent >= previous + 10 || percent == 100 {
                eprint!(
                    "\r  {:>3}%  {}/{}  {}",
                    percent, snapshot.completed, snapshot.total, snapshot.current_path
                );
            }
        }
        batchhash_hash::BatchEvent::EntryDone(_) => {}
        batchhash_hash::BatchEvent::Finished(_) => {}
    };
    let outcome = batchhash_hash::hash_entries(entries, &options, &cancel, &events);
    if progress {
        eprintln!();
    }
    Ok(outcome)
}

fn terminal_progress(parsed: &Parsed) -> bool {
    !parsed.switch("quiet") && std::io::IsTerminal::is_terminal(&std::io::stderr())
}

// ---------------------------------------------------------------------------
// hash
// ---------------------------------------------------------------------------

fn cmd_hash(args: &[String]) -> CliResult<()> {
    let parsed = Parsed::parse(
        args,
        &["algo", "format", "out", "root"],
        &["follow-links", "quiet"],
    )?;
    if parsed.positional.is_empty() {
        return Err("hash requires at least one path".to_string());
    }
    let algorithms = match parsed.value("algo") {
        Some(list) => parse_algorithms(list)?,
        None => default_algorithms(),
    };
    let format = match parsed.value("format") {
        Some(name) => parse_format(name)?,
        None => ReportFormat::Json,
    };
    let progress = terminal_progress(&parsed);

    let mut entries = Vec::new();
    let mut roots = Vec::new();
    for item in &parsed.positional {
        let path = PathBuf::from(item);
        if path.is_dir() {
            roots.push(display_path(&path));
        }
        entries.extend(collect_entries(&path, parsed.switch("follow-links"))?);
    }
    if entries.is_empty() {
        return Err("no files found to hash".to_string());
    }
    let root = match parsed.value("root") {
        Some(root) => root.to_string(),
        None => roots.first().cloned().unwrap_or_else(|| {
            display_path(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        }),
    };

    eprintln!(
        "hashing {} file(s) with {} algorithm(s): {}",
        entries.len(),
        algorithms.len(),
        algorithms
            .iter()
            .map(|algorithm| algorithm.stable_id())
            .collect::<Vec<_>>()
            .join(",")
    );
    let outcome = hash_batch(&entries, &algorithms, progress)?;
    let report = build_report(&root, &algorithms, &outcome.entries, hardware_report());
    report
        .validate()
        .map_err(|error| format!("internal report validation failed: {error}"))?;
    let text = export_report(&report, format).map_err(|error| error.to_string())?;
    emit(&text, parsed.value("out").map(Path::new))?;
    eprintln!(
        "done: {} ok, {} failed, {} skipped in {} ms ({} bytes read)",
        report.summary.ok,
        report.summary.failed,
        report.summary.skipped,
        outcome.elapsed_ms,
        outcome.bytes_hashed
    );
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
        report.summary.total, report.summary.ok, report.summary.failed, report.summary.skipped,
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
    let entries = collect_entries(&directory, parsed.switch("follow-links"))?;
    if entries.is_empty() {
        return Err("the directory scan produced no files".to_string());
    }
    let outcome = hash_batch(&entries, &report.algorithms, terminal_progress(&parsed))?;
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
    let diff = diff_report_vs_entries(&report, &outcome.entries, right_source, &options)
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

fn build_suffix_format(parsed: &Parsed) -> CliResult<SuffixFormat> {
    let algorithm = match parsed.value("algo") {
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
        Some("upper") | None => batchhash_types::HexCase::Upper,
        Some("lower") => batchhash_types::HexCase::Lower,
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
    let format = build_suffix_format(&parsed)?;
    let execute = apply && !parsed.switch("dry-run");

    let entries = collect_entries(&directory, parsed.switch("follow-links"))?;
    if entries.is_empty() {
        return Err("no files found".to_string());
    }
    let outcome = hash_batch(&entries, &[format.algorithm], terminal_progress(&parsed))?;

    let options = RenamePlanOptions {
        format,
        ..RenamePlanOptions::default()
    };
    let plan = plan_renames(&outcome.entries, &options, &StdRenameFs);

    if !execute {
        let text = render_plan(&plan, format);
        emit(&text, parsed.value("out").map(Path::new))?;
        eprintln!(
            "dry run: {} ready, {} conflict(s), {} skipped, {} missing hash, {} invalid name, {} no permission",
            plan.summary.ready,
            plan.summary.conflicts,
            plan.summary.skipped,
            plan.summary.missing_hash,
            plan.summary.invalid_name,
            plan.summary.no_permission
        );
        return Ok(());
    }

    if plan.summary.ready == 0 {
        return Err("nothing to rename: no row is ready".to_string());
    }
    let db_path = match parsed.value("db") {
        Some(path) => PathBuf::from(path),
        None => directory.join(".batchhash-rename.db"),
    };
    let mut store = open_store(&db_path)?;
    let batch = BatchRecord::new(
        display_path(&directory),
        RootKind::Fs,
        vec![format.algorithm],
        format,
    );
    store
        .create_batch(&batch)
        .map_err(|error| error.to_string())?;

    let mut operation_ids = BTreeMap::new();
    for row in plan
        .rows
        .iter()
        .filter(|row| row.status == RenameRowStatus::Ready)
    {
        let entry = outcome
            .entries
            .get(row.entry_index)
            .ok_or_else(|| format!("plan row {} points outside the entry list", row.entry_index))?;
        let full_hash = entry
            .hashes
            .get(&format.algorithm)
            .cloned()
            .ok_or_else(|| "internal error: missing hash for a ready row".to_string())?;
        let new_path = Path::new(&row.original_path).with_file_name(&row.proposed_name);
        let operation = RenameOperation::new(
            batch.id.clone(),
            row.original_path.clone(),
            display_path(&new_path),
            format.algorithm,
            full_hash,
            suffix_token(&row.proposed_name).unwrap_or_default(),
        );
        let id = store
            .insert_operation(&operation)
            .map_err(|error| error.to_string())?;
        operation_ids.insert(row.entry_index, id);
    }

    let results = execute_plan(&plan, &mut StdFsBackend);
    let mut renamed = 0usize;
    let mut failed = 0usize;
    for result in &results {
        let Some(id) = operation_ids.get(&result.entry_index).copied() else {
            continue;
        };
        match result.status {
            RenameResultStatus::Renamed => {
                renamed += 1;
                store
                    .record_result(id, "renamed", None, OperationState::Applied)
                    .map_err(|error| error.to_string())?;
            }
            RenameResultStatus::Failed => {
                failed += 1;
                store
                    .record_result(
                        id,
                        "failed",
                        Some(result.error.as_deref().unwrap_or("rename_failed")),
                        OperationState::Conflict,
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    eprintln!(
        "renamed {renamed} file(s), {failed} failure(s); batch {} logged to {}",
        batch.id,
        db_path.display()
    );
    Ok(())
}

fn open_store(path: &Path) -> CliResult<LogStore> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
    }
    LogStore::open(path)
        .map_err(|error| format!("cannot open rename log {}: {error}", path.display()))
}

fn suffix_token(name: &str) -> Option<String> {
    match parse_filename(name) {
        SuffixParseResult::Parsed(parsed) => Some(parsed.token),
        _ => None,
    }
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
            batchhash_types::HexCase::Upper => "upper",
            batchhash_types::HexCase::Lower => "lower",
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
    let mut store = open_store(Path::new(db))?;
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
        let results = apply_undo(&plan, &mut StdFsBackend, &mut store)
            .map_err(|error| error.to_string())?;
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
    let entries = collect_entries(Path::new(target), parsed.switch("follow-links"))?;
    let mut rows = Vec::new();
    let mut parsed_count = 0usize;
    let mut unsuffixed = 0usize;
    let mut unparseable = 0usize;
    for entry in &entries {
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
        "total": entries.len(),
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
    let hardware: HardwareReport = batchhash_hash::probe(policy).into();
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
    println!("  hash <path...>            Hash files/directories and emit a report");
    println!("      --algo <list>         Comma separated ids (default sha256,md5,blake3)");
    println!("      --format <fmt>        json | csv | markdown | txt | md5sum | bsd");
    println!("      --out <file>          Write to a file instead of stdout");
    println!("      --root <path>         Override the report root label");
    println!("      --follow-links        Follow symlinks (loops are still detected)");
    println!("  report <file>             Detect, import and re-export a report");
    println!("      --format <fmt>        Force the input format");
    println!("      --to <fmt> --out <file>");
    println!("  diff <left> <right>       Compare two report files");
    println!("      --to <fmt> --out <file> --case-sensitive | --case-insensitive");
    println!("  scan-diff <report> <dir>  Compare a report against a live directory scan");
    println!("  suffix plan <dir>         Dry-run the suffix rename plan");
    println!("  suffix apply <dir>        Execute the plan and write the SQLite rename log");
    println!("      --algo --length --case --prefix --db <path> --dry-run");
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
