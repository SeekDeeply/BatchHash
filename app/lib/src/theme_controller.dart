import 'package:flutter/material.dart';

/// Holds the seed color used to build the app color scheme.
///
/// Material 3 derives a whole tonal palette from one seed, so letting the user
/// pick a seed is enough to re-theme the app without shipping per-surface color
/// pickers. The value is stored as 0xAARRGGBB; parsing is deliberately strict
/// about length but accepts the shapes a user is likely to paste.
class ThemeController extends ChangeNotifier {
  ThemeController({Color? initial}) : _seed = initial ?? AppThemeDefaults.seed;

  Color _seed;

  /// Active seed color.
  Color get seed => _seed;

  /// Whether the active seed is the shipped default.
  bool get isDefault => _seed.value == AppThemeDefaults.seed.value;

  /// Replace the seed; passing the current value does not notify.
  void select(Color color) {
    if (color.value == _seed.value) {
      return;
    }
    _seed = color;
    notifyListeners();
  }

  /// Restore the shipped default seed.
  void reset() => select(AppThemeDefaults.seed);
}

/// The accent a seed actually produces in the running UI.
///
/// This is the root of the reported bug. Material 3 does not paint primary with
/// the seed: it rebuilds a tonal palette and takes a fixed tone (40 in light
/// mode) at the seed hue. Seeds that sit close in hue therefore collapse onto
/// nearly the same primary, which is precisely what "theme 1 and theme 2 look
/// the same" described.
///
/// Two attempts that do not work, recorded so they are not retried:
/// * swapping the hex values does not help, because the collapse is caused by
///   the tone normalisation, not by the seed itself;
/// * `DynamicSchemeVariant.fidelity` helps only when the seeds differ in chroma
///   or tone enough to survive the contrast rules. #00658F and #66CCFF are both
///   mid-tone blues, so they still derived to accents ~1 degree apart.
///
/// What actually works is to stop deriving: the chosen color becomes primary
/// unchanged. The picker is a deliberate act, the user is looking at the color
/// when they choose it, and a swatch that lies about the result is worse than a
/// primary that skips the tonal sanitising.
Color derivedPrimary(Color seed) {
  // Unchanged, deliberately. An earlier version darkened pale seeds to keep
  // white text legible, which reintroduced the original complaint in a subtler
  // form: #66CCFF came back as #38708C, so the swatch still lied. Contrast is
  // the job of onPrimary, which is chosen from the resulting lightness, not a
  // reason to repaint the colour the user just picked.
  return seed;
}
/// Perceptual lightness of [color] on a 0..1 scale, for comparison and tests.
///
/// Uses the standard luminance weights so "is this noticeably lighter" is
/// answered the way the eye answers it, not by comparing raw channel sums.
double relativeLightness(Color color) {
  // red/green/blue/alpha, not the r/g/b/a getters: those only exist from
  // Flutter 3.27 and CI pins 3.24.5.
  return (0.2126 * color.red + 0.7152 * color.green + 0.0722 * color.blue) /
      255;
}
/// Default seed color, kept next to the controller so both agree.
class AppThemeDefaults {
  const AppThemeDefaults._();

  /// Teal-blue seed the app has always shipped with.
  static const Color seed = Color(0xFF00658F);
}

/// Parse a color a user typed.
///
/// Accepts `#RRGGBB`, `RRGGBB`, `#RGB`, `#AARRGGBB`, `rgb(r,g,b)` and
/// `rgba(r,g,b,a)`. Returns null when the input is not a color, so the caller
/// can keep the previous value and show a message instead of guessing.
Color? parseColorInput(String input) {
  final String text = input.trim();
  if (text.isEmpty) {
    return null;
  }
  final RegExp rgbPattern = RegExp(
    r'^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*([0-9]*\.?[0-9]+)\s*)?\)$',
    caseSensitive: false,
  );
  final RegExpMatch? rgbMatch = rgbPattern.firstMatch(text);
  if (rgbMatch != null) {
    final int r = int.parse(rgbMatch.group(1)!);
    final int g = int.parse(rgbMatch.group(2)!);
    final int b = int.parse(rgbMatch.group(3)!);
    if (r > 255 || g > 255 || b > 255) {
      return null;
    }
    final String? alphaText = rgbMatch.group(4);
    double alpha = 1.0;
    if (alphaText != null) {
      final double? parsed = double.tryParse(alphaText);
      if (parsed == null || parsed < 0 || parsed > 1) {
        return null;
      }
      alpha = parsed;
    }
    return Color.fromRGBO(r, g, b, alpha);
  }
  String hex = text;
  if (hex.startsWith('#')) {
    hex = hex.substring(1);
  }
  hex = hex.replaceAll(' ', '');
  if (!RegExp(r'^[0-9a-fA-F]+$').hasMatch(hex)) {
    return null;
  }
  switch (hex.length) {
    case 3:
      // #RGB expands each nibble, the CSS shorthand rule.
      final int r = int.parse(hex[0] * 2, radix: 16);
      final int g = int.parse(hex[1] * 2, radix: 16);
      final int b = int.parse(hex[2] * 2, radix: 16);
      return Color.fromARGB(255, r, g, b);
    case 6:
      return Color(0xFF000000 | int.parse(hex, radix: 16));
    case 8:
      return Color(int.parse(hex, radix: 16));
    default:
      return null;
  }
}

/// Render a color as the canonical `#RRGGBB` string used in the settings UI.
String formatColorInput(Color color) {
  final int argb = color.value;
  final String rgb = (argb & 0xFFFFFF).toRadixString(16).padLeft(6, '0');
  return '#${rgb.toUpperCase()}';
}
