//! Pure full-outer-join diff engine (diff schema v1, project plan §3.7).
//!
//! The engine is intentionally pure: it takes already-produced reports (or
//! realtime entries), computes deterministic comparison keys with
//! [`PathNormalizer`], and joins the two sides. It never touches the filesystem,
//! never fabricates a removal after a cancelled scan and never panics on
//! imported data.

use std::collections::{BTreeMap, BTreeSet};

use batchhash_types::{
    canonical_algorithm_order, join_display_path, AlgorithmId, DiffEntry, DiffMode, DiffReport,
    DiffRow, DiffSource, DiffSourceKind, DiffStatus, DiffStrategy, DiffSummary, EntryStatus,
    ErrorKind, FileEntry, Report, ReportEntry, ReportValidationError, RootAlignment,
    DIFF_SCHEMA_VERSION, TOOL_NAME, TOOL_VERSION,
};

use crate::pathnorm::{NormalizeError, PathNormalizer};

/// Reason attached to rows compared only by presence and size because the two
/// sides do not declare a single common algorithm.
pub const NO_COMMON_ALGORITHMS_REASON: &str =
    "no common algorithms; presence/size comparison only";

/// Reason attached to rows where a file changed while it was being scanned.
pub const UNSTABLE_REASON: &str = "file changed during scan (unstable)";

/// Comparison policy for one diff run.
///
/// `generated_at` is caller-supplied so golden snapshots are byte-stable.
/// Fields are public and every `with_*` setter returns the builder value, so
/// both `DiffOptions { .. }` and fluent construction work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffOptions {
    /// Comparison key strategy.
    pub mode: DiffMode,
    /// Whether path keys are case sensitive.
    pub case_sensitive: bool,
    /// How the two roots were aligned (recorded in the diff strategy).
    pub root_alignment: RootAlignment,
    /// Deterministic timestamp stamped into the diff document.
    pub generated_at: String,
    /// Display label for the left side.
    pub left_label: String,
    /// Display label for the right side.
    pub right_label: String,
    /// Source kind of the left side.
    pub left_kind: DiffSourceKind,
    /// Source kind of the right side.
    pub right_kind: DiffSourceKind,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            mode: DiffMode::RootRelative,
            case_sensitive: false,
            root_alignment: RootAlignment::SameRoot,
            generated_at: String::new(),
            left_label: "left".to_string(),
            right_label: "right".to_string(),
            left_kind: DiffSourceKind::Report,
            right_kind: DiffSourceKind::Report,
        }
    }
}

impl DiffOptions {
    /// Create default options (root-relative, case-insensitive, empty timestamp).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the comparison key strategy.
    pub fn with_mode(mut self, mode: DiffMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the case-sensitivity policy.
    pub fn with_case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    /// Set the reported root alignment.
    pub fn with_root_alignment(mut self, root_alignment: RootAlignment) -> Self {
        self.root_alignment = root_alignment;
        self
    }

    /// Set the injectable timestamp stamped into the diff document.
    pub fn with_generated_at(mut self, generated_at: impl Into<String>) -> Self {
        self.generated_at = generated_at.into();
        self
    }

    /// Set the left side display label.
    pub fn with_left_label(mut self, left_label: impl Into<String>) -> Self {
        self.left_label = left_label.into();
        self
    }

    /// Set the right side display label.
    pub fn with_right_label(mut self, right_label: impl Into<String>) -> Self {
        self.right_label = right_label.into();
        self
    }

    /// Set the left source kind.
    pub fn with_left_kind(mut self, left_kind: DiffSourceKind) -> Self {
        self.left_kind = left_kind;
        self
    }

    /// Set the right source kind.
    pub fn with_right_kind(mut self, right_kind: DiffSourceKind) -> Self {
        self.right_kind = right_kind;
        self
    }
}

/// Failures that prevent a diff from being computed at all.
#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    /// The left report failed schema validation.
    #[error("left report is invalid: {0}")]
    LeftReport(ReportValidationError),
    /// The right report failed schema validation.
    #[error("right report is invalid: {0}")]
    RightReport(ReportValidationError),
}

