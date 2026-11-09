import 'package:batchhash/src/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

/// A determinate bar must appear as soon as any total is known, even at 0%.
/// An indeterminate bar at that point is indistinguishable from a stalled job,
/// which is exactly the symptom this round has to remove.
void main() {
  test('progress without a total is indeterminate', () {
    const HashProgress empty = HashProgress(completed: 0, total: 0);
    expect(empty.isDeterminate, isFalse);
    expect(empty.fraction, 0.0);
  });

  test('byte progress makes the bar determinate before any entry finishes', () {
    const HashProgress early = HashProgress(
      completed: 0,
      total: 1,
      bytesDone: 0,
      bytesTotal: 4 * 1024 * 1024 * 1024,
    );
    expect(
      early.isDeterminate,
      isTrue,
      reason: 'one huge file reports a total long before it finishes',
    );
    expect(early.fraction, 0.0, reason: '0% is honest, not unknown');
  });

  test('byte progress wins over entry count', () {
    const HashProgress sample = HashProgress(
      completed: 1,
      total: 4,
      bytesDone: 50,
      bytesTotal: 100,
    );
    expect(sample.fraction, 0.5);
  });

  test('entry count is used when no byte total is known', () {
    const HashProgress sample = HashProgress(completed: 2, total: 4);
    expect(sample.isDeterminate, isTrue);
    expect(sample.fraction, 0.5);
  });

  test('a phase string maps to its enum and unknown values stay safe', () {
    expect(HashPhase.fromWire('scanning'), HashPhase.scanning);
    expect(HashPhase.fromWire('comparing'), HashPhase.comparing);
    expect(HashPhase.fromWire(null), HashPhase.hashing);
    expect(HashPhase.fromWire('who-knows'), HashPhase.hashing);
  });
}