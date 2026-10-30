import 'package:batchhash/src/app.dart';
import 'package:batchhash/src/l10n/app_strings.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

List<String> navigationLabels(WidgetTester tester) {
  final NavigationBar bar = tester.widget<NavigationBar>(
    find.byType(NavigationBar),
  );
  return <String>[
    for (final Widget destination in bar.destinations)
      (destination as NavigationDestination).label,
  ];
}

void main() {
  test('Chinese is the default locale and both translations are complete', () {
    expect(AppStrings.defaultLocale, 'zh');
    expect(AppStrings.supportedLocales, <String>['zh', 'en']);
    // Every key resolves to copy in both languages, never to the raw key.
    for (final String key in <String>[
      'app.title',
      'nav.hash',
      'nav.settings',
      'hash.title',
      'rename.title',
      'report.title',
      'compare.title',
      'history.title',
      'settings.title',
      'about.title',
      'hardware.title',
      'demo.banner',
    ]) {
      expect(AppStrings.of(key, locale: 'zh'), isNot(key), reason: key);
      expect(AppStrings.of(key, locale: 'en'), isNot(key), reason: key);
    }
  });

  testWidgets('the app opens in Chinese even on an English platform', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(500, 900));
    await tester.pumpWidget(
      BatchHashApp(service: FakeBatchHashService(), enableDynamicColor: false),
    );
    await tester.pumpAndSettle();

    expect(navigationLabels(tester), <String>[
      '哈希',
      '写名',
      '报告',
      '对比',
      '记录',
      '设置',
    ]);
    expect(find.text('批量哈希'), findsWidgets);
  });

  testWidgets('an explicit locale overrides the default', (
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
    expect(navigationLabels(tester).first, 'Hash');
  });

  testWidgets('the settings switch retranslates the whole shell', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(500, 900));
    await tester.pumpWidget(
      BatchHashApp(service: FakeBatchHashService(), enableDynamicColor: false),
    );
    await tester.pumpAndSettle();
    expect(navigationLabels(tester).first, '哈希');

    await tester.tap(
      find.descendant(
        of: find.byType(NavigationBar),
        matching: find.text('设置'),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('界面语言'), findsOneWidget);

    await tester.tap(find.text('English'));
    await tester.pumpAndSettle();

    expect(navigationLabels(tester).last, 'Settings');
    expect(find.text('Language'), findsOneWidget);
  });
}
