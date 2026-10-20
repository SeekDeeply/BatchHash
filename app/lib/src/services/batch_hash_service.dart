import '../models/models.dart';

/// Boundary between the Flutter UI and the Rust core.
///
/// The P0 application ships with a deterministic fake implementation so every
/// page and widget test runs without native code. P0.5 replaces the fake with
/// a flutter_rust_bridge backed implementation without changing this contract.
abstract class BatchHashService {
  Future<HardwareInfoModel> probeHardware();

  Future<RenderingHardwareModel> probeRenderingHardware();

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

  Future<RenameExecuteResult> executeRenames(RenamePlanModel plan);

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

  Future<List<HistoryBatchModel>> loadHistory();

  Future<bool> undoBatch(String batchId);
}