/// Compare two validated reports with a pure full outer join.
///
/// Both reports are validated first; invalid input returns [`DiffError`]
/// instead of producing a misleading diff. Rows are sorted by normalized key
/// (Unicode code point order) and then raw path, and `generated_at` comes from
/// `options` so output is byte-reproducible.
pub fn diff_reports(
    left: &Report,
    right: &Report,
    options: &DiffOptions,
) -> Result<DiffReport, DiffError> {
    left.validate().map_err(DiffError::LeftReport)?;
    right.validate().map_err(DiffError::RightReport)?;

    let normalizer = PathNormalizer::new(options.case_sensitive);
    let left_index = index_entries(&left.entries, options.mode, &normalizer, &left.root);
    let right_index = index_entries(&right.entries, options.mode, &normalizer, &right.root);
    let compared = compared_algorithms(&left.algorithms, &right.algorithms);
    let rows = join_rows(&left_index, &right_index, &compared, options.mode, true);

    let left_source = DiffSource {
        kind: options.left_kind,
        label: options.left_label.clone(),
        generated_at: Some(left.generated_at.clone()),
        scan_time: None,
    };
    let right_source = DiffSource {
        kind: options.right_kind,
        label: options.right_label.clone(),
        generated_at: Some(right.generated_at.clone()),
        scan_time: None,
    };
    let strategy = build_strategy(
        &left.algorithms,
        &right.algorithms,
        &compared,
        left_source,
        right_source,
        options,
    );
    Ok(build_diff_report(options, strategy, rows, false))
}

/// Compare a validated report against realtime file entries.
///
/// The realtime side has no report header, so its declared algorithms are the
/// union of the hashes actually present on the entries and its display
/// provenance comes from `right_source`. Entries whose relative path escapes the
/// comparison root become [`DiffStatus::OutOfRoot`] rows instead of failing the
/// whole comparison.
pub fn diff_report_vs_entries(
    report: &Report,
    entries: &[FileEntry],
    right_source: DiffSource,
    options: &DiffOptions,
) -> Result<DiffReport, DiffError> {
    report.validate().map_err(DiffError::LeftReport)?;

    let right_entries: Vec<ReportEntry> = entries
        .iter()
        .map(report_entry_from_file_entry)
        .collect();
    let mut right_algorithm_list: Vec<AlgorithmId> = Vec::new();
    for entry in &right_entries {
        for algorithm in entry.hashes.keys() {
            right_algorithm_list.push(*algorithm);
        }
    }
    let right_algorithms = canonical_algorithm_order(&right_algorithm_list);
    let compared = compared_algorithms(&report.algorithms, &right_algorithms);

    let normalizer = PathNormalizer::new(options.case_sensitive);
    let left_index = index_entries(&report.entries, options.mode, &normalizer, &report.root);
    let right_index = index_entries(&right_entries, options.mode, &normalizer, &right_source.label);
    let rows = join_rows(&left_index, &right_index, &compared, options.mode, true);

    let left_source = DiffSource {
        kind: options.left_kind,
        label: options.left_label.clone(),
        generated_at: Some(report.generated_at.clone()),
        scan_time: None,
    };
    let strategy = build_strategy(
        &report.algorithms,
        &right_algorithms,
        &compared,
        left_source,
        right_source,
        options,
    );
    Ok(build_diff_report(options, strategy, rows, false))
}

/// Incremental report-versus-realtime diff.
///
/// [`StreamingDiff::push_entry`] resolves a key as soon as the first realtime
/// entry for it arrives and buffers later duplicates of the same key until
/// [`StreamingDiff::finish`]. The final [`DiffReport`] is authoritative: it is
/// recomputed from all accumulated entries, and unmatched report entries become
/// [`DiffStatus::Removed`] only when the caller reports `scan_completed = true`.
pub struct StreamingDiff {
    normalizer: PathNormalizer,
    options: DiffOptions,
    left: SideIndex,
    left_algorithms: Vec<AlgorithmId>,
    left_generated_at: String,
    right: BTreeMap<String, Vec<ReportEntry>>,
    right_out_of_root: BTreeMap<String, Vec<(ReportEntry, NormalizeError)>>,
    right_root: String,
}

impl StreamingDiff {
    /// Validate the report and start a streaming comparison against it.
    ///
    /// The realtime root is taken from `options.right_label` for absolute-path
    /// mode; root-relative and basename modes ignore it.
    pub fn new(report: &Report, options: &DiffOptions) -> Result<Self, DiffError> {
        report.validate().map_err(DiffError::LeftReport)?;
        let normalizer = PathNormalizer::new(options.case_sensitive);
        let left = index_entries(&report.entries, options.mode, &normalizer, &report.root);
        Ok(Self {
            normalizer,
            options: options.clone(),
            left,
            left_algorithms: report.algorithms.clone(),
            left_generated_at: report.generated_at.clone(),
            right: BTreeMap::new(),
            right_out_of_root: BTreeMap::new(),
            right_root: options.right_label.clone(),
        })
    }

