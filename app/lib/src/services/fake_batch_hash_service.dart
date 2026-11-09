import '../models/models.dart';
import '../platform/rendering_hardware.dart';
import 'batch_hash_service.dart';

/// Raised when the fake is asked to scan a path it does not model.
///
/// The fake only knows the demo root. Failing loudly (instead of returning demo
/// entries for any path) keeps a widget test from passing on data that no real
/// scan ever produced, which used to hide real service failures.
class DemoScanUnavailable implements Exception {
  const DemoScanUnavailable(this.path);

  /// The path the caller asked for.
  final String path;

  @override
  String toString() =>
      'DemoScanUnavailable: the demo service only knows '
      '${FakeBatchHashService.rootPath}, not $path';
}

/// Deterministic in-memory implementation of [BatchHashService].
///
/// Every value is derived from fixed paths so widget tests are stable across
/// runs and platforms. The shape mirrors what the Rust core will return.
class FakeBatchHashService implements BatchHashService {
  FakeBatchHashService({
    RenderingHardwareProvider renderingProvider =
        const _DetectedRenderingHardwareProvider(),
  }) : _renderingProvider = renderingProvider;

  static const String rootPath = '/home/user/Documents';

  static final DateTime generatedAt = DateTime.utc(2024, 6, 1, 12, 0, 0);

  static const List<String> _relativePaths = <String>[
    'annual-report-2024.pdf',
    'budget-2025.xlsx',
    'contracts/acme-nda.pdf',
    'contracts/vendor-terms.pdf',
    'images/team-photo.jpg',
    'images/office-map.png',
    'invoices/2024-01.pdf',
    'invoices/2024-02.pdf',
    'notes/meeting-2024-03-notes.md',
    'notes/roadmap.md',
    'presentations/q1-review.pptx',
    'presentations/q2-review.pptx',
    'receipts/2441.pdf',
    'receipts/2442.pdf',
    'scripts/build.sh',
    'scripts/deploy.sh',
    'source/main.dart',
    'source/utils.dart',
    'source/old/legacy.dart',
    'source/old/archive.zip',
    'videos/demo.mp4',
    'videos/interview.mp4',
    'web/index.html',
    'web/styles.css',
  ];

  final RenderingHardwareProvider _renderingProvider;

  /// Root of the most recent scan, or null before the demo root was scanned.
  String? _lastRoot;

  /// Restriction paths forwarded by the most recent rename execution.
  List<String> _lastRestrictPaths = const <String>[];

  final List<HistoryBatchModel> _history = <HistoryBatchModel>[
    HistoryBatchModel(
      id: 'batch-0003',
      createdAt: DateTime.utc(2024, 5, 30, 9, 12),
      fileCount: 24,
      appliedCount: 24,
    ),
    HistoryBatchModel(
      id: 'batch-0002',
      createdAt: DateTime.utc(2024, 5, 24, 16, 40),
      fileCount: 18,
      appliedCount: 17,
      undone: true,
    ),
    HistoryBatchModel(
      id: 'batch-0001',
      createdAt: DateTime.utc(2024, 5, 18, 8, 5),
      fileCount: 12,
      appliedCount: 12,
    ),
  ];

  /// Entries exactly as a fresh scan would return them (no hashes yet).
  List<BatchEntry> get sampleEntries => _buildEntries(hashed: false);

  /// Entries with hashes filled in, useful for rename and compare previews.
  List<BatchEntry> get sampleHashedEntries => _buildEntries(hashed: true);

  /// Fixed report used by the compare and report pages.
  ReportModel get sampleReport => ReportModel(
    root: rootPath,
    generatedAt: generatedAt,
    algorithms: const <String>['blake3', 'sha256'],
    entries: sampleHashedEntries,
  );

  /// Fixed comparison rows covering every status the UI renders.
  List<DiffRowModel> get sampleDiffRows => _buildDiffRows();

