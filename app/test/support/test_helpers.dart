import 'package:batchhash/src/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// Overrides the logical size of the test surface and resets it afterwards.
void setViewSize(WidgetTester tester, Size size) {
  tester.view.devicePixelRatio = 1.0;
  tester.view.physicalSize = size;
  addTearDown(tester.view.reset);
}

/// Wraps a widget in a MaterialApp with the production theme.
Widget wrapInApp(Widget child) {
  return MaterialApp(
    theme: AppTheme.light(null),
    home: Scaffold(body: child),
  );
}
