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
  final ValueNotifier<String> scanRoot = ValueNotifier<String>('');

  /// Most recent path-chooser inputs, newest first, at most eight entries.
  final ValueNotifier<List<String>> recentPaths =
      ValueNotifier<List<String>>(const <String>[]);

  /// Destination directory for report export; empty means the service default.
  final ValueNotifier<String> exportDirectory = ValueNotifier<String>('');

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
    recentPaths.dispose();
    exportDirectory.dispose();
  }
}