  /// Root of the most recent successful scan, or null before the first one.
  String? get lastRoot => _lastRoot;

  /// Root-relative paths the most recent rename execution was narrowed to.
  List<String> get lastRestrictPaths =>
      List<String>.unmodifiable(_lastRestrictPaths);

  @override
  Future<HardwareInfoModel> probeHardware() async {
    return const HardwareInfoModel(
      arch: 'x86_64',
      detectedFeatures: <String>['sse2', 'avx2', 'sha_ni'],
      blake3BackendReported: 'avx2',
      sha256BackendReported: 'sha_ni',
      backendPolicy: 'auto',
      multithread: true,
    );
  }

  @override
  Future<RenderingHardwareModel> probeRenderingHardware() =>
      _renderingProvider.probe();

  @override
  Future<List<BatchEntry>> scanDirectory(
    String path, {
    bool recursive = false,
  }) async {
    if (!_isSampleRoot(path)) {
      // Only the demo root can be "scanned" by the fake. Returning demo entries
      // for any other path used to make every scan look successful, which hid
      // real service failures behind plausible-looking demo data.
      throw DemoScanUnavailable(path);
    }
    _lastRoot = path;
    return _buildEntries(hashed: false, recursive: recursive);
  }

  @override
  Stream<HashProgress> computeHashes({
    required List<BatchEntry> entries,
    required List<String> algorithms,
    bool forceSoftware = false,
  }) async* {
    final List<String> sortedAlgorithms = List<String>.of(algorithms)..sort();
    if (entries.isEmpty) {
      yield const HashProgress(completed: 0, total: 0, done: true);
      return;
    }
    final List<BatchEntry> current = <BatchEntry>[];
    for (var index = 0; index < entries.length; index += 1) {
      current.add(_applyHashes(entries[index], sortedAlgorithms));
      yield HashProgress(
        completed: index + 1,
        total: entries.length,
        entries: List<BatchEntry>.unmodifiable(current),
        done: index == entries.length - 1,
      );
      await Future<void>.delayed(const Duration(milliseconds: 5));
    }
  }

  @override
  Future<RenamePlanModel> planRenames({
    required List<BatchEntry> entries,
    required String algorithm,
    required int length,
    bool uppercase = true,
  }) async {
    await Future<void>.delayed(const Duration(milliseconds: 5));
    final List<RenamePlanRowModel> rows = <RenamePlanRowModel>[];
    for (var index = 0; index < entries.length; index += 1) {
      final BatchEntry entry = entries[index];
      final String? hash = entry.hashes[algorithm];
      if (entry.status != EntryStatus.ok ||
          hash == null ||
          hash.length < length) {
        rows.add(
          RenamePlanRowModel(
            path: entry.path,
            status: RenameRowStatus.missingHash,
            message: 'No $algorithm digest available.',
          ),
        );
        continue;
      }
      final String suffix = uppercase
          ? hash.substring(0, length).toUpperCase()
          : hash.substring(0, length).toLowerCase();
      final String target = _suffixPath(entry.path, suffix);
      final RenameRowStatus status;
      if (index % 11 == 5) {
        status = RenameRowStatus.skipped;
      } else if (index % 6 == 0) {
        status = RenameRowStatus.conflict;
      } else {
        status = RenameRowStatus.ready;
      }
      rows.add(
        RenamePlanRowModel(
          path: entry.path,
          status: status,
          targetPath: target,
          message: switch (status) {
            RenameRowStatus.conflict => 'Target name already exists.',
            RenameRowStatus.skipped => 'File already carries a hash suffix.',
            _ => null,
          },
        ),
      );
    }
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
    await Future<void>.delayed(const Duration(milliseconds: 5));
    final List<String> paths = restrictPaths ?? _readyPaths(plan);
    if (paths.isEmpty) {
      // The real service refuses to apply a plan with nothing ready, because an
      // unrestricted run would rename files the preview never showed; the fake
      // refuses for the same reason so tests cannot mask the bug.
      throw StateError('executeRenames requires at least one ready plan row');
    }
    _lastRestrictPaths = paths;
    return RenameExecuteResult(
      applied: paths.length,
      skipped: plan.rows.length - paths.length,
      failed: 0,
      batchId: 'batch-0004',
    );
  }

