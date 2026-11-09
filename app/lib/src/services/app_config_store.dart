import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

/// Resolves the directory where the app may keep its own support files.
///
/// A typedef so a test can inject a temporary directory instead of calling the
/// path_provider platform channel.
typedef SupportDirectoryResolver = Future<Directory> Function();

/// The conventional rename log path for a scanned root.
///
/// The pipeline writes its log next to the files it renames unless the caller
/// supplies another path, so history can be rediscovered from a root alone.
String defaultLogDbFor(String root) {
  if (root.isEmpty) {
    return '.batchhash-rename.db';
  }
  final bool separated = root.endsWith('/') || root.endsWith('\\');
  return separated ? '${root}.batchhash-rename.db' : '$root/.batchhash-rename.db';
}

/// One batch root the app has used before.
///
/// A fresh launch has no in-memory scan root, so the only way to rediscover
/// history is to remember the roots (and their logs) between launches.
class RecentRoot {
  const RecentRoot({required this.root, this.logDb, required this.lastUsed});

  /// Root path the user scanned or renamed.
  final String root;

  /// Explicit rename log path, when the batch did not use the default.
  final String? logDb;

  /// When this root was last used, newest first in the store.
  final DateTime lastUsed;

  /// The rename log this root's history is stored in.
  String get resolvedLogDb {
    final String? explicit = logDb;
    if (explicit != null && explicit.isNotEmpty) {
      return explicit;
    }
    return defaultLogDbFor(root);
  }

  RecentRoot copyWith({String? root, String? logDb, DateTime? lastUsed}) {
    return RecentRoot(
      root: root ?? this.root,
      logDb: logDb ?? this.logDb,
      lastUsed: lastUsed ?? this.lastUsed,
    );
  }

  Map<String, Object?> toJson() {
    return <String, Object?>{
      'root': root,
      if (logDb != null) 'log_db': logDb,
      'last_used': lastUsed.toUtc().toIso8601String(),
    };
  }

  /// Decode one stored entry, skipping anything malformed.
  static RecentRoot? fromJson(Object? raw) {
    if (raw is! Map<String, Object?>) {
      return null;
    }
    final Object? root = raw['root'];
    if (root is! String || root.isEmpty) {
      return null;
    }
    final Object? logDb = raw['log_db'];
    final Object? lastUsed = raw['last_used'];
    return RecentRoot(
      root: root,
      logDb: logDb is String && logDb.isNotEmpty ? logDb : null,
      lastUsed: lastUsed is String
          ? DateTime.tryParse(lastUsed) ??
                DateTime.fromMillisecondsSinceEpoch(0, isUtc: true)
          : DateTime.fromMillisecondsSinceEpoch(0, isUtc: true),
    );
  }

  @override
  bool operator ==(Object other) {
    return other is RecentRoot &&
        other.root == root &&
        other.logDb == logDb &&
        other.lastUsed == lastUsed;
  }

  @override
  int get hashCode => Object.hash(root, logDb, lastUsed);

  @override
  String toString() => 'RecentRoot($root, logDb: $logDb, lastUsed: $lastUsed)';
}

/// Persistence for the batch roots the app has used.
///
/// Kept behind an interface so the service can be built with an in-memory store
/// in tests, and so a platform where the store is unavailable degrades to "no
/// remembered roots" instead of failing a scan.
abstract class AppConfigStore {
  /// Remembered roots, newest first.
  Future<List<RecentRoot>> loadRecentRoots();

  /// Record [entry] as the newest root and drop older duplicates of it.
  Future<void> rememberRoot(RecentRoot entry);
}

/// [AppConfigStore] backed by a small JSON file in the application support
/// directory (not the system temp directory, which the OS may clear).
class FileAppConfigStore implements AppConfigStore {
  FileAppConfigStore({
    SupportDirectoryResolver? supportDirectory,
    this.fileName = 'batchhash_recent.json',
    this.maxEntries = 20,
  }) : assert(maxEntries > 0, 'maxEntries must be positive'),
       _supportDirectory = supportDirectory;

