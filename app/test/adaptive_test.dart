import 'package:batchhash/src/app.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter/material.dart';

import 'support/test_helpers.dart';

Future<void> pumpAppAt(WidgetTester tester, Size size) async {
  setViewSize(tester, size);
  await tester.pumpWidget(
    BatchHashApp(
      service: FakeBatchHashService(),
      enableDynamicColor: false,
      locale: 'en',
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('narrow width uses a NavigationBar and a single page', (
    WidgetTester tester,
  ) async {
    await pumpAppAt(tester, const Size(500, 900));
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-bar')),
      findsOneWidget,
    );
    expect(find.byType(NavigationBar), findsOneWidget);
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-rail')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-destination-list')),
      findsNothing,
    );
  });

  testWidgets('medium width shows a list and detail pane', (
    WidgetTester tester,
  ) async {
    await pumpAppAt(tester, const Size(900, 900));
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-bar')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-rail')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-destination-list')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-detail-pane')),
      findsOneWidget,
    );
    // The list is the only navigation control here, so the page name still
    // appears once, as the page heading.
    expect(find.text('Batch hashing'), findsOneWidget);
    expect(find.text('Hash'), findsOneWidget);
  });

  testWidgets('wide width replaces the list with a single labelled rail', (
    WidgetTester tester,
  ) async {
    await pumpAppAt(tester, const Size(1400, 900));
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-rail')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-destination-list')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-detail-pane')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-bar')),
      findsNothing,
    );

    // Regression guard for the duplicated navigation the user reported: the
    // rail replaces the destination list and the app bar is gone, so each
    // destination label appears exactly once.
    final NavigationRail rail = tester.widget<NavigationRail>(
      find.byType(NavigationRail),
    );
    expect(rail.destinations.length, 6);
    expect(find.text('Hash'), findsOneWidget);
    expect(find.text('Settings'), findsOneWidget);
    expect(find.byType(AppBar), findsNothing);
  });
}
