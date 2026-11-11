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

  /// Debounce timer for platforms that only report continuous resizes.
  Timer? _debounce;

  /// Writes are serialised through this chain so a save cannot interleave with
  /// the close that follows it.
  Future<void> _pending = Future<void>.value();

  /// Incremented on every resize.
  ///
  /// A write takes milliseconds, and a resize can land in the middle of one.
  /// Comparing the captured value against the latest tells [_capture] whether
  /// what it just wrote is still current, so a resize during a write is not
  /// silently discarded.
  int _generation = 0;

  /// Whether the current size has not been persisted yet.
  ///
  /// This drives the only thing that can make a close slow: while [_dirty], the
  /// close is intercepted so the write can land. The moment the write lands the
  /// interception is released again, so a normal close never waits on anything.
  bool _dirty = false;

  /// Guards against re-entrant arming of the close interception.
  bool _guardArmed = false;

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
    // Linux reports only the continuous event, so a short debounce stands in
    // for the "finished" signal. No IPC happens here: the size is read once the
    // timer fires, not on every frame of a drag.
    _markDirty();
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 400), _flush);
  }

  @override
  void onWindowResized() {
    // macOS and Windows report the end of the drag directly, so the size can be
    // persisted immediately instead of waiting out a debounce.
    _debounce?.cancel();
    _debounce = null;
    _markDirty();
    _flush();
  }

  /// Arm the close interception, because a write is now outstanding.
  void _markDirty() {
    _dirty = true;
    _generation += 1;
    if (_guardArmed) {
      return;
    }
    _guardArmed = true;
    // Only armed while dirty. A close with nothing to save is left entirely to
    // the platform, which is what keeps it instant; intercepting every close
    // forced a native -> Dart -> destroy round trip on each one.
    unawaited(
      windowManager
          .setPreventClose(true)
          .catchError((Object error) {
            _guardArmed = false;
            debugPrint('close interception not armed: $error');
          }),
    );
  }

  /// Release the close interception once nothing is outstanding.
  Future<void> _disarm() async {
    if (!_guardArmed) {
      return;
    }
    _guardArmed = false;
    try {
      await windowManager.setPreventClose(false);
    } catch (error) {
      debugPrint('close interception not released: $error');
    }
  }

  /// Persist the current size and disarm when done.
  void _flush() {
    _pending = _pending.then((_) async {
      await _capture();
      if (!_dirty) {
        await _disarm();
      }
    });
  }

  /// Close the window, waiting for a write only when one is outstanding.
  ///
  /// Reached only while the interception is armed, i.e. only when a resize is
  /// waiting to be saved. [WindowListener.onWindowClose] returns void, so the
  /// work continues on [_pending].
  @override
  void onWindowClose() {
    _debounce?.cancel();
    _debounce = null;
    _pending = _pending.then((_) async {
      if (_dirty) {
        await _capture();
      }
      try {
        // Released unconditionally: if the write failed there is no reason to
        // trap the user in a window that will not close.
        _guardArmed = false;
        await windowManager.setPreventClose(false);
        await windowManager.destroy();
      } catch (error) {
        debugPrint('window close not completed: $error');
      }
    });
  }

  /// Read the current geometry and persist it.
  ///
  /// Three IPC round trips plus a write, so this never runs per frame. The three
  /// queries are issued together: they are independent, and awaiting them in
  /// sequence tripled the latency on the close path.
  Future<void> _capture() async {
    final int generation = _generation;
    try {
      final List<Object> answers = await Future.wait(<Future<Object>>[
        windowManager.getSize(),
        windowManager.isMaximized(),
        windowManager.isFullScreen(),
      ]);
      final Size size = answers[0] as Size;
      final bool maximised = answers[1] as bool;
      final bool fullScreen = answers[2] as bool;
      if (maximised || fullScreen) {
        // Keep the last normal size; that is what should come back when the
        // user un-maximises next time.
        _settle(generation);
        return;
      }
      await store.save(WindowState(width: size.width, height: size.height));
      _settle(generation);
    } catch (error) {
      _settle(generation);
      debugPrint('window geometry not saved: $error');
    }
  }

  /// Clear dirty only if no newer resize arrived while writing.
  void _settle(int generation) {
    if (generation == _generation) {
      _dirty = false;
    }
  }
}
