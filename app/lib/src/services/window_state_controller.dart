import 'dart:async';
import 'dart:io';
import 'dart:ui' show Size;

import 'package:flutter/foundation.dart';
import 'package:window_manager/window_manager.dart';

import 'window_state_store.dart';

/// Restores and remembers the desktop window size.
///
/// Only desktop has a resizable window; on mobile the whole class is inert so
/// no page has to branch on the platform.
class WindowStateController with WindowListener {
  WindowStateController({required this.store});

  final WindowStateStore store;

  /// Debounce timer for the resize handler.
  ///
  /// A drag fires dozens of resize events per second; writing on every one
  /// would hammer the disk while the user is still moving the window.
  Timer? _debounce;

  /// Writes are serialised through this chain so a save started during a
  /// debounce cannot interleave with the final write on close.
  Future<void> _pending = Future<void>.value();

  /// False on Android/iOS, where there is no window to size.
  static bool get isSupported =>
      !kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS);

  /// Apply the remembered size (or the default) and start following changes.
  ///
  /// Never throws: a window manager that is not ready must not prevent the app
  /// from starting, and the default size is a perfectly good window.
  Future<void> attach() async {
    if (!isSupported) {
      return;
    }
    try {
      await windowManager.ensureInitialized();
      final WindowState stored = await store.load() ?? WindowState.fallback;
      final WindowState initial = stored.clamped;
      await windowManager.waitUntilReadyToShow(
        WindowOptions(
          size: Size(initial.width, initial.height),
          center: true,
          minimumSize: const Size(
            WindowState.minWidth,
            WindowState.minHeight,
          ),
        ),
        () async {
          await windowManager.show();
          await windowManager.focus();
        },
      );
      windowManager.addListener(this);
    } catch (error) {
      debugPrint('window geometry not restored: $error');
    }
  }

  /// Stop listening and flush any pending write.
  Future<void> detach() async {
    if (!isSupported) {
      return;
    }
    windowManager.removeListener(this);
    _debounce?.cancel();
    _debounce = null;
    await _pending;
  }

  @override
  void onWindowResize() {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 400), _capture);
  }

  /// Also capture on close: a user who resizes and immediately quits would
  /// otherwise lose the last size because the debounce never fired.
  @override
  void onWindowClose() {
    _debounce?.cancel();
    _debounce = null;
    _pending = _pending.then((_) => _capture());
  }

  Future<void> _capture() async {
    try {
      // Report the normal size, not the maximised one: restoring a maximised
      // window should still remember where the user had put the edges.
      final Size size = await windowManager.getSize();
      final bool maximised = await windowManager.isMaximized();
      final bool fullScreen = await windowManager.isFullScreen();
      if (maximised || fullScreen) {
        return;
      }
      await store.save(
        WindowState(width: size.width, height: size.height),
      );
    } catch (error) {
      debugPrint('window geometry not saved: $error');
    }
  }
}
