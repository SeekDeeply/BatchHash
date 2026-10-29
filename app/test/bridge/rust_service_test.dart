import 'dart:io';

import 'package:batchhash/src/bridge/native_bridge.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/rust_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// Service level test for the Rust backed implementation.
///
/// CI builds the native library and exports BATCHHASH_FFI_LIB; without it the
/// whole group is skipped so a plain checkout still passes.
void main() {
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
      expect(progress, hasLength(1));
      expect(progress.single.done, isTrue);
      final hashed = progress.single.entries;
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
        entries: progress.single.entries,
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
        entries: progress.single.entries,
      );
      final written = await service.exportReport(
        report: report,
        formats: <ReportFormat>{ReportFormat.json},
        directory: '$root/out',
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
