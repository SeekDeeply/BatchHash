import '../models/models.dart';

/// Boundary between the Flutter UI and the Rust core.
///
/// The app ships with a deterministic fake implementation so every page and
/// widget test runs without native code. On a real build the contract is
/// satisfied by `RustBatchHashService`, which calls the Rust core through the
/// JSON C ABI + `dart:ffi` bridge (see docs/bridge.md). No page may depend on
/// which implementation it received.
abstract class BatchHashService {
  Future<HardwareInfoModel> probeHardware();

  Future<RenderingHardwareModel> probeRenderingHardware();

  /// Every algorithm this build can compute, in the core's canonical order.
  ///
  /// The UI builds its picker from this list, including
  /// [AlgorithmInfo.displayName] and [AlgorithmInfo.category]; it must not
  /// hardcode algorithm IDs. The returned list is never empty: an
  /// implementation
  /// without a native core falls back to [kAlgorithmCatalogue].
  Future<List<AlgorithmInfo>> listAlgorithms();

  /// List the files under [path].
  ///
  /// [recursive] must be honoured: a folder the user picked as "only this
  /// folder" may not silently grow into its whole subtree. Implementations that
  /// cannot honour it must fail loudly rather than return a superset.
  Future<List<BatchEntry>> scanDirectory(String path, {bool recursive = false});

  /// Picked files are kept as an explicit set; implementations must not
  /// rescan only the last selected file when hashing them.
  Future<List<BatchEntry>> scanFiles(List<String> paths);

  /// Directory the most recent scan or explicit selection is rooted at.
  ///
  /// Always a directory, so it can be used as a report root; empty before the
  /// first scan.
  String get lastScannedRoot;

  /// Compute [algorithms] for [entries].
  ///
  /// Entries come from either [scanDirectory] or [scanFiles]; implementations
  /// must hash exactly the entries they were given and never widen the set to
  /// a whole folder behind the caller's back.
  Stream<HashProgress> computeHashes({
    required List<BatchEntry> entries,
    required List<String> algorithms,
    bool forceSoftware = false,
  });

  Future<RenamePlanModel> planRenames({
    required List<BatchEntry> entries,
    required String algorithm,
    required int length,
    bool uppercase = true,
  });

  /// Apply [plan].
  ///
  /// [restrictPaths] narrows the run to exactly these root-relative paths; when
  /// null, the implementation derives them from the plan's ready rows. Passing
  /// an empty list is an error, never "no restriction": applying a preview must
  /// not touch a file the preview did not show. [recursive] overrides the
  /// retained scan mode when the caller knows better.
  Future<RenameExecuteResult> executeRenames(
    RenamePlanModel plan, {
    bool? recursive,
    List<String>? restrictPaths,
  });

  /// Hash in-memory [text] with [algorithms].
  ///
  /// Used by the text comparison view to show each side's digest, so a user
  /// can tell whether two pasted blobs really match without trusting the diff
  /// alone. Implementations must hash exactly the bytes given: no newline
  /// normalisation, no trailing-newline trimming, since either would make the
  /// digest disagree with hashing the same text as a file.
  Future<List<TextHashResult>> hashText({
    required String text,
    required List<String> algorithms,
  });
  /// Write [report] in [formats].
  ///
  /// Without [directory], the implementation picks a stable per-user location
  /// (never the system temp directory, which the OS may clear).
  ///
  /// [locale] selects the language of the human readable formats (Markdown,
  /// TXT, Tree). The machine readable formats (JSON, CSV, md5sum, BSD) never
  /// translate their keys or columns, because those are a parsing contract.
  /// Omit it to keep the historical English output.
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
    String? locale,
  });

  Future<ReportModel> importReport(String path);

  Future<List<DiffRowModel>> diffReports({
    required ReportModel left,
    required ReportModel right,
  });

  /// Ask the running operation to stop.
  ///
  /// Implementations that cannot cancel must still complete the future; the
  /// UI hides the cancel affordance when [supportsCancellation] is false.
  Future<void> cancelCurrentOperation();

  /// Whether [cancelCurrentOperation] can actually stop a run.
  bool get supportsCancellation => false;

  /// Compare [report] against a live [directory], streaming progress.
  ///
  /// A directory comparison hashes every file it scans, so it takes as long as
  /// a batch of the same size; the stream exists so the UI can show that work
  /// instead of freezing until it finishes.
  Stream<DiffProgress> compareReportWithDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = false,
  });
  Future<List<DiffRowModel>> diffReportAgainstDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = false,
  });

  /// Every batch this app has produced, newest first.
  ///
  /// A fresh launch has no in-memory state, so implementations must consult
  /// persisted roots and not only the root of the current session.
  Future<List<HistoryBatchModel>> loadHistory();

  /// Undo [batchId].
  ///
  /// A successful undo invalidates any cached report: serializing the pre-undo
  /// state afterwards would be a lie.
  Future<bool> undoBatch(String batchId);
}