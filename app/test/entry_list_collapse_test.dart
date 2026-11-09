import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/entry_list.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

const BatchEntry _entryA = BatchEntry(
  path: 'alpha.bin',
  size: 10,
  hashes: <String, String>{'sha256': 'a1', 'blake3': 'a2'},
);

const BatchEntry _entryB = BatchEntry(
  path: 'beta.bin',
  size: 20,
  hashes: <String, String>{'sha256': 'b1', 'blake3': 'b2'},
);

const BatchEntry _failed = BatchEntry(
  path: 'failed.bin',
  size: 0,
  status: EntryStatus.failed,
  errorKind: 'permission_denied',
);

const BatchEntry _skipped = BatchEntry(
  path: 'skipped.bin',
  size: 0,
  status: EntryStatus.skipped,
  errorKind: 'cancelled',
);

Finder toggleFor(String path) =>
    find.byKey(ValueKey<String>('entry-digest-toggle-$path'));

Future<void> pumpEntries(
  WidgetTester tester,
  List<BatchEntry> entries,
) async {
  setViewSize(tester, const Size(800, 1200));
  await tester.pumpWidget(
    wrapInApp(SingleChildScrollView(child: EntryList(entries: entries))),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('collapsed rows show only the first digest', (
    WidgetTester tester,
  ) async {
    await pumpEntries(tester, const <BatchEntry>[_entryA]);

    expect(find.text('a1'), findsOneWidget);
    expect(find.text('a2'), findsNothing);
    expect(find.byIcon(Icons.copy_outlined), findsNWidgets(1));
    expect(toggleFor('alpha.bin'), findsOneWidget);
  });

  testWidgets('expanding a row reveals every digest', (
    WidgetTester tester,
  ) async {
    await pumpEntries(tester, const <BatchEntry>[_entryA]);
    expect(find.text('a2'), findsNothing);

    await tester.tap(toggleFor('alpha.bin'));
    await tester.pumpAndSettle();

    expect(find.text('a1'), findsOneWidget);
    expect(find.text('a2'), findsOneWidget);
    expect(find.byIcon(Icons.copy_outlined), findsNWidgets(2));
  });

  testWidgets('two rows keep independent expansion state', (
    WidgetTester tester,
  ) async {
    await pumpEntries(tester, const <BatchEntry>[_entryA, _entryB]);

    await tester.tap(toggleFor('alpha.bin'));
    await tester.pumpAndSettle();

    expect(find.text('a2'), findsOneWidget);
    expect(find.text('b1'), findsOneWidget);
    expect(find.text('b2'), findsNothing);

    await tester.tap(toggleFor('beta.bin'));
    await tester.pumpAndSettle();

    expect(find.text('a2'), findsOneWidget);
    expect(find.text('b2'), findsOneWidget);
  });

  testWidgets('failed and skipped rows show no digest toggle', (
    WidgetTester tester,
  ) async {
    await pumpEntries(tester, const <BatchEntry>[_failed, _skipped]);

    expect(toggleFor('failed.bin'), findsNothing);
    expect(toggleFor('skipped.bin'), findsNothing);
    expect(find.byType(TextButton), findsNothing);
    expect(find.byIcon(Icons.copy_outlined), findsNothing);
  });

  testWidgets('expansion follows the entry path across a refresh', (
    WidgetTester tester,
  ) async {
    await pumpEntries(tester, const <BatchEntry>[_entryA, _entryB]);
    await tester.tap(toggleFor('beta.bin'));
    await tester.pumpAndSettle();
    expect(find.text('b2'), findsOneWidget);

    // Same paths, brand new BatchEntry objects, reversed order: the expanded
    // state must stay with the file, not with the row position.
    await pumpEntries(tester, const <BatchEntry>[
      BatchEntry(
        path: 'beta.bin',
        size: 21,
        hashes: <String, String>{'sha256': 'b1', 'blake3': 'b2'},
      ),
      BatchEntry(
        path: 'alpha.bin',
        size: 11,
        hashes: <String, String>{'sha256': 'a1', 'blake3': 'a2'},
      ),
    ]);

    expect(find.text('b2'), findsOneWidget);
    expect(find.text('a2'), findsNothing);
  });
}
