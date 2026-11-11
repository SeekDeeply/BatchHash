import 'package:batchhash/src/l10n/app_strings.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/algorithm_picker_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// Whether the row for [id] exists and is ticked.
bool _isTicked(WidgetTester tester, String id) {
  final Finder row = find.byKey(ValueKey<String>('algorithm-picker-$id'));
  if (row.evaluate().isEmpty) {
    return false;
  }
  return tester.widget<CheckboxListTile>(row).value == true;
}

/// Ids whose row is present in the built tree.
List<String> _builtIds(WidgetTester tester) {
  return <String>[
    for (final AlgorithmInfo info in kAlgorithmCatalogue)
      if (find
          .byKey(ValueKey<String>('algorithm-picker-${info.id}'))
          .evaluate()
          .isNotEmpty)
        info.id,
  ];
}

void main() {
  Future<void> pumpPicker(
    WidgetTester tester, {
    required Set<String> selected,
  }) async {
    setViewSize(tester, const Size(900, 1200));
    // Mounted through showDialog so the AlertDialog gets a real route and its
    // own constraints; dropping it straight into a Scaffold body collapses the
    // scrollable content and no row is ever built.
    await tester.pumpWidget(
      wrapInApp(
        Builder(
          builder: (BuildContext context) => TextButton(
            onPressed: () => showDialog<void>(
              context: context,
              builder: (BuildContext context) => AlgorithmPickerDialog(
                catalogue: kAlgorithmCatalogue,
                selected: selected,
              ),
            ),
            child: const Text('open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
  }

  testWidgets('the picker builds rows and honours the initial selection', (
    WidgetTester tester,
  ) async {
    await pumpPicker(tester, selected: <String>{'md5'});

    final List<String> built = _builtIds(tester);
    expect(built, isNotEmpty, reason: 'the list must render some rows');
    expect(_isTicked(tester, 'md5'), isTrue);
    expect(_isTicked(tester, 'sha256'), isFalse);
  });

  testWidgets('select all ticks every built row', (WidgetTester tester) async {
    await pumpPicker(tester, selected: <String>{'md5'});

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    // Every row the dialog laid out is now ticked. The catalogue scrolls inside
    // a fixed-height dialog, so off-screen rows are not built and cannot be
    // inspected; the invariant is that no built row is left unticked.
    for (final String id in _builtIds(tester)) {
      expect(_isTicked(tester, id), isTrue, reason: 'row $id');
    }
  });

  testWidgets('select all reports the full selection count', (
    WidgetTester tester,
  ) async {
    await pumpPicker(tester, selected: <String>{'md5'});

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    // The counter reflects the whole selection, not merely the visible rows.
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

  testWidgets('select all only covers search results', (
    WidgetTester tester,
  ) async {
    await pumpPicker(tester, selected: <String>{'md5'});

    await tester.enterText(
      find.byKey(const ValueKey<String>('algorithm-search')),
      'sha',
    );
    await tester.pumpAndSettle();

    final List<String> filtered = _builtIds(tester);
    expect(filtered, isNotEmpty, reason: 'the search must keep some rows');
    expect(filtered, isNot(contains('md5')));

    await tester.tap(find.byKey(const ValueKey<String>('algorithm-select-all')));
    await tester.pumpAndSettle();

    // Only the matching rows were added; the filtered-out ones stay untouched.
    for (final String id in filtered) {
      expect(_isTicked(tester, id), isTrue, reason: 'row $id');
    }
  });

  testWidgets('keep one leaves a single algorithm selected', (
    WidgetTester tester,
  ) async {
    await pumpPicker(tester, selected: <String>{'md5', 'sha256', 'blake3'});
    expect(_isTicked(tester, 'md5'), isTrue);
    expect(_isTicked(tester, 'sha256'), isTrue);

    await tester.tap(
      find.byKey(const ValueKey<String>('algorithm-select-none')),
    );
    await tester.pumpAndSettle();

    final List<String> ticked = <String>[
      for (final String id in _builtIds(tester))
        if (_isTicked(tester, id)) id,
    ];
    expect(ticked, hasLength(1));
  });
}
