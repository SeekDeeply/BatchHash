import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/widgets/entry_list.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

const List<BatchEntry> _entries = <BatchEntry>[
  BatchEntry(
    path: 'reports/ok.txt',
    size: 2048,
    hashes: <String, String>{'blake3': 'abc123'},
  ),
  BatchEntry(
    path: 'reports/failed.txt',
    size: 0,
    status: EntryStatus.failed,
    errorKind: 'permission_denied',
  ),
  BatchEntry(
    path: 'reports/skipped.txt',
    size: 0,
    status: EntryStatus.skipped,
    errorKind: 'cancelled',
  ),
];

Future<ColorScheme> pumpEntries(WidgetTester tester) async {
  await tester.pumpWidget(
    wrapInApp(
      const SingleChildScrollView(child: EntryList(entries: _entries)),
    ),
  );
  await tester.pumpAndSettle();
  return Theme.of(tester.element(find.byType(EntryList))).colorScheme;
}

Icon iconFor(WidgetTester tester, String path) {
  return tester.widget<Icon>(
    find.byKey(ValueKey<String>('entry-status-icon-$path')),
  );
}

void main() {
  testWidgets('ok, failed and skipped rows use icon plus semantics label', (
    WidgetTester tester,
  ) async {
    final SemanticsHandle handle = tester.ensureSemantics();
    addTearDown(handle.dispose);
    final ColorScheme scheme = await pumpEntries(tester);

    final Icon okIcon = iconFor(tester, 'reports/ok.txt');
    final Icon failedIcon = iconFor(tester, 'reports/failed.txt');
    final Icon skippedIcon = iconFor(tester, 'reports/skipped.txt');
    expect(okIcon.icon, Icons.check_circle);
    expect(failedIcon.icon, Icons.error);
    expect(skippedIcon.icon, Icons.remove_circle_outline);
    expect(okIcon.color, scheme.tertiary);
    expect(failedIcon.color, scheme.error);
    expect(skippedIcon.color, scheme.outline);
    expect(okIcon.semanticLabel, 'Status ok');
    expect(failedIcon.semanticLabel, 'Status failed');
    expect(skippedIcon.semanticLabel, 'Status skipped');

    expect(
      tester
          .getSemantics(
            find.byKey(const ValueKey<String>('entry-row-reports/ok.txt')),
          )
          .label,
      contains('Status ok'),
    );
    expect(
      tester
          .getSemantics(
            find.byKey(const ValueKey<String>('entry-row-reports/failed.txt')),
          )
          .label,
      contains('Status failed'),
    );
    expect(
      tester
          .getSemantics(
            find.byKey(const ValueKey<String>('entry-row-reports/skipped.txt')),
          )
          .label,
      contains('Status skipped'),
    );

    expect(find.text('ok'), findsOneWidget);
    expect(find.text('failed'), findsOneWidget);
    expect(find.text('skipped'), findsOneWidget);
  });
}
