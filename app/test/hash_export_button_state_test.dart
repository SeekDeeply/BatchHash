import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/l10n/app_strings.dart';
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

    // Reach the results block first. Without entries the export control does not
    // exist at all, and a finder that silently matches nothing would pass a
    // weaker assertion than intended.
    expect(
      find.text(AppStrings.of('hash.summary', locale: 'en')),
      findsOneWidget,
    );

    final Finder menu = find.byKey(const ValueKey<String>('hash-export-menu'));
    expect(menu, findsOneWidget);

    // The menu itself must be enabled; that is what makes the tap work.
    expect(tester.widget<PopupMenuButton<Object?>>(menu).enabled, isTrue);

    // The label is built by `OutlinedButton.icon`, which Flutter renders as a
    // private subclass, so a plain `byType(OutlinedButton)` finds nothing. Match
    // on the predicate instead of the exact runtime type.
    final Finder inner = find.descendant(
      of: menu,
      matching: find.byWidgetPredicate((Widget widget) => widget is OutlinedButton),
    );
    expect(inner, findsOneWidget);

    // Non-null is the whole point: a null callback is what paints it grey.
    final OutlinedButton button = tester.widget<OutlinedButton>(inner);
    expect(button.onPressed, isNotNull);
  });
}
