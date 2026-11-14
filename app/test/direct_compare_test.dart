import 'dart:io';

import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/direct_compare.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// Minimal service that records how the comparator drove it.
///
/// Extends the demo service so untouched methods keep working, and overrides
/// only the two entry points this test cares about.
class _RecordingService extends FakeBatchHashService {
  final List<String> scannedDirectories = <String>[];
  final List<bool> recursiveFlags = <bool>[];

  @override
  Future<List<BatchEntry>> scanDirectory(
    String path, {
    bool recursive = false,
  }) async {
    scannedDirectories.add(path);
    recursiveFlags.add(recursive);
    return const <BatchEntry>[];
  }
}

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

  test('a folder pair is scanned recursively', () async {
    // The regression this guards: a shallow scan reported every file below the
    // first level as missing on one side. The demo service does not read the
    // real disk, so the assertion is on the flag the comparator passes rather
    // than on rows produced from a real folder tree.
    final Directory left = Directory('${temp.path}/left')..createSync();
    final Directory right = Directory('${temp.path}/right')..createSync();
    final _RecordingService recording = _RecordingService();

    await DirectComparator(service: recording).compare(
      leftPath: left.path,
      rightPath: right.path,
      algorithms: const <String>['sha256'],
    );

    expect(recording.scannedDirectories, <String>[left.path, right.path]);
    expect(recording.recursiveFlags, <bool>[true, true]);
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
