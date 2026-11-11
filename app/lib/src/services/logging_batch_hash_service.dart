import 'dart:async';

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
    List<HashDigestRecord> Function(T value)? digests,
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
          hashes: digests?.call(value) ?? const <HashDigestRecord>[],
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
    // Digests only exist once the run finishes, so the samples are watched and
    // the log entry is written at the end rather than when the call starts.
    final DateTime startedAt = DateTime.now();
    final Stopwatch watch = Stopwatch()..start();
    // Keyed by path so a re-reported entry replaces its earlier sample instead
    // of duplicating it; a batch reports each file once per algorithm set.
    final Map<String, BatchEntry> latestByPath = <String, BatchEntry>{};

    void record({required bool ok, Object? error}) {
      watch.stop();
      final List<BatchEntry> finished = latestByPath.values.toList(growable: false);
      final ({List<HashDigestRecord> records, int truncated}) collected =
          collectDigests(finished);
      unawaited(
        store.append(
          OperationLogEntry(
            kind: 'op.hash',
            status: ok,
            startedAt: startedAt,
            detail: error == null
                ? '${finished.length} files'
                : '$error',
            durationMs: watch.elapsedMilliseconds,
            hashes: collected.records,
            hashesTruncated: collected.truncated,
          ),
        ),
      );
    }

    return inner
        .computeHashes(
          entries: entries,
          algorithms: algorithms,
          forceSoftware: forceSoftware,
        )
        .transform(
          StreamTransformer<HashProgress, HashProgress>.fromHandlers(
            handleData: (HashProgress progress, EventSink<HashProgress> sink) {
              for (final BatchEntry entry in progress.entries) {
                if (entry.hashes.isNotEmpty) {
                  latestByPath[entry.path] = entry;
                }
              }
              sink.add(progress);
            },
            handleDone: (EventSink<HashProgress> sink) {
              record(ok: true);
              sink.close();
            },
            handleError: (Object error, StackTrace stack, EventSink<HashProgress> sink) {
              record(ok: false, error: error);
              sink.addError(error, stack);
            },
          ),
        );
  }

  /// Record a stream operation once it completes or fails.
  ///
  /// The write is deliberately fire-and-forget: a stream listener must not wait
  /// on disk I/O between samples. [OperationLogStore.append] already swallows
  /// its own errors, so an unawaited call cannot surface as an unhandled
  /// exception.
  Stream<T> _recordStream<T>(String kind, Stream<T> source) {
    final DateTime startedAt = DateTime.now();
    final Stopwatch watch = Stopwatch()..start();
    void record({required bool ok, Object? error}) {
      watch.stop();
      unawaited(
        store.append(
          OperationLogEntry(
            kind: kind,
            status: ok,
            startedAt: startedAt,
            detail: error == null ? null : '$error',
            durationMs: watch.elapsedMilliseconds,
          ),
        ),
      );
    }

    return source.transform(
      StreamTransformer<T, T>.fromHandlers(
        handleDone: (EventSink<T> sink) {
          record(ok: true);
          sink.close();
        },
        handleError: (Object error, StackTrace stack, EventSink<T> sink) {
          record(ok: false, error: error);
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
    // The pasted text itself is not recorded: history may be exported or read
    // by someone else, and the digest is the verifiable part anyway.
    return _record(
      'op.hashText',
      () => inner.hashText(text: text, algorithms: algorithms),
      detail: (List<TextHashResult> results) =>
          '${text.length} chars · ${results.length}',
      digests: (List<TextHashResult> results) => <HashDigestRecord>[
        for (final TextHashResult result in results)
          HashDigestRecord(
            path: 'text',
            algorithm: result.algorithmId,
            digest: result.digest,
          ),
      ],
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