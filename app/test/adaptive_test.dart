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
    expect(find.text('Batch hashing'), findsOneWidget);
  });

  testWidgets('wide width shows a rail plus list and detail panes', (
    WidgetTester tester,
  ) async {
    await pumpAppAt(tester, const Size(1400, 900));
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-rail')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-destination-list')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-detail-pane')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('adaptive-navigation-bar')),
      findsNothing,
    );
  });
}
