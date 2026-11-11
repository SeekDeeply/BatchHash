import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models/operation_log.dart';
import 'app_config_store.dart';

/// Append-only store for [OperationLogEntry] records.
///
/// Backed by two small JSON files in one `history` directory next to the other
/// app support files. The store is deliberately simple: history is a
/// human-facing audit trail, so losing the newest entry on a crash costs
/// nothing, while a database would add a dependency and a migration path for
/// data that is read by eye.
class OperationLogStore {
  OperationLogStore({
    SupportDirectoryResolver? supportDirectory,
    this.maxEntries = defaultMaxEntries,
  }) : assert(maxEntries > 0, 'maxEntries must be positive'),
       _supportDirectory = supportDirectory ?? getApplicationSupportDirectory;

  /// Retention used when nothing has been persisted yet.
  ///
  /// The log page lets the user change it; the chosen value then lives in
  /// `retention.json` so it survives a restart, and this field is only the
  /// fallback for a fresh install or an unreadable preference file.
  static const int defaultMaxEntries = 500;

  /// Fallback retention when no value has been saved.
  final int maxEntries;

  final SupportDirectoryResolver _supportDirectory;

  Directory? _cachedDirectory;
  File? _cachedFile;
  File? _cachedRetentionFile;

  Future<Directory> _historyDirectory() async {
    final Directory? cached = _cachedDirectory;
    if (cached != null) {
      return cached;
    }
    final Directory directory = await _supportDirectory();
    final Directory target = Directory(
      '${directory.path}${Platform.pathSeparator}history',
    );
    if (!await target.exists()) {
      await target.create(recursive: true);
    }
    _cachedDirectory = target;
    return target;
  }

  Future<File> _file() async {
    final File? cached = _cachedFile;
    if (cached != null) {
      return cached;
    }
    final Directory target = await _historyDirectory();
    final File file = File(
      '${target.path}${Platform.pathSeparator}operations.json',
    );
    _cachedFile = file;
    return file;
  }

  Future<File> _retentionFile() async {
    final File? cached = _cachedRetentionFile;
    if (cached != null) {
      return cached;
    }
    final Directory target = await _historyDirectory();
    final File file = File(
      '${target.path}${Platform.pathSeparator}retention.json',
    );
    _cachedRetentionFile = file;
    return file;
  }

  /// Retention currently in force: the persisted choice, else [maxEntries].
  Future<int> loadRetention() async {
    try {
      final File file = await _retentionFile();
      if (!await file.exists()) {
        return maxEntries;
      }
      final Object? decoded = jsonDecode(await file.readAsString());
      if (decoded is Map<String, Object?>) {
        final Object? value = decoded['max_entries'];
        if (value is int && value > 0) {
          return value;
        }
      }
      return maxEntries;
    } catch (_) {
      // A corrupt preference must not make the log unusable, so degrading to
      // the constructor default is the conservative choice.
      return maxEntries;
    }
  }

  /// Persist [value] as the retention used by later appends.
  Future<void> saveRetention(int value) async {
    if (value <= 0) {
      return;
    }
    try {
      final File file = await _retentionFile();
      await file.writeAsString(
        jsonEncode(<String, Object?>{'max_entries': value}),
        flush: true,
      );
    } catch (_) {
      // Same reasoning as loadRetention(): retention is a convenience, not a
      // guarantee, and must never fail the operation that triggered the write.
    }
  }

  /// Every recorded entry, newest first.
  Future<List<OperationLogEntry>> load() async {
    try {
      final File file = await _file();
      if (!await file.exists()) {
        return const <OperationLogEntry>[];
      }
      final String raw = await file.readAsString();
      final Object? decoded = jsonDecode(raw);
      if (decoded is! List<Object?>) {
        return const <OperationLogEntry>[];
      }
      return <OperationLogEntry>[
        for (final Object? row in decoded)
          if (OperationLogEntry.fromJson(row) case final OperationLogEntry entry)
            entry,
      ];
    } catch (_) {
      // A corrupt or unreadable log must never break a page; history is
      // informational, so degrading to "no entries" is the correct failure.
      return const <OperationLogEntry>[];
    }
  }

  /// Append [entry], keeping the newest entries per [loadRetention].
  Future<void> append(OperationLogEntry entry) async {
    try {
      final int retention = await loadRetention();
      final List<OperationLogEntry> existing = await load();
      final List<OperationLogEntry> next = <OperationLogEntry>[
        entry,
        ...existing,
      ].take(retention).toList(growable: false);
      final File file = await _file();
      await file.writeAsString(
        jsonEncode(<Object?>[
          for (final OperationLogEntry item in next) item.toJson(),
        ]),
        flush: true,
      );
    } catch (_) {
      // Same reasoning as load(): a failed write must not surface as an error in
      // the operation the user actually asked for.
    }
  }

  /// Delete every recorded entry. The retention preference is kept.
  Future<void> clear() async {
    try {
      final File file = await _file();
      if (await file.exists()) {
        await file.delete();
      }
    } catch (_) {
      // Nothing to do: the caller refetches and sees whatever is on disk.
    }
  }
}
