import 'package:batchhash/src/l10n/app_strings.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/algorithm_picker_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

void main() {
  group('selectAllAmong', () {
    test('adds every visible algorithm to the selection', () {
      final Set<String> result = selectAllAmong(
        selected: <String>{'md5'},
        visible: <String>['sha256', 'blake3'],
      );
      expect(result, <String>{'md5', 'sha256', 'blake3'});
    });

    test('leaves algorithms outside the filter alone', () {
      // The point of respecting the search box: hidden algorithms must not be
      // selected behind the user's back.
      final Set<String> result = selectAllAmong(
        selected: <String>{'md5'},
        visible: const <String>[],
      );
      expect(result, <String>{'md5'});
    });

    test('does not mutate the input set', () {
      final Set<String> original = <String>{'md5'};
      selectAllAmong(selected: original, visible: <String>['sha256']);
      expect(original, <String>{'md5'});
    });
  });

  group('keepSingleAmong', () {
    test('collapses a multi selection to one', () {
      final Set<String> result = keepSingleAmong(
        selected: <String>{'md5', 'sha256', 'blake3'},
        fallback: 'sha256',
      );
      expect(result, hasLength(1));
    });

    test('never returns an empty selection', () {
      final Set<String> result = keepSingleAmong(
        selected: const <String>{},
        fallback: 'sha256',
      );
      expect(result, <String>{'sha256'});
    });
  });

  group('AlgorithmPickerDialog', () {
    Future<void> openPicker(
      WidgetTester tester, {
      required Set<String> selected,
    }) async {
      setViewSize(tester, const Size(1200, 2000));
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

    String countLabel(int count) {
      return AppStrings.of(
        'algorithm.selectedCount',
        locale: 'en',
        args: <String, String>{
          'count': '$count',
          'total': '${kAlgorithmCatalogue.length}',
        },
      );
    }

    testWidgets('shows the search box and both bulk actions', (
      WidgetTester tester,
    ) async {
      await openPicker(tester, selected: <String>{'md5'});

      expect(
        find.byKey(const ValueKey<String>('algorithm-search')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey<String>('algorithm-select-all')),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey<String>('algorithm-select-none')),
        findsOneWidget,
      );
      expect(find.text(countLabel(1)), findsOneWidget);
    });

    testWidgets('select all updates the counter to the whole catalogue', (
      WidgetTester tester,
    ) async {
      await openPicker(tester, selected: <String>{'md5'});

      await tester.tap(
        find.byKey(const ValueKey<String>('algorithm-select-all')),
      );
      await tester.pumpAndSettle();

      // The counter is the observable proof that every algorithm was selected,
      // including the ones scrolled out of view.
      expect(
        find.text(countLabel(kAlgorithmCatalogue.length)),
        findsOneWidget,
      );
    });

    testWidgets('select all respects an active search filter', (
      WidgetTester tester,
    ) async {
      await openPicker(tester, selected: <String>{'md5'});

      await tester.enterText(
        find.byKey(const ValueKey<String>('algorithm-search')),
        'sha',
      );
      await tester.pumpAndSettle();

      await tester.tap(
        find.byKey(const ValueKey<String>('algorithm-select-all')),
      );
      await tester.pumpAndSettle();

      final int matching = kAlgorithmCatalogue
          .where(
            (AlgorithmInfo info) =>
                info.displayName.toLowerCase().contains('sha') ||
                info.id.toLowerCase().contains('sha') ||
                info.category.wireName.contains('sha'),
          )
          .length;
      // md5 was already selected and is filtered out, so it stays counted too.
      expect(find.text(countLabel(matching + 1)), findsOneWidget);
    });

    testWidgets('keep one collapses the selection to a single algorithm', (
      WidgetTester tester,
    ) async {
      await openPicker(tester, selected: <String>{'md5', 'sha256', 'blake3'});
      expect(find.text(countLabel(3)), findsOneWidget);

      await tester.tap(
        find.byKey(const ValueKey<String>('algorithm-select-none')),
      );
      await tester.pumpAndSettle();

      expect(find.text(countLabel(1)), findsOneWidget);
    });
  });
}
