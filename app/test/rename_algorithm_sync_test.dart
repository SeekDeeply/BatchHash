import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/pages/rename_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// The rename page must offer what the files actually have, not what the hash
/// page happens to have ticked.
///
/// The two drift apart constantly: the user runs a hash, switches to this page,
/// then changes the selection on the hash page. The stored rename config still
/// named the old algorithm, the core reported every row as `missingHash`, and
/// that reads as corrupt data rather than a stale setting.
void main() {
  testWidgets('the page offers the algorithms the entries were hashed with', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(900, 1200));
    final FakeBatchHashService fake = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: fake);
    state.entries.value = fake.sampleHashedEntries;
    // The hash page now says something the entries were never hashed with.
    state.algorithms.value = <String>{'sha512'};
    state.renameConfig.value = const RenameConfig(
      algorithms: <String>['sha512'],
      length: 6,
      uppercase: true,
      strict: false,
    );

    await tester.pumpWidget(wrapInApp(RenamePage(state: state)));
    await tester.pumpAndSettle();

    // The preview must be available: the entries carry usable hashes, so there
    // is nothing for the user to fix. Before the fix the stored `sha512`
    // selection survived and the request failed.
    final FilledButton preview = tester.widget<FilledButton>(
      find.byKey(const ValueKey<String>('rename-dry-run-button')),
    );
    expect(preview.onPressed, isNotNull);

    // And the stale hash-page selection must not be what the dropdown shows.
    expect(find.text('SHA512'), findsNothing);
  });

  testWidgets('the preview is refused with a hint when nothing is hashed', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(900, 1200));
    final FakeBatchHashService fake = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: fake);
    // Entries that exist but carry no digest at all.
    state.entries.value = <BatchEntry>[
      for (final BatchEntry entry in fake.sampleHashedEntries)
        entry.copyWith(hashes: const <String, String>{}),
    ];

    await tester.pumpWidget(wrapInApp(RenamePage(state: state)));
    await tester.pumpAndSettle();

    // Refused up front rather than sent: the core would answer with every row
    // marked `missingHash`, which looks like corrupt data.
    final FilledButton preview = tester.widget<FilledButton>(
      find.byKey(const ValueKey<String>('rename-dry-run-button')),
    );
    expect(preview.onPressed, isNull);
  });
}
