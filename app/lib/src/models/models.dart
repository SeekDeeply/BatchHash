import 'dart:math' as math;

/// Algorithms exposed in the P0 user interface.
const List<String> kAvailableAlgorithms = <String>[
  'md5',
  'sha1',
  'sha256',
  'sha512',
  'sha3-256',
  'blake2b',
  'blake2s',
  'blake3',
  'xxh3',
  'crc32',
  'crc32c',
];

/// Algorithms offered for suffix based renaming.
const List<String> kSuffixAlgorithms = <String>[
  'blake3',
  'sha256',
  'sha1',
  'md5',
  'xxh3',
];

/// Outcome of hashing a single file.
enum EntryStatus {
  ok,
  failed,
  skipped;

  String get label => switch (this) {
    EntryStatus.ok => 'ok',
    EntryStatus.failed => 'failed',
    EntryStatus.skipped => 'skipped',
  };
}

/// Comparison status of one row produced by the diff engine.
enum DiffStatus {
  unchanged,
  changed,
  added,
  removed,
  unknown,
  ambiguous,
  duplicateKey,
  outOfRoot,
  corrupt,
  unstable;

  String get label => switch (this) {
    DiffStatus.unchanged => 'unchanged',
    DiffStatus.changed => 'changed',
    DiffStatus.added => 'added',
    DiffStatus.removed => 'removed',
    DiffStatus.unknown => 'unknown',
    DiffStatus.ambiguous => 'ambiguous',
    DiffStatus.duplicateKey => 'duplicate key',
    DiffStatus.outOfRoot => 'outside root',
    DiffStatus.corrupt => 'corrupt',
    DiffStatus.unstable => 'unstable',
  };

  bool get isDifference => this != DiffStatus.unchanged;
}

/// Reported acceleration state of a hardware subsystem.
enum AccelerationState { detected, softwareFallback, unknown }

/// Report export and import formats supported by the core.
enum ReportFormat {
  json,
  csv,
  markdown,
  txt,
  md5sum,
  bsd;

  String get label => switch (this) {
    ReportFormat.json => 'JSON',
    ReportFormat.csv => 'CSV',
    ReportFormat.markdown => 'Markdown',
    ReportFormat.txt => 'TXT',
    ReportFormat.md5sum => 'md5sum',
    ReportFormat.bsd => 'BSD',
  };

  String get description => switch (this) {
    ReportFormat.json =>
      'Canonical machine readable report with a schema version.',
    ReportFormat.csv => 'Fixed columns for spreadsheets.',
    ReportFormat.markdown => 'Human readable table, truncated at 500 rows.',
    ReportFormat.txt => 'Aligned plain text, truncated at 500 rows.',
    ReportFormat.md5sum => 'GNU coreutils compatible digest list.',
    ReportFormat.bsd => 'BSD style digest list.',
  };

  String get fileExtension => switch (this) {
    ReportFormat.json => 'json',
    ReportFormat.csv => 'csv',
    ReportFormat.markdown => 'md',
    ReportFormat.txt => 'txt',
    ReportFormat.md5sum => 'md5',
    ReportFormat.bsd => 'bsd',
  };
}

/// One scanned or hashed file.
class BatchEntry {
  const BatchEntry({
    required this.path,
    required this.size,
    this.mtime,
    this.hashes = const <String, String>{},
    this.status = EntryStatus.ok,
    this.errorKind,
    this.suffixApplied,
    this.renamedTo,
  });

  final String path;
  final int size;
  final DateTime? mtime;
  final Map<String, String> hashes;
  final EntryStatus status;
  final String? errorKind;
  final String? suffixApplied;
  final String? renamedTo;

  BatchEntry copyWith({
    String? path,
    int? size,
    DateTime? mtime,
    Map<String, String>? hashes,
    EntryStatus? status,
    String? errorKind,
    String? suffixApplied,
    String? renamedTo,
  }) {
    return BatchEntry(
      path: path ?? this.path,
      size: size ?? this.size,
      mtime: mtime ?? this.mtime,
      hashes: hashes ?? this.hashes,
      status: status ?? this.status,
      errorKind: errorKind ?? this.errorKind,
      suffixApplied: suffixApplied ?? this.suffixApplied,
      renamedTo: renamedTo ?? this.renamedTo,
    );
  }
}

/// A complete hashing report for one root.
class ReportModel {
  const ReportModel({
    this.schemaVersion = 1,
    required this.root,
    required this.generatedAt,
    this.algorithms = const <String>[],
    this.entries = const <BatchEntry>[],
  });

  final int schemaVersion;
  final String root;
  final DateTime generatedAt;
  final List<String> algorithms;
  final List<BatchEntry> entries;
}

