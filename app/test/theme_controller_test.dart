import 'package:batchhash/src/theme.dart';
import 'package:batchhash/src/theme_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('parseColorInput', () {
    test('accepts six digit hex with and without a hash', () {
      expect(parseColorInput('#00658F'), const Color(0xFF00658F));
      expect(parseColorInput('00658F'), const Color(0xFF00658F));
      // Case must not matter: users paste from many places.
      expect(parseColorInput('#00658f'), const Color(0xFF00658F));
    });

    test('expands three digit hex like CSS', () {
      expect(parseColorInput('#0af'), const Color(0xFF00AAFF));
    });

    test('accepts eight digit hex as ARGB', () {
      expect(parseColorInput('#8000658F'), const Color(0x8000658F));
    });

    test('accepts rgb and rgba', () {
      expect(parseColorInput('rgb(0, 101, 143)'), const Color(0xFF00658F));
      expect(parseColorInput('rgb(0,101,143)'), const Color(0xFF00658F));
      expect(
        parseColorInput('rgba(0, 101, 143, 0.5)'),
        const Color.fromRGBO(0, 101, 143, 0.5),
      );
    });

    test('rejects anything that is not a color', () {
      expect(parseColorInput(''), isNull);
      expect(parseColorInput('   '), isNull);
      expect(parseColorInput('#12345'), isNull, reason: 'five digits');
      expect(parseColorInput('not-a-color'), isNull);
      expect(parseColorInput('rgb(300, 0, 0)'), isNull, reason: 'channel > 255');
      expect(parseColorInput('rgba(0, 0, 0, 2)'), isNull, reason: 'alpha > 1');
      expect(parseColorInput('#GGGGGG'), isNull);
    });

    test('tolerates surrounding whitespace', () {
      expect(parseColorInput('  #00658F  '), const Color(0xFF00658F));
    });
  });

  group('formatColorInput', () {
    test('renders canonical uppercase hex', () {
      expect(formatColorInput(const Color(0xFF00658F)), '#00658F');
      expect(formatColorInput(const Color(0xFF0A0B0C)), '#0A0B0C');
    });

    test('round trips through the parser', () {
      for (final Color color in <Color>[
        const Color(0xFF00658F),
        const Color(0xFF000000),
        const Color(0xFFFFFFFF),
      ]) {
        expect(parseColorInput(formatColorInput(color)), color);
      }
    });
  });

  group('ThemeController', () {
    test('starts on the shipped default', () {
      final ThemeController controller = ThemeController();
      expect(controller.isDefault, isTrue);
      expect(controller.seed, AppThemeDefaults.seed);
    });

    test('notifies only when the color actually changes', () {
      final ThemeController controller = ThemeController();
      int notifications = 0;
      controller.addListener(() => notifications++);

      controller.select(AppThemeDefaults.seed);
      expect(notifications, 0, reason: 'same color must not notify');

      controller.select(const Color(0xFF3F51B5));
      expect(notifications, 1);
      expect(controller.isDefault, isFalse);
    });

    test('reset restores the default seed', () {
      final ThemeController controller = ThemeController(
        initial: const Color(0xFF3F51B5),
      );
      controller.reset();
      expect(controller.seed, AppThemeDefaults.seed);
      expect(controller.isDefault, isTrue);
    });
  });
  group('seed precedence over platform dynamic color', () {
    const Color platformPrimary = Color(0xFF123456);
    final ColorScheme dynamicScheme = ColorScheme.fromSeed(
      seedColor: platformPrimary,
    );

    test('an explicit choice overrides the dynamic scheme', () {
      // This is the fix for the reported bug: the wallpaper palette used to win,
      // so changing the accent color appeared to do nothing on Android.
      final ThemeData built = AppTheme.light(
        dynamicScheme,
        seed: const Color(0xFF66CCFF),
      );
      expect(
        built.colorScheme.primary,
        isNot(ColorScheme.fromSeed(seedColor: platformPrimary).primary),
      );
    });

    test('the default seed still allows dynamic color', () {
      final ThemeData built = AppTheme.light(
        dynamicScheme,
        seed: AppThemeDefaults.seed,
      );
      expect(built.colorScheme.primary, dynamicScheme.primary);
    });

    test('with no dynamic scheme the seed decides', () {
      final ThemeData built = AppTheme.light(
        null,
        seed: const Color(0xFF66CCFF),
      );
      // A chosen seed is pinned as primary instead of being re-derived. The
      // tonal pass used to rewrite it so heavily that two different blues ended
      // up as the same button colour, which is what the swatches were hiding.
      expect(built.colorScheme.primary, derivedPrimary(const Color(0xFF66CCFF)));
      expect(
        built.colorScheme.onPrimary,
        relativeLightness(built.colorScheme.primary) > 0.5
            ? Colors.black
            : Colors.white,
      );
    });

    test('a chosen seed is not collapsed onto the derived tonal tone', () {
      // Guards the actual regression: the default teal and the sky preset are
      // close in hue, so the generated scheme gave them nearly the same primary
      // and the two settings looked identical. A chosen seed is pinned instead.
      //
      // The default seed is deliberately excluded: it is not a choice, so it
      // keeps following the platform/derived scheme.
      final ThemeData sky = AppTheme.light(null, seed: const Color(0xFF66CCFF));
      expect(sky.colorScheme.primary, const Color(0xFF66CCFF));

      final Color separatorTone = relativeLightness(const Color(0xFF66CCFF));
      final Color tealUnderDefault = AppTheme.light(
        null,
        seed: AppTheme.seedColor,
      ).colorScheme.primary;
      expect(
        (separatorTone - relativeLightness(tealUnderDefault)).abs(),
        greaterThanOrEqualTo(0.2),
        reason: 'the sky preset must not read as the default teal',
      );
    });
  });
}
