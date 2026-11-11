import 'dart:io';

import 'package:batchhash/src/models/operation_log.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:batchhash/src/services/logging_batch_hash_service.dart';
import 'package:batchhash/src/services/operation_log_store.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late Directory temp;
  late OperationLogStore store;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('batchhash-log-test');
    store = OperationLogStore(supportDirectory: () async => temp);
  });

  tearDown(() async {
    if (await temp.exists()) {
      await temp.delete(recursive: true);
    }
  });

  test('a successful call is recorded with its duration', () async {
    final LoggingBatchHashService service = LoggingBatchHashService(
      inner: FakeBatchHashService(),
      store: store,
    );
    await service.listAlgorithms();

    final List<OperationLogEntry> entries = await store.load();
    expect(entries, hasLength(1));
    expect(entries.first.kind, 'op.listAlgorithms');
    expect(entries.first.status, isTrue);
    expect(entries.first.durationMs, isNotNull);
  });

  test('a failure is recorded and still rethrown', () async {
    final LoggingBatchHashService service = LoggingBatchHashService(
      inner: FakeBatchHashService(),
      store: store,
    );
    await expectLater(
      service.importReport('${temp.path}/definitely-missing.json'),
      throwsA(anything),
    );

    final List<OperationLogEntry> entries = await store.load();
    expect(entries, hasLength(1));
    expect(entries.first.status, isFalse);
    expect(entries.first.detail, isNotNull);
  });

  test('newest entry comes first', () async {
    final LoggingBatchHashService service = LoggingBatchHashService(
      inner: FakeBatchHashService(),
      store: store,
    );
    await service.listAlgorithms();
    await service.probeHardware();

    final List<OperationLogEntry> entries = await store.load();
    expect(entries, hasLength(2));
    expect(entries.first.kind, 'op.probe');
    expect(entries.last.kind, 'op.listAlgorithms');
  });

  test('clearing removes every record', () async {
    final LoggingBatchHashService service = LoggingBatchHashService(
      inner: FakeBatchHashService(),
      store: store,
    );
    await service.listAlgorithms();
    await store.clear();
    expect(await store.load(), isEmpty);
  });

  test('entries survive a new store instance', () async {
    final LoggingBatchHashService service = LoggingBatchHashService(
      inner: FakeBatchHashService(),
      store: store,
    );
    await service.listAlgorithms();

    final OperationLogStore reopened = OperationLogStore(
      supportDirectory: () async => temp,
    );
    final List<OperationLogEntry> entries = await reopened.load();
    expect(entries, hasLength(1));
    expect(entries.first.kind, 'op.listAlgorithms');
  });

  test('a corrupt log degrades to an empty list', () async {
    final Directory historyDir = Directory('${temp.path}/history');
    await historyDir.create(recursive: true);
    await File('${historyDir.path}/operations.json').writeAsString('{not json');
    expect(await store.load(), isEmpty);
  });
}
