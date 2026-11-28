import 'package:batchhash/src/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// The text theme baseline must follow the brightness.
///
/// `Typography.material2021` exposes a `black` and a `white` text theme. Taking
/// `black` unconditionally painted dark mode with near-black text on a near-black
/// surface: readable in light mode, effectively invisible in dark.
void main() {
  test('dark mode uses light text', () {
    final ThemeData dark = AppTheme.dark(null);
    expect(dark.colorScheme.brightness, Brightness.dark);

    final Color? color = dark.textTheme.bodyMedium?.color;
    expect(color, isNotNull);
    // High luminance is the property that matters, not an exact colour: the
    // baseline supplies an opacity-adjusted white that varies by role.
    expect(
      color!.computeLuminance(),
      greaterThan(0.5),
      reason: 'dark mode text must be light, got $color',
    );
  });

  test('light mode uses dark text', () {
    final ThemeData light = AppTheme.light(null);
    expect(light.colorScheme.brightness, Brightness.light);

    final Color? color = light.textTheme.bodyMedium?.color;
    expect(color, isNotNull);
    expect(
      color!.computeLuminance(),
      lessThan(0.5),
      reason: 'light mode text must be dark, got $color',
    );
  });

  test('the two modes are actually different', () {
    // Guards against a regression that makes both branches pick the same
    // baseline, which a per-mode assertion alone would not catch if both were
    // wrong in the same direction.
    final Color dark = AppTheme.dark(null).textTheme.bodyMedium!.color!;
    final Color light = AppTheme.light(null).textTheme.bodyMedium!.color!;
    expect(dark.computeLuminance(), greaterThan(light.computeLuminance()));
  });

  test('the monospace stack is reachable and ordered', () {
    // It was declared and never used, so every digest rendered in whatever the
    // platform calls "monospace". A non-empty ordered list is what the widgets
    // now pass to TextStyle.
    expect(AppTheme.monoFontFallback, isNotEmpty);
    expect(AppTheme.monoFontFallback.first, isNotEmpty);
    expect(AppTheme.monoFontFallback, contains('monospace'));
  });
}