    /// Feed one realtime entry and return the rows that are resolvable now.
    ///
    /// The first entry for a key returns its comparison (or an out-of-root)
    /// row. A second entry with the same key is buffered and returns no row:
    /// duplicates are consolidated into a single [`DiffStatus::DuplicateKey`]
    /// (or [`DiffStatus::Ambiguous`]) row by [`StreamingDiff::finish`].
    pub fn push_entry(&mut self, entry: FileEntry) -> Vec<DiffRow> {
        let report_entry = report_entry_from_file_entry(&entry);
        match key_for_entry(
            &report_entry,
            self.options.mode,
            &self.normalizer,
            &self.right_root,
        ) {
            Ok(key) => {
                let group_len = {
                    let group = self.right.entry(key.clone()).or_default();
                    group.push(report_entry.clone());
                    group.len()
                };
                if group_len > 1 {
                    return Vec::new();
                }
                let compared = self.streaming_compared(&report_entry);
                let right_group = self.right.get(&key).map(|group| group.as_slice());
                rows_for_key(
                    &key,
                    self.left.by_key.get(&key).map(|group| group.as_slice()),
                    right_group,
                    &compared,
                    self.options.mode,
                    true,
                )
                .into_iter()
                .collect()
            }
            Err(error) => {
                let key = report_entry.path.clone();
                self.right_out_of_root
                    .entry(key.clone())
                    .or_default()
                    .push((report_entry, error));
                let (entry_ref, reason) = match self
                    .right_out_of_root
                    .get(&key)
                    .and_then(|group| group.first())
                {
                    Some((entry, error)) => (Some(entry), error.to_string()),
                    None => (None, "path escapes the comparison root".to_string()),
                };
                vec![make_row(
                    key,
                    DiffStatus::OutOfRoot,
                    reason,
                    None,
                    entry_ref,
                    Vec::new(),
                    Vec::new(),
                )]
            }
        }
    }

    /// Finish the comparison.
    ///
    /// Unmatched report entries are marked [`DiffStatus::Removed`] only when
    /// `scan_completed` is true; otherwise they are left untouched and the
    /// summary is flagged `incomplete` so callers never present a cancellation
    /// as a removal.
    pub fn finish(self, scan_completed: bool) -> DiffReport {
        let mut right = SideIndex::default();
        right.by_key = self.right;
        for group in right.by_key.values_mut() {
            group.sort_by_key(|entry| entry.path.clone());
        }
        right.out_of_root = self.right_out_of_root;

        let mut right_algorithm_list: Vec<AlgorithmId> = Vec::new();
        for group in right.by_key.values() {
            for entry in group {
                for algorithm in entry.hashes.keys() {
                    right_algorithm_list.push(*algorithm);
                }
            }
        }
        for group in right.out_of_root.values() {
            for (entry, _) in group {
                for algorithm in entry.hashes.keys() {
                    right_algorithm_list.push(*algorithm);
                }
            }
        }
        let right_algorithms = canonical_algorithm_order(&right_algorithm_list);
        let compared = compared_algorithms(&self.left_algorithms, &right_algorithms);
        let rows = join_rows(
            &self.left,
            &right,
            &compared,
            self.options.mode,
            scan_completed,
        );

        let left_source = DiffSource {
            kind: self.options.left_kind,
            label: self.options.left_label.clone(),
            generated_at: Some(self.left_generated_at),
            scan_time: None,
        };
        let right_source = DiffSource {
            kind: self.options.right_kind,
            label: self.options.right_label.clone(),
            generated_at: None,
            scan_time: None,
        };
        let strategy = build_strategy(
            &self.left_algorithms,
            &right_algorithms,
            &compared,
            left_source,
            right_source,
            &self.options,
        );
        build_diff_report(&self.options, strategy, rows, !scan_completed)
    }

    fn streaming_compared(&self, entry: &ReportEntry) -> Vec<AlgorithmId> {
        self.left_algorithms
            .iter()
            .copied()
            .filter(|algorithm| entry.hashes.contains_key(algorithm))
            .collect()
    }
}

