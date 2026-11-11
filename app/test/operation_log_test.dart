import 'dart:io';

import 'package:batchhash/src/models/models.dart';
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
    // The fake refuses to scan anything except its demo root, which is the
    // simplest way to provoke a failure through the real service surface.
    await expectLater(
      service.scanDirectory('${temp.path}/not-the-demo-root'),
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

  test('computeHashes records the digests it produced', () async {
    final FakeBatchHashService fake = FakeBatchHashService();
    final LoggingBatchHashService service = LoggingBatchHashService(
      inner: fake,
      store: store,
    );

    // The demo service reports the files it knows about; any entry with digests
    // must end up in the log, which is the whole point of the feature.
    final List<BatchEntry> entries = await fake.scanDirectory(FakeBatchHashService.rootPath);
    await service
        .computeHashes(
          entries: entries,
          algorithms: const <String>['sha256'],
        )
        .drain<void>();

    final List<OperationLogEntry> logged = await store.load();
    final OperationLogEntry hashEntry = logged.firstWhere(
      (OperationLogEntry e) => e.kind == 'op.hash',
    );
    expect(hashEntry.hashes, isNotEmpty);
    expect(
      hashEntry.hashes.every((HashDigestRecord r) => r.digest.isNotEmpty),
      isTrue,
    );
    expect(hashEntry.hashes.first.algorithm, 'sha256');
  });

  test('hashText records the digests but never the text', () async {
    final LoggingBatchHashService service = LoggingBatchHashService(
      inner: FakeBatchHashService(),
      store: store,
    );

    await service.hashText(
      text: 'secret-ish payload',
      algorithms: const <String>['sha256', 'blake3'],
    );

    final List<OperationLogEntry> logged = await store.load();
    final OperationLogEntry entry = logged.firstWhere(
      (OperationLogEntry e) => e.kind == 'op.hashText',
    );
    expect(entry.hashes, hasLength(2));
    // The pasted text is deliberately not persisted: history may be shared, and
    // the digest is the verifiable part.
    expect(
      entry.hashes.any(
        (HashDigestRecord r) => r.path.contains('secret-ish'),
      ),
      isFalse,
    );
  });

  test('collectDigests caps the sample and counts the remainder', () {
    final List<BatchEntry> entries = <BatchEntry>[
      for (var i = 0; i < 5; i++)
        BatchEntry(
          path: '/root/f$i',
          size: 1,
          mtime: null,
          hashes: const <String, String>{'sha256': 'aa'},
          status: EntryStatus.ok,
        ),
    ];

    final ({List<HashDigestRecord> records, int truncated}) capped =
        collectDigests(entries, limit: 2);
    expect(capped.records, hasLength(2));
    expect(capped.truncated, 3);
  });
}
