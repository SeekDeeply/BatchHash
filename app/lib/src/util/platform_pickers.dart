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
}
