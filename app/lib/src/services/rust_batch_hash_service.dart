import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../bridge/native_bridge.dart';
import '../models/models.dart';
import '../platform/rendering_hardware.dart';
import 'app_config_store.dart';
import 'batch_hash_service.dart';

/// Convert an absolute path under [root] into the root-relative form the
/// native batch request expects.
///
/// The rename planner emits paths in whatever form the entries carried; the
/// pipeline only accepts normalized forward-slash root-relative paths. A path
/// that is not under [root] is returned unchanged, so the native layer rejects
/// it loudly instead of this layer silently retargeting a rename.
String rootRelativePath(String root, String path) {
  final normalizedRoot = root.replaceAll('\\', '/');
  final normalizedPath = path.replaceAll('\\', '/');
  if (normalizedRoot.isEmpty) {
    return _stripLeadingDotSlash(normalizedPath);
  }
  final trimmedRoot =
      normalizedRoot.endsWith('/') && normalizedRoot.length > 1
      ? normalizedRoot.substring(0, normalizedRoot.length - 1)
      : normalizedRoot;
  if (trimmedRoot.isEmpty) {
    return _stripLeadingDotSlash(normalizedPath);
  }
  if (normalizedPath == trimmedRoot) {
    return '';
  }
  final prefix = '$trimmedRoot/';
  if (normalizedPath.startsWith(prefix)) {
    return _stripLeadingDotSlash(normalizedPath.substring(prefix.length));
  }
  return _stripLeadingDotSlash(normalizedPath);
}

String _stripLeadingDotSlash(String path) {
  var current = path;
  while (current.startsWith('./')) {
    current = current.substring(2);
  }
  if (current == '.') {
    return '';
  }
  return current;
}

/// [BatchHashService] backed by the Rust core through the native bridge.
///
/// Every operation that touches files, digests or the rename log is executed by
/// Rust; this class only maps JSON DTOs onto the UI models. When the native
/// library is missing the app falls back to FakeBatchHashService, so widget
/// tests and unsupported platforms keep working.
class RustBatchHashService implements BatchHashService {
  RustBatchHashService({
    required this.native,
    RenderingHardwareProvider renderingProvider =
        const UnknownRenderingHardwareProvider(),
    String? logDbPath,
    AppConfigStore? configStore,
    SupportDirectoryResolver? supportDirectory,
  }) : _renderingProvider = renderingProvider,
       _logDbPath = logDbPath,
       _configStore = configStore,
       _supportDirectory = supportDirectory;

  /// Open the native library and return null when it is not available.
  static RustBatchHashService? tryCreate({
    Map<String, String>? environment,
    String? overridePath,
    RenderingHardwareProvider renderingProvider =
        const UnknownRenderingHardwareProvider(),
    AppConfigStore? configStore,
    SupportDirectoryResolver? supportDirectory,
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
      configStore: configStore,
      supportDirectory: supportDirectory,
    );
  }

  final BatchHashNative native;
  final RenderingHardwareProvider _renderingProvider;
  final SupportDirectoryResolver? _supportDirectory;
  AppConfigStore? _configStore;
  String? _logDbPath;
  String? _lastRoot;
  bool _lastScanRecursive = false;
  String? _lastReportJson;
  /// Root the batch behind [_lastReportJson] worked on, or null when the cached
  /// JSON does not describe the report the caller is asking about.
  String? _lastReportRoot;

  /// Root of the most recent scan; used by rename execution and history.
  String? get lastRoot => _lastRoot;

  /// Rename log path used by this service.
  String? get logDbPath => _logDbPath;

  /// Whether the most recent scan walked subdirectories.
  bool get lastScanRecursive => _lastScanRecursive;

  /// Report JSON retained from the most recent run, or null when nothing has
  /// run yet or an undo invalidated it.
  ///
  /// Exposed so a test can prove that a successful undo drops the cached
  /// serialization instead of handing a pre-undo report to the exporter.
  String? get cachedReportJson => _lastReportJson;

  /// Store for the roots the user has used, so history survives a restart.
  AppConfigStore get configStore =>
      _configStore ??= FileAppConfigStore(supportDirectory: _supportDirectory);

