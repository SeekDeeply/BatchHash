import 'package:batchhash/src/l10n/app_strings.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/algorithm_picker_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// Ticked checkboxes among the rows currently built.
int _checkedCount(WidgetTester tester) {
  int checked = 0;
  for (final CheckboxListTile tile in tester.widgetList<CheckboxListTile>(
    find.byType(CheckboxListTile),
  )) {
    if (tile.value == true) {
      checked++;
    }
  }
  return checked;
}

/// Rows currently built; the list scrolls, so this is only what is on screen.
int _builtRows(WidgetTester tester) {
  return tester.widgetList<CheckboxListTile>(
    find.byType(CheckboxListTile),
  ).length;
}

void main() {
  Future<void> pumpPicker(
    WidgetTester tester, {
    required Set<String> selected,
  }) async {
    setViewSize(tester, const Size(900, 1200));
    await tester.pumpWidget(
      wrapInApp(
        AlgorithmPickerDialog(
          catalogue: kAlgorithmCatalogue,
          selected: selected,
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('select all ticks every built row', (WidgetTester tester) async {
    await pumpPicker(tester, selected: <String>{'md5'});
    expect(_checkedCount(tester), 1);

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    // Everything the dialog has laid out is now ticked. The catalogue is a
    // scrolling list inside a fixed-height dialog, so off-screen rows are not
    // built and cannot be counted; the invariant is 'no unticked built row'.
    expect(_checkedCount(tester), _builtRows(tester));
  });

  testWidgets('select all reports the full selection count', (WidgetTester tester) async {
    await pumpPicker(tester, selected: <String>{'md5'});

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    // The counter reflects the whole selection, not just the visible rows.
    expect(
      find.text(
        AppStrings.of(
          'algorithm.selectedCount',
          locale: 'en',
          args: <String, String>{
            'count': '${kAlgorithmCatalogue.length}',
            'total': '${kAlgorithmCatalogue.length}',
          },
        ),
      ),
      findsOneWidget,
    );
  });

  testWidgets('select all only covers search results', (WidgetTester tester) async {
    await pumpPicker(tester, selected: <String>{'md5'});

    await tester.enterText(
      find.byKey(const ValueKey<String>('algorithm-search')),
      'sha',
    );
    await tester.pumpAndSettle();

    final int visible = _builtRows(tester);
    expect(visible, lessThan(kAlgorithmCatalogue.length));

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    // Only the filtered rows were added; the count proves the hidden ones were
    // not silently selected.
    expect(_checkedCount(tester), visible);
    expect(
      find.text(
        AppStrings.of(
          'algorithm.selectedCount',
          locale: 'en',
          args: <String, String>{
            'count': '${visible}',
            'total': '${kAlgorithmCatalogue.length}',
          },
        ),
      ),
      findsOneWidget,
    );
  });

  testWidgets('keep one leaves a single algorithm selected', (WidgetTester tester) async {
    await pumpPicker(tester, selected: <String>{'md5', 'sha256', 'blake3'});
    expect(_checkedCount(tester), 3);

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-none')));
    await tester.pumpAndSettle();

    expect(_checkedCount(tester), 1);
  });
}
