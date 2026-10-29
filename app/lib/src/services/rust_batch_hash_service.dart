import 'dart:convert';
import 'dart:io';

import '../bridge/native_bridge.dart';
import '../models/models.dart';
import '../platform/rendering_hardware.dart';
import 'batch_hash_service.dart';

/// [BatchHashService] backed by the Rust core through the native bridge.
///
/// Every operation that touches files, digests or the rename log is executed by
/// Rust; this class only maps JSON DTOs onto the UI models. When the native
/// library is missing the app falls back to [FakeBatchHashService], so widget
/// tests and unsupported platforms keep working.
class RustBatchHashService implements BatchHashService {
  RustBatchHashService({
    required this.native,
    RenderingHardwareProvider renderingProvider =
        const UnknownRenderingHardwareProvider(),
    String? logDbPath,
  }) : _renderingProvider = renderingProvider,
       _logDbPath = logDbPath;

  /// Open the native library and return null when it is not available.
  static RustBatchHashService? tryCreate({
    Map<String, String>? environment,
    String? overridePath,
    RenderingHardwareProvider renderingProvider =
        const UnknownRenderingHardwareProvider(),
  }) {
    final native = BatchHashNative.tryOpen(
      environment: environment,
      overridePath: overridePath,
    );
    if (native == null) {
      return null;
    }
    return RustBatchHashService(
      native: native,
      renderingProvider: renderingProvider,
    );
  }

  final BatchHashNative native;
  final RenderingHardwareProvider _renderingProvider;
  String? _logDbPath;
  String? _lastRoot;
  String? _lastReportJson;

  /// Root of the most recent scan; used by rename execution and history.
  String? get lastRoot => _lastRoot;

  /// Rename log path used by this service.
  String? get logDbPath => _logDbPath;

  @override
  Future<HardwareInfoModel> probeHardware() async {
    final data = native.probe();
    return HardwareInfoModel(
      arch: data['arch'] as String? ?? 'unknown',
      detectedFeatures:
          (data['detected_features'] as List<Object?>? ?? const [])
              .map((feature) => feature.toString())
              .toList(),
      blake3BackendReported:
          data['blake3_backend_reported'] as String? ?? 'unknown',
      sha256BackendReported:
          data['sha256_backend_reported'] as String? ?? 'unknown',
      backendPolicy: data['backend_policy'] as String? ?? 'unknown',
      multithread: ((data['multithread'] as num?)?.toInt() ?? 1) > 1,
    );
  }

  @override
  Future<RenderingHardwareModel> probeRenderingHardware() {
    return _renderingProvider.probe();
  }

  @override
  Future<List<BatchEntry>> scanDirectory(
    String path, {
    bool recursive = false,
  }) async {
    final data = native.scan(path);
    _lastRoot = path;
    final entries = _entries(data['entries']);
    if (recursive) {
      return entries;
    }
    return entries.where((entry) => !entry.path.contains('/')).toList();
  }

  @override
  Stream<HashProgress> computeHashes({
    required List<BatchEntry> entries,
    required List<String> algorithms,
    bool forceSoftware = false,
  }) async* {
    if (entries.isEmpty) {
      yield const HashProgress(completed: 0, total: 0, done: true);
      return;
    }
    final root = _lastRoot ?? _commonRoot(entries);
    final data = native.runBatch(
      root: root,
      algorithms: algorithms,
      caseSensitive: false,
    );
    _lastRoot = root;
    _lastReportJson = data['report_json'] as String?;
    // The C ABI call is a single blocking run, so progress is reported as one
    // completion event; a streaming variant needs a callback handle model.
    final hashed = _entries(data['entries']);
    final result = hashed.isEmpty ? entries : hashed;
    yield HashProgress(
      completed: result.length,
      total: result.length,
      entries: result,
      done: true,
    );
  }

  @override
  Future<RenamePlanModel> planRenames({
    required List<BatchEntry> entries,
    required String algorithm,
    required int length,
    bool uppercase = true,
  }) async {
    final data = native.planRenames(
      root: _lastRoot,
      entries: entries.map(_entryJson).toList(),
      algorithm: algorithm,
      length: length,
      uppercase: uppercase,
    );
    final rows = (data['rows'] as List<Object?>? ?? const [])
        .cast<Map<String, Object?>>()
        .map(
          (row) => RenamePlanRowModel(
            path:
                row['original_path'] as String? ??
                row['original_name'] as String? ??
                '',
            status: _renameStatus(row['status'] as String?),
            targetPath: row['proposed_path'] as String?,
            message: row['reason'] as String?,
          ),
        )
        .toList();
    return RenamePlanModel(
      algorithm: algorithm,
      length: length,
      uppercase: uppercase,
      rows: rows,
    );
  }