/// One side of the join after key normalization.
#[derive(Debug, Default)]
struct SideIndex {
    by_key: BTreeMap<String, Vec<ReportEntry>>,
    out_of_root: BTreeMap<String, Vec<(ReportEntry, NormalizeError)>>,
}

fn index_entries(
    entries: &[ReportEntry],
    mode: DiffMode,
    normalizer: &PathNormalizer,
    root: &str,
) -> SideIndex {
    let mut index = SideIndex::default();
    for entry in entries {
        match key_for_entry(entry, mode, normalizer, root) {
            Ok(key) => index.by_key.entry(key).or_default().push(entry.clone()),
            Err(error) => {
                index
                    .out_of_root
                    .entry(entry.path.clone())
                    .or_default()
                    .push((entry.clone(), error));
            }
        }
    }
    for group in index.by_key.values_mut() {
        group.sort_by_key(|entry| entry.path.clone());
    }
    index
}

fn key_for_entry(
    entry: &ReportEntry,
    mode: DiffMode,
    normalizer: &PathNormalizer,
    root: &str,
) -> Result<String, NormalizeError> {
    match mode {
        DiffMode::RootRelative => normalizer.normalize(&entry.path),
        DiffMode::Absolute => normalizer.normalize(&join_display_path(root, &entry.path)),
        DiffMode::Basename => normalizer.normalize_basename(&entry.path),
    }
}

fn compared_algorithms(left: &[AlgorithmId], right: &[AlgorithmId]) -> Vec<AlgorithmId> {
    canonical_algorithm_order(left)
        .into_iter()
        .filter(|algorithm| right.contains(algorithm))
        .collect()
}

fn build_strategy(
    left_algorithms: &[AlgorithmId],
    right_algorithms: &[AlgorithmId],
    compared: &[AlgorithmId],
    left_source: DiffSource,
    right_source: DiffSource,
    options: &DiffOptions,
) -> DiffStrategy {
    let mut declared: Vec<AlgorithmId> =
        Vec::with_capacity(left_algorithms.len() + right_algorithms.len());
    declared.extend(left_algorithms.iter().copied());
    declared.extend(right_algorithms.iter().copied());
    DiffStrategy {
        mode: options.mode,
        case_sensitive: options.case_sensitive,
        root_alignment: options.root_alignment,
        algorithms: canonical_algorithm_order(&declared),
        algorithms_compared: compared.to_vec(),
        left: left_source,
        right: right_source,
    }
}

fn build_diff_report(
    options: &DiffOptions,
    strategy: DiffStrategy,
    rows: Vec<DiffRow>,
    incomplete: bool,
) -> DiffReport {
    let summary = DiffSummary::from_rows(&rows, incomplete);
    DiffReport {
        schema_version: DIFF_SCHEMA_VERSION,
        tool: TOOL_NAME.to_string(),
        tool_version: TOOL_VERSION.to_string(),
        generated_at: options.generated_at.clone(),
        strategy,
        summary,
        rows,
    }
}

fn join_rows(
    left: &SideIndex,
    right: &SideIndex,
    compared: &[AlgorithmId],
    mode: DiffMode,
    mark_removed: bool,
) -> Vec<DiffRow> {
    let mut rows: Vec<DiffRow> = Vec::new();

    let mut keys: BTreeSet<&str> = BTreeSet::new();
    for key in left.by_key.keys().chain(right.by_key.keys()) {
        keys.insert(key.as_str());
    }
    for key in keys {
        let left_group = left.by_key.get(key).map(|group| group.as_slice());
        let right_group = right.by_key.get(key).map(|group| group.as_slice());
        if let Some(row) = rows_for_key(
            key,
            left_group,
            right_group,
            compared,
            mode,
            mark_removed,
        ) {
            rows.push(row);
        }
    }

    let mut out_of_root_keys: BTreeSet<&str> = BTreeSet::new();
    for key in left.out_of_root.keys().chain(right.out_of_root.keys()) {
        out_of_root_keys.insert(key.as_str());
    }
    for key in out_of_root_keys {
        let left_item = left.out_of_root.get(key).and_then(|group| group.first());
        let right_item = right.out_of_root.get(key).and_then(|group| group.first());
        let reason = left_item
            .map(|(_, error)| error.to_string())
            .or_else(|| right_item.map(|(_, error)| error.to_string()))
            .unwrap_or_else(|| "path escapes the comparison root".to_string());
        let left_entry = left_item.map(|(entry, _)| entry);
        let right_entry = right_item.map(|(entry, _)| entry);
        rows.push(make_row(
            key.to_string(),
            DiffStatus::OutOfRoot,
            reason,
            left_entry,
            right_entry,
            Vec::new(),
            Vec::new(),
        ));
    }

    rows.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| row_raw_path(a).cmp(row_raw_path(b)))
    });
    rows
}