  @override
  Future<HardwareInfoModel> probeHardware() async {
    // The probe answers "what can this machine do"; forcing the software path
    // is the hashing caller's choice, not a property of the hardware.
    final data = native.probe();
    return HardwareInfoModel(
      arch: data['arch'] as String? ?? 'unknown',
      detectedFeatures:
          (data['detected_features'] as List<Object?>? ?? const <Object?>[])
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
    _lastScanRecursive = recursive;
    await _rememberRoot(path);
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
    final scannedRoot = _lastRoot;
    final root = scannedRoot ?? _commonRoot(entries);
    // A folder the user picked without recursing must not have its whole tree
    // hashed behind their back; a derived root has no retained scan mode.
    final recursive = scannedRoot == null ? true : _lastScanRecursive;
    // Case sensitivity follows the platform default; the native layer knows
    // the host better than this layer does.
    final data = native.runBatch(
      root: root,
      algorithms: algorithms,
      recursive: recursive,
    );
    _lastRoot = root;
    _lastReportRoot = root;
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
    final rows = (data['rows'] as List<Object?>? ?? const <Object?>[])
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
  Future<RenameExecuteResult> executeRenames(
    RenamePlanModel plan, {
    bool? recursive,
    List<String>? restrictPaths,
  }) async {
    // The plan is proof enough that a scan happened; without a remembered root
    // the plan rows are forwarded as they are, and the native layer rejects
    // anything that is not a root-relative path.
    final root = _lastRoot ?? '.';
    // Only the rows the user actually previewed may be touched; the core would
    // otherwise rescan the whole tree and rename files the plan never showed.
    final paths = restrictPaths ?? _readyPaths(plan, root);
    if (paths.isEmpty) {
      throw StateError(
        'executeRenames requires at least one ready plan row: an unrestricted '
        'run would rename files the preview never showed',
      );
    }
    final recurse = recursive ?? _lastScanRecursive;
    final logDb = _logDbPath ?? defaultLogDbFor(root);
    final data = native.runBatch(
      root: root,
      algorithms: <String>[plan.algorithm],
      recursive: recurse,
      restrictPaths: paths,
      suffix: <String, Object?>{
        'algorithm': plan.algorithm,
        'length': plan.length,
        'case': plan.uppercase ? 'upper' : 'lower',
      },
      applyRenames: true,
      logDb: logDb,
    );
    _logDbPath = logDb;
    _lastScanRecursive = recurse;
    _lastReportRoot = root;
    _lastReportJson = data['report_json'] as String?;
    await _rememberRoot(root, logDb: logDb);
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
    final target = Directory(await _exportDirectory(directory));
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
    // A fresh launch has no in-memory root, so every remembered root and its
    // log must be consulted; otherwise a user who restarts cannot find the
    // batches they just applied.
    final candidates = <String>{};
    final db = _logDbPath;
    if (db != null && db.isNotEmpty) {
      candidates.add(db);
    }
    final root = _lastRoot;
    if (root != null && root.isNotEmpty) {
      candidates.add(defaultLogDbFor(root));
    }
    for (final remembered in await configStore.loadRecentRoots()) {
      candidates.add(remembered.resolvedLogDb);
    }
    final batches = <HistoryBatchModel>[];
    final seenIds = <String>{};
    final List<BridgeException> errors = <BridgeException>[];
    for (final candidate in candidates) {
      if (!File(candidate).existsSync()) {
        continue;
      }
      final List<Map<String, Object?>> rows;
      try {
        rows = native.listBatches(candidate);
      } on BridgeException catch (error) {
        // One unreadable log must not hide the batches of the others.
        errors.add(error);
        continue;
      }
      for (final batch in rows) {
        final id = batch['id'] as String? ?? '';
        if (id.isEmpty || !seenIds.add(id)) {
          continue;
        }
        final createdAt =
            DateTime.tryParse(batch['created_at'] as String? ?? '') ??
            DateTime.fromMillisecondsSinceEpoch(0, isUtc: true);
        batches.add(
          HistoryBatchModel(
            id: id,
            createdAt: createdAt,
            fileCount: (batch['file_count'] as num?)?.toInt() ?? 0,
            appliedCount: (batch['applied_count'] as num?)?.toInt() ?? 0,
            undone: ((batch['undone_count'] as num?)?.toInt() ?? 0) > 0,
          ),
        );
      }
    }
    // Only report a failure when nothing at all could be read; otherwise a
    // single stale log would blank out the history the user does have.
    if (batches.isEmpty && errors.isNotEmpty) {
      throw errors.first;
    }
    batches.sort(
      (HistoryBatchModel left, HistoryBatchModel right) =>
          right.createdAt.compareTo(left.createdAt),
    );
    return batches;
  }

  @override
  Future<bool> undoBatch(String batchId) async {
    final db = await _resolveLogDb();
    if (db == null) {
      return false;
    }
    final data = native.undo(db: db, batchId: batchId, apply: true);
    final undone = (data['undone'] as num?)?.toInt() ?? 0;
    if (undone > 0) {
      // The tree changed, so a report serialized before the undo must not be
      // handed to the exporter or the diff engine again: the cached JSON and
      // the entry list it described are both stale now.
      _lastReportJson = null;
      _lastReportRoot = null;
    }
    return undone > 0;
  }

  /// The log that holds [batchId].
  ///
  /// The caller may be looking at a batch discovered through history rather
  /// than through this session, so a remembered root's log counts too; the
  /// first existing candidate wins.
  Future<String?> _resolveLogDb() async {
    final explicit = _logDbPath;
    if (explicit != null && explicit.isNotEmpty) {
      return explicit;
    }
    final root = _lastRoot;
    if (root != null && root.isNotEmpty) {
      final candidate = defaultLogDbFor(root);
      if (File(candidate).existsSync()) {
        return candidate;
      }
    }
    for (final remembered in await configStore.loadRecentRoots()) {
      final candidate = remembered.resolvedLogDb;
      if (File(candidate).existsSync()) {
        return candidate;
      }
    }
    return null;
  }

  /// Ready rows of [plan] in the root-relative form the pipeline accepts.
  List<String> _readyPaths(RenamePlanModel plan, String root) {
    final paths = <String>[];
    for (final row in plan.rows) {
      if (row.status != RenameRowStatus.ready) {
        continue;
      }
      final relative = rootRelativePath(root, row.path);
      if (relative.isEmpty || paths.contains(relative)) {
        continue;
      }
      paths.add(relative);
    }
    return paths;
  }

  /// Record [root] so a later launch can find its history again.
  Future<void> _rememberRoot(String root, {String? logDb}) async {
    if (root.isEmpty) {
      return;
    }
    try {
      await configStore.rememberRoot(
        RecentRoot(
          root: root,
          logDb: logDb ?? _logDbPath,
          lastUsed: DateTime.now().toUtc(),
        ),
      );
    } on Exception catch (_) {
      // Remembering a root is a convenience; a read-only store must not fail
      // the scan or the rename that triggered it.
    } catch (_) {
      // A platform-channel failure arrives as an Error, not an Exception.
    }
  }

  /// Directory an export lands in when the caller names none.
  ///
  /// A stable per-user exports directory beats the system temp dir, which the
  /// OS may clear between launches; every export gets its own timestamped
  /// subdirectory so a second export never silently overwrites the first.
  Future<String> _exportDirectory(String? directory) async {
    final explicit = directory;
    if (explicit != null && explicit.isNotEmpty) {
      return explicit;
    }
    final support = await _supportDirectoryOrNull();
    final base = support?.path ?? Directory.systemTemp.path;
    final stamp = _timestampName(DateTime.now().toUtc());
    return '$base/exports/$stamp';
  }

  Future<Directory?> _supportDirectoryOrNull() async {
    try {
      final SupportDirectoryResolver resolver =
          _supportDirectory ?? getApplicationSupportDirectory;
      return await resolver();
    } on Exception catch (_) {
      // No path_provider implementation on this host; the caller falls back.
      return null;
    } catch (_) {
      // Calling path_provider before a binding exists throws an Error; treat it
      // like a missing implementation and let the caller fall back.
      return null;
    }
  }

  String _timestampName(DateTime stamp) {
    String two(int value) => value.toString().padLeft(2, '0');
    final year = stamp.year.toString();
    final month = two(stamp.month);
    final day = two(stamp.day);
    final hour = two(stamp.hour);
    final minute = two(stamp.minute);
    final second = two(stamp.second);
    final millis = stamp.millisecond.toString().padLeft(3, '0');
    return '$year$month$day-$hour$minute$second$millis';
  }

  String _reportJson(ReportModel report) {
    final cached = _lastReportJson;
    if (cached != null && report.root == _lastReportRoot) {
      return cached;
    }
    final algorithms = report.algorithms.isEmpty
        ? const <String>['sha256']
        : report.algorithms;
    return native.buildReport(
      root: report.root,
      algorithms: algorithms,
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
    final algorithmList = (decoded['algorithms'] as List<Object?>? ??
            const <Object?>[])
        .map((algorithm) => algorithm.toString())
        .toList();
    final generatedAt =
        DateTime.tryParse(decoded['generated_at'] as String? ?? '') ??
        fallback.generatedAt;
    return ReportModel(
      schemaVersion: (decoded['schema_version'] as num?)?.toInt() ?? 1,
      root: decoded['root'] as String? ?? fallback.root,
      generatedAt: generatedAt,
      algorithms: algorithmList,
      entries: _entries(decoded['entries']),
    );
  }

  List<DiffRowModel> _diffRows(Map<String, Object?> data) {
    return (data['rows'] as List<Object?>? ?? const <Object?>[])
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
    final mtime = entry.mtime;
    return <String, Object?>{
      'path': entry.path,
      'size': entry.size,
      if (mtime != null) 'mtime': mtime.toUtc().toIso8601String(),
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
      // "error" has no dedicated enum value; it is a row that cannot be
      // applied, which is exactly what "unsupported" reports.
      _ => RenameRowStatus.unsupported,
    };
  }
}