  @override
  Future<RenameExecuteResult> executeRenames(RenamePlanModel plan) async {
    final root = _lastRoot;
    if (root == null) {
      throw StateError('executeRenames requires a previously scanned root');
    }
    final logDb = _logDbPath ?? '$root/.batchhash-rename.db';
    final data = native.runBatch(
      root: root,
      algorithms: <String>[plan.algorithm],
      suffix: <String, Object?>{
        'algorithm': plan.algorithm,
        'length': plan.length,
        'case': plan.uppercase ? 'upper' : 'lower',
      },
      applyRenames: true,
      logDb: logDb,
    );
    _logDbPath = logDb;
    _lastReportJson = data['report_json'] as String?;
    final summary = data['plan_summary'] as Map<String, Object?>?;
    return RenameExecuteResult(
      applied: (data['renamed'] as num?)?.toInt() ?? 0,
      skipped: (summary?['skipped'] as num?)?.toInt() ?? 0,
      failed: (data['rename_failed'] as num?)?.toInt() ?? 0,
      batchId: data['batch_id'] as String?,
    );
  }

  @override
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
  }) async {
    final reportJson = _reportJson(report);
    final target = Directory(
      directory ?? Directory.systemTemp.createTempSync('batchhash_export').path,
    );
    if (!target.existsSync()) {
      target.createSync(recursive: true);
    }
    final dirPath = target.path;
    final written = <ExportedReport>[];
    for (final format in formats) {
      final text = native.exportReport(
        reportJson: reportJson,
        format: format.name,
      );
      final extension = format.fileExtension;
      final file = File('$dirPath/batchhash-report.$extension');
      file.writeAsStringSync(text);
      written.add(ExportedReport(format: format, path: file.path));
    }
    return written;
  }

  @override
  Future<ReportModel> importReport(String path) async {
    final json = native.importReport(path);
    return _reportFromJson(
      json,
      ReportModel(root: '', generatedAt: DateTime.now().toUtc()),
    );
  }

  @override
  Future<List<DiffRowModel>> diffReports({
    required ReportModel left,
    required ReportModel right,
  }) async {
    final data = native.diffReports(
      leftJson: _reportJson(left),
      rightJson: _reportJson(right),
    );
    return _diffRows(data);
  }

  @override
  Future<List<DiffRowModel>> diffReportAgainstDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = true,
  }) async {
    final data = native.diffDirectory(
      reportJson: _reportJson(report),
      directory: directory,
      followLinks: false,
    );
    return _diffRows(data);
  }

  @override
  Future<List<HistoryBatchModel>> loadHistory() async {
    final db =
        _logDbPath ??
        (_lastRoot == null ? null : '$_lastRoot/.batchhash-rename.db');
    if (db == null || !File(db).existsSync()) {
      return const <HistoryBatchModel>[];
    }
    final batches = native.listBatches(db);
    return batches
        .map(
          (batch) => HistoryBatchModel(
            id: batch['id'] as String? ?? '',
            createdAt:
                DateTime.tryParse(batch['created_at'] as String? ?? '') ??
                DateTime.fromMillisecondsSinceEpoch(0, isUtc: true),
            fileCount: (batch['file_count'] as num?)?.toInt() ?? 0,
            appliedCount: (batch['applied_count'] as num?)?.toInt() ?? 0,
            undone: ((batch['undone_count'] as num?)?.toInt() ?? 0) > 0,
          ),
        )
        .toList();
  }

  @override
  Future<bool> undoBatch(String batchId) async {
    final db = _logDbPath;
    if (db == null) {
      return false;
    }
    final data = native.undo(db: db, batchId: batchId, apply: true);
    return ((data['undone'] as num?)?.toInt() ?? 0) > 0;
  }

  String _reportJson(ReportModel report) {
    final cached = _lastReportJson;
    if (cached != null && report.root == _lastRoot) {
      return cached;
    }
    return native.buildReport(
      root: report.root,
      algorithms: report.algorithms.isEmpty
          ? const <String>['sha256']
          : report.algorithms,
      entries: report.entries.map(_entryJson).toList(),
      generatedAt: report.generatedAt.toUtc().toIso8601String(),
    );
  }

  ReportModel _reportFromJson(String json, ReportModel fallback) {
    if (json.isEmpty) {
      return fallback;
    }
    final decoded = jsonDecode(json);
    if (decoded is! Map<String, Object?>) {
      return fallback;
    }
    return ReportModel(
      schemaVersion: (decoded['schema_version'] as num?)?.toInt() ?? 1,
      root: decoded['root'] as String? ?? fallback.root,
      generatedAt:
          DateTime.tryParse(decoded['generated_at'] as String? ?? '') ??
          fallback.generatedAt,
      algorithms: (decoded['algorithms'] as List<Object?>? ?? const [])
          .map((algorithm) => algorithm.toString())
          .toList(),
      entries: _entries(decoded['entries']),
    );
  }

  List<DiffRowModel> _diffRows(Map<String, Object?> data) {
    return (data['rows'] as List<Object?>? ?? const [])
        .cast<Map<String, Object?>>()
        .map(
          (row) => DiffRowModel(
            path: row['path'] as String? ?? '',
            status: _diffStatus(row['status'] as String?),
            left: _optionalEntry(row['left']),
            right: _optionalEntry(row['right']),
            note: row['reason'] as String?,
          ),
        )
        .toList();
  }

  List<BatchEntry> _entries(Object? raw) {
    if (raw is! List<Object?>) {
      return const <BatchEntry>[];
    }
    return raw
        .cast<Map<String, Object?>>()
        .map(
          (row) => BatchEntry(
            path: row['path'] as String? ?? '',
            size: (row['size'] as num?)?.toInt() ?? 0,
            mtime: DateTime.tryParse(row['mtime'] as String? ?? ''),
            hashes: _hashes(row['hashes']),
            status: _status(row['status'] as String?),
            errorKind: row['error_kind'] as String?,
            suffixApplied: row['suffix_applied'] as String?,
            renamedTo: row['renamed_to'] as String?,
          ),
        )
        .toList();
  }

  BatchEntry? _optionalEntry(Object? raw) {
    if (raw is Map<String, Object?>) {
      final entries = _entries(<Object?>[raw]);
      return entries.isEmpty ? null : entries.first;
    }
    return null;
  }

  /// Accepts both shapes: the bridge DTO sends a list of algorithm/hex pairs,
  /// the canonical report JSON stores hashes as a map keyed by algorithm id.
  Map<String, String> _hashes(Object? raw) {
    final hashes = <String, String>{};
    if (raw is List<Object?>) {
      for (final item in raw.cast<Map<String, Object?>>()) {
        final algorithm = item['algorithm'];
        final hex = item['hex'];
        if (algorithm is String && hex is String) {
          hashes[algorithm] = hex;
        }
      }
      return hashes;
    }
    if (raw is Map<String, Object?>) {
      for (final entry in raw.entries) {
        final value = entry.value;
        if (value is String) {
          hashes[entry.key] = value;
        }
      }
    }
    return hashes;
  }

  Map<String, Object?> _entryJson(BatchEntry entry) {
    return <String, Object?>{
      'path': entry.path,
      'size': entry.size,
      if (entry.mtime != null) 'mtime': entry.mtime!.toUtc().toIso8601String(),
      'hashes': entry.hashes.entries
          .map(
            (hash) => <String, Object?>{
              'algorithm': hash.key,
              'hex': hash.value,
            },
          )
          .toList(),
    };
  }

  String _commonRoot(List<BatchEntry> entries) {
    if (entries.isEmpty) {
      return '.';
    }
    final first = entries.first.path;
    final index = first.lastIndexOf('/');
    return index <= 0 ? '.' : first.substring(0, index);
  }

  EntryStatus _status(String? value) {
    return switch (value) {
      'failed' => EntryStatus.failed,
      'skipped' => EntryStatus.skipped,
      _ => EntryStatus.ok,
    };
  }

  DiffStatus _diffStatus(String? value) {
    return switch (value) {
      'changed' => DiffStatus.changed,
      'added' => DiffStatus.added,
      'removed' => DiffStatus.removed,
      'unknown' => DiffStatus.unknown,
      'ambiguous' => DiffStatus.ambiguous,
      'duplicate_key' => DiffStatus.duplicateKey,
      'out_of_root' => DiffStatus.outOfRoot,
      'corrupt' => DiffStatus.corrupt,
      'unstable' => DiffStatus.unstable,
      _ => DiffStatus.unchanged,
    };
  }

  RenameRowStatus _renameStatus(String? value) {
    return switch (value) {
      'ready' => RenameRowStatus.ready,
      'conflict' => RenameRowStatus.conflict,
      'skipped' => RenameRowStatus.skipped,
      'missing_hash' => RenameRowStatus.missingHash,
      'invalid_name' => RenameRowStatus.invalidName,
      'no_permission' => RenameRowStatus.noPermission,
      _ => RenameRowStatus.unsupported,
    };
  }
}
