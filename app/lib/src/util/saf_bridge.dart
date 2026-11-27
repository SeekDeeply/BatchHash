import 'package:flutter/services.dart';

/// One node of a Storage Access Framework tree.
///
/// [mtimeMs] is milliseconds since the epoch, which is what
/// `DocumentsContract.Document.COLUMN_LAST_MODIFIED` reports and what the Dart
/// side of this app already uses for a filesystem stat.
class SafNode {
  const SafNode({
    required this.path,
    required this.documentId,
    required this.isDirectory,
    required this.size,
    required this.mtimeMs,
  });

  /// Path relative to the granted tree root, slash separated.
  final String path;

  /// The provider document id that addresses this node.
  ///
  /// Opening a document needs this id. Rebuilding it from the tree root id and
  /// the relative path only works for providers whose ids happen to be path
  /// shaped, so the listing has to carry the real one.
  final String documentId;

  final bool isDirectory;

  /// Size in bytes, or null when the provider does not report one.
  ///
  /// Null rather than 0: a directory has no size, and a provider is allowed to
  /// leave the column empty. Reporting 0 made such files look empty in reports.
  final int? size;

  final int mtimeMs;
}

/// A folder the user granted through the Storage Access Framework.
class SafDirectory {
  const SafDirectory({required this.treeUri, required this.displayName});

  /// The granted tree URI.
  final String treeUri;

  /// Name shown to the user, so the UI can name the folder without decoding
  /// the URI.
  final String displayName;
}
/// One file opened through the Storage Access Framework.
class SafOpenFile {
  const SafOpenFile({
    required this.path,
    required this.handle,
    required this.fd,
    required this.size,
  });

  /// Path relative to the granted tree root.
  final String path;

  /// Opaque handle identifying this open, used to close it again.
  ///
  /// Not the path: the same path can be open twice, and closing by path left the
  /// earlier descriptor open with nothing left to reference it.
  final String handle;

  /// Raw file descriptor number. Valid only until [SafBridge.closeFiles].
  final int fd;

  /// Size in bytes as reported by the descriptor, or null when unavailable.
  final int? size;
}
/// Storage Access Framework bridge for Android.
///
/// On Android 11+ the folder picker hands back a `content://` tree URI, and
/// `dart:io` cannot list or read through one: `File` only understands POSIX
/// paths. That is an upstream limitation, not a bug in this app — see
/// flutter_file_picker#1552 and dart-lang/sdk#54878 — so the only way to make
/// "pick a folder" work is to walk the tree through the platform.
///
/// Reading file bytes is deliberately absent: the Dart side opens the
/// descriptors it needs and hands them to the Rust core, so hashing never
/// happens on a thread this bridge does not control.
class SafBridge {
  const SafBridge._();

  static const MethodChannel _channel = MethodChannel('batchhash/saf');

  /// Open the system folder picker.
  ///
  /// Resolves to the granted tree URI, or null when the user backed out.
  static Future<String?> pickDirectory() async {
    try {
      return await _channel.invokeMethod<String>('pickDirectory');
    } on PlatformException catch (error) {
      if (error.code == 'ALREADY_PICKING') {
        // A second pick while one is open is a programming error, not a user
        // outcome; surface it rather than silently returning nothing.
        throw StateError('a folder picker is already open');
      }
      throw SafException(error.code, error.message);
    }
    on MissingPluginException {
      // Not Android, or the native side is not linked in this build.
      return null;
    }
  }

  /// Open a read-only descriptor for each of [documents] inside [treeUri].
  ///
  /// [documents] carries each node's provider document id, which the native side
  /// needs to address the file. Returns the fd, a close handle, and the
  /// descriptor's own size for each document that could be opened. Documents
  /// that cannot be opened are omitted rather than reported as an error, so the
  /// caller hashes what it could open and says so.
  ///
  /// The descriptor stays open until [closeFiles] is called with its handle.
  /// The Rust core dups what it is given, so it never owns this handle.
  static Future<List<SafOpenFile>> openFiles({
    required String treeUri,
    required List<SafNode> documents,
  }) async {
    if (documents.isEmpty) {
      return const <SafOpenFile>[];
    }
    try {
      final List<Object?>? raw = await _channel.invokeListMethod<Object?>(
        'openFiles',
        <String, Object?>{
          'uri': treeUri,
          'documents': <Map<String, String>>[
            for (final SafNode node in documents)
              <String, String>{
                'path': node.path,
                'document_id': node.documentId,
              },
          ],
        },
      );
      if (raw == null) {
        return const <SafOpenFile>[];
      }
      return <SafOpenFile>[
        for (final Object? item in raw)
          if (item is Map)
            SafOpenFile(
              path: '${item['path'] ?? ''}',
              handle: '${item['handle'] ?? ''}',
              fd: (item['fd'] as num?)?.toInt() ?? -1,
              // Absent means the provider reported no size, which is different
              // from a zero-byte file.
              size: (item['size'] as num?)?.toInt(),
            ),
      ];
    } on PlatformException catch (error) {
      throw SafException(error.code, error.message);
    }
    on MissingPluginException {
      return const <SafOpenFile>[];
    }
  }

