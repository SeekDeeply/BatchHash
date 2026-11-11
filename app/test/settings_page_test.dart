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

  testWidgets('manual entry applies a valid hex color through the dialog', (
    WidgetTester tester,
  ) async {
    await pumpSettings(tester);

    await tester.tap(find.byKey(const ValueKey<String>('accent-open-manual')));
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(const ValueKey<String>('accent-input')),
      '#3F51B5',
    );
    await tester.tap(find.byKey(const ValueKey<String>('accent-apply')));
    await tester.pumpAndSettle();

    expect(theme.seed, const Color(0xFF3F51B5));
  });

  testWidgets('manual entry rejects an invalid value and keeps the old seed', (
    WidgetTester tester,
  ) async {
    await pumpSettings(tester);
    final Color before = theme.seed;

    await tester.tap(find.byKey(const ValueKey<String>('accent-open-manual')));
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(const ValueKey<String>('accent-input')),
      'not-a-color',
    );
    await tester.tap(find.byKey(const ValueKey<String>('accent-apply')));
    await tester.pumpAndSettle();

    // The dialog stays open with an error rather than closing on bad input.
    expect(theme.seed, before);
    expect(find.byKey(const ValueKey<String>('accent-input-dialog')), findsOneWidget);
  });

  testWidgets('tapping a preset applies it without a confirm step', (
    WidgetTester tester,
  ) async {
    await pumpSettings(tester);

    await tester.tap(
      find.byKey(const ValueKey<String>('accent-preset-accent.indigo')),
    );
    await tester.pump();

    expect(theme.seed, const Color(0xFF3F51B5));
  });

  testWidgets('the #66CCFF preset is offered and applies', (
    WidgetTester tester,
  ) async {
    await pumpSettings(tester);

    await tester.tap(
      find.byKey(const ValueKey<String>('accent-preset-accent.sky')),
    );
    await tester.pump();

    expect(theme.seed, const Color(0xFF66CCFF));
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
