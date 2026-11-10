import 'package:flutter_test/flutter_test.dart';

import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/text_diff.dart';

void main() {
  group('diffLines', () {
    test('identical texts produce only same rows', () {
      final List<TextDiffRow> rows = diffLines('a\nb\nc', 'a\nb\nc');
      expect(rows, hasLength(3));
      expect(
        rows.every((TextDiffRow r) => r.kind == TextDiffKind.same),
        isTrue,
      );
    });

    test('an inserted line does not cascade into changes', () {
      final List<TextDiffRow> rows = diffLines('a\nb\nc', 'a\nX\nb\nc');
      final TextDiffSummary summary = TextDiffSummary.of(rows);
      expect(summary.added, 1);
      expect(summary.removed, 0);
      expect(summary.changed, 0);
      expect(summary.same, 3);
    });

    test('a deleted line is reported as removed', () {
      final TextDiffSummary summary = TextDiffSummary.of(
        diffLines('a\nb\nc', 'a\nc'),
      );
      expect(summary.removed, 1);
      expect(summary.added, 0);
      expect(summary.changed, 0);
    });

    test('a substituted line is reported as changed', () {
      final TextDiffSummary summary = TextDiffSummary.of(
        diffLines('a\nb\nc', 'a\nB\nc'),
      );
      expect(summary.changed, 1);
      expect(summary.added, 0);
      expect(summary.removed, 0);
    });

    test('trailing append is an addition, not a change', () {
      final TextDiffSummary summary = TextDiffSummary.of(
        diffLines('a\nb', 'a\nb\nc'),
      );
      expect(summary.added, 1);
      expect(summary.same, 2);
    });

    test('empty text on one side yields additions only', () {
      final TextDiffSummary summary = TextDiffSummary.of(diffLines('', 'a\nb'));
      expect(summary.added, 2);
      expect(summary.removed, 0);
    });

    test('both sides empty is identical', () {
      final TextDiffSummary summary = TextDiffSummary.of(diffLines('', ''));
      expect(summary.isIdentical, isTrue);
    });

    test('CRLF and LF compare equal', () {
      final TextDiffSummary summary = TextDiffSummary.of(
        diffLines('a\r\nb', 'a\nb'),
      );
      expect(summary.isIdentical, isTrue);
    });
  });

  group('AlgorithmCategory', () {
    test('unknown wire values fall back to legacy', () {
      expect(AlgorithmCategory.fromWire('crypto'), AlgorithmCategory.crypto);
      expect(AlgorithmCategory.fromWire('bogus'), AlgorithmCategory.legacy);
      expect(AlgorithmCategory.fromWire(null), AlgorithmCategory.legacy);
    });
  });

  group('kDefaultPinnedAlgorithms', () {
    test('pins are distinct and present in the catalogue', () {
      expect(kDefaultPinnedAlgorithms, hasLength(5));
      expect(
        kDefaultPinnedAlgorithms.toSet(),
        hasLength(kDefaultPinnedAlgorithms.length),
      );
      final Set<String> catalogueIds = <String>{
        for (final AlgorithmInfo info in kAlgorithmCatalogue) info.id,
      };
      for (final String id in kDefaultPinnedAlgorithms) {
        expect(catalogueIds.contains(id), isTrue, reason: id);
      }
    });
  });

  group('groupAlgorithmsByCategory', () {
    test('groups every entry exactly once', () {
      final Map<AlgorithmCategory, List<AlgorithmInfo>> grouped =
          groupAlgorithmsByCategory(kAlgorithmCatalogue);
      int total = 0;
      for (final List<AlgorithmInfo> members in grouped.values) {
        total += members.length;
        for (final AlgorithmInfo info in members) {
          expect(grouped[info.category], contains(info));
        }
      }
      expect(total, kAlgorithmCatalogue.length);
    });
  });
}