  /// Release descriptors opened by [openFiles].
  ///
  /// Safe to call for paths that were never opened. Must be called for every
  /// successful open, or the process leaks a descriptor per file until the
  /// activity is destroyed.
  static Future<void> closeFiles(List<String> handles) async {
    if (handles.isEmpty) {
      return;
    }
    try {
      await _channel.invokeMethod<void>('closeFiles', <String, Object?>{
        'handles': handles,
      });
    } on PlatformException {
      // Nothing useful can be done about a failed close here; the caller is
      // about to drop the handles anyway.
    } on MissingPluginException {
      // No native side, so nothing was ever opened.
    }
  }
  /// Walk [treeUri] and return every node, directories included.
  ///
  /// [maxDepth] bounds how many levels of subdirectories below the root are
  /// descended into; the native side stops there rather than erroring, so the
  /// caller gets a truncated tree it can report as truncated.
  static Future<List<SafNode>> listTree({
    required String treeUri,
    int maxDepth = 8,
  }) async {
    try {
      final List<Object?>? raw = await _channel.invokeListMethod<Object?>(
        'listTree',
        <String, Object?>{
          'uri': treeUri,
          'maxDepth': maxDepth,
        },
      );
      if (raw == null) {
        return const <SafNode>[];
      }
      return <SafNode>[
        for (final Object? item in raw)
          if (item is Map)
            SafNode(
              path: '${item['path'] ?? ''}',
              documentId: '${item['document_id'] ?? ''}',
              isDirectory: item['is_dir'] == true,
              size: (item['size'] as num?)?.toInt(),
              mtimeMs: (item['mtime_ms'] as num?)?.toInt() ?? 0,
            ),
      ];
    } on PlatformException catch (error) {
      throw SafException(error.code, error.message);
    }
    on MissingPluginException {
      return const <SafNode>[];
    }
  }
}

/// A SAF call the platform refused.
class SafException implements Exception {
  const SafException(this.code, this.message);

  /// Platform error code, for example `LIST_FAILED`.
  final String code;

  /// Human readable reason from the platform, when it gave one.
  final String? message;

  @override
  String toString() =>
      'SafException($code${message == null ? '' : ': $message'})';
}

/// Convert an epoch-milliseconds timestamp to the form this app stores.
///
/// The platform reports milliseconds while every mtime in a report is an
/// RFC3339 string with second precision, so the conversion has to happen
/// exactly once, here. Doing it at each call site is how two formats end up
/// in one report.
String safMtimeToIso(int mtimeMs) {
  if (mtimeMs <= 0) {
    return '';
  }
  final DateTime at = DateTime.fromMillisecondsSinceEpoch(
    mtimeMs,
    isUtc: true,
  );
  // Formatted by hand rather than with toIso8601String(), which appends
  // ".000": every mtime in a report is second-precision RFC3339, and a stray
  // millisecond field would make the two sources of mtime differ.
  String two(int value) => value.toString().padLeft(2, '0');
  final String month = two(at.month);
  final String day = two(at.day);
  final String hour = two(at.hour);
  final String minute = two(at.minute);
  final String second = two(at.second);
  return '${at.year}-$month-${day}T$hour:$minute:${second}Z';
}

/// Split [items] into consecutive groups of at most [size].
///
/// Used to bound how many file descriptors are open at once: a folder with a
/// few thousand files cannot open all of them simultaneously, and the OS fd
/// limit turns that into a failure that looks like a corrupt file.
List<List<T>> chunk<T>(List<T> items, int size) {
  if (size <= 0) {
    throw ArgumentError('chunk size must be positive, got $size');
  }
  if (items.isEmpty) {
    return <List<T>>[];
  }
  return <List<T>>[
    for (int start = 0; start < items.length; start += size)
      items.sublist(
        start,
        start + size > items.length ? items.length : start + size,
      ),
  ];
}
