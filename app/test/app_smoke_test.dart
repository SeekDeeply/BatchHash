import 'package:batchhash/src/app.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

void main() {
  testWidgets('app shell exposes every navigation destination', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(500, 900));
    await tester.pumpWidget(
      BatchHashApp(
        service: FakeBatchHashService(),
        enableDynamicColor: false,
        locale: 'en',
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-bar')),
      findsOneWidget,
    );
    final NavigationBar navigationBar = tester.widget<NavigationBar>(
      find.byType(NavigationBar),
    );
    final List<String> labels = <String>[
      for (final Widget destination in navigationBar.destinations)
        (destination as NavigationDestination).label,
    ];
    expect(labels, <String>[
      'Hash',
      'Rename',
      'Report',
      'Compare',
      'History',
      'Settings',
    ]);
  });

  testWidgets('navigation bar switches pages', (WidgetTester tester) async {
    setViewSize(tester, const Size(500, 900));
    await tester.pumpWidget(
      BatchHashApp(
        service: FakeBatchHashService(),
        enableDynamicColor: false,
        locale: 'en',
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Batch hashing'), findsOneWidget);
    await tester.tap(
      find.descendant(
        of: find.byType(NavigationBar),
        matching: find.text('History'),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Batch history'), findsOneWidget);
  });

  testWidgets('app renders with the dynamic color builder enabled', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(500, 900));
    await tester.pumpWidget(
      BatchHashApp(service: FakeBatchHashService(), locale: 'en'),
    );
    await tester.pumpAndSettle();
    expect(find.byType(NavigationBar), findsOneWidget);
    expect(find.text('Batch hashing'), findsOneWidget);
  });
}
