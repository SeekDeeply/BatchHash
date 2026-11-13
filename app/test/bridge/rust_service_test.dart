import 'dart:io';

import 'package:batchhash/src/bridge/native_bridge.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/app_config_store.dart';
import 'package:batchhash/src/services/rust_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// Service level test for the Rust backed implementation.
///
/// CI builds the native library and exports BATCHHASH_FFI_LIB; without it the
/// whole group is skipped so a plain checkout still passes.
void main() {
  // The service reaches path_provider for its config store. Without a binding
  // that call throws an Error instead of returning a directory.
  TestWidgetsFlutterBinding.ensureInitialized();

  final libraryPath = Platform.environment['BATCHHASH_FFI_LIB'];
  final available = libraryPath != null && libraryPath.isNotEmpty;

  group('RustBatchHashService', () {
    late Directory temp;
    late String root;
    late RustBatchHashService service;

    setUp(() {
      temp = Directory.systemTemp.createTempSync('batchhash_service');
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

    test('degrades to no history when the config store has no platform channel', () async {
      // In a plain `flutter test` there is no path_provider implementation, and
      // calling it without a binding throws an Error rather than an Exception.
      // Remembering roots is a convenience, so the scan itself must still work.
      File('$root/a.txt').writeAsStringSync('alpha');
      final bare = RustBatchHashService(
        native: BatchHashNative.open(libraryPath!),
        logDbPath: '$root/log/rename.db',
        configStore: FileAppConfigStore(
          supportDirectory: () async =>
              throw StateError('no support directory on this host'),
        ),
      );
      final scanned = await bare.scanDirectory(root, recursive: true);
      expect(scanned, hasLength(1));
      expect(await bare.loadHistory(), isEmpty);
    });

    test('hashes every explicitly picked file across directories', () async {
      // A native picker returns absolute paths that may live in different
      // folders. Hashing must cover the whole selection instead of silently
      // re-scanning only the first file's folder.
      Directory('$root/one').createSync();
      Directory('$root/two').createSync();
      File('$root/one/a.txt').writeAsStringSync('alpha');
      File('$root/two/b.txt').writeAsStringSync('bravo');
      File('$root/two/unpicked.txt').writeAsStringSync('charlie');

      final picked = await service.scanFiles(<String>[
        '$root/one/a.txt',
        '$root/two/b.txt',
      ]);
      expect(picked, hasLength(2));

      final progress = await service
          .computeHashes(entries: picked, algorithms: <String>['sha256', 'xxh3-64'])
          .toList();
      // The engine streams progress samples; the last one carries the result.
      final hashed = progress.last.entries;
      expect(hashed, hasLength(2));
      for (final entry in hashed) {
        expect(entry.hashes['sha256'], hasLength(64));
        expect(entry.hashes['xxh3-64'], hasLength(16));
      }
      // The file the user did not pick must never appear in the result.
      expect(hashed.map((entry) => entry.path), isNot(contains('unpicked.txt')));
    });

    test('every advertised algorithm is accepted by the engine',
        () async {
      // The xxh3 bug was a name mismatch: the UI sent 'xxh3' while the engine
      // only knows 'xxh3-64', so the whole request failed as invalid_request.
      // Hashing one file with each advertised id pins the contract.
      File('$root/probe.txt').writeAsStringSync('batchhash');
      final scanned = await service.scanFiles(<String>['$root/probe.txt']);
      final algorithms = await service.listAlgorithms();
      expect(algorithms, hasLength(44));
      for (final AlgorithmInfo algorithm in algorithms) {
        final progress = await service
            .computeHashes(
              entries: scanned,
              algorithms: <String>[algorithm.id],
            )
            .toList();
        final entry = progress.last.entries.single;
        expect(
          entry.hashes[algorithm.id],
          isNotNull,
          reason: 'algorithm ${algorithm.id} was rejected by the engine',
        );
        expect(
          entry.hashes[algorithm.id],
          hasLength(algorithm.hexLength),
          reason: '${algorithm.id} produced the wrong digest length',
        );
      }
    });

    test('the engine reports the full catalogue with categories', () async {
      // The picker is built from this call, so a broken answer here silently
      // empties the UI. Every entry has to be usable and carry a known group.
      final algorithms = await service.listAlgorithms();
      expect(algorithms, hasLength(44));
      expect(
        algorithms.map((algorithm) => algorithm.id).toSet(),
        hasLength(44),
        reason: 'algorithm ids must be unique',
      );
      for (final AlgorithmInfo algorithm in algorithms) {
        expect(algorithm.id, isNotEmpty);
        expect(algorithm.displayName, isNotEmpty);
        expect(algorithm.hexLength, algorithm.digestLength * 2);
      }
      expect(
        algorithms.map((algorithm) => algorithm.category).toSet().length,
        greaterThanOrEqualTo(3),
        reason: 'categories must actually partition the catalogue',
      );
      // The classification the UI groups by, pinned for the four best known.
      final byId = <String, AlgorithmInfo>{
        for (final AlgorithmInfo algorithm in algorithms)
          algorithm.id: algorithm,
      };
      expect(byId['sha256']!.category, AlgorithmCategory.crypto);
      expect(byId['blake3']!.category, AlgorithmCategory.crypto);
      expect(byId['crc32']!.category, AlgorithmCategory.checksum);
      expect(byId['xxh3-64']!.category, AlgorithmCategory.fast);
      expect(byId['md5']!.category, AlgorithmCategory.legacy);
      expect(byId['md5']!.cryptographic, isFalse);
      expect(byId['md5']!.suffixWarning, isNotNull);
      expect(byId['blake3']!.defaultSuffixAlgorithm, isTrue);
      // Grouping keeps registry order inside a group and drops empty groups.
      final grouped = groupAlgorithmsByCategory(algorithms);
      expect(grouped.keys.toList(), isNotEmpty);
      expect(
        grouped.values.expand((group) => group).length,
        algorithms.length,
      );
    });
    test('tryCreate returns a service when the library loads', () {
      final created = RustBatchHashService.tryCreate(overridePath: libraryPath);
      expect(created, isNotNull);
    });

    test('tryCreate returns null for a missing library', () {
      final created = RustBatchHashService.tryCreate(
        overridePath: '/nonexistent/libbatchhash_ffi.so',
      );
      expect(created, isNull);
    });

    test('probes hardware without guessing backends', () async {
      final hardware = await service.probeHardware();
      expect(hardware.arch, isNotEmpty);
      expect(hardware.backendPolicy, 'auto');
      expect(hardware.blake3BackendReported, isNotEmpty);
    });

    test('scans, hashes, plans, renames and undoes end to end', () async {
      File('$root/a.txt').writeAsStringSync('alpha');
      Directory('$root/nested').createSync();
      File('$root/nested/b.txt').writeAsStringSync('bravo');

      final scanned = await service.scanDirectory(root, recursive: true);
      expect(scanned, hasLength(2));
      expect(scanned.every((entry) => entry.hashes.isEmpty), isTrue);

      final progress = await service
          .computeHashes(entries: scanned, algorithms: <String>['md5'])
          .toList();
      // Progress is a stream: every sample except the last is intermediate.
      expect(progress, isNotEmpty);
      expect(progress.last.done, isTrue);
      final hashed = progress.last.entries;
      expect(hashed, hasLength(2));
      for (final entry in hashed) {
        expect(entry.hashes['md5'], isNotNull);
        expect(entry.hashes['md5']!.length, 32);
      }

      final plan = await service.planRenames(
        entries: hashed,
        algorithm: 'md5',
        length: 8,
      );
      expect(plan.rows, hasLength(2));
      expect(plan.readyCount, 2);
      expect(plan.rows.first.targetPath, contains('['));

      final executed = await service.executeRenames(plan);
      expect(executed.applied, 2);
      expect(executed.batchId, isNotNull);

      final history = await service.loadHistory();
      expect(history, hasLength(1));
      expect(history.single.appliedCount, 2);

      final undone = await service.undoBatch(executed.batchId!);
      expect(undone, isTrue);
      expect(File('$root/a.txt').existsSync(), isTrue);
    });

    test('exports every requested format', () async {
      File('$root/a.txt').writeAsStringSync('alpha');
      final entries = await service.scanDirectory(root);
      final progress = await service
          .computeHashes(entries: entries, algorithms: <String>['sha256'])
          .toList();
      final report = ReportModel(
        root: root,
        generatedAt: DateTime.utc(2026, 1, 1),
        algorithms: const <String>['sha256'],
        entries: progress.last.entries,
      );

      final written = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.json, ReportFormat.csv},
        directory: '$root/out',
      );
      expect(written, hasLength(2));
      for (final exported in written) {
        expect(File(exported.path).existsSync(), isTrue);
      }
      final csv = File('$root/out/batchhash-report.csv').readAsStringSync();
      expect(csv, startsWith('path,size,mtime,status'));
    });

    test('exports a localized tree, machine formats stay fixed',
        () async {
      File('$root/a.txt').writeAsStringSync('alpha');
      final entries = await service.scanDirectory(root);
      final progress = await service
          .computeHashes(entries: entries, algorithms: <String>['sha256'])
          .toList();
      final report = ReportModel(
        root: root,
        generatedAt: DateTime.utc(2026, 1, 1),
        algorithms: const <String>['sha256'],
        entries: progress.last.entries,
      );

      // The human readable formats follow the requested language.
      final zh = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.tree, ReportFormat.markdown},
        directory: '$root/out-zh',
        locale: 'zh-CN',
      );
      expect(zh, hasLength(2));
      final tree = zh
          .firstWhere((item) => item.format == ReportFormat.tree)
          .path;
      expect(File(tree).existsSync(), isTrue);
      expect(tree, endsWith('.txt'));
      final treeText = File(tree).readAsStringSync();
      expect(treeText, contains('a.txt'));
      expect(treeText, contains('sha256:'));
      expect(treeText, contains('汇总'));

      // The same report in English renders different labels.
      final en = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.tree},
        directory: '$root/out-en',
        locale: 'en',
      );
      final enText = File(en.single.path).readAsStringSync();
      expect(enText, isNot(equals(treeText)));

      // JSON carries no localized text, so the locale must not change it.
      final jsonZh = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.json},
        directory: '$root/out-json-zh',
        locale: 'zh',
      );
      final jsonEn = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.json},
        directory: '$root/out-json-en',
        locale: 'en',
      );
      expect(
        File(jsonZh.single.path).readAsStringSync(),
        File(jsonEn.single.path).readAsStringSync(),
      );
    });

    test('imports a report and diffs it against the directory', () async {
      File('$root/a.txt').writeAsStringSync('alpha');
      final entries = await service.scanDirectory(root);
      final progress = await service
          .computeHashes(entries: entries, algorithms: <String>['md5'])
          .toList();
      final report = ReportModel(
        root: root,
        generatedAt: DateTime.utc(2026, 1, 1),
        algorithms: const <String>['md5'],
        entries: progress.last.entries,
      );
      // Export outside the scanned root so the export itself cannot show up as
      // an added file in the directory comparison below.
      final out = Directory.systemTemp.createTempSync('batchhash_report');
      addTearDown(() {
        if (out.existsSync()) {
          out.deleteSync(recursive: true);
        }
      });
      final written = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.json},
        directory: out.path,
      );
      final imported = await service.importReport(written.single.path);
      expect(imported.entries, hasLength(1));
      expect(imported.entries.first.hashes['md5'], isNotNull);

      // Identical directory: everything unchanged.
      var rows = await service.diffReportAgainstDirectory(
        report: imported,
        directory: root,
      );
      expect(rows.where((row) => row.status != DiffStatus.unchanged), isEmpty);

      // Changing the file content is detected.
      File('$root/a.txt').writeAsStringSync('alpha-changed');
      rows = await service.diffReportAgainstDirectory(
        report: imported,
        directory: root,
      );
      expect(rows.any((row) => row.status == DiffStatus.changed), isTrue);

      // Comparing the report with itself is unchanged.
      final self = await service.diffReports(left: imported, right: imported);
      expect(self.every((row) => row.status == DiffStatus.unchanged), isTrue);
    });
  }, skip: available ? false : 'BATCHHASH_FFI_LIB is not set');
}
