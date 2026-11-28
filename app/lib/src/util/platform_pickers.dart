
import 'dart:io' show File, Platform;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart' show WidgetsBinding;

import 'saf_bridge.dart';

/// Native pickers with a guaranteed fallback.
///
/// Every helper returns null when the user cancels, and never throws for a
/// missing platform implementation: on a build without the plugin (or on a
/// platform that cannot show a dialog) the caller supplies a manual fallback,
/// so choosing a path is never impossible.
class PlatformPickers {
  const PlatformPickers._();

  /// Whether a system picker is currently open.
  ///
  /// A desktop file dialog is not modal to Flutter: the window keeps receiving
  /// taps, so the user could open a second dialog, start a run, or switch tabs
  /// while the first one is still waiting. The shell listens to this and blocks
  /// input for as long as it is true.
  static final ValueNotifier<bool> picking = ValueNotifier<bool>(false);

  /// Run [body] with [picking] set, restoring the previous value afterwards.
  ///
  /// A counter rather than a plain flag: nested pickers (a directory picker that
  /// falls back to the app's own dialog) would otherwise clear the guard while
  /// the outer dialog is still open.
  static Future<T> _guarded<T>(Future<T> Function() body) async {
    _depth += 1;
    picking.value = true;
    try {
      return await body();
    } finally {
      _depth -= 1;
      if (_depth == 0) {
        // Released one frame later, not immediately. The dialog closes on the
        // same pointer gesture that dismissed it, and the matching mouse-up can
        // still arrive after the future resolves; clearing the guard right away
        // let that residual event land on whatever button sits under the cursor.
        // Waiting for a frame drains it first.
        //
        // Guarded by a binding check so a unit test without a Flutter binding
        // (or a synchronous fake) still works: there is no frame to wait for and
        // nothing to protect against.
        await _releaseAfterFrame();
        // Re-checked: another picker may have started while waiting.
        if (_depth == 0) {
          picking.value = false;
        }
      }
    }
  }

  /// Wait for the next frame, if there is one to wait for.
  ///
  /// `WidgetsBinding.instance` throws rather than returning null when no binding
  /// exists, so the presence check is a try/catch: a unit test that never
  /// initialises a binding must not fail here.
  static Future<void> _releaseAfterFrame() async {
    Future<void>? frame;
    try {
      frame = WidgetsBinding.instance.endOfFrame;
    } catch (_) {
      // No binding: nothing to wait for.
      return;
    }
    // `endOfFrame` never completes when no frame is scheduled, which is exactly
    // the case in a test that never pumps. Racing a zero delay keeps the guard
    // from becoming a deadlock.
    await Future.any(<Future<void>>[frame, Future<void>.delayed(Duration.zero)]);
  }

  static int _depth = 0;

  /// Drop the guard while [body] runs, then restore it.
  ///
  /// The guard exists because a *system* dialog leaves the Flutter window live.
  /// The app's own fallback dialog is a route inside this app: it already blocks
  /// the page it covers, and holding the guard through it would also block the
  /// dialog's own buttons, which is worse than the problem being solved.
  static Future<T> _unguarded<T>(Future<T> Function() body) async {
    final int previous = _depth;
    _depth = 0;
    picking.value = false;
    try {
      return await body();
    } finally {
      _depth = previous;
      picking.value = previous > 0;
    }
  }

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
  ) {
    return _guarded(() => _directory(fallback));
  }

  static Future<String?> _directory(
    Future<String?> Function() fallback,
  ) async {
    try {
      // `lockParentWindow: true` binds the Win32 dialog to this window.
      // Without it the plugin passes `hwndOwner = NULL`, which makes the dialog
      // unowned and therefore non-modal at the OS level: the main window can
      // still be dragged, focused, and moved in front of the dialog. A Flutter
      // `AbsorbPointer` cannot help here, because the taps never reach the Dart
      // engine — the OS decides window activation before Flutter sees anything.
      //
      // With an owner set, Windows disables the parent for the duration, so a
      // click on it only flashes the dialog. Input is discarded by the OS
      // instead of queuing up behind the blocking call and firing all at once
      // when the dialog closes.
      //
      // A black frame while the dialog is open is still possible: the plugin
      // calls Win32 on the main isolate, so the Flutter render pipeline is
      // paused. That is a repaint problem, not an input problem, and it cannot
      // be fixed by moving the call to another isolate — passing an `HWND` to
      // `IFileOpenDialog::Show` from a non-owning thread deadlocks.
      final String? picked = await FilePicker.platform.getDirectoryPath(
        lockParentWindow: true,
      );
      if (picked == null || picked.trim().isEmpty) {
        return null;
      }
      return picked.trim();
    } on PlatformException {
      return _unguarded(fallback);
    } on MissingPluginException {
      return _unguarded(fallback);
    } catch (_) {
      // A plugin can also fail with an Error (for example a missing channel
      // registration); picking a path must still be possible.
      return _unguarded(fallback);
    }
  }

  /// Open the Android system folder picker and return the granted tree.
  ///
  /// Returns null on any platform other than Android, where a plain path is
  /// always available, and null when the user cancels. Throws [SafException]
  /// when the platform refuses, which the caller must report: silently falling
  /// back to the in-app path dialog would send the user down a route that
  /// cannot read the folder they just picked.
  static Future<SafDirectory?> safDirectory() {
    return _guarded(_safDirectory);
  }

  static Future<SafDirectory?> _safDirectory() async {
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
  ) {
    return _guarded(() => _files(fallback));
  }

  static Future<List<String>> _files(
    Future<List<String>> Function() fallback,
  ) async {
    try {
      final FilePickerResult? result = await FilePicker.platform.pickFiles(
        allowMultiple: true,
        type: FileType.any,
        withData: false,
        // Same reason as getDirectoryPath above: an unowned Win32 dialog is
        // not modal to the OS, so the main window stays draggable and clickable
        // behind it.
        lockParentWindow: true,
      );
      return <String>[
        for (final PlatformFile file in result?.files ?? const <PlatformFile>[])
          if (file.path != null && file.path!.isNotEmpty) file.path!,
      ];
    } on PlatformException {
      return _unguarded(fallback);
    } on MissingPluginException {
      return _unguarded(fallback);
    } catch (_) {
      return _unguarded(fallback);
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
  }) {
    return _guarded(
      () => _saveBytes(suggestedName: suggestedName, bytes: bytes),
    );
  }

  static Future<String?> _saveBytes({
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
        // Same reason as getDirectoryPath above: without an owner the Win32
        // dialog is not modal and the main window can be dragged over it.
        lockParentWindow: true,
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
