# BatchHash internal API contracts (P0)

This document is the integration contract between the Rust crates. It is owned by
the Lead. Public items listed here **must** keep the given names and signatures;
additive changes are fine, renames are not.

Owner map:

| Area | Crate | Owner |
|---|---|---|
| AlgorithmRegistry, FileEntry, report/diff schema types | `crates/batchhash-types` | Lead |
| Hash engine, fd input, batch scheduler, HardwareProbe | `crates/batchhash-hash` | hash-engine |
| PathNormalizer, Diff Engine, report export/import | `crates/batchhash-report` | report-diff |
| Suffix, rename plan/execute, SQLite log store | `crates/batchhash-rename` | rename-log |
| Facade, CLI, CI, Flutter shell, docs | rest | Lead |

All public items carry rustdoc. Library crates deny `missing_docs`. No `unwrap()`
outside tests except for documented invariants (use `expect`).

## Environment

```sh
export PATH=/data/dsh/home/.cargo/bin:/data/dsh/home/bin:$PATH
export CC=/data/dsh/home/bin/cc
export CXX=/data/dsh/home/bin/c++
cd /data/dsh/home/BatchHash
cargo test -p <crate>
```

Do not run `git` commands, do not edit files outside your crate, do not touch
`Cargo.toml` workspace members or the shared types crate.

## batchhash-hash

```rust
pub mod hasher;   // single-pass multi-hash core
pub mod probe;    // HardwareProbe
pub mod scheduler;// Rayon batch execution, progress, cancel
pub mod fd;       // #[cfg(unix)] dup + seek(offset) + limited length

pub use hasher::{HashOptions, Blake3Policy, compute_hashes, HashError, hash_reader, HashSink};
pub use probe::{probe, HardwareInfo, BackendPolicy};
pub use scheduler::{hash_entries, BatchEvent, BatchOutcome, CancelToken, ProgressSnapshot, HashEntryOptions};
pub use fd::compute_hashes_from_fd;
```

* `HashOptions { algorithms: Vec<AlgorithmId>, buffer_size: usize, blake3: Blake3Policy, forced_soft: bool, mmap_threshold: Option<u64> }`
  * `buffer_size` default 4 MiB, clamped 64 KiB..=64 MiB, one buffer per worker.
  * `Blake3Policy { Auto, Disabled }`; multi-algorithm runs must use `Disabled` (single-pass I/O).
* `compute_hashes(reader: &mut dyn Read, options: &HashOptions) -> Result<IndexMap<AlgorithmId, String>, HashError>`
  reads the stream **exactly once**, feeds all hashers per chunk, returns lowercase hex in registry order.
* `HashSink` trait allowing a caller-supplied I/O counter; the single-pass test uses it.
* `compute_hashes_from_fd(fd: RawFd, offset: u64, length: Option<u64>, options: &HashOptions) -> Result<IndexMap<AlgorithmId,String>, HashError>`
  must `dup(2)` the fd, `seek` to `offset`, read at most `length`, and **never close the original fd**.
* `hash_entries(entries: &[FileEntry], options: &HashEntryOptions, cancel: &CancelToken, events: &(dyn Fn(BatchEvent) + Sync)) -> BatchOutcome`
  * Rayon cross-file parallelism, progress aggregated atomically, `BatchEvent::{Progress, EntryDone, Finished}`.
  * cancellation is checked at chunk boundaries; cancelled entries are `Skipped` with `ErrorKind::Cancelled`.
  * `BatchOutcome { entries, cancelled, elapsed_ms, bytes_hashed }`.
* `probe() -> HardwareInfo` never lies: unknown values are `"unknown"`, `source` is
  `crate-runtime`/`cpufeatures`/`inferred`. Never assert a specific accelerator in tests.
* fd/offset tests: have a test proving offset != 0, length-limited reading and that the
  caller's fd is still usable after the call.

## batchhash-report

```rust
pub mod pathnorm;
pub mod diff;
pub mod format;

pub use pathnorm::{PathNormalizer, NormalizeMode, NormalizeError};
pub use diff::{DiffOptions, diff_reports, diff_report_vs_entries, StreamingDiff};
pub use format::{ReportFormat, export_report, export_diff, import_report, detect_report_format, ExportError, ImportError};
```

