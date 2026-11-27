import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/pages/hash_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// The export control is a menu wearing a button.
///
/// `PopupMenuButton` owns the tap, so the inner button exists only as a label.
/// It used to be `onPressed: null`, which is how Material 3 spells "disabled":
/// the menu opened fine, but the control was painted in the disabled grey, so an
/// available action looked unavailable.
void main() {
  testWidgets('the export button does not render as disabled when enabled', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1400));
    final FakeBatchHashService fake = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: fake);
    state.entries.value = fake.sampleHashedEntries;

    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    final Finder menu = find.byKey(
      const ValueKey<String>('hash-export-menu'),
    );
    expect(menu, findsOneWidget);

    final OutlinedButton button = tester.widget<OutlinedButton>(
      find.descendant(of: menu, matching: find.byType(OutlinedButton)),
    );
    // Non-null is the whole point: a null callback is what paints it grey.
    expect(button.onPressed, isNotNull);

    // And the tap still reaches the menu rather than being swallowed.
    await tester.tap(menu);
    await tester.pumpAndSettle();
    expect(find.byType(PopupMenuItem<Object?>), findsWidgets);
  });
}
