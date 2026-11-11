import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models/operation_log.dart';
import 'app_config_store.dart';

/// Append-only store for [OperationLogEntry] records.
///
/// Backed by one small JSON file next to the other app support files. The store
/// is deliberately simple: history is a human-facing audit trail, so losing the
/// newest entry on a crash costs nothing, while a database would add a
/// dependency and a migration path for data that is read by eye.
class OperationLogStore {
  OperationLogStore({SupportDirectoryResolver? supportDirectory})
    : _supportDirectory = supportDirectory ?? getApplicationSupportDirectory;

  final SupportDirectoryResolver _supportDirectory;

  /// Newest entries are kept; older ones drop off so the file stays small.
  static const int maxEntries = 500;

  File? _cachedFile;

  Future<File> _file() async {
    final File? cached = _cachedFile;
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
    final File file = File(
      '${target.path}${Platform.pathSeparator}operations.json',
    );
    _cachedFile = file;
    return file;
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

  /// Append [entry], keeping the newest [maxEntries].
  Future<void> append(OperationLogEntry entry) async {
    try {
      final List<OperationLogEntry> existing = await load();
      final List<OperationLogEntry> next = <OperationLogEntry>[
        entry,
        ...existing,
      ].take(maxEntries).toList(growable: false);
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

  /// Delete every recorded entry.
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