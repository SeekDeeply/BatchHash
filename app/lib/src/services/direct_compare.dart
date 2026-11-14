import 'dart:io';

import '../models/models.dart';
import 'batch_hash_service.dart';

/// Result of a comparison that was produced without a report file.
class DirectCompareResult {
  const DirectCompareResult({
    required this.rows,
    required this.leftRoot,
    required this.rightRoot,
  });

  /// Rows from the underlying diff, in the same shape report diffs produce.
  final List<DiffRowModel> rows;

  /// Roots the two sides were hashed from, so the UI can name them.
  final String leftRoot;
  final String rightRoot;
}

/// Compares two paths directly: file to file, or folder to folder.
///
/// A report file was previously the only way in, which forced the user to hash
/// both sides and save two reports just to answer "are these the same". This
/// hashes each side in memory and reuses the existing report diff, so the
/// comparison semantics (path normalisation, duplicate keys, ordering) stay
/// exactly the ones the report path already has instead of growing a second,
/// subtly different implementation.
class DirectComparator {
  const DirectComparator({required this.service});

  final BatchHashService service;

  /// Hash [paths] on both sides with [algorithms] and diff the results.
  ///
  /// Both sides must be the same kind: comparing a file against a folder is a
  /// user mistake the diff cannot express meaningfully, because every path on
  /// one side would be a directory prefix of the other.
  Future<DirectCompareResult> compare({
    required String leftPath,
    required String rightPath,
    required List<String> algorithms,
  }) async {
    if (algorithms.isEmpty) {
      throw ArgumentError('at least one algorithm is required');
    }
    final FileSystemEntityType leftType = await _typeOf(leftPath);
    final FileSystemEntityType rightType = await _typeOf(rightPath);
    if (leftType != rightType) {
      throw StateError(
        'both sides must be the same kind: left is ${leftType.name}, '
        'right is ${rightType.name}',
      );
    }

    final ReportModel left = await _reportFor(
      leftPath,
      isDirectory: leftType == FileSystemEntityType.directory,
      algorithms: algorithms,
    );
    final ReportModel right = await _reportFor(
      rightPath,
      isDirectory: rightType == FileSystemEntityType.directory,
      algorithms: algorithms,
    );

    final List<DiffRowModel> rows = await service.diffReports(
      left: left,
      right: right,
    );
    return DirectCompareResult(
      rows: rows,
      leftRoot: left.root,
      rightRoot: right.root,
    );
  }

  /// Hash one side into a report-shaped value.
  ///
  /// A directory is scanned recursively on purpose: a folder comparison that
  /// skipped subfolders would report their contents as missing on one side,
  /// which is the bug this path exists to avoid.
  Future<ReportModel> _reportFor(
    String path, {
    required bool isDirectory,
    required List<String> algorithms,
  }) async {
    final List<BatchEntry> scanned = isDirectory
      ? await service.scanDirectory(path, recursive: true)
      : await service.scanFiles(<String>[path]);

    final Map<String, BatchEntry> latest = <String, BatchEntry>{
      for (final BatchEntry entry in scanned) entry.path: entry,
    };
    await for (final HashProgress progress in service.computeHashes(
      entries: scanned,
      algorithms: algorithms,
    )) {
      for (final BatchEntry entry in progress.entries) {
        if (entry.hashes.isNotEmpty) {
          latest[entry.path] = entry;
        }
      }
    }

    final String? root = isDirectory
      ? path
      : File(path).parent.path;
    return ReportModel(
      root: root,
      generatedAt: DateTime.now(),
      algorithms: algorithms,
      entries: latest.values.toList(growable: false),
    );
  }

  static Future<FileSystemEntityType> _typeOf(String path) =>
      FileSystemEntity.type(path);
}
