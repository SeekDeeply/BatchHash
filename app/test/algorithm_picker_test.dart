import 'package:batchhash/src/l10n/app_strings.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/algorithm_picker_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// Number of catalogue rows whose checkbox is currently ticked.
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

void main() {
  Future<void> pumpPicker(
    WidgetTester tester, {
    required Set<String> selected,
  }) async {
    // The dialog lists every algorithm, so the test surface must be tall
    // enough for all of them to be built.
    setViewSize(tester, const Size(900, 2400));
    await tester.pumpWidget(
      wrapInApp(
        SingleChildScrollView(
          child: AlgorithmPickerDialog(
            catalogue: kAlgorithmCatalogue,
            selected: selected,
          ),
        ),
      ),
    );
  }

  testWidgets('select all ticks every algorithm', (WidgetTester tester) async {
    await pumpPicker(tester, selected: <String>{'md5'});
    expect(_checkedCount(tester), 1);

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    expect(_checkedCount(tester), kAlgorithmCatalogue.length);
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

    final int visible = tester
        .widgetList<CheckboxListTile>(find.byType(CheckboxListTile))
        .length;
    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    // Only the rows still on screen were added; filtering must not silently
    // select the algorithms the user hid.
    expect(_checkedCount(tester), visible);
    expect(visible, lessThan(kAlgorithmCatalogue.length));
  });

  testWidgets('keep one leaves a single algorithm selected', (WidgetTester tester) async {
    await pumpPicker(tester, selected: <String>{'md5', 'sha256', 'blake3'});
    expect(_checkedCount(tester), 3);

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-none')));
    await tester.pumpAndSettle();

    expect(_checkedCount(tester), 1);
  });
}