fn rows_for_key(
    key: &str,
    left_group: Option<&[ReportEntry]>,
    right_group: Option<&[ReportEntry]>,
    compared: &[AlgorithmId],
    mode: DiffMode,
    mark_removed: bool,
) -> Option<DiffRow> {
    let left_count = left_group.map_or(0, |group| group.len());
    let right_count = right_group.map_or(0, |group| group.len());
    let left_first = left_group.and_then(|group| group.first());
    let right_first = right_group.and_then(|group| group.first());

    if mode == DiffMode::Basename && (left_count > 1 || right_count > 1) {
        return Some(make_row(
            key.to_string(),
            DiffStatus::Ambiguous,
            format!(
                "basename appears {} time(s) on left and {} time(s) on right",
                left_count, right_count
            ),
            left_first,
            right_first,
            Vec::new(),
            Vec::new(),
        ));
    }
    if left_count > 1 || right_count > 1 {
        return Some(make_row(
            key.to_string(),
            DiffStatus::DuplicateKey,
            format!(
                "duplicate normalized key: {} on left, {} on right",
                left_count, right_count
            ),
            left_first,
            right_first,
            Vec::new(),
            Vec::new(),
        ));
    }

    match (left_first, right_first) {
        (Some(left), Some(right)) => {
            let (status, reason, algorithms_compared, changed_algorithms) =
                compare_pair(left, right, compared);
            Some(make_row(
                key.to_string(),
                status,
                reason,
                Some(left),
                Some(right),
                algorithms_compared,
                changed_algorithms,
            ))
        }
        (Some(left), None) => {
            if !mark_removed {
                return None;
            }
            if is_unstable(left) {
                Some(make_row(
                    key.to_string(),
                    DiffStatus::Unstable,
                    UNSTABLE_REASON.to_string(),
                    Some(left),
                    None,
                    Vec::new(),
                    Vec::new(),
                ))
            } else if !left.status.is_ok() {
                Some(make_row(
                    key.to_string(),
                    DiffStatus::Unknown,
                    format!(
                        "not comparable: left {}; missing on the right side",
                        describe(left)
                    ),
                    Some(left),
                    None,
                    Vec::new(),
                    Vec::new(),
                ))
            } else {
                Some(make_row(
                    key.to_string(),
                    DiffStatus::Removed,
                    "only in left".to_string(),
                    Some(left),
                    None,
                    Vec::new(),
                    Vec::new(),
                ))
            }
        }
        (None, Some(right)) => {
            if is_unstable(right) {
                Some(make_row(
                    key.to_string(),
                    DiffStatus::Unstable,
                    UNSTABLE_REASON.to_string(),
                    None,
                    Some(right),
                    Vec::new(),
                    Vec::new(),
                ))
            } else if !right.status.is_ok() {
                Some(make_row(
                    key.to_string(),
                    DiffStatus::Unknown,
                    format!(
                        "not comparable: right {}; missing on the left side",
                        describe(right)
                    ),
                    None,
                    Some(right),
                    Vec::new(),
                    Vec::new(),
                ))
            } else {
                Some(make_row(
                    key.to_string(),
                    DiffStatus::Added,
                    "only in right".to_string(),
                    None,
                    Some(right),
                    Vec::new(),
                    Vec::new(),
                ))
            }
        }
        (None, None) => None,
    }
}

