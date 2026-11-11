import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import 'app_config_store.dart';

/// Remembered desktop window geometry.
///
/// Pure data so the clamping rules can be tested without a window.
class WindowState {
  const WindowState({required this.width, required this.height});

  final double width;
  final double height;

  /// Sizes below this are not usable: the shell would clip its own chrome and
  /// a few pixels of window is impossible to grab back with the mouse.
  static const double minWidth = 720;
  static const double minHeight = 520;

  /// Defaults for a first run, sized for the three-column batch layout.
  static const WindowState fallback = WindowState(width: 1180, height: 820);

  WindowState get clamped => WindowState(
    width: width < minWidth ? minWidth : width,
    height: height < minHeight ? minHeight : height,
  );

  Map<String, Object?> toJson() => <String, Object?>{
    'width': width,
    'height': height,
  };

  /// Decode a stored geometry, rejecting anything unusable.
  ///
  /// A window can be closed while maximised to an absurd size or on a monitor
  /// that is no longer attached; those values must not be restored blindly, so
  /// a non-finite or non-positive number is treated as absent.
  static WindowState? fromJson(Object? raw) {
    if (raw is! Map<String, Object?>) {
      return null;
    }
    final Object? width = raw['width'];
    final Object? height = raw['height'];
    if (width is! num || height is! num) {
      return null;
    }
    final double w = width.toDouble();
    final double h = height.toDouble();
    if (!w.isFinite || !h.isFinite || w <= 0 || h <= 0) {
      return null;
    }
    return WindowState(width: w, height: h).clamped;
  }
}

/// Persists the desktop window size between runs.
///
/// Desktop only: on Android and iOS there is no resizable window, so [load]
/// returns null and [save] is never called by the shell.
class WindowStateStore {
  WindowStateStore({SupportDirectoryResolver? supportDirectory})
    : _supportDirectory = supportDirectory ?? getApplicationSupportDirectory;

  final SupportDirectoryResolver _supportDirectory;

  File? _cachedFile;

  Future<File> _file() async {
    final File? cached = _cachedFile;
    if (cached != null) {
      return cached;
    }
    final Directory directory = await _supportDirectory();
    final File file = File(
      '${directory.path}${Platform.pathSeparator}window.json',
    );
    _cachedFile = file;
    return file;
  }

  /// The remembered geometry, or null when nothing usable was stored.
  Future<WindowState?> load() async {
    try {
      final File file = await _file();
      if (!await file.exists()) {
        return null;
      }
      final String raw = await file.readAsString();
      return WindowState.fromJson(jsonDecode(raw));
    } catch (_) {
      // A broken geometry file must never stop the app from opening; falling
      // back to the default size is always correct.
      return null;
    }
  }

  /// Store [state], clamped so a bad value cannot be written.
  Future<void> save(WindowState state) async {
    try {
      final File file = await _file();
      await file.parent.create(recursive: true);
      await file.writeAsString(
        jsonEncode(state.clamped.toJson()),
        flush: true,
      );
    } catch (_) {
      // Same reasoning as load(): window geometry is a convenience, not data
      // the user asked to keep, so a failed write stays silent.
    }
  }
}
