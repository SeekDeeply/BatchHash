import 'package:batchhash/src/util/saf_bridge.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('safMtimeToIso', () {
    test('converts epoch milliseconds to second-precision UTC', () {
      // The platform reports milliseconds; every mtime in a report is an
      // RFC3339 string truncated to seconds. Round-tripping through the wrong
      // unit is how a file ends up dated 1970.
      expect(
        safMtimeToIso(
          DateTime.utc(2026, 11, 14, 19, 3, 23).millisecondsSinceEpoch,
        ),
        '2026-11-14T19:03:23Z',
      );
    });

    test('a zero timestamp means unknown, not the epoch', () {
      // DocumentsContract reports 0 when a provider has no modification time.
      // Rendering that as 1970-01-01 would be a claim the data does not make.
      expect(safMtimeToIso(0), isEmpty);
    });

    test('a negative timestamp is treated as unknown', () {
      expect(safMtimeToIso(-1), isEmpty);
    });
  });

  group('chunk', () {
    test('splits into consecutive groups of at most the given size', () {
      expect(
        chunk(<int>[1, 2, 3, 4, 5], 2),
        <List<int>>[
          <int>[1, 2],
          <int>[3, 4],
          <int>[5],
        ],
      );
    });

    test('a size larger than the list yields one group', () {
      expect(chunk(<int>[1, 2], 10), <List<int>>[<int>[1, 2]]);
    });

    test('an empty list yields no groups', () {
      expect(chunk(<int>[], 4), isEmpty);
    });

    test('a non-positive size is rejected rather than looping forever', () {
      expect(() => chunk(<int>[1], 0), throwsArgumentError);
    });
  });
}
