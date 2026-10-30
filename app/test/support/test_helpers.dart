import 'package:batchhash/src/l10n/app_strings.dart';
import 'package:batchhash/src/l10n/app_strings_scope.dart';
import 'package:batchhash/src/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// Overrides the logical size of the test surface and resets it afterwards.
void setViewSize(WidgetTester tester, Size size) {
  tester.view.devicePixelRatio = 1.0;
  tester.view.physicalSize = size;
  addTearDown(tester.view.reset);
}

/// Wraps a widget in a MaterialApp with the theme and a string scope.
///
/// Tests pin English so assertions read stable copy; the shipping application
/// defaults to Chinese ([AppStrings.defaultLocale]). Pass a different
/// [locale] to exercise another language.
Widget wrapInApp(Widget child, {String locale = 'en'}) {
  final String language = AppStrings.normalize(locale);
  return MaterialApp(
    theme: AppTheme.light(null),
    locale: Locale(language),
    builder: (BuildContext context, Widget? inner) => AppStringsScope.wrap(
      locale: language,
      child: inner ?? const SizedBox.shrink(),
    ),
    home: Scaffold(body: child),
  );
}
