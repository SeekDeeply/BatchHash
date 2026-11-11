import 'dart:convert';
import 'dart:io';

import 'package:batchhash/src/services/window_state_store.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('WindowState', () {
    test('round trips through JSON', () {
      const WindowState state = WindowState(width: 1400, height: 900);
      final WindowState? decoded = WindowState.fromJson(
        jsonDecode(jsonEncode(state.toJson())),
      );
      expect(decoded?.width, 1400);
      expect(decoded?.height, 900);
    });

    test('clamps a window too small to use', () {
      // A few pixels of window cannot be grabbed back with the mouse, so a
      // stored size below the minimum is raised instead of restored.
      const WindowState tiny = WindowState(width: 40, height: 30);
      final WindowState clamped = tiny.clamped;
      expect(clamped.width, WindowState.minWidth);
      expect(clamped.height, WindowState.minHeight);
    });

    test('rejects a non-positive or non-finite size', () {
      expect(
        WindowState.fromJson(<String, Object?>{'width': 0, 'height': 800}),
        isNull,
      );
      expect(
        WindowState.fromJson(<String, Object?>{'width': -5, 'height': 800}),
        isNull,
      );
      expect(
        WindowState.fromJson(<String, Object?>{'width': double.nan, 'height': 800}),
        isNull,
      );
    });

    test('ignores a payload that is not geometry', () {
      expect(WindowState.fromJson(null), isNull);
      expect(WindowState.fromJson(<String, Object?>{'width': 'wide'}), isNull);
      expect(WindowState.fromJson(<Object?>[1, 2]), isNull);
    });
  });

  group('WindowStateStore', () {
    late Directory temp;
    late WindowStateStore store;

    setUp(() async {
      temp = await Directory.systemTemp.createTemp('batchhash-window-test');
      store = WindowStateStore(supportDirectory: () async => temp);
    });

    tearDown(() async {
      if (await temp.exists()) {
        await temp.delete(recursive: true);
      }
    });

    test('returns null before anything was saved', () async {
      expect(await store.load(), isNull);
    });

    test('saved geometry survives a new store instance', () async {
      await store.save(const WindowState(width: 1320, height: 860));

      final WindowStateStore reopened = WindowStateStore(
        supportDirectory: () async => temp,
      );
      final WindowState? restored = await reopened.load();
      expect(restored?.width, 1320);
      expect(restored?.height, 860);
    });

    test('a corrupt file degrades to null', () async {
      await File('${temp.path}/window.json').writeAsString('{not json');
      expect(await store.load(), isNull);
    });

    test('an undersized save is clamped on disk', () async {
      await store.save(const WindowState(width: 10, height: 10));
      final WindowState? restored = await store.load();
      expect(restored?.width, WindowState.minWidth);
    });
  });

  group('close path stays cheap', () {
    late Directory temp;
    late WindowStateStore store;

    setUp(() async {
      temp = await Directory.systemTemp.createTemp('batchhash-window-close');
      store = WindowStateStore(supportDirectory: () async => temp);
    });

    tearDown(() async {
      if (await temp.exists()) {
        await temp.delete(recursive: true);
      }
    });

    test('a save leaves the file readable without an explicit sync', () async {
      // The store used to pass flush: true, which forces a disk sync. Window
      // geometry is a convenience, so that cost bought nothing and made every
      // close that saved a size stall.
      await store.save(const WindowState(width: 1200, height: 800));
      final WindowState? reloaded = await WindowStateStore(
        supportDirectory: () async => temp,
      ).load();
      expect(reloaded?.width, 1200);
    });

    test('geometry is clamped before it reaches the disk', () async {
      await store.save(const WindowState(width: 1, height: 1));
      final WindowState? reloaded = await store.load();
      expect(reloaded?.width, WindowState.minWidth);
      expect(reloaded?.height, WindowState.minHeight);
    });
  });
}
