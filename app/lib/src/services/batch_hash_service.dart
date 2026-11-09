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

  /// List the files under [path].
  ///
  /// [recursive] must be honoured: a folder the user picked as "only this
  /// folder" may not silently grow into its whole subtree. Implementations that
  /// cannot honour it must fail loudly rather than return a superset.
  Future<List<BatchEntry>> scanDirectory(String path, {bool recursive = false});

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

  /// Write [report] in [formats].
  ///
  /// Without [directory], the implementation picks a stable per-user location
  /// (never the system temp directory, which the OS may clear).
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
  });

  Future<ReportModel> importReport(String path);

  Future<List<DiffRowModel>> diffReports({
    required ReportModel left,
    required ReportModel right,
  });

  Future<List<DiffRowModel>> diffReportAgainstDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = true,
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
