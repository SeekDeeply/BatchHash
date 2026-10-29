//! End-to-end smoke tests for the developer CLI.
//!
//! These run in CI only (local builds are forbidden in this repository). They
//! exercise the real binary so argument parsing, report export and the suffix
//! planner stay wired to the engines.

use std::fs;
use std::process::Command;

use batchhash_report::format::{import_report, ReportFormat};
use batchhash_types::AlgorithmId;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_batchhash")
}

fn write_file(path: &std::path::Path, contents: &[u8]) {
    fs::create_dir_all(path.parent().expect("file has a parent")).expect("create parent");
    fs::write(path, contents).expect("write fixture");
}

#[test]
fn version_prints_product_and_version() {
    let output = Command::new(bin()).arg("version").output().expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BatchHash"), "stdout: {stdout}");
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "stdout: {stdout}"
    );
}

#[test]
fn algorithms_lists_the_sixteen_registry_entries() {
    let output = Command::new(bin())
        .arg("algorithms")
        .output()
        .expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // One header line plus one line per registered algorithm.
    let rows = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(
        rows,
        batchhash_types::REGISTRY.len() + 1,
        "stdout: {stdout}"
    );
}

#[test]
fn unknown_option_is_rejected() {
    let output = Command::new(bin())
        .args(["hash", "--nonsense", "."])
        .output()
        .expect("run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option"), "stderr: {stderr}");
}

#[test]
fn hash_directory_emits_an_importable_report() {
    let temp = tempfile::tempdir().expect("tempdir");
    write_file(&temp.path().join("a.txt"), b"alpha");
    write_file(&temp.path().join("nested/b.txt"), b"bravo");

    let output = Command::new(bin())
        .args(["hash", "--algo", "md5,sha256", "--format", "json"])
        .arg(temp.path())
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = import_report(&output.stdout, ReportFormat::Json).expect("import report");
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.summary.total, 2);
    assert_eq!(report.summary.ok, 2);
    assert!(report.algorithms.contains(&AlgorithmId::Md5));
    assert!(report.algorithms.contains(&AlgorithmId::Sha256));
    for entry in &report.entries {
        let md5 = entry.hashes.get(&AlgorithmId::Md5).expect("md5 hash");
        assert_eq!(md5.len(), AlgorithmId::Md5.hex_len());
        assert!(md5.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
    let paths: Vec<&str> = report.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"a.txt"), "paths: {paths:?}");
    assert!(paths.contains(&"nested/b.txt"), "paths: {paths:?}");
}

#[test]
fn report_command_round_trips_json_to_csv() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("a.txt");
    write_file(&file, b"alpha");
    let report_path = temp.path().join("report.json");

    let hash = Command::new(bin())
        .args(["hash", "--algo", "md5", "--format", "json", "--out"])
        .arg(&report_path)
        .arg(&file)
        .output()
        .expect("run");
    assert!(
        hash.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&hash.stderr)
    );

    let convert = Command::new(bin())
        .args(["report"])
        .arg(&report_path)
        .args(["--to", "csv"])
        .output()
        .expect("run");
    assert!(
        convert.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&convert.stderr)
    );
    let csv = String::from_utf8_lossy(&convert.stdout);
    assert!(csv.starts_with("path,size,mtime,status"), "csv: {csv}");
    let reimported = import_report(convert.stdout.as_slice(), ReportFormat::Csv).expect("import csv");
    assert_eq!(reimported.entries.len(), 1);
    assert_eq!(reimported.entries[0].path, "a.txt");
}

#[test]
fn suffix_plan_is_a_dry_run() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write_file(&file, b"movie-bytes");

    let output = Command::new(bin())
        .args(["suffix", "plan", "--algo", "md5", "--length", "8"])
        .arg(temp.path())
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dry run"), "stdout: {stdout}");
    assert!(stdout.contains("movie"), "stdout: {stdout}");
    // A dry run never renames anything.
    let names: Vec<String> = fs::read_dir(temp.path())
        .expect("read dir")
        .filter_map(|item| item.ok())
        .map(|item| item.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["movie.mkv".to_string()]);
}

#[test]
fn suffix_apply_then_undo_restores_the_original_name() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file = temp.path().join("movie.mkv");
    write_file(&file, b"movie-bytes");
    let db = temp.path().join("rename.db");

    let apply = Command::new(bin())
        .args(["suffix", "apply", "--algo", "md5", "--length", "8"])
        .arg(temp.path())
        .arg("--db")
        .arg(&db)
        .output()
        .expect("run");
    assert!(
        apply.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let renamed: Vec<String> = fs::read_dir(temp.path())
        .expect("read dir")
        .filter_map(|item| item.ok())
        .map(|item| item.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.ends_with(".db"))
        .collect();
    assert_eq!(renamed.len(), 1, "renamed: {renamed:?}");
    let new_name = renamed[0].clone();
    assert!(
        new_name.starts_with("movie") && new_name.ends_with(".mkv") && new_name.len() > "movie.mkv".len(),
        "new_name: {new_name}"
    );

    let undo = Command::new(bin())
        .args(["suffix", "undo", "--db"])
        .arg(&db)
        .arg("--apply")
        .output()
        .expect("run");
    assert!(
        undo.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&undo.stderr)
    );
    assert!(temp.path().join("movie.mkv").exists(), "original name restored");
}

#[test]
fn extract_classifies_suffixed_and_plain_names() {
    let temp = tempfile::tempdir().expect("tempdir");
    write_file(&temp.path().join("plain.txt"), b"x");
    write_file(&temp.path().join("tagged[ABCDE1234].txt"), b"y");

    let output = Command::new(bin())
        .arg("extract")
        .arg(temp.path())
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(document["total"], 2);
    assert_eq!(document["parsed"], 1);
    assert_eq!(document["not_suffixed"], 1);
    let entries = document["entries"].as_array().expect("entries array");
    let tagged = entries
        .iter()
        .find(|entry| entry["status"] == "parsed")
        .expect("parsed entry");
    assert_eq!(tagged["hex"], "ABCDE1234");
}
