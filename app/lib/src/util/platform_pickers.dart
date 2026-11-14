import 'dart:io' show Platform;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/services.dart';

/// Native pickers with a guaranteed fallback.
///
/// Every helper returns null when the user cancels, and never throws for a
/// missing platform implementation: on a build without the plugin (or on a
/// platform that cannot show a dialog) the caller supplies a manual fallback,
/// so choosing a path is never impossible.
class PlatformPickers {
  const PlatformPickers._();

  /// Ask for a directory. Returns null when cancelled or unavailable.
  ///
  /// [fallback] is invoked when the native dialog cannot be shown; it should
  /// present the app's own path dialog and resolve to the same contract.
  ///
  /// No starting-folder hint is passed: `getDirectoryPath` exposes only a
  /// dialog title, so a path handed to it would be rendered as a title string
  /// rather than opening that folder.
  static Future<String?> directory(
    Future<String?> Function() fallback,
  ) async {
    try {
      final String? picked = await FilePicker.platform.getDirectoryPath();
      if (picked == null || picked.trim().isEmpty) {
        return null;
      }
      final String path = picked.trim();
      // On Android getDirectoryPath hands back a storage-access-framework tree
      // such as content://com.android.externalstorage.documents/tree/primary%3A
      // which no file API can open: the app holds a content URI, not a readable
      // directory. Scanning it silently yields nothing, which is exactly the
      // "picking a folder does nothing" report. Ask for real files instead and
      // let the caller derive the folder from them.
      if (Platform.isAndroid && !path.startsWith('/')) {
        return null;
      }
      return path;
    } on PlatformException {
      return fallback();
    } on MissingPluginException {
      return fallback();
    } catch (_) {
      // A plugin can also fail with an Error (for example a missing channel
      // registration); picking a path must still be possible.
      return fallback();
    }
  }

  /// Pick files on Android to stand in for a folder.
  ///
  /// Returns the common parent directory of the chosen files, or null when the
  /// user cancelled. This is the only way to reach a real, readable folder on
  /// Android without requesting all-files access: the document tree URI that
  /// the folder picker returns is not openable through dart:io.
  static Future<String?> folderViaFiles() async {
    final List<String> picked = await files(() async => const <String>[]);
    if (picked.isEmpty) {
      return null;
    }
    return commonParent(picked);
  }

  /// Deepest directory containing every path, without a trailing separator.
  ///
  /// Returns an empty string when the paths share nothing beyond the root, so a
  /// caller can treat that as "no usable folder" instead of scanning /.
  static String commonParent(List<String> paths) {
    if (paths.isEmpty) {
      return '';
    }
    List<String> shared = _split(paths.first);
    for (final String path in paths.skip(1)) {
      final List<String> other = _split(path);
      var keep = 0;
      while (keep < shared.length &&
          keep < other.length &&
          shared[keep] == other[keep]) {
        keep += 1;
      }
      shared = shared.sublist(0, keep);
    }
    // Drop the last segment only when the shared prefix is itself one of the
    // inputs. When two files differ in their final segment the prefix already
    // describes their directory (a.txt + b.txt -> .../Download), and blindly
    // cutting a segment there would walk one level too high.
    final String prefix = '/${shared.join('/')}';
    if (paths.contains(prefix) && shared.isNotEmpty) {
      shared = shared.sublist(0, shared.length - 1);
    }
    if (shared.isEmpty) {
      return '';
    }
    return '/${shared.join('/')}';
  }

  static List<String> _split(String path) =>
      path.split('/').where((String part) => part.isNotEmpty).toList();

  /// Ask for one or more files. Returns an empty list when cancelled.
  static Future<List<String>> files(
    Future<List<String>> Function() fallback,
  ) async {
    try {
      final FilePickerResult? result = await FilePicker.platform.pickFiles(
        allowMultiple: true,
        type: FileType.any,
        withData: false,
      );
      return <String>[
        for (final PlatformFile file in result?.files ?? const <PlatformFile>[])
          if (file.path != null && file.path!.isNotEmpty) file.path!,
      ];
    } on PlatformException {
      return fallback();
    } on MissingPluginException {
      return fallback();
    } catch (_) {
      return fallback();
    }
  }

  /// Save [bytes] to a file the user picks.
  ///
  /// On Android this is the only reliable way to reach shared storage: since
  /// scoped storage (API 29) an app cannot open an arbitrary path under
  /// /storage/emulated/0 for writing, so a directory picker plus File.write
  /// fails with errno 1. `saveFile` goes through the system document provider,
  /// which already holds the grant for whatever the user chose.
  ///
  /// Desktop still works the same way; the plugin writes the bytes itself, so
  /// callers do not need to know which mechanism was used.
  ///
  /// Returns the saved path, or null when the user cancelled.
  static Future<String?> saveBytes({
    required String suggestedName,
    required List<int> bytes,
  }) async {
    try {
      final String? path = await FilePicker.platform.saveFile(
        dialogTitle: suggestedName,
        fileName: suggestedName,
        bytes: Uint8List.fromList(bytes),
      );
      if (path == null || path.trim().isEmpty) {
        return null;
      }
      return path.trim();
    } on PlatformException {
      return null;
    } on MissingPluginException {
      return null;
    } catch (_) {
      return null;
    }
  }
}
