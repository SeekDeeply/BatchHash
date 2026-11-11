import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:batchhash/src/widgets/text_compare_panel.dart';
import 'package:batchhash/src/widgets/text_diff.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

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

  group('TextComparePanel button state', () {
    testWidgets('compare button enables once either pane has text', (
      WidgetTester tester,
    ) async {
      await tester.pumpWidget(wrapInApp(const TextComparePanel()));

      final Finder button = find.byKey(
        const ValueKey<String>('text-compare-run'),
      );
      FilledButton widget() => tester.widget<FilledButton>(button);

      // Both panes empty: nothing to compare yet.
      expect(widget().onPressed, isNull);

      // Typing in one pane must enable it. This is the regression guard: the
      // button used to stay disabled because nothing rebuilt on input.
      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-left')),
        'alpha',
      );
      await tester.pump();
      expect(widget().onPressed, isNotNull);
    });

    testWidgets('comparing shows the row list and a summary', (
      WidgetTester tester,
    ) async {
      await tester.pumpWidget(wrapInApp(const TextComparePanel()));
      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-left')),
        'a\\nb',
      );
      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-right')),
        'a\\nB',
      );
      await tester.pump();
      await tester.tap(
        find.byKey(const ValueKey<String>('text-compare-run')),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey<String>('text-compare-rows')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey<String>('text-compare-summary')),
        findsOneWidget,
      );
    });
  });

  group('TextComparePanel digests', () {
    testWidgets('shows a digest per algorithm once a compare runs', (
      WidgetTester tester,
    ) async {
      setViewSize(tester, const Size(900, 1400));
      final FakeBatchHashService service = FakeBatchHashService();
      await tester.pumpWidget(
        wrapInApp(
          TextComparePanel(
            service: service,
            algorithms: const <String>['sha256', 'blake3'],
          ),
        ),
      );

      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-left')),
        'alpha',
      );
      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-right')),
        'beta',
      );
      await tester.tap(find.byKey(const ValueKey<String>('text-compare-run')));
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey<String>('text-compare-hashes')),
        findsOneWidget,
      );

      final List<TextHashResult> expectedLeft = await service.hashText(
        text: 'alpha',
        algorithms: const <String>['sha256', 'blake3'],
      );
      final Finder card = find.byKey(
        const ValueKey<String>('text-compare-hashes'),
      );
      for (final TextHashResult result in expectedLeft) {
        expect(
          find.descendant(of: card, matching: find.text(result.digest)),
          findsWidgets,
        );
      }
    });

    testWidgets('identical text produces identical digests for both sides', (
      WidgetTester tester,
    ) async {
      setViewSize(tester, const Size(900, 1400));
      final FakeBatchHashService service = FakeBatchHashService();
      await tester.pumpWidget(
        wrapInApp(TextComparePanel(service: service)),
      );

      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-left')),
        'same',
      );
      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-right')),
        'same',
      );
      await tester.tap(find.byKey(const ValueKey<String>('text-compare-run')));
      await tester.pumpAndSettle();

      final List<TextHashResult> left = await service.hashText(
        text: 'same',
        algorithms: const <String>['sha256', 'blake3'],
      );
      // Scope to the digest card: the same string can also live inside an
      // EditableText, and a bare text finder would match those too.
      final Finder card = find.byKey(
        const ValueKey<String>('text-compare-hashes'),
      );
      for (final TextHashResult result in left) {
        expect(
          find.descendant(of: card, matching: find.text(result.digest)),
          findsWidgets,
        );
      }
    });

    testWidgets('editing text after a compare clears the digests', (
      WidgetTester tester,
    ) async {
      // A tall viewport keeps the digest card inside the laid-out area; on the
      // default 800x600 surface the Expanded diff list squeezes it out.
      setViewSize(tester, const Size(900, 1400));
      await tester.pumpWidget(
        wrapInApp(TextComparePanel(service: FakeBatchHashService())),
      );

      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-left')),
        'alpha',
      );
      await tester.tap(find.byKey(const ValueKey<String>('text-compare-run')));
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey<String>('text-compare-hashes')),
        findsOneWidget,
      );

      await tester.enterText(
        find.byKey(const ValueKey<String>('text-compare-left')),
        'changed',
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey<String>('text-compare-hashes')),
        findsNothing,
      );
    });
  });
}