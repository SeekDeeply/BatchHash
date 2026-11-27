import 'dart:async';
import 'dart:io';

import 'package:batchhash/src/util/atomic_file.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('writeStringAtomic', () {
    late Directory temp;

    setUp(() async {
      temp = await Directory.systemTemp.createTemp('atomic_file_test');
    });

    tearDown(() async {
      if (await temp.exists()) {
        await temp.delete(recursive: true);
      }
    });

    test('creates the file with the given contents', () async {
      final String path = '${temp.path}/a.json';
      await writeStringAtomic(path, 'hello');
      expect(await File(path).readAsString(), 'hello');
    });

    test('overwrites an existing file', () async {
      final String path = '${temp.path}/a.json';
      await File(path).writeAsString('old');
      await writeStringAtomic(path, 'new');
      expect(await File(path).readAsString(), 'new');
    });

    test('leaves no temporary file behind', () async {
      final String path = '${temp.path}/a.json';
      await writeStringAtomic(path, 'x');
      final List<String> names = await temp
          .list()
          .map((FileSystemEntity entity) => entity.path)
          .toList();
      expect(
        names.where((String name) => name.endsWith('.tmp')),
        isEmpty,
        reason: 'a completed write must not leave debris: $names',
      );
    });

    test('creates a missing parent directory', () async {
      final String path = '${temp.path}/nested/deep/a.json';
      await writeStringAtomic(path, 'x');
      expect(await File(path).readAsString(), 'x');
    });
  });

  group('AsyncSerialQueue', () {
    test('a failing action does not poison later actions', () async {
      final AsyncSerialQueue queue = AsyncSerialQueue();
      final List<String> ran = <String>[];

      final Future<void> first = queue.run<void>(() async {
        ran.add('first');
        throw StateError('boom');
      });
      // Attach the expectation before awaiting so the error is observed.
      await expectLater(first, throwsA(isA<StateError>()));

      await queue.run<void>(() async {
        ran.add('second');
      });
      // The second action must still run. A naive `_tail = _tail.then(...)`
      // chain leaves _tail holding the first error and skips everything after.
      expect(ran, <String>['first', 'second']);
    });

    test('actions run in call order', () async {
      final AsyncSerialQueue queue = AsyncSerialQueue();
      final List<int> order = <int>[];
      final List<Future<void>> pending = <Future<void>>[];
      for (int i = 0; i < 5; i += 1) {
        pending.add(
          queue.run<void>(() async {
            // A delay makes an unordered implementation visible.
            await Future<void>.delayed(Duration(milliseconds: 10 - i));
            order.add(i);
          }),
        );
      }
      await Future.wait(pending);
      expect(order, <int>[0, 1, 2, 3, 4]);
    });

    test('idle completes after queued work', () async {
      final AsyncSerialQueue queue = AsyncSerialQueue();
      bool done = false;
      unawaited(queue.run<void>(() async {
        await Future<void>.delayed(const Duration(milliseconds: 10));
        done = true;
      }));
      await queue.idle();
      expect(done, isTrue);
    });
  });
}