  @override
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
  }) async {
    // Without an explicit directory the real service writes into the user's
    // exports folder rather than the repo or the system temp dir; the fake
    // reports an equivalent placeholder path so the UI sees the same shape.
    final String targetDirectory =
        directory ?? '$rootPath/exports/${_exportStamp()}';
    final List<ReportFormat> ordered = formats.toList()
      ..sort(
        (ReportFormat left, ReportFormat right) =>
            left.index.compareTo(right.index),
      );
    return <ExportedReport>[
      for (final ReportFormat format in ordered)
        ExportedReport(
          format: format,
          path: '$targetDirectory/batchhash-report.${format.fileExtension}',
        ),
    ];
  }

  @override
  Future<ReportModel> importReport(String path) async {
    return sampleReport;
  }

  @override
  Future<List<DiffRowModel>> diffReports({
    required ReportModel left,
    required ReportModel right,
  }) async {
    return _buildDiffRows();
  }

  @override
  Future<List<DiffRowModel>> diffReportAgainstDirectory({
    required ReportModel report,
    required String directory,
    bool recursive = true,
  }) async {
    return _buildDiffRows();
  }

  @override
  Future<List<HistoryBatchModel>> loadHistory() async {
    return List<HistoryBatchModel>.unmodifiable(_history);
  }

  @override
  Future<bool> undoBatch(String batchId) async {
    final int index = _history.indexWhere(
      (HistoryBatchModel batch) => batch.id == batchId,
    );
    if (index < 0 || _history[index].undone) {
      return false;
    }
    _history[index] = _history[index].copyWith(undone: true);
    // The tree changed, so the restriction set of the last run no longer
    // describes anything the UI may still be showing.
    _lastRestrictPaths = const <String>[];
    return true;
  }

  /// Demo entries under [rootPath].
  ///
  /// [recursive] is honoured here rather than at the call site so a non
  /// recursive scan cannot accidentally return the whole demo tree.
  List<BatchEntry> _buildEntries({
    required bool hashed,
    bool recursive = true,
  }) {
    final List<BatchEntry> entries = <BatchEntry>[];
    for (var index = 0; index < _relativePaths.length; index += 1) {
      final String relativePath = _relativePaths[index];
      if (!recursive && relativePath.contains('/')) {
        continue;
      }
      final bool failed = index % 7 == 3;
      final bool skipped = !failed && index % 9 == 4;
      final EntryStatus status = failed
          ? EntryStatus.failed
          : (skipped ? EntryStatus.skipped : EntryStatus.ok);
      final String? errorKind = failed
          ? 'permission_denied'
          : (skipped ? 'cancelled' : null);
      final String path = '$rootPath/$relativePath';
      final Map<String, String> hashes = hashed && status == EntryStatus.ok
          ? _hashesFor(path, kAvailableAlgorithms)
          : const <String, String>{};
      entries.add(
        BatchEntry(
          path: path,
          size: 4096 + index * 137,
          mtime: DateTime.utc(2024, 1, 1).add(Duration(minutes: index * 17)),
          hashes: hashes,
          status: status,
          errorKind: errorKind,
        ),
      );
    }
    return entries;
  }

  BatchEntry _applyHashes(BatchEntry entry, List<String> algorithms) {
    if (entry.status != EntryStatus.ok) {
      return entry;
    }
    return entry.copyWith(hashes: _hashesFor(entry.path, algorithms));
  }

  Map<String, String> _hashesFor(String path, List<String> algorithms) {
    return <String, String>{
      for (final String algorithm in algorithms)
        algorithm: _digest('$algorithm:$path', _hexLength(algorithm)),
    };
  }

  List<DiffRowModel> _buildDiffRows() {
    final BatchEntry alphaLeft = _diffEntry('docs/alpha.md', hashSeed: 'alpha');
    final BatchEntry betaLeft = _diffEntry('docs/beta.md', hashSeed: 'beta');
    final BatchEntry gammaLeft = _diffEntry(
      'docs/gamma.md',
      hashSeed: 'gamma-v1',
      size: 2048,
    );
    final BatchEntry deltaLeft = _diffEntry(
      'docs/delta.md',
      hashSeed: 'delta-v1',
      size: 3072,
    );
    final BatchEntry mainLeft = _diffEntry('src/main.rs', hashSeed: 'main');
    final BatchEntry mainRight = _diffEntry('tests/main.rs', hashSeed: 'main');
    final BatchEntry libLeft = _diffEntry('src/lib.rs', hashSeed: 'lib');
    final BatchEntry corruptLeft = _diffEntry(
      'data/corrupt.bin',
      hashSeed: 'corrupt',
      size: 1024,
    );
    final BatchEntry unstableLeft = _diffEntry(
      'logs/unstable.log',
      hashSeed: 'unstable-v1',
    );
    final BatchEntry duplicateLeft = _diffEntry('tmp/dup.rs', hashSeed: 'dup');
    final BatchEntry outOfRootLeft = _diffEntry(
      'outside/out-of-root.txt',
      hashSeed: 'outside',
    );
    return <DiffRowModel>[
      DiffRowModel(
        path: 'docs/alpha.md',
        status: DiffStatus.unchanged,
        left: alphaLeft,
        right: alphaLeft,
      ),
      DiffRowModel(
        path: 'docs/beta.md',
        status: DiffStatus.unchanged,
        left: betaLeft,
        right: betaLeft,
      ),
      DiffRowModel(
        path: 'docs/gamma.md',
        status: DiffStatus.changed,
        left: gammaLeft,
        right: _diffEntry('docs/gamma.md', hashSeed: 'gamma-v2', size: 2048),
      ),
      DiffRowModel(
        path: 'docs/delta.md',
        status: DiffStatus.changed,
        left: deltaLeft,
        right: _diffEntry('docs/delta.md', hashSeed: 'delta-v2', size: 3072),
      ),
      DiffRowModel(
        path: 'images/logo.png',
        status: DiffStatus.added,
        right: _diffEntry('images/logo.png', hashSeed: 'logo'),
      ),
      DiffRowModel(
        path: 'legacy/old.cfg',
        status: DiffStatus.removed,
        left: _diffEntry('legacy/old.cfg', hashSeed: 'old'),
      ),
      DiffRowModel(
        path: 'src/main.rs',
        status: DiffStatus.ambiguous,
        left: mainLeft,
        right: mainRight,
        note: 'Basename collision between src/main.rs and tests/main.rs.',
      ),
      DiffRowModel(
        path: 'src/lib.rs',
        status: DiffStatus.unknown,
        left: libLeft,
        right: _diffEntry('src/lib.rs', hashSeed: 'lib'),
        note: 'No algorithm present in both reports.',
      ),
      DiffRowModel(
        path: 'data/corrupt.bin',
        status: DiffStatus.corrupt,
        left: corruptLeft,
        right: _diffEntry('data/corrupt.bin', hashSeed: 'corrupt', size: 2048),
        note: 'Identical digest but a different file size.',
      ),
      DiffRowModel(
        path: 'logs/unstable.log',
        status: DiffStatus.unstable,
        left: unstableLeft,
        right: _diffEntry('logs/unstable.log', hashSeed: 'unstable-v2'),
        note: 'File changed while it was being hashed.',
      ),
      DiffRowModel(
        path: 'tmp/dup.rs',
        status: DiffStatus.duplicateKey,
        left: duplicateLeft,
        right: duplicateLeft,
        note: 'Duplicate normalized key within one report.',
      ),
      DiffRowModel(
        path: 'outside/out-of-root.txt',
        status: DiffStatus.outOfRoot,
        left: outOfRootLeft,
        right: _diffEntry('outside/out-of-root.txt', hashSeed: 'outside'),
        note: 'Path is outside the baseline root.',
      ),
    ];
  }

  BatchEntry _diffEntry(String path, {String hashSeed = '', int size = 1024}) {
    return BatchEntry(
      path: '$rootPath/$path',
      size: size,
      mtime: DateTime.utc(2024, 5, 1),
      hashes: hashSeed.isEmpty
          ? const <String, String>{}
          : <String, String>{
              'blake3': _digest('blake3:$hashSeed', 64),
              'sha256': _digest('sha256:$hashSeed', 64),
            },
    );
  }

  /// Whether [path] is the demo root this fake models.
  bool _isSampleRoot(String path) {
    final String normalized = path.replaceAll('\\', '/');
    final String trimmed = normalized.endsWith('/') && normalized.length > 1
        ? normalized.substring(0, normalized.length - 1)
        : normalized;
    return trimmed == rootPath;
  }

  /// Ready rows of [plan] in the root-relative form the pipeline expects.
  ///
  /// The real service derives the same list from the plan; sharing the rule
  /// keeps the fake from accepting a plan the native layer would refuse.
  List<String> _readyPaths(RenamePlanModel plan) {
    final String prefix = '$rootPath/';
    final List<String> paths = <String>[];
    for (final RenamePlanRowModel row in plan.rows) {
      if (row.status != RenameRowStatus.ready) {
        continue;
      }
      final String normalized = row.path.replaceAll('\\', '/');
      final String relative = normalized.startsWith(prefix)
          ? normalized.substring(prefix.length)
          : normalized;
      if (relative.isEmpty || paths.contains(relative)) {
        continue;
      }
      paths.add(relative);
    }
    return paths;
  }

  /// Fixed timestamp fragment for the fake's default export directory.
  ///
  /// A real export gets a fresh timestamp; the fake stays deterministic so a
  /// widget test can assert on the path.
  String _exportStamp() => '20240601-120000';

  String _suffixPath(String path, String suffix) {
    final int slash = path.lastIndexOf('/');
    final int dot = path.lastIndexOf('.');
    if (dot > slash) {
      return '${path.substring(0, dot)}[$suffix]${path.substring(dot)}';
    }
    return '$path[$suffix]';
  }

  int _hexLength(String algorithm) => switch (algorithm) {
    'md5' => 32,
    'sha1' => 40,
    'sha512' => 128,
    'sha3-256' => 64,
    'blake2b' => 128,
    'blake2s' => 64,
    'sha256' => 64,
    'blake3' => 64,
    'xxh3' => 16,
    'crc32' => 8,
    'crc32c' => 8,
    _ => 64,
  };

  String _digest(String seed, int hexChars) {
    final StringBuffer buffer = StringBuffer();
    var round = 0;
    while (buffer.length < hexChars) {
      var hash = 0x811C9DC5;
      for (final int unit in '$seed#$round'.codeUnits) {
        hash ^= unit;
        hash = (hash * 0x01000193) & 0xFFFFFFFF;
      }
      buffer.write(hash.toRadixString(16).padLeft(8, '0'));
      round += 1;
    }
    return buffer.toString().substring(0, hexChars);
  }
}

class _DetectedRenderingHardwareProvider implements RenderingHardwareProvider {
  const _DetectedRenderingHardwareProvider();

  @override
  Future<RenderingHardwareModel> probe() async {
    return const RenderingHardwareModel(
      accelerated: true,
      gpuBackend: 'vulkan',
    );
  }
}
