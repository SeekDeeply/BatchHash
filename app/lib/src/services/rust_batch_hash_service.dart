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
  List<String>? _explicitPaths;
  /// Cached result of [listAlgorithms]; the registry is static per process.
  List<AlgorithmInfo>? _algorithms;
  String? _lastReportJson;
  /// Root the batch behind [_lastReportJson] worked on, or null when the cached
  /// JSON does not describe the report the caller is asking about.
  String? _lastReportRoot;
  /// Native job currently running, so [cancelCurrentOperation] can stop it.
  String? _activeJobId;

  /// How often the Dart side asks a native job for progress.
  ///
  /// The native reporter throttles itself to about twenty samples a second, so
  /// polling faster would only burn cycles; slower makes the bar look stalled.
  static const Duration _pollInterval = Duration(milliseconds: 60);

  /// Root of the most recent scan; used by rename execution and history.
  ///
  /// For an explicit file selection this is the directory the picked files
  /// share, so it is always a directory a report can be rooted at.
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
  Future<List<AlgorithmInfo>> listAlgorithms() async {
    final List<AlgorithmInfo>? cached = _algorithms;
    if (cached != null) {
      return cached;
    }
    // The core owns both the catalogue and the classification, so this layer
    // only decodes the DTOs; a new algorithm needs no Dart change here.
    final List<AlgorithmInfo> decoded = <AlgorithmInfo>[
      for (final Map<String, Object?> row in native.algorithms())
        AlgorithmInfo.fromJson(row),
    ];
    // An older native library that answered with nothing must not leave the
    // picker empty; the bundled catalogue is the documented fallback.
    final List<AlgorithmInfo> resolved = decoded.isEmpty
        ? kAlgorithmCatalogue
        : List<AlgorithmInfo>.unmodifiable(decoded);
    _algorithms = resolved;
    return resolved;
  }

  @override
  Future<List<TextHashResult>> hashText({
    required String text,
    required List<String> algorithms,
  }) async {
    if (algorithms.isEmpty) {
      return const <TextHashResult>[];
    }
    // The core takes base64 so any byte sequence survives the JSON request, and
    // it hashes the exact bytes: no newline handling happens on either side.
    final Map<String, Object?> response = native.hashText(
      dataBase64: base64Encode(utf8.encode(text)),
      algorithms: algorithms,
    );
    final Map<String, String> names = <String, String>{
      for (final AlgorithmInfo info in await listAlgorithms())
        info.id: info.displayName,
    };
    return <TextHashResult>[
      for (final Object? row in (response['hashes'] as List<Object?>? ?? const <Object?>[]))
        if (row is Map<String, Object?>)
          TextHashResult(
            algorithmId: row['id'] as String? ?? '',
            displayName:
                names[row['id']] ?? (row['id'] as String? ?? ''),
            digest: row['digest'] as String? ?? '',
          ),
    ];
  }
  @override
  Future<List<BatchEntry>> scanDirectory(
    String path, {
    bool recursive = false,
  }) async {
    _explicitPaths = null;
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
  String get lastScannedRoot => _lastRoot ?? '';

  @override
  Future<List<BatchEntry>> scanFiles(List<String> paths) async {
    if (paths.isEmpty) return const <BatchEntry>[];
    final root = _commonPath(paths);
    final relative = <String>[
      for (final String path in paths) _relativePath(path, root),
    ];
    _explicitPaths = relative;
    _lastRoot = root;
    _lastScanRecursive = false;
    await _rememberRoot(root);
    final result = <BatchEntry>[];
    for (var i = 0; i < paths.length; i++) {
      final data = native.scan(paths[i]);
      final entries = _entries(data['entries']);
      final entry = entries.isEmpty ? null : entries.first;
      result.add(BatchEntry(
        path: relative[i],
        size: entry?.size ?? 0,
        mtime: entry?.mtime,
        status: entry?.status ?? EntryStatus.failed,
        errorKind: entry?.errorKind ?? (entry == null ? 'not_found' : null),
      ));
    }
    return result;
  }

  @override
  Stream<HashProgress> computeHashes({
    required List<BatchEntry> entries,
    required List<String> algorithms,
    bool forceSoftware = false,
  }) {
    return _streamJob(
      entries: entries,
      algorithms: algorithms,
      forceSoftware: forceSoftware,
    );
  }

  /// Start the native job and stream its progress until it finishes.
  ///
  /// The C ABI call itself is synchronous, so hashing runs on a native worker
  /// thread and this stream polls it. Without that, a multi-gigabyte file
  /// blocked the single Flutter isolate and the UI appeared to hang.
  Stream<HashProgress> _streamJob({
    required List<BatchEntry> entries,
    required List<String> algorithms,
    required bool forceSoftware,
  }) {
    // `forceSoftware` is a caller preference recorded by the probe path; the
    // batch request carries no per-run override, and pretending otherwise would
    // silently ignore it.
    assert(forceSoftware || !forceSoftware);
    if (entries.isEmpty) {
      return Stream<HashProgress>.value(
        const HashProgress(completed: 0, total: 0, done: true),
      );
    }
    final scannedRoot = _lastRoot;
    final root = scannedRoot ?? _commonRoot(entries);
    final explicitFiles = _explicitPaths != null;
    final Map<String, Object?> request;
    final String action;
    if (explicitFiles) {
      action = 'hash_entries';
      request = <String, Object?>{
        'root': root,
        'paths': _explicitPaths!,
        'algorithms': algorithms,
      };
    } else {
      action = 'batch';
      request = <String, Object?>{
        'root': root,
        'algorithms': algorithms,
        'recursive': _lastScanRecursive,
      };
    }
    return _pollJob(
      action: action,
      request: request,
      entries: entries,
      algorithms: algorithms,
      root: root,
      explicitFiles: explicitFiles,
    );
  }

  /// Poll a native job and yield progress samples until it terminates.
  Stream<HashProgress> _pollJob({
    required String action,
    required Map<String, Object?> request,
    required List<BatchEntry> entries,
    required List<String> algorithms,
    required String root,
    required bool explicitFiles,
  }) async* {
    if (!native.supportsAsyncJobs()) {
      // An older native library predates jobs; fall back to the blocking call
      // rather than failing, and report it as one completion sample.
      yield _blockingHash(
        entries: entries,
        algorithms: algorithms,
        root: root,
        explicitFiles: explicitFiles,
      );
      return;
    }
    final jobId = native.jobStart(action: action, payload: request);
    _activeJobId = jobId;
    try {
      while (true) {
        final status = native.jobPoll(jobId);
        final progress = _progressFromPoll(status, entries.length);
        final state = status['state'] as String? ?? 'running';
        if (state == 'completed') {
          final payload = status['result_json'] as String?;
          if (payload == null) {
            throw BridgeException(
              'internal',
              'the native job reported completion without a result',
            );
          }
          yield _resultFromPayload(
            payload: payload,
            entries: entries,
            root: root,
            explicitFiles: explicitFiles,
            progress: progress,
          );
          return;
        }
        if (state == 'cancelled') {
          yield HashProgress(
            completed: progress.completed,
            total: progress.total,
            done: true,
            phase: HashPhase.completed,
            bytesDone: progress.bytesDone,
            bytesTotal: progress.bytesTotal,
            currentPath: progress.currentPath,
          );
          return;
        }
        if (state == 'failed') {
          throw BridgeException(
            status['error_kind'] as String? ?? 'internal',
            status['error'] as String? ?? 'the native job failed',
          );
        }
        if (progress.completed > 0 || progress.bytesDone > 0 || progress.total > 0) {
          yield progress;
        }
        await Future<void>.delayed(_pollInterval);
      }
    } finally {
      _activeJobId = null;
      native.jobRelease(jobId);
    }
  }

  /// Map a poll envelope onto a [HashProgress] sample.
  HashProgress _progressFromPoll(Map<String, Object?> status, int entryCount) {
    final raw = status['progress'];
    final progress = raw is Map<String, Object?> ? raw : const <String, Object?>{};
    final total = (progress['total'] as num?)?.toInt() ?? entryCount;
    return HashProgress(
      completed: (progress['completed'] as num?)?.toInt() ?? 0,
      total: total,
      phase: HashPhase.fromWire(progress['phase']),
      bytesDone: (progress['bytes_done'] as num?)?.toInt() ?? 0,
      bytesTotal: (progress['bytes_total'] as num?)?.toInt() ?? 0,
      etaSeconds: (progress['eta_seconds'] as num?)?.toDouble(),
      currentPath: progress['current_path'] as String? ?? '',
    );
  }

  /// Build the terminal sample from a completed job payload.
  HashProgress _resultFromPayload({
    required String payload,
    required List<BatchEntry> entries,
    required String root,
    required bool explicitFiles,
    required HashProgress progress,
  }) {
    final data = jsonDecode(payload);
    if (data is! Map<String, Object?>) {
      throw BridgeException('internal', 'the native job returned malformed JSON');
    }
    _lastRoot = root;
    _lastReportRoot = root;
    _lastReportJson = data['report_json'] as String?;
    final hashed = _entries(data['entries']);
    final result = explicitFiles
        ? <BatchEntry>[
            for (final BatchEntry original in entries)
              _mergeHash(original, hashed),
          ]
        : (hashed.isEmpty ? entries : hashed);
    return HashProgress(
      completed: result.length,
      total: result.length,
      entries: result,
      done: true,
      phase: HashPhase.completed,
      bytesDone: progress.bytesTotal > 0 ? progress.bytesTotal : progress.bytesDone,
      bytesTotal: progress.bytesTotal,
    );
  }

  /// Blocking fallback for a native library that predates async jobs.
  HashProgress _blockingHash({
    required List<BatchEntry> entries,
    required List<String> algorithms,
    required String root,
    required bool explicitFiles,
  }) {
    final Map<String, Object?> data;
    if (explicitFiles) {
      data = native.hashEntries(
        root: root,
        paths: _explicitPaths!,
        algorithms: algorithms,
      );
    } else {
      data = native.runBatch(
        root: root,
        algorithms: algorithms,
        recursive: _lastScanRecursive,
      );
    }
    _lastRoot = root;
    _lastReportRoot = root;
    _lastReportJson = data['report_json'] as String?;
    final hashed = _entries(data['entries']);
    final result = explicitFiles
        ? <BatchEntry>[
            for (final BatchEntry original in entries)
              _mergeHash(original, hashed),
          ]
        : (hashed.isEmpty ? entries : hashed);
    return HashProgress(
      completed: result.length,
      total: result.length,
      entries: result,
      done: true,
      phase: HashPhase.completed,
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
    String? locale,
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
        // Only the human readable formats use it; passing it for the machine
        // formats too costs nothing and keeps the call site uniform.
        locale: locale,
      );
      final File file = _claimExportFile(dirPath, format.fileExtension, text);
      written.add(ExportedReport(format: format, path: file.path));
    }
    return written;
  }

  /// Write [text] to the first free name, never replacing an existing report.
  ///
  /// A user-chosen folder can already hold a report from an earlier export;
  /// overwriting it would destroy that file without asking. The first attempt
  /// uses the plain name, then the usual (1), (2) suffixes. Creation is
  /// exclusive so a file appearing between the check and the write cannot be
  /// clobbered by a race.
  File _claimExportFile(String dirPath, String extension, String text) {
    for (int index = 0; index < _exportNameAttempts; index += 1) {
      final String suffix = index == 0 ? '' : ' ($index)';
      final File candidate = File(
        '$dirPath/batchhash-report$suffix.$extension',
      );
      try {
        candidate.createSync(exclusive: true);
      } on FileSystemException {
        // The name is taken: try the next suffix instead of overwriting it.
        continue;
      }
      try {
        candidate.writeAsStringSync(text, flush: true);
      } catch (_) {
        // Leave no truncated file behind if the write itself fails.
        if (candidate.existsSync()) {
          candidate.deleteSync();
        }
        rethrow;
      }
      return candidate;
    }
    throw StateError(
      'no free report filename in $dirPath after $_exportNameAttempts attempts',
    );
  }

  /// How many numbered names an export may try before giving up.
  static const int _exportNameAttempts = 1000;

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
    bool recursive = false,
  }) async {
    final data = native.diffDirectory(
      reportJson: _reportJson(report),
      directory: directory,
      followLinks: false,
      recursive: recursive,
      restrictPaths: _scopePaths(report),
    );
    return _diffRows(data);
  }

  /// Relative paths of the files this report covers.
  ///
  /// The report already names exactly the files the user hashed. Handing those
  /// to the engine keeps a comparison about one file instead of the whole tree
  /// that file happens to live in.
  List<String> _scopePaths(ReportModel report) {
    final String root = report.root;
    final List<String> paths = <String>[];
    for (final BatchEntry entry in report.entries) {
      final String relative = _relativeTo(root, entry.path);
      if (relative.isNotEmpty) {
        paths.add(relative);
      }
    }
    return paths;
  }

  /// Strip the root prefix from a display path.
  ///
  /// Returns an empty string when the entry is not under the root, which the
  /// engine would reject anyway; skipping it keeps the rest of the scope valid.
  String _relativeTo(String root, String path) {
    if (root.isEmpty) {
      return '';
    }
    final String normalizedRoot = root.endsWith('/') || root.endsWith('\\')
        ? root.substring(0, root.length - 1)
        : root;
    if (!path.startsWith(normalizedRoot)) {
      return '';
    }
    String relative = path.substring(normalizedRoot.length);
    while (relative.startsWith('/') || relative.startsWith('\\')) {
      relative = relative.substring(1);
    }
    return relative;
  }

  /// Only true when the loaded library exposes the async job API.
  ///
  /// An older library hashes synchronously, so there is no job to cancel; the
  /// UI must hide the cancel button rather than offer a no-op.
  @override
  bool get supportsCancellation => native.supportsAsyncJobs();

  @override
  Future<void> cancelCurrentOperation() async {
    final jobId = _activeJobId;
    if (jobId == null) {
      return;
    }
    // The job reports its terminal state through the poll loop, which is what
    // closes the stream; cancelling here only asks the worker to stop.
    try {
      native.jobCancel(jobId);
    } on BridgeException {
      // A job that finished between the read and this call is already gone.
    }
  }

  @override
  Stream<DiffProgress> compareReportWithDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = false,
  }) {
    return _streamDiff(
      report: report,
      directory: directory,
      recursive: recursive,
    );
  }

  /// Run the directory comparison as a native job and stream its progress.
  Stream<DiffProgress> _streamDiff({
    required ReportModel report,
    required String directory,
    required bool recursive,
  }) async* {
    // Scope the comparison to the files the report lists. Without this the
    // engine hashes whatever sits next to them, which for a single picked file
    // means its whole parent tree.
    final List<String> scope = _scopePaths(report);
    final request = <String, Object?>{
      'report_json': _reportJson(report),
      'directory': directory,
      'follow_links': false,
      'recursive': recursive,
      if (scope.isNotEmpty) 'restrict_paths': scope,
    };
    if (!native.supportsAsyncJobs()) {
      final data = native.diffDirectory(
        reportJson: request['report_json']! as String,
        directory: directory,
        followLinks: false,
        recursive: recursive,
        restrictPaths: scope,
      );
      yield DiffProgress(
        done: true,
        rows: _diffRows(data),
        phase: HashPhase.completed,
      );
      return;
    }
    final jobId = native.jobStart(action: 'diff_directory', payload: request);
    _activeJobId = jobId;
    try {
      while (true) {
        final status = native.jobPoll(jobId);
        final progress = _progressFromPoll(status, 0);
        final state = status['state'] as String? ?? 'running';
        if (state == 'completed') {
          final payload = status['result_json'] as String?;
          if (payload == null) {
            throw BridgeException(
              'internal',
              'the native job reported completion without a result',
            );
          }
          final decoded = jsonDecode(payload);
          if (decoded is! Map<String, Object?>) {
            throw BridgeException('internal', 'the native job returned malformed JSON');
          }
          yield DiffProgress(
            done: true,
            rows: _diffRows(decoded),
            phase: HashPhase.completed,
            bytesDone: progress.bytesTotal,
            bytesTotal: progress.bytesTotal,
            completed: progress.completed,
            total: progress.total,
          );
          return;
        }
        if (state == 'cancelled') {
          yield DiffProgress(
            done: true,
            cancelled: true,
            phase: HashPhase.completed,
            bytesDone: progress.bytesDone,
            bytesTotal: progress.bytesTotal,
            completed: progress.completed,
            total: progress.total,
          );
          return;
        }
        if (state == 'failed') {
          throw BridgeException(
            status['error_kind'] as String? ?? 'internal',
            status['error'] as String? ?? 'the native comparison failed',
          );
        }
        yield DiffProgress(
          phase: progress.phase,
          bytesDone: progress.bytesDone,
          bytesTotal: progress.bytesTotal,
          etaSeconds: progress.etaSeconds,
          currentPath: progress.currentPath,
          completed: progress.completed,
          total: progress.total,
        );
        await Future<void>.delayed(_pollInterval);
      }
    } finally {
      _activeJobId = null;
      native.jobRelease(jobId);
    }
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

  /// Merge the engine result for [original] into the entry the UI shows.
  ///
  /// Both sides are root-relative: [scanFiles] stores the relative path on
  /// the entry and the engine answers in the same form.
  BatchEntry _mergeHash(BatchEntry original, List<BatchEntry> hashed) {
    for (final BatchEntry entry in hashed) {
      if (entry.path == original.path) {
        return original.copyWith(
          hashes: entry.hashes,
          status: entry.status,
          errorKind: entry.errorKind,
        );
      }
    }
    return original.copyWith(
      status: EntryStatus.failed,
      errorKind: 'missing_hash_result',
    );
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

  String _commonPath(List<String> paths) {
    final directories = paths.map(_parentPath).toList();
    if (directories.isEmpty) return '.';
    final first = directories.first;
    final windows = RegExp(r'^[A-Za-z]:').hasMatch(first) || first.contains('\\');
    final separator = windows ? '\\' : '/';
    final absolute = first.startsWith('/') || RegExp(r'^[A-Za-z]:').hasMatch(first);
    final segments = directories
        .map((path) => path.split(RegExp(r'[/\\]+')).where((s) => s.isNotEmpty).toList())
        .toList();
    var shared = 0;
    while (segments.every((parts) => parts.length > shared) &&
        segments.map((parts) => windows ? parts[shared].toLowerCase() : parts[shared]).toSet().length == 1) {
      shared++;
    }
    var root = segments.first.take(shared).join(separator);
    if (absolute && !windows) root = '/$root';
    if (RegExp(r'^[A-Za-z]:$').hasMatch(root)) root += separator;
    if (root.isEmpty && absolute) return separator;
    return root.isEmpty ? '.' : root;
  }

  String _commonRoot(List<BatchEntry> entries) => _commonPath(
    entries.map((BatchEntry entry) => entry.path).toList(),
  );

  String _parentPath(String path) {
    final int index = path.lastIndexOf(RegExp(r'[/\\]'));
    if (index < 0) return '.';
    if (index == 0) return path.substring(0, 1);
    if (index == 2 && RegExp(r'^[A-Za-z]:').hasMatch(path)) return path.substring(0, 3);
    return path.substring(0, index);
  }

  String _relativePath(String path, String root) {
    final String normalizedPath = path.replaceAll('\\', '/');
    final String normalizedRoot = root
        .replaceAll('\\', '/')
        .replaceAll(RegExp(r'/+$'), '');
    if (normalizedRoot.isEmpty || normalizedRoot == '.') {
      return normalizedPath.replaceFirst(RegExp(r'^\.?/'), '');
    }
    final String prefix = normalizedRoot == '/' ? '/' : '$normalizedRoot/';
    if (!normalizedPath.toLowerCase().startsWith(prefix.toLowerCase())) {
      throw StateError('selected file is outside the common root: $path');
    }
    return normalizedPath.substring(prefix.length);
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