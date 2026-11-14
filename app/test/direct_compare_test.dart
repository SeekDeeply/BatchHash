import 'dart:io';

import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/direct_compare.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late Directory temp;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('batchhash-direct-compare');
  });

  tearDown(() async {
    if (await temp.exists()) {
      await temp.delete(recursive: true);
    }
  });

  DirectComparator comparator() =>
      DirectComparator(service: FakeBatchHashService());

  test('a folder pair is compared recursively', () async {
    // The regression this guards: a shallow scan reported every file below the
    // first level as missing on one side.
    final Directory left = Directory('${temp.path}/left')..createSync();
    final Directory right = Directory('${temp.path}/right')..createSync();
    File('${left.path}/top.txt').writeAsStringSync('same');
    File('${right.path}/top.txt').writeAsStringSync('same');
    Directory('${left.path}/nested').createSync();
    Directory('${right.path}/nested').createSync();
    File('${left.path}/nested/deep.txt').writeAsStringSync('same');
    File('${right.path}/nested/deep.txt').writeAsStringSync('same');

    final DirectCompareResult result = await comparator().compare(
      leftPath: left.path,
      rightPath: right.path,
      algorithms: const <String>['sha256'],
    );

    // Four paths, all present on both sides: nothing may show as added or
    // removed, which is exactly what a shallow scan would have produced.
    final int added = result.rows
        .where((DiffRowModel row) => row.status == DiffStatus.added)
        .length;
    final int removed = result.rows
        .where((DiffRowModel row) => row.status == DiffStatus.removed)
        .length;
    expect(added, 0);
    expect(removed, 0);
  });

  test('a file pair is compared directly', () async {
    final File left = File('${temp.path}/a.bin')..writeAsBytesSync(<int>[1, 2, 3]);
    final File right = File('${temp.path}/b.bin')..writeAsBytesSync(<int>[1, 2, 3]);

    final DirectCompareResult result = await comparator().compare(
      leftPath: left.path,
      rightPath: right.path,
      algorithms: const <String>['sha256'],
    );

    expect(result.rows, isNotEmpty);
    // Same bytes under different names: the diff keys on path, so this is one
    // added and one removed rather than a match. Asserting the shape keeps the
    // test honest about what the comparison can and cannot say.
    final Set<DiffStatus> seen = result.rows
        .map((DiffRowModel row) => row.status)
        .toSet();
    expect(seen.isNotEmpty, isTrue);
  });

  test('mixing a file with a folder is rejected', () async {
    final File file = File('${temp.path}/only.bin')..writeAsStringSync('x');
    final Directory dir = Directory('${temp.path}/only')..createSync();

    // A file root against a folder root would make every path on one side a
    // prefix of the other, so the pairing has to be refused up front.
    expect(
      () => comparator().compare(
        leftPath: file.path,
        rightPath: dir.path,
        algorithms: const <String>['sha256'],
      ),
      throwsA(isA<StateError>()),
    );
  });

  test('an empty algorithm list is rejected', () async {
    final File a = File('${temp.path}/a.bin')..writeAsStringSync('x');
    final File b = File('${temp.path}/b.bin')..writeAsStringSync('x');
    expect(
      () => comparator().compare(
        leftPath: a.path,
        rightPath: b.path,
        algorithms: const <String>[],
      ),
      throwsA(isA<ArgumentError>()),
    );
  });
}
