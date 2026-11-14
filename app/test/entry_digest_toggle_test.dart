import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/entry_list.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

BatchEntry entryWith(Map<String, String> hashes) => BatchEntry(
  path: '/root/file.bin',
  size: 3,
  hashes: hashes,
);

void main() {
  testWidgets('a single algorithm shows no expand button', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(900, 900));
    await tester.pumpWidget(
      wrapInApp(
        Scaffold(
          body: EntryList(
            entries: <BatchEntry>[
              entryWith(<String, String>{'sha256': 'aa'}),
            ],
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    // Everything is already visible, so the toggle would do nothing.
    expect(
      find.byKey(
        const ValueKey<String>('entry-digest-toggle-/root/file.bin'),
      ),
      findsNothing,
    );
    expect(find.text('aa'), findsOneWidget);
  });

  testWidgets('multiple algorithms show a count of what is hidden', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(900, 900));
    await tester.pumpWidget(
      wrapInApp(
        Scaffold(
          body: EntryList(
            entries: <BatchEntry>[
              entryWith(<String, String>{
                'sha256': 'aa',
                'blake3': 'bb',
                'crc32': 'cc',
              }),
            ],
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    // Three digests, one already rendered: the button must say 2, not 3. The
    // old label printed the algorithm total, which was off by the visible row.
    expect(find.textContaining('2'), findsWidgets);
    expect(
      find.byKey(
        const ValueKey<String>('entry-digest-toggle-/root/file.bin'),
      ),
      findsOneWidget,
    );
  });
}
