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

    /// Returns the write, so the stream can stay open until it lands.
    ///
    /// Closing the stream first would let a caller read the log before the
    /// entry exists, which is a real race: a hash run that finishes and is
    /// followed immediately by a history refresh would show nothing.
    Future<void> record({required bool ok, Object? error}) {
      watch.stop();
      final List<BatchEntry> finished = latestByPath.values.toList(growable: false);
      final ({List<HashDigestRecord> records, int truncated}) collected =
          collectDigests(finished);
      return store.append(
        OperationLogEntry(
          kind: 'op.hash',
          status: ok,
          startedAt: startedAt,
          detail: error == null ? '${finished.length} files' : '$error',
          durationMs: watch.elapsedMilliseconds,
          hashes: collected.records,
          hashesTruncated: collected.truncated,
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
            handleDone: (EventSink<HashProgress> sink) async {
              // Await the write, then close: the listener's onDone is the
              // earliest point a caller can reasonably read the log back.
              await record(ok: true);
              sink.close();
            },
            handleError: (
              Object error,
              StackTrace stack,
              EventSink<HashProgress> sink,
            ) async {
              await record(ok: false, error: error);
              sink.addError(error, stack);
            },
          ),
        );
  }

  /// Record a stream operation once it completes or fails.
  ///
  /// The write is awaited before the stream closes. Waiting on disk I/O between
  /// samples would stall the listener, but waiting at the very end costs nothing
  /// and removes a real race: a caller that refreshes right after `onDone` must
  /// see the entry it just caused.
  ///
  /// [OperationLogStore.append] swallows its own errors, so this cannot turn a
  /// completed operation into a failure.
  Stream<T> _recordStream<T>(String kind, Stream<T> source) {
    final DateTime startedAt = DateTime.now();
    final Stopwatch watch = Stopwatch()..start();
    Future<void> record({required bool ok, Object? error}) {
      watch.stop();
      return store.append(
        OperationLogEntry(
          kind: kind,
          status: ok,
          startedAt: startedAt,
          detail: error == null ? null : '$error',
          durationMs: watch.elapsedMilliseconds,
        ),
      );
    }

    return source.transform(
      StreamTransformer<T, T>.fromHandlers(
        handleDone: (EventSink<T> sink) async {
          await record(ok: true);
          sink.close();
        },
        handleError: (Object error, StackTrace stack, EventSink<T> sink) async {
          await record(ok: false, error: error);
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
    // A verification is only useful in the log if it says what it found. The
    // previous record pointed at the directory and nothing else, so history
    // showed "compared" without ever answering "did it match?".
    final DateTime startedAt = DateTime.now();
    final Stopwatch watch = Stopwatch()..start();
    final List<DiffRowModel> latest = <DiffRowModel>[];

    Future<void> record({required bool ok, Object? error}) async {
      watch.stop();
      final String detail = ok
          ? describeVerification(latest, directory: directory)
          : '$error';
      await store.append(
        OperationLogEntry(
          kind: 'op.compareDirectory',
          status: ok,
          startedAt: startedAt,
          detail: detail,
          durationMs: watch.elapsedMilliseconds,
          hashes: ok
              ? digestsFromDiff(latest)
              : const <HashDigestRecord>[],
        ),
      );
    }

    return inner
        .compareReportWithDirectory(
          report: report,
          directory: directory,
          recursive: recursive,
        )
        .transform(
          StreamTransformer<DiffProgress, DiffProgress>.fromHandlers(
            handleData: (DiffProgress progress, EventSink<DiffProgress> sink) {
              // Keep the newest sample: rows are only complete once done.
              if (progress.rows.isNotEmpty) {
                latest
                  ..clear()
                  ..addAll(progress.rows);
              }
              sink.add(progress);
            },
            handleDone: (EventSink<DiffProgress> sink) async {
              await record(ok: true);
              sink.close();
            },
            handleError: (
              Object error,
              StackTrace stack,
              EventSink<DiffProgress> sink,
            ) async {
              await record(ok: false, error: error);
              sink.addError(error, stack);
            },
          ),
        );
  }

  /// One-line verdict for a verification run.
  ///
  /// The wording is derived from the row statuses rather than from the report
  /// alone: `unchanged` is a match, everything else means the file differs or
  /// could not be judged, and lumping those together would report a corrupt row
  /// as a plain mismatch.
  String describeVerification(
    List<DiffRowModel> rows, {
    required String directory,
  }) {
    if (rows.isEmpty) {
      return '$directory · 0';
    }
    int matched = 0;
    int mismatched = 0;
    int unresolved = 0;
    for (final DiffRowModel row in rows) {
      switch (row.status) {
        case DiffStatus.unchanged:
          matched += 1;
        case DiffStatus.changed:
        case DiffStatus.added:
        case DiffStatus.removed:
          mismatched += 1;
        case DiffStatus.unknown:
        case DiffStatus.ambiguous:
        case DiffStatus.duplicateKey:
        case DiffStatus.outOfRoot:
        case DiffStatus.corrupt:
        case DiffStatus.unstable:
          unresolved += 1;
      }
    }
    final List<String> parts = <String>[directory];
    parts.add('match $matched');
    if (mismatched > 0) {
      parts.add('mismatch $mismatched');
    }
    if (unresolved > 0) {
      parts.add('unresolved $unresolved');
    }
    return parts.join(' · ');
  }

  /// Digests of the rows that actually matched.
  ///
  /// Only matching rows are kept: recording every compared file would bury the
  /// log under the contents of the report, while the matching digests are what a
  /// later reader wants to see to believe the verification.
  List<HashDigestRecord> digestsFromDiff(List<DiffRowModel> rows) {
    final List<HashDigestRecord> records = <HashDigestRecord>[];
    for (final DiffRowModel row in rows) {
      if (row.status != DiffStatus.unchanged) {
        continue;
      }
      final Map<String, String> hashes =
          row.right?.hashes ?? row.left?.hashes ?? const <String, String>{};
      for (final MapEntry<String, String> hash in hashes.entries) {
        if (hash.value.isEmpty) {
          continue;
        }
        if (records.length >= kMaxRecordedDigests) {
          return records;
        }
        records.add(
          HashDigestRecord(
            path: row.path,
            algorithm: hash.key,
            digest: hash.value,
          ),
        );
      }
    }
    return records;
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