fn compare_pair(
    left: &ReportEntry,
    right: &ReportEntry,
    compared: &[AlgorithmId],
) -> (DiffStatus, String, Vec<AlgorithmId>, Vec<AlgorithmId>) {
    if is_unstable(left) || is_unstable(right) {
        return (
            DiffStatus::Unstable,
            UNSTABLE_REASON.to_string(),
            Vec::new(),
            Vec::new(),
        );
    }
    if !left.status.is_ok() || !right.status.is_ok() {
        return (
            DiffStatus::Unknown,
            format!(
                "not comparable: left {}, right {}",
                describe(left),
                describe(right)
            ),
            Vec::new(),
            Vec::new(),
        );
    }
    if compared.is_empty() {
        // Degraded policy: no shared algorithm, so presence plus size is all we
        // can honestly claim.
        let status = if left.size == right.size {
            DiffStatus::Unchanged
        } else {
            DiffStatus::Changed
        };
        return (
            status,
            NO_COMMON_ALGORITHMS_REASON.to_string(),
            Vec::new(),
            Vec::new(),
        );
    }

    let mut algorithms_compared: Vec<AlgorithmId> = Vec::new();
    let mut changed_algorithms: Vec<AlgorithmId> = Vec::new();
    for algorithm in compared {
        if let (Some(left_digest), Some(right_digest)) =
            (left.hashes.get(algorithm), right.hashes.get(algorithm))
        {
            algorithms_compared.push(*algorithm);
            if !left_digest.eq_ignore_ascii_case(right_digest) {
                changed_algorithms.push(*algorithm);
            }
        }
    }

    if algorithms_compared.is_empty() {
        return (
            DiffStatus::Unknown,
            "no comparable hash values for the declared algorithms".to_string(),
            Vec::new(),
            Vec::new(),
        );
    }
    if !changed_algorithms.is_empty() {
        let joined = changed_algorithms
            .iter()
            .map(|algorithm| algorithm.stable_id())
            .collect::<Vec<_>>()
            .join(", ");
        return (
            DiffStatus::Changed,
            format!("hash mismatch: {joined}"),
            algorithms_compared,
            changed_algorithms,
        );
    }
    if left.size != right.size {
        return (
            DiffStatus::Corrupt,
            format!(
                "hashes match but size differs: left {}, right {}",
                left.size, right.size
            ),
            algorithms_compared,
            Vec::new(),
        );
    }
    (
        DiffStatus::Unchanged,
        "all compared hashes match".to_string(),
        algorithms_compared,
        Vec::new(),
    )
}

fn is_unstable(entry: &ReportEntry) -> bool {
    entry.error_kind == Some(ErrorKind::Unstable)
}

fn status_word(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Ok => "ok",
        EntryStatus::Failed => "failed",
        EntryStatus::Skipped => "skipped",
    }
}

fn describe(entry: &ReportEntry) -> String {
    match entry.error_kind {
        Some(kind) => format!("{} ({})", status_word(entry.status), kind.as_str()),
        None => status_word(entry.status).to_string(),
    }
}

fn make_row(
    key: String,
    status: DiffStatus,
    reason: String,
    left: Option<&ReportEntry>,
    right: Option<&ReportEntry>,
    algorithms_compared: Vec<AlgorithmId>,
    changed_algorithms: Vec<AlgorithmId>,
) -> DiffRow {
    DiffRow {
        key,
        status,
        reason,
        algorithms_compared,
        changed_algorithms,
        left_status: left.map(|entry| entry.status),
        right_status: right.map(|entry| entry.status),
        left: left.map(to_diff_entry),
        right: right.map(to_diff_entry),
    }
}

fn to_diff_entry(entry: &ReportEntry) -> DiffEntry {
    DiffEntry {
        path: entry.path.clone(),
        size: entry.size,
        mtime: entry.mtime.clone(),
        status: entry.status,
        hashes: entry.hashes.clone(),
        error_kind: entry.error_kind,
        error_message: entry.error_message.clone(),
    }
}

