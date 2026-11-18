import 'package:batchhash/src/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group(
    'collisionProbability',
    () {
      test(
        'is zero for a single file and for a zero length',
        () {
          expect(collisionProbability(0, 6), 0.0);
          expect(collisionProbability(1, 6), 0.0);
          expect(collisionProbability(100, 0), 0.0);
        },
      );

      test(
        'never reports certainty at the maximum suffix length',
        () {
          // The regression: 16^16 is exactly 2^64, which overflows an int and
          // became 0, so the birthday bound collapsed to exactly 1.0. Dragging
          // the slider to its right-hand end therefore showed 100%, the worst
          // possible answer at the safest setting.
          for (final int files in <int>[2, 10, 1000, 100000, 1000000]) {
            final double p = collisionProbability(files, kMaxRenameLength);
            expect(
              p,
              lessThan(0.01),
              reason:
                  '$files files at the maximum length must be near-certainly safe',
            );
          }
        },
      );

      test(
        'grows with file count and shrinks with length',
        () {
          expect(
            collisionProbability(10000, 6),
            greaterThan(collisionProbability(1000, 6)),
          );
          expect(
            collisionProbability(10000, 6),
            greaterThan(collisionProbability(10000, 12)),
          );
        },
      );

      test(
        'stays inside the unit interval for every allowed length',
        () {
          for (int length = kMinRenameLength;
              length <= kMaxRenameLength;
              length++) {
            for (final int files in <int>[2, 100, 100000, 10000000]) {
              final double p = collisionProbability(files, length);
              expect(p, greaterThanOrEqualTo(0.0));
              expect(p, lessThanOrEqualTo(1.0));
            }
          }
        },
      );

      test(
        'matches the closed-form birthday bound',
        () {
          // 1 - exp(-n(n-1)/(2*16^6)) for n = 1000.
          expect(
            collisionProbability(1000, 6),
            closeTo(0.029334, 0.0001),
          );
          // And the same formula at n = 100, two orders of magnitude smaller.
          expect(
            collisionProbability(100, 6),
            closeTo(0.000295, 0.00001),
          );
        },
      );

      test(
        'reaches certainty only when the space is truly exhausted',
        () {
          // 4 hex characters hold 65536 tokens. With far more files than that,
          // a collision is certain and reporting it is correct.
          expect(collisionProbability(10000000, 4), greaterThan(0.99));
        },
      );
    },
  );
}