/// One row of a report-to-report or report-to-directory comparison.
class DiffRowModel {
  const DiffRowModel({
    required this.path,
    required this.status,
    this.left,
    this.right,
    this.note,
  });

  final String path;
  final DiffStatus status;
  final BatchEntry? left;
  final BatchEntry? right;
  final String? note;
}

/// Progress event emitted while a batch is hashed.
class HashProgress {
  const HashProgress({
    required this.completed,
    required this.total,
    this.entries = const <BatchEntry>[],
    this.done = false,
  });

  final int completed;
  final int total;
  final List<BatchEntry> entries;
  final bool done;

  double get fraction => total <= 0 ? 0.0 : completed / total;
}

/// Status of one row in a rename dry run.
enum RenameRowStatus {
  ready,
  conflict,
  skipped,
  missingHash,
  invalidName,
  noPermission,
  unsupported;

  String get label => switch (this) {
    RenameRowStatus.ready => 'ready',
    RenameRowStatus.conflict => 'conflict',
    RenameRowStatus.skipped => 'skipped',
    RenameRowStatus.missingHash => 'missing hash',
    RenameRowStatus.invalidName => 'invalid name',
    RenameRowStatus.noPermission => 'no permission',
    RenameRowStatus.unsupported => 'unsupported',
  };
}

/// One row of a rename dry run.
class RenamePlanRowModel {
  const RenamePlanRowModel({
    required this.path,
    required this.status,
    this.targetPath,
    this.message,
  });

  final String path;
  final RenameRowStatus status;
  final String? targetPath;
  final String? message;
}

/// A complete rename dry run.
class RenamePlanModel {
  const RenamePlanModel({
    required this.algorithm,
    required this.length,
    required this.uppercase,
    required this.rows,
  });

  final String algorithm;
  final int length;
  final bool uppercase;
  final List<RenamePlanRowModel> rows;

  int get readyCount => rows
      .where((RenamePlanRowModel row) => row.status == RenameRowStatus.ready)
      .length;

  int get conflictCount => rows
      .where((RenamePlanRowModel row) => row.status == RenameRowStatus.conflict)
      .length;

  int get blockedCount => rows.length - readyCount;
}

/// Result of applying a rename plan.
class RenameExecuteResult {
  const RenameExecuteResult({
    required this.applied,
    required this.skipped,
    required this.failed,
    this.batchId,
  });

  final int applied;
  final int skipped;
  final int failed;
  final String? batchId;
}

/// One file written by the report exporter.
class ExportedReport {
  const ExportedReport({required this.format, required this.path});

  final ReportFormat format;
  final String path;
}

/// One batch stored in the rename log.
class HistoryBatchModel {
  const HistoryBatchModel({
    required this.id,
    required this.createdAt,
    required this.fileCount,
    required this.appliedCount,
    this.undone = false,
  });

  final String id;
  final DateTime createdAt;
  final int fileCount;
  final int appliedCount;
  final bool undone;

  HistoryBatchModel copyWith({bool? undone}) {
    return HistoryBatchModel(
      id: id,
      createdAt: createdAt,
      fileCount: fileCount,
      appliedCount: appliedCount,
      undone: undone ?? this.undone,
    );
  }
}

/// CPU side hardware information reported by the core probe.
class HardwareInfoModel {
  const HardwareInfoModel({
    required this.arch,
    this.detectedFeatures = const <String>[],
    this.blake3BackendReported = 'unknown',
    this.sha256BackendReported = 'unknown',
    this.backendPolicy = 'unknown',
    this.multithread = false,
  });

  final String arch;
  final List<String> detectedFeatures;
  final String blake3BackendReported;
  final String sha256BackendReported;
  final String backendPolicy;
  final bool multithread;
}

/// GPU rendering acceleration information reported by the platform layer.
class RenderingHardwareModel {
  const RenderingHardwareModel({
    this.accelerated = false,
    this.gpuBackend = 'unknown',
  });

  final bool accelerated;
  final String gpuBackend;
}

/// Recommended suffix length for a given number of files.
int recommendedSuffixLength(int fileCount) {
  if (fileCount <= 100) {
    return 5;
  }
  if (fileCount <= 1000) {
    return 8;
  }
  if (fileCount <= 100000) {
    return 10;
  }
  return 12;
}

/// Birthday bound estimate of a suffix collision over the hex alphabet.
double collisionProbability(int fileCount, int length) {
  if (fileCount <= 1 || length <= 0) {
    return 0.0;
  }
  final double space = math.pow(16, length).toDouble();
  final double pairs = fileCount * (fileCount - 1) / 2;
  return 1 - math.exp(-pairs / space);
}
