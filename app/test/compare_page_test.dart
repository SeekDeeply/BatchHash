import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/pages/compare_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:batchhash/src/widgets/diff_table.dart';
import 'package:batchhash/src/widgets/entry_list.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

Future<FakeBatchHashService> pumpCompare(WidgetTester tester, Size size) async {
  setViewSize(tester, size);
  final FakeBatchHashService fake = FakeBatchHashService();
  final BatchHashAppState state = BatchHashAppState(service: fake);
  state.report.value = fake.sampleReport;
  await tester.pumpWidget(wrapInApp(ComparePage(state: state)));
  await tester.pumpAndSettle();
  return fake;
}

void main() {
  testWidgets('filter chips filter the comparison rows', (
    WidgetTester tester,
  ) async {
    await pumpCompare(tester, const Size(1000, 1400));
    expect(find.byType(DiffRowTile), findsNWidgets(12));

    await tester.tap(
      find.byKey(const ValueKey<String>('compare-filter-changed')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(DiffRowTile), findsNWidgets(2));
    expect(
      find.byKey(const ValueKey<String>('diff-row-docs/gamma.md')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('diff-row-docs/alpha.md')),
      findsNothing,
    );

    await tester.tap(
      find.byKey(const ValueKey<String>('compare-filter-removed')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(DiffRowTile), findsNWidgets(1));

    await tester.tap(
      find.byKey(const ValueKey<String>('compare-filter-differences')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(DiffRowTile), findsNWidgets(10));
  });

  testWidgets('changed rows use the error container color and 4px status bar', (
    WidgetTester tester,
  ) async {
    await pumpCompare(tester, const Size(1000, 1400));
    final ThemeData theme = Theme.of(
      tester.element(
        find.byKey(const ValueKey<String>('compare-policy-banner')),
      ),
    );
    final Material changedRow = tester.widget<Material>(
      find.byKey(const ValueKey<String>('diff-row-bg-docs/gamma.md')),
    );
    expect(changedRow.color, theme.colorScheme.errorContainer);

    final Material unchangedRow = tester.widget<Material>(
      find.byKey(const ValueKey<String>('diff-row-bg-docs/alpha.md')),
    );
    expect(unchangedRow.color, Colors.transparent);

    expect(
      tester
          .getSize(
            find.byKey(const ValueKey<String>('diff-status-bar-docs/gamma.md')),
          )
          .width,
      DiffRowTile.statusBarWidth,
    );
  });

  testWidgets('policy banner is always visible', (WidgetTester tester) async {
    await pumpCompare(tester, const Size(1000, 1400));
    expect(
      find.byKey(const ValueKey<String>('compare-policy-banner')),
      findsOneWidget,
    );
    expect(find.textContaining('Comparison policy'), findsOneWidget);
  });

  testWidgets('narrow layout shows left, right and differences tabs', (
    WidgetTester tester,
  ) async {
    await pumpCompare(tester, const Size(500, 1000));
    expect(
      find.byKey(const ValueKey<String>('compare-pane-selector')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('compare-policy-banner')),
      findsOneWidget,
    );
    expect(find.byType(DiffRowTile), findsWidgets);

    final Finder leftTab = find.text('Left');
    await tester.ensureVisible(leftTab);
    await tester.pumpAndSettle();
    await tester.tap(leftTab);
    await tester.pumpAndSettle();
    expect(find.byType(EntryTile), findsWidgets);
    expect(find.byType(DiffRowTile), findsNothing);
  });
}