  /// Where the support directory comes from; overridable so tests can point at
  /// a temporary directory without the platform channel.
  final SupportDirectoryResolver? _supportDirectory;

  /// Name of the JSON file inside the support directory.
  final String fileName;

  /// How many roots to keep; older entries are dropped.
  final int maxEntries;

  /// The store file, or null when the platform cannot provide a directory.
  Future<File?> _file() async {
    try {
      final SupportDirectoryResolver resolver =
          _supportDirectory ?? getApplicationSupportDirectory;
      final Directory directory = await resolver();
      if (!directory.existsSync()) {
        directory.createSync(recursive: true);
      }
      return File('${directory.path}/$fileName');
    } on Exception catch (_) {
      // No path_provider implementation (unit tests, an unsupported host):
      // configuration memory is best effort, never fatal.
      return null;
    }
  }

  @override
  Future<List<RecentRoot>> loadRecentRoots() async {
    final File? file = await _file();
    if (file == null || !file.existsSync()) {
      return const <RecentRoot>[];
    }
    try {
      final Object? decoded = jsonDecode(await file.readAsString());
      if (decoded is! Map<String, Object?>) {
        return const <RecentRoot>[];
      }
      final Object? raw = decoded['roots'];
      if (raw is! List<Object?>) {
        return const <RecentRoot>[];
      }
      final List<RecentRoot> roots = <RecentRoot>[];
      for (final Object? item in raw) {
        final RecentRoot? entry = RecentRoot.fromJson(item);
        if (entry != null) {
          roots.add(entry);
        }
      }
      return List<RecentRoot>.unmodifiable(roots);
    } on Exception catch (_) {
      // A truncated or hand-edited file must not break history.
      return const <RecentRoot>[];
    }
  }

  @override
  Future<void> rememberRoot(RecentRoot entry) async {
    final File? file = await _file();
    if (file == null) {
      return;
    }
    final List<RecentRoot> merged = _merge(
      await loadRecentRoots(),
      entry,
      maxEntries,
    );
    final Map<String, Object?> payload = <String, Object?>{
      'version': 1,
      'roots': <Object?>[for (final RecentRoot root in merged) root.toJson()],
    };
    try {
      await file.writeAsString(jsonEncode(payload), flush: true);
    } on Exception catch (_) {
      // A read-only volume must not fail the scan that triggered the write.
    }
  }
}

/// [AppConfigStore] that keeps everything in memory; used by tests.
class InMemoryAppConfigStore implements AppConfigStore {
  InMemoryAppConfigStore({List<RecentRoot> initial = const <RecentRoot>[]})
    : _roots = List<RecentRoot>.of(initial);

  /// How many roots to keep; older entries are dropped.
  static const int maxEntries = 20;

  final List<RecentRoot> _roots;

  /// Remembered roots, newest first.
  List<RecentRoot> get roots => List<RecentRoot>.unmodifiable(_roots);

  @override
  Future<List<RecentRoot>> loadRecentRoots() async {
    return List<RecentRoot>.unmodifiable(_roots);
  }

  @override
  Future<void> rememberRoot(RecentRoot entry) async {
    final List<RecentRoot> merged = _merge(_roots, entry, maxEntries);
    _roots
      ..clear()
      ..addAll(merged);
  }
}

/// Put [entry] first, drop earlier duplicates of the same root and trim.
List<RecentRoot> _merge(
  List<RecentRoot> existing,
  RecentRoot entry,
  int maxEntries,
) {
  final List<RecentRoot> merged = <RecentRoot>[entry];
  for (final RecentRoot other in existing) {
    if (other.root == entry.root) {
      continue;
    }
    merged.add(other);
  }
  if (merged.length > maxEntries) {
    merged.removeRange(maxEntries, merged.length);
  }
  return merged;
}
