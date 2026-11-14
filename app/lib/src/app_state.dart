import 'package:flutter/foundation.dart';

import 'models/models.dart';
import 'services/batch_hash_service.dart';

/// Shared state owned by the app shell and passed to the pages.
///
/// The shell rebuilds the selected page from scratch on every navigation, so
/// everything a page must survive a tab switch lives here: the scanned entries,
/// the hash progress summary, the last error, the chosen algorithms and the
/// recent path-chooser inputs.
class BatchHashAppState {
  BatchHashAppState({required this.service});

  final BatchHashService service;

  /// Algorithms selected on a fresh session.
  static const List<String> defaultAlgorithms = <String>['blake3', 'sha256'];

  final ValueNotifier<List<BatchEntry>> entries =
      ValueNotifier<List<BatchEntry>>(const <BatchEntry>[]);
  final ValueNotifier<ReportModel?> report = ValueNotifier<ReportModel?>(null);
  final ValueNotifier<Set<String>> algorithms =
      ValueNotifier<Set<String>>(<String>{...defaultAlgorithms});
  final ValueNotifier<HashProgress?> progress =
      ValueNotifier<HashProgress?>(null);
  final ValueNotifier<Duration?> elapsed = ValueNotifier<Duration?>(null);
  final ValueNotifier<String?> error = ValueNotifier<String?>(null);
  final ValueNotifier<bool> cancelled = ValueNotifier<bool>(false);

  /// Root path of the last scan, used as the report root.
  ///
  /// For an explicit file selection this is the directory the picked files
  /// share, never a single picked file: a report root has to be a directory.
  final ValueNotifier<String> scanRoot = ValueNotifier<String>('');

  /// Absolute files the user picked one by one, newest selection only.
  ///
  /// Empty when the input came from a folder scan. Kept so later steps
  /// (rename, report) can tell an explicit selection from a directory walk.
  final ValueNotifier<List<String>> pickedFiles =
      ValueNotifier<List<String>>(const <String>[]);

  /// Most recent path-chooser inputs, newest first, at most eight entries.
  final ValueNotifier<List<String>> recentPaths =
      ValueNotifier<List<String>>(const <String>[]);

  /// Destination directory for report export; empty means the service default.
  final ValueNotifier<String> exportDirectory = ValueNotifier<String>('');

  /// Result of the last comparison run.
  ///
  /// Held here rather than in the page because the shell rebuilds each page from
  /// scratch on every tab switch. Keeping the rows only in the page meant a
  /// finished comparison vanished as soon as the user looked at another tab.
  final ValueNotifier<List<DiffRowModel>> compareRows =
      ValueNotifier<List<DiffRowModel>>(const <DiffRowModel>[]);

  /// Reports loaded into the comparison panes.
  final ValueNotifier<ReportModel?> compareLeftReport =
      ValueNotifier<ReportModel?>(null);
  final ValueNotifier<ReportModel?> compareRightReport =
      ValueNotifier<ReportModel?>(null);

  /// Paths typed or picked for the comparison panes.
  final ValueNotifier<String> compareLeftPath = ValueNotifier<String>('');
  final ValueNotifier<String> compareRightPath = ValueNotifier<String>('');

  /// Remember a path so the chooser can suggest it again.
  void rememberPath(String path) {
    final String trimmed = path.trim();
    if (trimmed.isEmpty) {
      return;
    }
    final List<String> next = <String>[
      trimmed,
      for (final String existing in recentPaths.value)
        if (existing != trimmed) existing,
    ];
    recentPaths.value = List<String>.unmodifiable(next.take(8));
  }

  void dispose() {
    entries.dispose();
    report.dispose();
    algorithms.dispose();
    progress.dispose();
    elapsed.dispose();
    error.dispose();
    cancelled.dispose();
    scanRoot.dispose();
    pickedFiles.dispose();
    recentPaths.dispose();
    exportDirectory.dispose();
    compareRows.dispose();
    compareLeftReport.dispose();
    compareRightReport.dispose();
    compareLeftPath.dispose();
    compareRightPath.dispose();
  }
}
