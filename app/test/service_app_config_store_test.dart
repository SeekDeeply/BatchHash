import 'dart:convert';
import 'dart:io';

import 'package:batchhash/src/services/app_config_store.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  // The file store falls back to path_provider when no resolver is injected.
  TestWidgetsFlutterBinding.ensureInitialized();

  group('defaultLogDbFor', () {
    test('appends the conventional log file name', () {
      expect(
        defaultLogDbFor('/home/user/Documents'),
        '/home/user/Documents/.batchhash-rename.db',
      );
    });

    test('does not double the separator', () {
      expect(
        defaultLogDbFor('/home/user/Documents/'),
        '/home/user/Documents/.batchhash-rename.db',
      );
    });

    test('falls back to a bare file name for an empty root', () {
      expect(defaultLogDbFor(''), '.batchhash-rename.db');
    });
  });

  group('RecentRoot', () {
    test('resolvedLogDb prefers an explicit log path', () {
      final RecentRoot explicit = RecentRoot(
        root: '/data',
        logDb: '/data/log/rename.db',
        lastUsed: DateTime.utc(2024, 1, 1),
      );
      expect(explicit.resolvedLogDb, '/data/log/rename.db');

      final RecentRoot derived = RecentRoot(
        root: '/data',
        lastUsed: DateTime.utc(2024, 1, 1),
      );
      expect(derived.resolvedLogDb, '/data/.batchhash-rename.db');
    });

    test('survives a JSON round trip', () {
      final RecentRoot entry = RecentRoot(
        root: '/data/photos',
        logDb: '/data/photos/custom.db',
        lastUsed: DateTime.utc(2024, 5, 1, 10, 30),
      );
      final RecentRoot? decoded = RecentRoot.fromJson(
        jsonDecode(jsonEncode(entry.toJson())),
      );
      expect(decoded, isNotNull);
      expect(decoded!.root, entry.root);
      expect(decoded.logDb, entry.logDb);
      expect(decoded.lastUsed, entry.lastUsed);
      expect(decoded, entry);
    });

    test('skips malformed entries instead of throwing', () {
      expect(RecentRoot.fromJson(null), isNull);
      expect(RecentRoot.fromJson(<String, Object?>{'root': ''}), isNull);
      expect(RecentRoot.fromJson(<String, Object?>{'root': 42}), isNull);

      final RecentRoot tolerant = RecentRoot.fromJson(
        <String, Object?>{'root': '/data', 'last_used': 'not-a-date'},
      )!;
      expect(tolerant.root, '/data');
      expect(tolerant.logDb, isNull);
      expect(tolerant.lastUsed.isUtc, isTrue);
    });
  });

  group('FileAppConfigStore', () {
    late Directory support;

    setUp(() {
      support = Directory.systemTemp.createTempSync('batchhash_config');
    });

    tearDown(() {
      if (support.existsSync()) {
        support.deleteSync(recursive: true);
      }
    });

    FileAppConfigStore build({int maxEntries = 20}) {
      return FileAppConfigStore(
        supportDirectory: () async => support,
        maxEntries: maxEntries,
      );
    }

    test('persists roots across instances and keeps the newest first', () async {
      final FileAppConfigStore store = build();
      expect(await store.loadRecentRoots(), isEmpty);

      await store.rememberRoot(
        RecentRoot(root: '/a', lastUsed: DateTime.utc(2024, 1, 1)),
      );
      await store.rememberRoot(
        RecentRoot(root: '/b', lastUsed: DateTime.utc(2024, 2, 1)),
      );

      // A fresh store over the same file is what a relaunch sees.
      final FileAppConfigStore reopened = build();
      final List<RecentRoot> loaded = await reopened.loadRecentRoots();
      expect(
        loaded.map((RecentRoot entry) => entry.root),
        <String>['/b', '/a'],
      );

      final File file = File('${support.path}/batchhash_recent.json');
      expect(file.existsSync(), isTrue);
      final Object? decoded = jsonDecode(file.readAsStringSync());
      expect(decoded, isA<Map<String, Object?>>());
      expect(
        (decoded! as Map<String, Object?>)['roots'],
        isA<List<Object?>>(),
      );
    });

    test('deduplicates a root that is used twice', () async {
      final FileAppConfigStore store = build();
      await store.rememberRoot(
        RecentRoot(root: '/a', lastUsed: DateTime.utc(2024, 1, 1)),
      );
      await store.rememberRoot(
        RecentRoot(root: '/b', lastUsed: DateTime.utc(2024, 2, 1)),
      );
      await store.rememberRoot(
        RecentRoot(root: '/a', lastUsed: DateTime.utc(2024, 3, 1)),
      );

      final List<RecentRoot> loaded = await store.loadRecentRoots();
      expect(loaded, hasLength(2));
      expect(loaded.first.root, '/a');
      expect(loaded.first.lastUsed, DateTime.utc(2024, 3, 1));
      expect(loaded.last.root, '/b');
    });

    test('drops the oldest roots past the cap', () async {
      final FileAppConfigStore store = build(maxEntries: 2);
      for (final String root in <String>['/a', '/b', '/c']) {
        await store.rememberRoot(
          RecentRoot(root: root, lastUsed: DateTime.utc(2024, 1, 1)),
        );
      }
      final List<RecentRoot> loaded = await store.loadRecentRoots();
      expect(
        loaded.map((RecentRoot entry) => entry.root),
        <String>['/c', '/b'],
      );
    });

    test('a corrupt file degrades to no remembered roots', () async {
      final File file = File('${support.path}/batchhash_recent.json');
      file.writeAsStringSync('{not json');

      final FileAppConfigStore store = build();
      expect(await store.loadRecentRoots(), isEmpty);
      // A later write repairs the file instead of failing.
      await store.rememberRoot(
        RecentRoot(root: '/a', lastUsed: DateTime.utc(2024, 1, 1)),
      );
      final List<RecentRoot> loaded = await store.loadRecentRoots();
      expect(loaded.single.root, '/a');
    });

    test('an unavailable support directory is not fatal', () async {
      final FileAppConfigStore store = FileAppConfigStore(
        supportDirectory: () async =>
            throw const FileSystemException('no support directory'),
      );
      expect(await store.loadRecentRoots(), isEmpty);
      await store.rememberRoot(
        RecentRoot(root: '/a', lastUsed: DateTime.utc(2024, 1, 1)),
      );
      expect(await store.loadRecentRoots(), isEmpty);
    });
  });

  group('InMemoryAppConfigStore', () {
    test('applies the same merge rules as the file store', () async {
      final InMemoryAppConfigStore store = InMemoryAppConfigStore();
      await store.rememberRoot(
        RecentRoot(root: '/a', lastUsed: DateTime.utc(2024, 1, 1)),
      );
      await store.rememberRoot(
        RecentRoot(root: '/b', lastUsed: DateTime.utc(2024, 2, 1)),
      );
      await store.rememberRoot(
        RecentRoot(root: '/a', lastUsed: DateTime.utc(2024, 3, 1)),
      );

      final List<RecentRoot> loaded = await store.loadRecentRoots();
      expect(
        loaded.map((RecentRoot entry) => entry.root),
        <String>['/a', '/b'],
      );
      expect(store.roots, hasLength(2));
    });

    test('honours the seed list', () async {
      final InMemoryAppConfigStore store = InMemoryAppConfigStore(
        initial: <RecentRoot>[
          RecentRoot(root: '/seed', lastUsed: DateTime.utc(2024, 1, 1)),
        ],
      );
      final List<RecentRoot> loaded = await store.loadRecentRoots();
      expect(loaded.single.root, '/seed');
    });
  });
}
