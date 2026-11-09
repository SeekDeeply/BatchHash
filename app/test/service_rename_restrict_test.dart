import 'dart:io';

import 'package:batchhash/src/bridge/native_bridge.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:batchhash/src/services/rust_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// Rename execution must be narrowed to exactly what the user previewed.
///
/// The bug this guards: executeRenames re-ran the whole batch, so files the
/// preview never showed were renamed too. The fix forwards the plan's ready
/// rows as restrict_paths.
void main() {
  group('rootRelativePath', () {
    test('strips the root prefix', () {
      expect(
        rootRelativePath('/home/user/Documents', '/home/user/Documents/a.txt'),
        'a.txt',
      );
      expect(
        rootRelativePath(
          '/home/user/Documents',
          '/home/user/Documents/docs/b.md',
        ),
        'docs/b.md',
      );
    });

    test('ignores a trailing separator on the root', () {
      expect(
        rootRelativePath('/home/user/Documents/', '/home/user/Documents/a.txt'),
        'a.txt',
      );
    });

    test('normalizes backslashes to forward slashes', () {
      expect(
        rootRelativePath(
          r'C:\Users\me\Documents',
          r'C:\Users\me\Documents\docs\b.md',
        ),
        'docs/b.md',
      );
    });

    test('drops a leading dot slash', () {
      expect(rootRelativePath('/root', '/root/./a.txt'), 'a.txt');
      expect(rootRelativePath('', './a.txt'), 'a.txt');
    });

    test('keeps an already relative path', () {
      expect(rootRelativePath('/root', 'docs/b.md'), 'docs/b.md');
    });

    test('returns the root itself as an empty relative path', () {
      expect(rootRelativePath('/root', '/root'), '');
    });
  });

  group('FakeBatchHashService rename restriction', () {
    late FakeBatchHashService fake;

    setUp(() {
      fake = FakeBatchHashService();
    });

    test('forwards only the ready plan rows', () async {
      final List<BatchEntry> entries = fake.sampleHashedEntries;
      final RenamePlanModel plan = await fake.planRenames(
        entries: entries,
        algorithm: 'blake3',
        length: 8,
      );
      expect(plan.rows, isNotEmpty);
      // The demo plan deliberately contains blocked rows, so the subset is
      // smaller than the full row list.
      expect(plan.readyCount, lessThan(plan.rows.length));

      final List<String> readyPaths = <String>[
        for (final RenamePlanRowModel row in plan.rows)
          if (row.status == RenameRowStatus.ready)
            rootRelativePath(FakeBatchHashService.rootPath, row.path),
      ];
      expect(readyPaths, hasLength(plan.readyCount));

      final RenameExecuteResult result = await fake.executeRenames(plan);
      expect(result.applied, plan.readyCount);
      expect(fake.lastRestrictPaths, readyPaths);
    });

    test('never forwards a blocked row', () async {
      final List<BatchEntry> entries = fake.sampleHashedEntries;
      final RenamePlanModel plan = await fake.planRenames(
        entries: entries,
        algorithm: 'blake3',
        length: 8,
      );
      final Set<String> blocked = <String>{
        for (final RenamePlanRowModel row in plan.rows)
          if (row.status != RenameRowStatus.ready)
            rootRelativePath(FakeBatchHashService.rootPath, row.path),
      };
      await fake.executeRenames(plan);
      for (final String path in fake.lastRestrictPaths) {
        expect(blocked.contains(path), isFalse, reason: 'forwarded $path');
      }
    });

    test('accepts an explicit restriction that overrides the plan', () async {
      const RenamePlanModel plan = RenamePlanModel(
        algorithm: 'blake3',
        length: 8,
        uppercase: true,
        rows: <RenamePlanRowModel>[
          RenamePlanRowModel(
            path: '/home/user/Documents/a.txt',
            status: RenameRowStatus.ready,
          ),
          RenamePlanRowModel(
            path: '/home/user/Documents/b.txt',
            status: RenameRowStatus.ready,
          ),
        ],
      );
      await fake.executeRenames(
        plan,
        restrictPaths: <String>['only-this.txt'],
      );
      expect(fake.lastRestrictPaths, <String>['only-this.txt']);
    });

    test('refuses a plan with nothing ready instead of running wide', () async {
      const RenamePlanModel plan = RenamePlanModel(
        algorithm: 'blake3',
        length: 8,
        uppercase: true,
        rows: <RenamePlanRowModel>[
          RenamePlanRowModel(
            path: '/home/user/Documents/a.txt',
            status: RenameRowStatus.conflict,
          ),
        ],
      );
      await expectLater(
        fake.executeRenames(plan),
        throwsA(isA<StateError>()),
      );
      expect(fake.lastRestrictPaths, isEmpty);
    });
  });

  // Everything below needs the native library, exactly like the existing
  // service test: CI sets BATCHHASH_FFI_LIB and a plain checkout skips.
  // The service reaches path_provider for its config store; without a binding
  // the channel call would throw an Error instead of MissingPluginException.
  TestWidgetsFlutterBinding.ensureInitialized();

  final String? libraryPath = Platform.environment['BATCHHASH_FFI_LIB'];
  final bool available = libraryPath != null && libraryPath.isNotEmpty;

  group('RustBatchHashService rename restriction', () {
    late Directory temp;
    late String root;
    late RustBatchHashService service;

    setUp(() {
      temp = Directory.systemTemp.createTempSync('batchhash_restrict');
      root = temp.path;
      service = RustBatchHashService(
        native: BatchHashNative.open(libraryPath!),
        logDbPath: '$root/log/rename.db',
      );
    });

    tearDown(() {
      if (temp.existsSync()) {
        temp.deleteSync(recursive: true);
      }
    });

    test('renames only the files the plan previewed', () async {
      for (final String name in <String>['a.txt', 'b.txt', 'c.txt']) {
        File('$root/$name').writeAsStringSync(name);
      }
      final List<BatchEntry> scanned = await service.scanDirectory(root);
      final List<HashProgress> progress = await service
          .computeHashes(entries: scanned, algorithms: <String>['md5'])
          .toList();
      final List<BatchEntry> hashed = progress.single.entries;

      final RenamePlanModel plan = await service.planRenames(
        entries: hashed,
        algorithm: 'md5',
        length: 8,
      );
      expect(plan.rows, hasLength(3));

      // Hand the service a plan covering one file only, the way a user who
      // unticks rows would. a.txt must be renamed and the others untouched.
      final RenamePlanRowModel onlyA = plan.rows.firstWhere(
        (RenamePlanRowModel row) => row.path.endsWith('a.txt'),
      );
      final RenameExecuteResult result = await service.executeRenames(
        RenamePlanModel(
          algorithm: plan.algorithm,
          length: plan.length,
          uppercase: plan.uppercase,
          rows: <RenamePlanRowModel>[onlyA],
        ),
      );
      expect(result.applied, 1);

      final List<String> after = Directory(root)
          .listSync()
          .whereType<File>()
          .map((File file) => file.uri.pathSegments.last)
          .toList();
      final List<String> renamed = after
          .where((String name) => name.startsWith('a['))
          .toList();
      expect(renamed, hasLength(1));
      expect(after.contains('b.txt'), isTrue);
      expect(after.contains('c.txt'), isTrue);
    });

    test('refuses an empty restriction instead of renaming everything', () async {
      File('$root/a.txt').writeAsStringSync('alpha');
      final List<BatchEntry> scanned = await service.scanDirectory(root);
      final List<HashProgress> progress = await service
          .computeHashes(entries: scanned, algorithms: <String>['md5'])
          .toList();
      final RenamePlanModel plan = await service.planRenames(
        entries: progress.single.entries,
        algorithm: 'md5',
        length: 8,
      );
      final RenamePlanModel empty = RenamePlanModel(
        algorithm: plan.algorithm,
        length: plan.length,
        uppercase: plan.uppercase,
        rows: const <RenamePlanRowModel>[],
      );
      await expectLater(
        service.executeRenames(empty),
        throwsA(isA<StateError>()),
      );
    });
  }, skip: available ? false : 'BATCHHASH_FFI_LIB is not set');
}
