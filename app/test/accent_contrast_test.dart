import 'package:batchhash/src/theme_controller.dart';
import 'package:batchhash/src/widgets/accent_color_card.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// Hue of [color] in degrees, or null when it carries no hue.
double? _hue(Color color) {
  final double r = color.red / 255;
  final double g = color.green / 255;
  final double b = color.blue / 255;
  final double max = <double>[r, g, b].reduce((double a, double b) => a > b ? a : b);
  final double min = <double>[r, g, b].reduce((double a, double b) => a < b ? a : b);
  final double delta = max - min;
  if (delta == 0) {
    return null;
  }
  double hue;
  if (max == r) {
    hue = 60 * (((g - b) / delta) % 6);
  } else if (max == g) {
    hue = 60 * ((b - r) / delta + 2);
  } else {
    hue = 60 * ((r - g) / delta + 4);
  }
  return hue < 0 ? hue + 360 : hue;
}

/// Shortest angular distance between two hues, in degrees.
double _hueGap(Color a, Color b) {
  final double? ha = _hue(a);
  final double? hb = _hue(b);
  if (ha == null || hb == null) {
    // A greyscale accent has no hue; lightness has to carry the difference.
    return 0;
  }
  final double raw = (ha - hb).abs();
  return raw > 180 ? 360 - raw : raw;
}

void main() {
  group('derived accents are distinguishable', () {
    // The reported bug: two presets looked identical on screen even though
    // their swatches differed. Material 3 paints primary from a fixed tonal
    // step, so seeds that are close in hue collapse to the same accent. These
    // assertions pin the guarantee that each preset is visibly its own color.
    test('every pair of presets differs in hue or in lightness', () {
      const List<({String key, Color color})> presets =
          AccentColorCard.presets;
      expect(presets, hasLength(greaterThanOrEqualTo(6)));

      for (var i = 0; i < presets.length; i++) {
        for (var j = i + 1; j < presets.length; j++) {
          final Color a = derivedPrimary(presets[i].color);
          final Color b = derivedPrimary(presets[j].color);
          final double hueGap = _hueGap(a, b);
          final double lightGap =
              (relativeLightness(a) - relativeLightness(b)).abs();
          expect(
            hueGap >= 8 || lightGap >= 0.05,
            isTrue,
            reason:
                '${presets[i].key} and ${presets[j].key} derive to accents '
                'that look the same (hue gap $hueGap, lightness gap $lightGap)',
          );
        }
      }
    });

    test('the shipped default and the sky preset no longer collide', () {
      // The concrete regression: #00658F vs #66CCFF produced almost the same
      // primary, which is what the user saw. They are close in hue by nature,
      // so lightness has to carry the difference.
      final Color teal = derivedPrimary(const Color(0xFF00658F));
      final Color sky = derivedPrimary(const Color(0xFF66CCFF));
      final double lightGap =
          (relativeLightness(teal) - relativeLightness(sky)).abs();
      expect(lightGap, greaterThanOrEqualTo(0.05));
    });

    test('derivedPrimary is stable for the same seed', () {
      const Color seed = Color(0xFF3F51B5);
      expect(
        derivedPrimary(seed).value,
        derivedPrimary(seed).value,
      );
    });
  });
}
