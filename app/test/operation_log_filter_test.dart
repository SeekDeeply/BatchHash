import 'package:batchhash/src/models/operation_log.dart';
import 'package:batchhash/src/models/operation_log_filter.dart';
import 'package:flutter_test/flutter_test.dart';

OperationLogEntry _entry({
  required String kind,
  required DateTime at,
  bool ok = true,
  String? detail,
  List<HashDigestRecord> hashes = const <HashDigestRecord>[],
}) {
  return OperationLogEntry(
    kind: kind,
    status: ok,
    startedAt: at,
    detail: detail,
    hashes: hashes,
  );
}

List<String> _kinds(List<OperationLogEntry> entries) {
  return <String>[for (final OperationLogEntry entry in entries) entry.kind];
}

void main() {
  final DateTime t0 = DateTime(2026, 1, 1, 10);
  final DateTime t1 = DateTime(2026, 1, 1, 11);
  final DateTime t2 = DateTime(2026, 1, 1, 12);

  final OperationLogEntry scan = _entry(
    kind: 'op.scan',
    at: t0,
    detail: '/root/demo',
  );
  final OperationLogEntry hashOk = _entry(
    kind: 'op.hash',
    at: t1,
    hashes: const <HashDigestRecord>[
      HashDigestRecord(
        path: '/root/a.bin',
        algorithm: 'sha256',
        digest: 'AA11BB',
      ),
    ],
  );
  final OperationLogEntry hashFailed = _entry(
    kind: 'op.hash',
    at: t2,
    ok: false,
    detail: 'permission DENIED',
  );

  test('keeps every entry and sorts newest first by default', () {
    final List<OperationLogEntry> result = filterLogEntries(
      <OperationLogEntry>[scan, hashOk, hashFailed],
    );
    expect(_kinds(result), <String>['op.hash', 'op.hash', 'op.scan']);
    expect(result.first, same(hashFailed));
    expect(result.last, same(scan));
  });

  test('newestFirst false sorts oldest first', () {
    final List<OperationLogEntry> result = filterLogEntries(
      <OperationLogEntry>[hashFailed, scan, hashOk],
      newestFirst: false,
    );
    expect(result.first, same(scan));
    expect(result.last, same(hashFailed));
  });

  test('equal timestamps keep their original order', () {
    final List<OperationLogEntry> entries = <OperationLogEntry>[
      _entry(kind: 'op.a', at: t1),
      _entry(kind: 'op.b', at: t1),
      _entry(kind: 'op.c', at: t1),
    ];
    expect(_kinds(filterLogEntries(entries)), <String>['op.a', 'op.b', 'op.c']);
    expect(
      _kinds(filterLogEntries(entries, newestFirst: false)),
      <String>['op.a', 'op.b', 'op.c'],
    );
  });

  test('query matches the kind key case-insensitively', () {
    expect(
      _kinds(filterLogEntries(<OperationLogEntry>[scan, hashOk], query: 'SCAN')),
      <String>['op.scan'],
    );
  });

  test('query matches detail case-insensitively', () {
    expect(
      _kinds(
        filterLogEntries(<OperationLogEntry>[scan, hashFailed], query: 'denied'),
      ),
      <String>['op.hash'],
    );
  });

  test('query matches digest paths and digests', () {
    expect(
      _kinds(
        filterLogEntries(<OperationLogEntry>[hashOk], query: '/root/a.bin'),
      ),
      <String>['op.hash'],
    );
    expect(
      _kinds(filterLogEntries(<OperationLogEntry>[hashOk], query: 'aa11')),
      <String>['op.hash'],
    );
  });

  test('query is trimmed and an empty query keeps every entry', () {
    expect(
      filterLogEntries(
        <OperationLogEntry>[scan, hashOk, hashFailed],
        query: '   ',
      ),
      hasLength(3),
    );
    expect(
      _kinds(
        filterLogEntries(<OperationLogEntry>[scan, hashOk], query: '  op.scan '),
      ),
      <String>['op.scan'],
    );
  });

  test('kind filter keeps only the requested kind', () {
    final List<OperationLogEntry> result = filterLogEntries(
      <OperationLogEntry>[scan, hashOk, hashFailed],
      kind: 'op.hash',
    );
    expect(result, hasLength(2));
    expect(result.every((OperationLogEntry e) => e.kind == 'op.hash'), isTrue);
  });

  test('status filter separates successes from failures', () {
    expect(
      _kinds(
        filterLogEntries(
          <OperationLogEntry>[scan, hashOk, hashFailed],
          ok: false,
        ),
      ),
      <String>['op.hash'],
    );
    expect(
      filterLogEntries(
        <OperationLogEntry>[scan, hashOk, hashFailed],
        ok: true,
      ),
      hasLength(2),
    );
  });

  test('filters combine', () {
    expect(
      filterLogEntries(
        <OperationLogEntry>[scan, hashOk, hashFailed],
        kind: 'op.hash',
        ok: true,
      ),
      <OperationLogEntry>[hashOk],
    );
  });

  test('kindLabel lets a localized label match', () {
    final List<OperationLogEntry> result = filterLogEntries(
      <OperationLogEntry>[scan, hashOk],
      query: '扫描',
      kindLabel: (String kind) => kind == 'op.scan' ? '扫描' : kind,
    );
    expect(_kinds(result), <String>['op.scan']);
  });

  test('does not mutate the input list', () {
    final List<OperationLogEntry> input = <OperationLogEntry>[scan, hashOk];
    filterLogEntries(input, newestFirst: false);
    expect(input, hasLength(2));
    expect(input.first, same(scan));
    expect(input.last, same(hashOk));
  });
}
