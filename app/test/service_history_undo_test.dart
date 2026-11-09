import 'dart:io';

import 'package:batchhash/src/bridge/native_bridge.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/app_config_store.dart';
import 'package:batchhash/src/services/rust_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// History must stay reachable after a restart, and an undo must invalidate
/// every cached artifact that described the pre-undo tree.
void main() {
  // The service reaches path_provider for its config store; without a binding
  // the channel call would throw an Error instead of MissingPluginException.
  TestWidgetsFlutterBinding.ensureInitialized();

  // Non-nullable so no test body needs a `!`: the groups below are skipped
  // entirely when the variable is unset, so the empty value is never opened.
  final String libraryPath = Platform.environment['BATCHHASH_FFI_LIB'] ?? '';
  final bool available = libraryPath.isNotEmpty;

  group('RustBatchHashService history and undo', () {
    late Directory temp;
    late String root;
    late Directory support;
    late String scannedRoot;

    setUp(() {
      temp = Directory.systemTemp.createTempSync('batchhash_history');
      root = temp.path;
      scannedRoot = '$root/files';
      Directory(scannedRoot).createSync();
      support = Directory('${temp.path}/support')..createSync();
    });

    tearDown(() {
      if (temp.existsSync()) {
        temp.deleteSync(recursive: true);
      }
    });

    RustBatchHashService build({AppConfigStore? store, String? logDb}) {
      return RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        logDbPath: logDb ?? '$scannedRoot/.batchhash-rename.db',
        configStore:
            store ??
            FileAppConfigStore(supportDirectory: () async => support),
      );
    }

    Future<RenameExecuteResult> applyOneBatch(
      RustBatchHashService service, {
      required List<String> names,
    }) async {
      for (final String name in names) {
        File('$scannedRoot/$name').writeAsStringSync(name);
      }
      final List<BatchEntry> scanned = await service.scanDirectory(scannedRoot);
      final List<HashProgress> progress = await service
          .computeHashes(entries: scanned, algorithms: <String>['md5'])
          .toList();
      final RenamePlanModel plan = await service.planRenames(
        entries: progress.single.entries,
        algorithm: 'md5',
        length: 8,
      );
      return service.executeRenames(plan);
    }

    test('records the scanned root so a fresh launch can find it', () async {
      final InMemoryAppConfigStore store = InMemoryAppConfigStore();
      final RustBatchHashService service = build(store: store);
      await service.scanDirectory(scannedRoot, recursive: true);

      expect(store.roots, hasLength(1));
      expect(store.roots.single.root, scannedRoot);
      expect(
        store.roots.single.resolvedLogDb,
        '$scannedRoot/.batchhash-rename.db',
      );
    });

    test('loads history from a remembered root after a restart', () async {
      final FileAppConfigStore store = FileAppConfigStore(
        supportDirectory: () async => support,
      );
      final RustBatchHashService first = build(store: store);
      final RenameExecuteResult executed = await applyOneBatch(
        first,
        names: <String>['a.txt', 'b.txt'],
      );
      expect(executed.batchId, isNotNull);

      // A brand new service with no in-memory root is what a relaunch gets.
      final RustBatchHashService relaunched = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        configStore: store,
      );
      final List<HistoryBatchModel> history = await relaunched.loadHistory();
      expect(history, hasLength(1));
      expect(history.single.id, executed.batchId);
      expect(history.single.appliedCount, greaterThan(0));
    });

    test('merges history across every remembered root', () async {
      final Directory otherRoot = Directory('${temp.path}/other')..createSync();
      final FileAppConfigStore store = FileAppConfigStore(
        supportDirectory: () async => support,
      );
      final RustBatchHashService first = build(store: store);
      await applyOneBatch(first, names: <String>['a.txt']);

      final RustBatchHashService second = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        logDbPath: '${otherRoot.path}/.batchhash-rename.db',
        configStore: store,
      );
      File('${otherRoot.path}/c.txt').writeAsStringSync('charlie');
      final List<BatchEntry> scanned = await second.scanDirectory(
        otherRoot.path,
      );
      final List<HashProgress> progress = await second
          .computeHashes(entries: scanned, algorithms: <String>['md5'])
          .toList();
      final RenamePlanModel plan = await second.planRenames(
        entries: progress.single.entries,
        algorithm: 'md5',
        length: 8,
      );
      await second.executeRenames(plan);

      final RustBatchHashService relaunched = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        configStore: store,
      );
      final List<HistoryBatchModel> history = await relaunched.loadHistory();
      expect(history, hasLength(2));
      // Newest first.
      expect(
        history.first.createdAt.isBefore(history.last.createdAt),
        isFalse,
      );
      expect(
        history.map((HistoryBatchModel batch) => batch.id).toSet(),
        hasLength(2),
      );
    });

    test('ignores a remembered root whose log is gone', () async {
      final InMemoryAppConfigStore store = InMemoryAppConfigStore(
        initial: <RecentRoot>[
          RecentRoot(
            root: '${temp.path}/deleted',
            lastUsed: DateTime.utc(2024, 1, 1),
          ),
          RecentRoot(
            root: scannedRoot,
            lastUsed: DateTime.utc(2024, 1, 1),
          ),
        ],
      );
      final RustBatchHashService first = build(store: store);
      await applyOneBatch(first, names: <String>['a.txt']);

      final RustBatchHashService relaunched = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        configStore: store,
      );
      final List<HistoryBatchModel> history = await relaunched.loadHistory();
      // The missing log contributes nothing but must not hide the real batch.
      expect(history, hasLength(1));
    });

    test('undo drops the cached report', () async {
      final InMemoryAppConfigStore store = InMemoryAppConfigStore();
      final RustBatchHashService service = build(store: store);
      final RenameExecuteResult executed = await applyOneBatch(
        service,
        names: <String>['a.txt'],
      );
      expect(service.cachedReportJson, isNotNull);

      final bool undone = await service.undoBatch(executed.batchId!);
      expect(undone, isTrue);
      // Serializing the pre-undo tree now would describe files that no longer
      // exist under those names.
      expect(service.cachedReportJson, isNull);
    });

    test('undo finds the log of a batch discovered through history', () async {
      final FileAppConfigStore store = FileAppConfigStore(
        supportDirectory: () async => support,
      );
      final RustBatchHashService first = build(store: store);
      final RenameExecuteResult executed = await applyOneBatch(
        first,
        names: <String>['a.txt'],
      );
      expect(executed.batchId, isNotNull);

      // The user relaunches, opens history, and undoes from there: this service
      // was never given a log path, so the remembered root must supply it.
      final RustBatchHashService relaunched = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        configStore: store,
      );
      final bool undone = await relaunched.undoBatch(executed.batchId!);
      expect(undone, isTrue);
      expect(File('$scannedRoot/a.txt').existsSync(), isTrue);
    });

    test('a failed undo leaves the cached report alone', () async {
      final InMemoryAppConfigStore store = InMemoryAppConfigStore();
      final RustBatchHashService service = build(store: store);
      final RenameExecuteResult executed = await applyOneBatch(
        service,
        names: <String>['a.txt'],
      );
      expect(service.cachedReportJson, isNotNull);

      // The core rejects an unknown batch id; that must not clear the cache of
      // a tree that was in fact left untouched.
      await expectLater(
        service.undoBatch('batch-does-not-exist'),
        throwsA(isA<BridgeException>()),
      );
      expect(service.cachedReportJson, isNotNull);
      expect(executed.batchId, isNotNull);
    });
  }, skip: available ? false : 'BATCHHASH_FFI_LIB is not set');

  group('RustBatchHashService export directory', () {
    late Directory temp;
    late String root;
    late Directory support;

    setUp(() {
      temp = Directory.systemTemp.createTempSync('batchhash_export');
      root = temp.path;
      support = Directory('${temp.path}/support')..createSync();
      File('$root/a.txt').writeAsStringSync('alpha');
    });

    tearDown(() {
      if (temp.existsSync()) {
        temp.deleteSync(recursive: true);
      }
    });

    /// A report the exporter accepts, built from a real file.
    Future<ReportModel> realReport(RustBatchHashService service) async {
      final List<BatchEntry> scanned = await service.scanDirectory(root);
      final List<HashProgress> progress = await service
          .computeHashes(entries: scanned, algorithms: <String>['md5'])
          .toList();
      return ReportModel(
        root: root,
        generatedAt: DateTime.utc(2024, 1, 1),
        algorithms: const <String>['md5'],
        entries: progress.single.entries,
      );
    }

    test('defaults to the support directory, never the temp directory', () async {
      final RustBatchHashService service = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        supportDirectory: () async => support,
      );
      final List<ExportedReport> written = await service.exportReport(
        report: await realReport(service),
        formats: <ReportFormat>{ReportFormat.json},
      );
      expect(written, hasLength(1));
      // The old behaviour dropped the file in a random system temp directory
      // the OS may clear; it must live under the app's exports folder now.
      expect(
        written.single.path.startsWith('${support.path}/exports/'),
        isTrue,
        reason: written.single.path,
      );
      expect(File(written.single.path).existsSync(), isTrue);
    });

    test('a second export does not overwrite the first', () async {
      final RustBatchHashService service = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        supportDirectory: () async => support,
      );
      final ReportModel report = await realReport(service);
      final List<ExportedReport> first = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.json},
      );
      // Every export gets its own timestamped directory.
      // Long enough that two exports cannot share a millisecond stamp.
      await Future<void>.delayed(const Duration(milliseconds: 100));
      final List<ExportedReport> second = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.json},
      );
      expect(File(first.single.path).existsSync(), isTrue);
      expect(File(second.single.path).existsSync(), isTrue);
      expect(first.single.path == second.single.path, isFalse);
    });

    test('an explicit directory still wins', () async {
      final String explicit = '$root/out';
      final RustBatchHashService service = RustBatchHashService(
        native: BatchHashNative.open(libraryPath),
        supportDirectory: () async => support,
      );
      final List<ExportedReport> written = await service.exportReport(
        report: await realReport(service),
        formats: <ReportFormat>{ReportFormat.csv},
        directory: explicit,
      );
      expect(written.single.path, '$explicit/batchhash-report.csv');
      expect(File(written.single.path).existsSync(), isTrue);
    });
  }, skip: available ? false : 'BATCHHASH_FFI_LIB is not set');
}
