import 'dart:io';

import 'package:batchhash/src/bridge/native_bridge.dart';
import 'package:flutter_test/flutter_test.dart';

/// Integration test for the Rust bridge.
///
/// CI builds the native library first and exports its path in
/// BATCHHASH_FFI_LIB. Without that variable the test is skipped, so a plain
/// checkout still passes (local builds are forbidden in this repository).
void main() {
  final libraryPath = Platform.environment['BATCHHASH_FFI_LIB'];
  final available = libraryPath != null && libraryPath.isNotEmpty;

  group('native bridge', () {
    late BatchHashNative bridge;
    late Directory temp;
    late String root;

    setUp(() {
      bridge = BatchHashNative.open(libraryPath!);
      temp = Directory.systemTemp.createTempSync('batchhash_bridge');
      root = temp.path;
    });

    tearDown(() {
      if (temp.existsSync()) {
        temp.deleteSync(recursive: true);
      }
    });

    test('reports the contract version', () {
      final version = bridge.version();
      expect(version['tool'], 'BatchHash');
      expect(version['contract_version'], 1);
      expect(version['c_abi_version'], 1);
    });

    test('lists the registered algorithms', () {
      final algorithms = bridge.algorithms();
      expect(algorithms, hasLength(16));
      final ids = algorithms.map((item) => item['id']).toList();
      expect(ids, contains('sha256'));
      expect(ids, contains('blake3'));
      final sha256 = algorithms.firstWhere((item) => item['id'] == 'sha256');
      expect(sha256['hex_len'], 64);
    });

    test('scans a directory without hashing', () {
      File('$root/a.txt').writeAsStringSync('alpha');
      Directory('$root/nested').createSync();
      File('$root/nested/b.txt').writeAsStringSync('bravo');

      final scan = bridge.scan(root);
      expect(scan['total_bytes'], 10);
      final entries = (scan['entries'] as List).cast<Map<String, Object?>>();
      expect(
        entries.map((entry) => entry['path']),
        containsAll(<String>['a.txt', 'nested/b.txt']),
      );
      for (final entry in entries) {
        expect(entry['hashes'], isEmpty);
      }
    });

    test('hashes a directory and returns report JSON', () {
      File('$root/a.txt').writeAsStringSync('alpha');

      final run = bridge.runBatch(
        root: root,
        algorithms: <String>['md5', 'sha256'],
      );
      expect(run['ok'], 1);
      expect(run['total'], 1);
      final entries = (run['entries'] as List).cast<Map<String, Object?>>();
      expect(entries, hasLength(1));
      final hashes = (entries.first['hashes'] as List)
          .cast<Map<String, Object?>>();
      expect(
        hashes.map((hash) => hash['algorithm']),
        containsAll(<String>['md5', 'sha256']),
      );
      final md5 = hashes.firstWhere((hash) => hash['algorithm'] == 'md5');
      expect((md5['hex'] as String).length, 32);

      final csv = bridge.exportReport(
        reportJson: run['report_json'] as String,
        format: 'csv',
      );
      expect(csv, startsWith('path,size,mtime,status'));
    });

    test('plans suffixes, applies them, then undoes them', () {
      final file = File('$root/movie.mkv');
      file.writeAsStringSync('movie');

      final dry = bridge.runBatch(
        root: root,
        algorithms: <String>['md5'],
        suffix: <String, Object?>{
          'algorithm': 'md5',
          'length': 8,
          'case': 'upper',
        },
      );
      expect(dry['renamed'], 0);
      expect(file.existsSync(), isTrue, reason: 'a dry run must not rename');
      final summary = dry['plan_summary'] as Map<String, Object?>;
      expect(summary['ready'], 1);

      final logDb = '$root/log/rename.db';
      final applied = bridge.runBatch(
        root: root,
        algorithms: <String>['md5'],
        suffix: <String, Object?>{
          'algorithm': 'md5',
          'length': 8,
          'case': 'upper',
        },
        applyRenames: true,
        logDb: logDb,
      );
      expect(applied['renamed'], 1);
      expect(file.existsSync(), isFalse);
      final batchId = applied['batch_id'] as String;
      expect(batchId, isNotEmpty);

      final undone = bridge.undo(db: logDb, batchId: batchId, apply: true);
      expect(undone['applied'], isTrue);
      expect(undone['undone'], 1);
      expect(file.existsSync(), isTrue, reason: 'undo restores the name');
    });

    test('surfaces native errors as BridgeException', () {
      expect(
        () => bridge.runBatch(root: root, algorithms: <String>[]),
        throwsA(
          isA<BridgeException>().having(
            (error) => error.kind,
            'kind',
            'invalid_request',
          ),
        ),
      );
      expect(
        () => bridge.runBatch(root: root, algorithms: <String>['nope']),
        throwsA(isA<BridgeException>()),
      );
    });

    test('reports hardware capabilities', () {
      final hardware = bridge.probe();
      expect(hardware['arch'], isA<String>());
      expect(hardware['backend_policy'], 'auto');
      expect((hardware['multithread'] as int) >= 1, isTrue);
    });
  }, skip: available ? false : 'BATCHHASH_FFI_LIB is not set');
}
