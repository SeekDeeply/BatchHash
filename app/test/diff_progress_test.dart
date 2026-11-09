import 'dart:async';

import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// The comparison page depends on this stream to show progress instead of
/// freezing; the contract below is what keeps the UI honest.
void main() {
  test('comparison streams intermediate progress before the final rows', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    final List<DiffProgress> samples = await service
        .compareReportWithDirectory(
          report: service.sampleReport,
          directory: '/tmp/example',
        )
        .toList();

    expect(samples.length, greaterThan(1), reason: 'progress must not be one frame');
    final List<DiffProgress> running =
        samples.where((DiffProgress sample) => !sample.done).toList();
    expect(running, isNotEmpty, reason: 'the UI needs moving samples');
    for (final DiffProgress sample in running) {
      expect(sample.bytesDone, greaterThan(0));
      expect(sample.bytesTotal, greaterThan(0));
      expect(sample.fraction, inInclusiveRange(0.0, 1.0));
    }

    final DiffProgress last = samples.last;
    expect(last.done, isTrue);
    expect(last.cancelled, isFalse);
    expect(last.rows, isNotEmpty, reason: 'the terminal sample carries the rows');
  });

  test('cancelling a comparison ends the stream without rows', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    final Stream<DiffProgress> stream = service.compareReportWithDirectory(
      report: service.sampleReport,
      directory: '/tmp/example',
    );
    final Completer<List<DiffProgress>> collected = Completer<List<DiffProgress>>();
    final List<DiffProgress> samples = <DiffProgress>[];
    late final StreamSubscription<DiffProgress> subscription;
    subscription = stream.listen(
      (DiffProgress sample) async {
        samples.add(sample);
        if (!sample.done && samples.length == 1) {
          await service.cancelCurrentOperation();
        }
      },
      onDone: () {
        if (!collected.isCompleted) {
          collected.complete(samples);
        }
      },
    );

    final List<DiffProgress> result = await collected.future.timeout(
      const Duration(seconds: 5),
    );
    await subscription.cancel();

    expect(result, isNotEmpty);
    expect(result.last.done, isTrue);
    expect(result.last.cancelled, isTrue, reason: 'a cancel is not a result');
    expect(result.last.rows, isEmpty);
  });

  test('the fake advertises cancellation like the native service', () {
    expect(FakeBatchHashService().supportsCancellation, isTrue);
  });
}