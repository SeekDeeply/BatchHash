
import 'dart:io' show File, Platform;

import 'package:file_picker/file_picker.dart';

import 'saf_bridge.dart';
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
      // Known upstream defect: on Windows `getDirectoryPath` runs a modal
      // system dialog on the platform thread, so the app window stops
      // repainting while it is open and can show a stale (black) frame
      // (flutter_file_picker #1478). Locking the window to work around it was
      // worse than the defect: `setResizable(false)` strips WS_THICKFRAME, so
      // the window could not be resized or moved at all until the dialog
      // closed. The frame is restored as soon as the dialog returns, so the
      // cosmetic glitch is accepted rather than traded for a frozen window.
      final String? picked = await FilePicker.platform.getDirectoryPath();
      if (picked == null || picked.trim().isEmpty) {
        return null;
      }
      return picked.trim();
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

  /// Open the Android system folder picker and return the granted tree.
  ///
  /// Returns null on any platform other than Android, where a plain path is
  /// always available, and null when the user cancels. Throws [SafException]
  /// when the platform refuses, which the caller must report: silently falling
  /// back to the in-app path dialog would send the user down a route that
  /// cannot read the folder they just picked.
  static Future<SafDirectory?> safDirectory() async {
    if (!Platform.isAndroid) {
      return null;
    }
    final String? uri = await SafBridge.pickDirectory();
    if (uri == null) {
      return null;
    }
    // A tree URI ends with the chosen document id, whose last segment is the
    // folder name; decode it so the UI can name the folder without showing a
    // percent-encoded URI.
    final String raw = uri.split('/').last;
    final String name = Uri.decodeComponent(raw.split(':').last);
    return SafDirectory(treeUri: uri, displayName: name);
  }
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
  /// Returns the written path, or null when the user cancelled. A platform
  /// failure is thrown rather than reported as a cancellation: swallowing it
  /// made a broken save look exactly like a cancelled one, and the caller had
  /// no way to tell the user what went wrong.
  static Future<String?> saveBytes({
    required String suggestedName,
    required List<int> bytes,
  }) async {
    try {
      final String? path = await FilePicker.platform.saveFile(
        dialogTitle: suggestedName,
        fileName: suggestedName,
        // Only honoured on mobile. Desktop ignores this argument and merely
        // returns the chosen path, so the bytes are written below instead of
        // trusting the plugin to have done it.
        bytes: Platform.isAndroid || Platform.isIOS
            ? Uint8List.fromList(bytes)
            : null,
      );
      if (path == null || path.trim().isEmpty) {
        return null;
      }
      final String target = path.trim();
      if (!Platform.isAndroid && !Platform.isIOS) {
        // Desktop: the dialog chose a destination, nothing was written yet.
        // Writing here is what makes the reported success true; without it the
        // UI announced an export that never reached the disk.
        final File file = File(target);
        await file.parent.create(recursive: true);
        await file.writeAsBytes(bytes, flush: true);
      }
      return target;
    } on MissingPluginException catch (error) {
      // No picker at all: this platform cannot save through a dialog.
      throw StateError('save dialog unavailable: $error');
    } on PlatformException catch (error) {
      // A real failure, with a code the caller can show.
      throw StateError('${error.code}: ${error.message ?? error.details ?? ''}');
    }
  }

}