fn report_entry_from_file_entry(entry: &FileEntry) -> ReportEntry {
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

fn row_raw_path(row: &DiffRow) -> &str {
    if let Some(left) = &row.left {
        left.path.as_str()
    } else if let Some(right) = &row.right {
        right.path.as_str()
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use batchhash_types::{HardwareReport, RootKind};
    use indexmap::IndexMap;

    fn make_entry(
        path: &str,
        size: u64,
        hashes: &[(AlgorithmId, String)],
    ) -> ReportEntry {
        let mut map = IndexMap::new();
        for (algorithm, digest) in hashes {
            map.insert(*algorithm, digest.clone());
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

    fn make_report(
        generated_at: &str,
        algorithms: Vec<AlgorithmId>,
        entries: Vec<ReportEntry>,
    ) -> Report {
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
            .with_generated_at("2026-10-20T00:00:00Z")
            .with_left_label("left.json")
            .with_right_label("right.json")
    }

    fn md5(fill: char) -> String {
        fill.to_string().repeat(AlgorithmId::Md5.hex_len())
    }

    fn crc32(fill: char) -> String {
        fill.to_string().repeat(AlgorithmId::Crc32.hex_len())
    }

    #[test]
    fn unchanged_and_changed_rows() {
        let left = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Crc32],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Crc32, crc32('0'))])],
        );
        let right = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Crc32],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Crc32, crc32('0'))])],
        );
        let diff = diff_reports(&left, &right, &options()).expect("valid reports");
        assert_eq!(diff.rows.len(), 1);
        assert_eq!(diff.rows[0].status, DiffStatus::Unchanged);
        assert_eq!(diff.rows[0].algorithms_compared, vec![AlgorithmId::Crc32]);
        assert_eq!(diff.summary.unchanged, 1);

        let right_changed = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Crc32],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Crc32, crc32('1'))])],
        );
        let diff = diff_reports(&left, &right_changed, &options()).expect("valid reports");
        assert_eq!(diff.rows[0].status, DiffStatus::Changed);
        assert_eq!(diff.rows[0].changed_algorithms, vec![AlgorithmId::Crc32]);
        assert_eq!(diff.summary.changed, 1);
    }

    #[test]
    fn corrupt_is_same_hashes_with_different_size() {
        let left = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))])],
        );
        let right = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 11, &[(AlgorithmId::Md5, md5('a'))])],
        );
        let diff = diff_reports(&left, &right, &options()).expect("valid reports");
        assert_eq!(diff.rows[0].status, DiffStatus::Corrupt);
        assert_eq!(diff.summary.corrupt, 1);
    }

    #[test]
    fn empty_algorithm_intersection_degrades_to_presence_and_size() {
        let left = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))])],
        );
        let right = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Crc32],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Crc32, crc32('b'))])],
        );
        let diff = diff_reports(&left, &right, &options()).expect("valid reports");
        assert_eq!(diff.rows[0].status, DiffStatus::Unchanged);
        assert_eq!(diff.rows[0].reason, NO_COMMON_ALGORITHMS_REASON);
        assert!(diff.rows[0].algorithms_compared.is_empty());
        assert!(diff.strategy.algorithms_compared.is_empty());
    }

    #[test]
    fn invalid_report_is_rejected() {
        let mut invalid = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))])],
        );
        // Path traversal must never reach the join.
        invalid.entries[0].path = "../a.mkv".to_string();
        let valid = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Md5],
            Vec::new(),
        );
        assert!(matches!(
            diff_reports(&invalid, &valid, &options()),
            Err(DiffError::LeftReport(_))
        ));
    }

    #[test]
    fn cancelled_entries_do_not_fabricate_removed_or_added() {
        let mut cancelled = make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))]);
        cancelled.status = EntryStatus::Skipped;
        cancelled.error_kind = Some(ErrorKind::Cancelled);
        cancelled.hashes.clear();
        let left = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![cancelled],
        );
        let right = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Md5],
            Vec::new(),
        );
        let diff = diff_reports(&left, &right, &options()).expect("valid reports");
        assert_eq!(diff.rows[0].status, DiffStatus::Unknown);
        assert_eq!(diff.summary.removed, 0);

        let diff = diff_reports(&right, &left, &options()).expect("valid reports");
        assert_eq!(diff.rows[0].status, DiffStatus::Unknown);
        assert_eq!(diff.summary.added, 0);
    }

    #[test]
    fn unstable_entries_are_flagged() {
        let mut unstable = make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))]);
        unstable.status = EntryStatus::Failed;
        unstable.error_kind = Some(ErrorKind::Unstable);
        let left = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![unstable],
        );
        let right = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))])],
        );
        let diff = diff_reports(&left, &right, &options()).expect("valid reports");
        assert_eq!(diff.rows[0].status, DiffStatus::Unstable);
        assert_eq!(diff.summary.unstable, 1);
    }

    #[test]
    fn duplicate_normalized_keys_are_one_row_per_key() {
        let left = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![
                make_entry("A.mkv", 10, &[(AlgorithmId::Md5, md5('a'))]),
                make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))]),
            ],
        );
        let right = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))])],
        );
        let diff = diff_reports(&left, &right, &options()).expect("valid reports");
        assert_eq!(diff.rows.len(), 1);
        assert_eq!(diff.rows[0].status, DiffStatus::DuplicateKey);
        assert_eq!(diff.rows[0].key, "a.mkv");
        assert_eq!(diff.summary.duplicate_key, 1);
    }

    #[test]
    fn basename_mode_flags_ambiguity() {
        let left = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Crc32],
            vec![
                make_entry("a/x.mkv", 10, &[(AlgorithmId::Crc32, crc32('0'))]),
                make_entry("b/x.mkv", 10, &[(AlgorithmId::Crc32, crc32('0'))]),
            ],
        );
        let right = make_report(
            "2026-01-02T00:00:00Z",
            vec![AlgorithmId::Crc32],
            vec![make_entry("c/x.mkv", 10, &[(AlgorithmId::Crc32, crc32('0'))])],
        );
        let diff = diff_reports(
            &left,
            &right,
            &options().with_mode(DiffMode::Basename),
        )
        .expect("valid reports");
        assert_eq!(diff.rows.len(), 1);
        assert_eq!(diff.rows[0].status, DiffStatus::Ambiguous);
        assert_eq!(diff.rows[0].key, "x.mkv");
    }

    #[test]
    fn out_of_root_entries_become_rows_in_realtime_mode() {
        let report = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))])],
        );
        let escape = FileEntry::new(
            "root",
            RootKind::Fs,
            "../escape.mkv",
            "/tmp/escape.mkv",
            10,
            None,
        );
        let diff = diff_report_vs_entries(
            &report,
            &[escape],
            DiffSource {
                kind: DiffSourceKind::RealtimeDirectory,
                label: "/data/media".to_string(),
                generated_at: None,
                scan_time: Some("2026-01-02T00:00:00Z".to_string()),
            },
            &options(),
        )
        .expect("valid report");
        assert_eq!(diff.rows.len(), 2);
        let out_of_root = diff
            .rows
            .iter()
            .find(|row| row.status == DiffStatus::OutOfRoot)
            .expect("out-of-root row");
        assert_eq!(diff.summary.out_of_root, 1);
        assert!(out_of_root.right.is_some());
        // The realtime side did not hash the report entry, but it is still
        // present on the report side, so it is reported as removed only because
        // this is a complete (non-streaming) comparison.
        assert_eq!(diff.summary.removed, 1);
    }

    fn file_entry(path: &str, digest: impl Into<String>) -> FileEntry {
        let mut entry = FileEntry::new("root", RootKind::Fs, path, path, 10, None);
        entry.hashes.insert(AlgorithmId::Md5, digest.into());
        entry
    }

    #[test]
    fn streaming_reports_incrementally_and_respects_cancellation() {
        let report = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![
                make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))]),
                make_entry("b.mkv", 10, &[(AlgorithmId::Md5, md5('b'))]),
                make_entry("c.mkv", 10, &[(AlgorithmId::Md5, md5('c'))]),
            ],
        );
        let mut streaming = StreamingDiff::new(&report, &options()).expect("valid report");
        let first = streaming.push_entry(file_entry("a.mkv", md5('a')));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].status, DiffStatus::Unchanged);
        let second = streaming.push_entry(file_entry("b.mkv", md5('b')));
        assert_eq!(second.len(), 1);
        let cancelled = streaming.finish(false);
        assert!(cancelled.summary.incomplete);
        assert_eq!(cancelled.summary.removed, 0);
        assert!(cancelled
            .rows
            .iter()
            .all(|row| row.status != DiffStatus::Removed));
        assert_eq!(cancelled.rows.len(), 2);

        let mut streaming = StreamingDiff::new(&report, &options()).expect("valid report");
        let _ = streaming.push_entry(file_entry("a.mkv", md5('a')));
        let complete = streaming.finish(true);
        assert!(!complete.summary.incomplete);
        assert_eq!(complete.summary.removed, 2);
        assert_eq!(complete.rows.len(), 3);
    }

    #[test]
    fn streaming_buffers_duplicate_keys_until_finish() {
        let report = make_report(
            "2026-01-01T00:00:00Z",
            vec![AlgorithmId::Md5],
            vec![make_entry("a.mkv", 10, &[(AlgorithmId::Md5, md5('a'))])],
        );
        let mut streaming = StreamingDiff::new(&report, &options()).expect("valid report");
        let first = streaming.push_entry(file_entry("a.mkv", md5('a')));
        assert_eq!(first.len(), 1);
        let duplicate = streaming.push_entry(file_entry("sub/a.mkv", md5('a')));
        assert!(duplicate.is_empty());
        let diff = streaming.finish(true);
        assert_eq!(diff.rows.len(), 1);
        assert_eq!(diff.rows[0].status, DiffStatus::DuplicateKey);
    }
}