* `PathNormalizer::new(case_sensitive: bool)`; `normalize(&str) -> Result<String, NormalizeError>`
  implements the 9-step pipeline of plan §3.7.2: dot-segment resolution (escape → `OutOfRoot`),
  `\\`→`/`, collapse/trim separators, NFC, case policy, lowercase-then-NFC.
  `normalize_basename(&str)` for degraded mode. `relative_to_root(absolute, root)` returns
  `OutOfRoot` when the path is not under the root.
* `diff_reports(left: &Report, right: &Report, options: &DiffOptions) -> DiffReport`
  pure full outer join; statuses exactly as `batchhash_types::DiffStatus`;
  algorithm intersection for comparison; empty intersection degrades to
  presence/size comparison with a reason string; `corrupt` = same hash + different size;
  one-sided duplicate normalized keys become `duplicate_key` rows; basename-mode collisions
  become `ambiguous`; sorting by key code points then raw path.
* `DiffOptions { mode, case_sensitive, root_alignment, generated_at, left_label, right_label }`
  (fields may be private with a builder; `Default` + `with_*` setters are fine).
  `generated_at` must be injectable so golden snapshots are byte-stable.
* `diff_report_vs_entries(...)` and `StreamingDiff` implement realtime comparison:
  `push_entry` emits rows as soon as one file is hashed; `finish(scan_completed)` marks
  unmatched report entries `removed` **only** when `scan_completed`; otherwise `incomplete = true`.
* `ReportFormat { Json, Csv, Markdown, Txt, Md5sum, Bsd }` with `can_import()`/`can_diff()`.
  Exports: JSON (canonical pretty), CSV (fixed columns, registry algorithm order, RFC 4180,
  UTF-8 no BOM by default), Markdown (500-row truncation + `truncated`), TXT (aligned,
  500-row truncation), md5sum, BSD.
  Imports: JSON/CSV/md5sum/BSD only. md5sum/BSD imports infer a single algorithm from digest
  length (ambiguity → `ImportError::AmbiguousAlgorithm`), set `root = ""`, `root_kind = fs`.
  JSON/CSV export → import → export must be **byte identical**; add round-trip tests.
  Import enforces `MAX_IMPORT_BYTES`, `MAX_IMPORT_ENTRIES`, rejects future schema versions,
  and rejects path traversal via `batchhash_types::validate_relative_path`.

## batchhash-rename

```rust
pub mod suffix;
pub mod planner;
pub mod log_store;

pub use suffix::{SuffixFormat, SuffixParseResult, ParsedSuffix, format_filename, parse_filename,
                 collision_probability, recommended_length, SuffixError};
pub use planner::{RenamePlan, RenamePlanRow, RenameRowStatus, RenamePlanOptions, plan_renames,
                  execute_plan, RenameBackend, StdFsBackend, RenameFs, RenameResult};
pub use log_store::{LogStore, BatchRecord, RenameOperation, OperationState, UndoPlan, UndoStatus};
```

* Filename format `original[ABCDE].ext`, 4..=16 hex chars, upper default, extension preserved,
  no-extension files get `filename[ABCDE]`, long-name precheck counts UTF-16 units and truncates
  the stem (never the hash), already-suffixed names are skipped (`SuffixParseResult::Parsed`).
  Optional `prefix:` form (`original[blake3:ABCDE].ext`) is off by default.
* `recommended_length(count)` table: ≤100 → 5, ≤1,000 → 8, ≤100,000 → 10, else 12.
  `collision_probability(count, len)` is the birthday estimate over `4^len` (hex alphabet, 16^len).
* Planner is pure: `plan_renames(entries, options, fs: &dyn RenameFs)` produces dry-run rows with
  statuses (ready / conflict target exists / skipped already suffixed / missing hash / invalid name /
  no permission / unsupported). Execution takes a `RenameBackend` so Android can substitute Kotlin.
* LogStore (SQLite, rusqlite bundled): tables `batch`, `rename_operation`, `schema_migration`;
  state machine `pending → applied → undone|conflict`; undo plans batches newest-first, checks the
  renamed file still exists and the original name is free, marks conflicts instead of overwriting.
  Every write is an explicit call returning `Result`; use transactions.

## Cross-crate rules

* `batchhash-types` is read-only for everyone except the Lead.
* Hex digests are lowercase everywhere; `AlgorithmId` ordering is canonical registry order.
* Errors: `thiserror` enums, no panics on malformed input (imports are untrusted).
* Tests: unit + golden tests in your crate; `cargo test -p <crate>` must be green before you report.
* Run `cargo fmt` and `cargo clippy -p <crate> --all-targets -- -D warnings` before reporting.
* Report back: files created, public API deviations, test command + summary, known gaps.
