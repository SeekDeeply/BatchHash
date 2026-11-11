import '../models/models.dart';
import '../models/operation_log.dart';
import 'batch_hash_service.dart';
import 'operation_log_store.dart';

/// Decorator that records every service call to [OperationLogStore].
///
/// Wrapping the service instead of logging at each call site means a page
/// cannot forget to record an operation, and the recorded result is the real
/// outcome: a thrown error is logged as a failure before it propagates.
class LoggingBatchHashService implements BatchHashService {
  LoggingBatchHashService({required this.inner, required this.store});

  /// Real service performing the work.
  final BatchHashService inner;

  /// Destination for the audit trail.
  final OperationLogStore store;

  /// Run [body], record the outcome, and rethrow failures unchanged.
  Future<T> _record<T>(
    String kind,
    Future<T> Function() body, {
    String? Function(T value)? detail,
  }) async {
    final DateTime startedAt = DateTime.now();
    final Stopwatch watch = Stopwatch()..start();
    try {
      final T value = await body();
      watch.stop();
      await store.append(
        OperationLogEntry(
          kind: kind,
          status: true,
          startedAt: startedAt,
          detail: detail?.call(value),
          durationMs: watch.elapsedMilliseconds,
        ),
      );
      return value;
    } catch (error) {
      watch.stop();
      await store.append(
        OperationLogEntry(
          kind: kind,
          status: false,
          startedAt: startedAt,
          detail: '$error',
          durationMs: watch.elapsedMilliseconds,
        ),
      );
      rethrow;
    }
  }

  @override
  Future<HardwareInfoModel> probeHardware() =>
      _record('op.probe', inner.probeHardware);

  @override
  Future<RenderingHardwareModel> probeRenderingHardware() =>
      _record('op.probeRendering', inner.probeRenderingHardware);

  @override
  Future<List<AlgorithmInfo>> listAlgorithms() =>
      _record('op.listAlgorithms', inner.listAlgorithms);

  @override
  Future<List<BatchEntry>> scanDirectory(String path, {bool recursive = false}) {
    return _record(
      'op.scan',
      () => inner.scanDirectory(path, recursive: recursive),
      detail: (List<BatchEntry> entries) => '$path · ${entries.length}',
    );
  }

  @override
  Future<List<BatchEntry>> scanFiles(List<String> paths) {
    return _record(
      'op.scanFiles',
      () => inner.scanFiles(paths),
      detail: (List<BatchEntry> entries) => '${entries.length}',
    );
  }

  @override
  String get lastScannedRoot => inner.lastScannedRoot;

  @override
  Stream<HashProgress> computeHashes({
    required List<BatchEntry> entries,
    required List<String> algorithms,
    bool forceSoftware = false,
  }) {
    // The stream itself is not awaited here; the log entry is written when the
    // last sample arrives, which is when the batch actually finished.
    return _recordStream('op.hash', inner.computeHashes(
      entries: entries,
      algorithms: algorithms,
      forceSoftware: forceSoftware,
    ));
  }

  /// Record a stream operation once it completes or fails.
  Stream<T> _recordStream<T>(String kind, Stream<T> source) {
    final DateTime startedAt = DateTime.now();
    final Stopwatch watch = Stopwatch()..start();
    return source.transform(
      StreamTransformer<T, T>.fromHandlers(
        handleDone: (EventSink<T> sink) {
          watch.stop();
          store.append(
            OperationLogEntry(
              kind: kind,
              status: true,
              startedAt: startedAt,
              durationMs: watch.elapsedMilliseconds,
            ),
          );
          sink.close();
        },
        handleError: (Object error, StackTrace stack, EventSink<T> sink) {
          watch.stop();
          store.append(
            OperationLogEntry(
              kind: kind,
              status: false,
              startedAt: startedAt,
              detail: '$error',
              durationMs: watch.elapsedMilliseconds,
            ),
          );
          sink.addError(error, stack);
        },
      ),
    );
  }

  @override
  Future<RenamePlanModel> planRenames({
    required List<BatchEntry> entries,
    required String algorithm,
    required int length,
    bool uppercase = true,
  }) {
    return _record(
      'op.planRenames',
      () => inner.planRenames(
        entries: entries,
        algorithm: algorithm,
        length: length,
        uppercase: uppercase,
      ),
      detail: (RenamePlanModel plan) =>
          '$algorithm · ${plan.rows.length}',
    );
  }

  @override
  Future<RenameExecuteResult> executeRenames(
    RenamePlanModel plan, {
    bool? recursive,
    List<String>? restrictPaths,
  }) {
    return _record(
      'op.executeRenames',
      () => inner.executeRenames(
        plan,
        recursive: recursive,
        restrictPaths: restrictPaths,
      ),
      detail: (RenameExecuteResult result) => '${result.applied}',
    );
  }

  @override
  Future<List<TextHashResult>> hashText({
    required String text,
    required List<String> algorithms,
  }) {
    return _record(
      'op.hashText',
      () => inner.hashText(text: text, algorithms: algorithms),
    );
  }

  @override
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
  }) {
    return _record(
      'op.export',
      () => inner.exportReport(
        report: report,
        formats: formats,
        directory: directory,
      ),
      detail: (List<ExportedReport> files) => '${files.length}',
    );
  }

  @override
  Future<ReportModel> importReport(String path) =>
      _record('op.import', () => inner.importReport(path), detail: (_) => path);

  @override
  Future<List<DiffRowModel>> diffReports({
    required ReportModel left,
    required ReportModel right,
  }) => _record('op.diffReports', () => inner.diffReports(left: left, right: right));

  @override
  Future<void> cancelCurrentOperation() =>
      _record('op.cancel', inner.cancelCurrentOperation);

  @override
  bool get supportsCancellation => inner.supportsCancellation;

  @override
  Stream<DiffProgress> compareReportWithDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = false,
  }) {
    return _recordStream('op.compareDirectory', inner.compareReportWithDirectory(
      report: report,
      directory: directory,
      recursive: recursive,
    ));
  }

  @override
  Future<List<DiffRowModel>> diffReportAgainstDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = false,
  }) {
    return _record(
      'op.diffDirectory',
      () => inner.diffReportAgainstDirectory(
        report: report,
        directory: directory,
        recursive: recursive,
      ),
      detail: (List<DiffRowModel> rows) => '$directory · ${rows.length}',
    );
  }

  @override
  Future<List<HistoryBatchModel>> loadHistory() => inner.loadHistory();

  @override
  Future<bool> undoBatch(String batchId) {
    return _record(
      'op.undo',
      () => inner.undoBatch(batchId),
      detail: (_) => batchId,
    );
  }
}