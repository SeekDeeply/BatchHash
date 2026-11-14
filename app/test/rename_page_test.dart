import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/pages/rename_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

Future<void> pumpRename(WidgetTester tester) async {
  setViewSize(tester, const Size(900, 1200));
  final FakeBatchHashService fake = FakeBatchHashService();
  final BatchHashAppState state = BatchHashAppState(service: fake);
  state.entries.value = fake.sampleHashedEntries;
  await tester.pumpWidget(wrapInApp(RenamePage(state: state)));
  await tester.pumpAndSettle();
}

String estimateOf(WidgetTester tester) {
  final Text text = tester.widget<Text>(
    find.byKey(const ValueKey<String>('rename-collision-estimate')),
  );
  return text.data ?? '';
}

FilledButton executeButton(WidgetTester tester) {
  return tester.widget<FilledButton>(
    find.byKey(const ValueKey<String>('rename-execute-button')),
  );
}

void main() {
  testWidgets('collision estimate updates when the suffix length changes', (
    WidgetTester tester,
  ) async {
    await pumpRename(tester);
    expect(find.text('BLAKE3'), findsOneWidget);
    final String before = estimateOf(tester);
    expect(before, contains('Recommended length'));
    // Default moved from 5 to 6 together with the recommendation table, so the
    // slider and the hint agree instead of recommending a different number.
    expect(before, contains('length 6'));

    await tester.drag(
      find.byKey(const ValueKey<String>('rename-length-slider')),
      const Offset(600, 0),
    );
    await tester.pumpAndSettle();
    final String after = estimateOf(tester);
    expect(after, isNot(before));
    expect(after, contains('length 16'));
  });

  testWidgets('execute is disabled until a dry run has been requested', (
    WidgetTester tester,
  ) async {
    await pumpRename(tester);
    expect(executeButton(tester).onPressed, isNull);

    await tester.tap(
      find.byKey(const ValueKey<String>('rename-dry-run-button')),
    );
    await tester.pumpAndSettle();

    expect(executeButton(tester).onPressed, isNotNull);
    expect(find.text('Dry run'), findsOneWidget);
  });

  testWidgets('the recording log entry point sits in the header', (
    WidgetTester tester,
  ) async {
    await pumpRename(tester);
    final Finder opener = find.byKey(
      const ValueKey<String>('rename-open-history'),
    );
    expect(opener, findsOneWidget);

    // It is in the title row, i.e. to the right of the heading and above the
    // controls, not down in the action buttons.
    final Offset title = tester.getCenter(
      find.text('Filename suffixes').first,
    );
    final Offset button = tester.getCenter(opener);
    expect(button.dx, greaterThan(title.dx));
    expect(button.dy, lessThan(title.dy + 40));

    await tester.tap(opener);
    await tester.pumpAndSettle();

    // The page pushes the log as a route, so the log replaces this page and
    // offers a back button.
    expect(find.text('Batch history'), findsOneWidget);
    expect(find.byType(BackButton), findsOneWidget);
  });
}
