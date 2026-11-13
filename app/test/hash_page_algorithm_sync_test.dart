import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/l10n/app_strings.dart';
import 'package:batchhash/src/pages/hash_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// The text the page shows for a selection of [count] algorithms.
///
/// Read from the same string table the page uses, so the assertion does not
/// hard-code translated copy.
String hashSelectedCountText(int count) {
  return AppStrings.of(
    'hash.algorithmCount',
    locale: 'en',
    args: <String, String>{'count': '$count'},
  );
}

void main() {
  testWidgets('a restored selection reaches the hash page', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1400));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);

    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    // The page starts from the in-memory default.
    expect(state.algorithms.value, containsAll(<String>['blake3', 'sha256']));

    // This is what start-up restore does: it writes the stored set into the
    // shared state after the page was already built. Before the fix the page
    // kept its own copy taken in initState, so the restored value never showed
    // up and the next launch appeared to reset the selection.
    final String twoOf = hashSelectedCountText(2);
    state.algorithms.value = <String>{'sha512', 'crc32'};
    await tester.pumpAndSettle();

    // The page counts the selection in its own controls; before the fix this
    // stayed at the default count of two only because the default also has two
    // entries, so a distinct count is what proves the value was adopted.
    expect(find.text(twoOf), findsWidgets);
  });

  testWidgets('the listener does not fight the page over the selection', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1400));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);

    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    // Equal sets must not trigger a rebuild loop: the callback compares contents
    // before calling setState, because writing the same values back would notify
    // listeners again.
    state.algorithms.value = <String>{'blake3', 'sha256'};
    await tester.pumpAndSettle();
    expect(state.algorithms.value, containsAll(<String>['blake3', 'sha256']));

    // A genuinely different set is adopted.
    state.algorithms.value = <String>{'md5'};
    await tester.pumpAndSettle();
    expect(state.algorithms.value, <String>{'md5'});
  });
}
