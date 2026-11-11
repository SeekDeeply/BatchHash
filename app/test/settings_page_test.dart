import 'dart:io';

import 'package:batchhash/src/pages/settings_page.dart';
import 'package:batchhash/src/pages/legal_page.dart';
import 'package:batchhash/src/pages/operation_log_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:batchhash/src/services/operation_log_store.dart';
import 'package:batchhash/src/theme_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

void main() {
  late Directory temp;
  late OperationLogStore store;
  late ThemeController theme;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('batchhash-settings-test');
    store = OperationLogStore(supportDirectory: () async => temp);
    theme = ThemeController();
  });

  tearDown(() async {
    theme.dispose();
    if (await temp.exists()) {
      await temp.delete(recursive: true);
    }
  });

  Future<void> pumpSettings(WidgetTester tester) async {
    setViewSize(tester, const Size(1000, 4000));
    await tester.pumpWidget(
      wrapInApp(
        SettingsPage(
          service: FakeBatchHashService(),
          theme: theme,
          logStore: store,
        ),
      ),
    );
    await tester.pump();
  }

  testWidgets('accent input applies a valid hex color', (WidgetTester tester) async {
    await pumpSettings(tester);

    await tester.enterText(
      find.byKey(const ValueKey<String>('accent-input')),
      '#3F51B5',
    );
    await tester.tap(find.byKey(const ValueKey<String>('accent-apply')));
    await tester.pump();

    expect(theme.seed, const Color(0xFF3F51B5));
  });

  testWidgets('accent input rejects an invalid value and keeps the old seed', (
    WidgetTester tester,
  ) async {
    await pumpSettings(tester);
    final Color before = theme.seed;

    await tester.enterText(
      find.byKey(const ValueKey<String>('accent-input')),
      'not-a-color',
    );
    await tester.tap(find.byKey(const ValueKey<String>('accent-apply')));
    await tester.pump();

    expect(theme.seed, before);
  });

  testWidgets('accent preset updates the controller', (WidgetTester tester) async {
    await pumpSettings(tester);

    await tester.tap(
      find.byKey(const ValueKey<String>('accent-preset-accent.indigo')),
    );
    await tester.pump();

    expect(theme.seed, const Color(0xFF3F51B5));
  });

  testWidgets('history entry opens the operation log', (WidgetTester tester) async {
    await pumpSettings(tester);

    await tester.tap(
      find.byKey(const ValueKey<String>('settings-history')),
    );
    // Not pumpAndSettle: the settings page hosts a hardware panel that keeps a
    // progress animation running, so the tree never becomes fully idle.
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));

    expect(find.byType(OperationLogPage), findsOneWidget);
  });

  testWidgets('legal entry opens the legal page', (WidgetTester tester) async {
    await pumpSettings(tester);

    await tester.tap(find.byKey(const ValueKey<String>('settings-legal')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));

    expect(find.byType(LegalPage), findsOneWidget);
  });
}
