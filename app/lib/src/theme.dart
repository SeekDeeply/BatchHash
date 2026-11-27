import 'dart:io' show Platform;

import 'package:flutter/foundation.dart' show defaultTargetPlatform, TargetPlatform;
import 'package:flutter/material.dart';

import 'models/models.dart';
import 'theme_controller.dart';

/// Material 3 theme configuration with a teal-blue seed fallback.
class AppTheme {
  const AppTheme._();

  /// Seed the app shipped with; kept as an alias for the settings copy.
  static const Color seedColor = AppThemeDefaults.seed;

  /// Whether [seed] represents a deliberate user choice.
  ///
  /// The shipped default is not a choice: it is what the app uses when the user
  /// never opened the picker. Treating it as one would silently disable platform
  /// dynamic color for everyone, so only a different value counts.
  static bool _isUserChoice(Color? seed) {
    return seed != null && seed.value != AppThemeDefaults.seed.value;
  }

  /// Build the light theme.
  ///
  /// An explicit [seed] wins over the platform dynamic (wallpaper) scheme:
  /// preferring the wallpapers palette here would make the accent setting look
  /// broken on any device that supplies one, which is every recent Android.
  /// Without a user choice the dynamic scheme is still used, so the default
  /// experience keeps following the system.
  static ThemeData light(ColorScheme? dynamicScheme, {Color? seed}) {
    if (_isUserChoice(seed)) {
      return _themeFrom(_schemeFromChoice(seed!));
    }
    return _themeFrom(
      dynamicScheme ?? ColorScheme.fromSeed(seedColor: seed ?? seedColor),
    );
  }

  /// Build the dark theme, honouring [seed] the same way as [light].
  static ThemeData dark(ColorScheme? dynamicScheme, {Color? seed}) {
    if (_isUserChoice(seed)) {
      return _themeFrom(
        _schemeFromChoice(seed!, brightness: Brightness.dark),
      );
    }
    return _themeFrom(
      dynamicScheme ??
          ColorScheme.fromSeed(
            seedColor: seed ?? seedColor,
            brightness: Brightness.dark,
          ),
    );
  }

  /// Colour scheme for a deliberate choice.
  ///
  /// The seed is generated first and then overridden on `primary` and its
  /// companions. Deriving alone cannot keep the promise the picker makes: the
  /// tonal pass rewrites the hue neighbours into one tone, so two mid-tone blues
  /// arrived at nearly the same primary and the setting looked broken. Pinning
  /// the primary to the chosen colour makes the swatch honest, while everything
  /// else (surfaces, containers, error roles) still comes from the generated
  /// scheme so contrast and dark mode keep working.
  static ColorScheme _schemeFromChoice(
    Color seed, {
    Brightness brightness = Brightness.light,
  }) {
    final ColorScheme base = ColorScheme.fromSeed(
      seedColor: seed,
      brightness: brightness,
    );
    final Color primary = derivedPrimary(seed);
    final Color onPrimary =
        relativeLightness(primary) > 0.5 ? Colors.black : Colors.white;
    return base.copyWith(primary: primary, onPrimary: onPrimary);
  }

  /// Sans-serif families to try, in order, after the primary font.
  ///
  /// Flutter ships only Roboto, which has no CJK glyphs. On Windows the missing
  /// glyphs fell back to SimSun, an old Song-style face that looks nothing like
  /// the rest of the UI; on Linux the choice was whatever fontconfig picked, so
  /// two machines rendered the same build differently. Naming the modern system
  /// faces keeps the text consistent without embedding a font.
  static List<String> get _cjkFallback {
    if (Platform.isWindows) {
      return const <String>[
        'Segoe UI',
        'Microsoft YaHei UI',
        'Microsoft YaHei',
        'Noto Sans SC',
      ];
    }
    if (Platform.isMacOS) {
      return const <String>['PingFang SC', 'Hiragino Sans GB', 'Noto Sans SC'];
    }
    if (Platform.isAndroid) {
      return const <String>['Noto Sans CJK SC', 'Noto Sans SC'];
    }
    // Linux and anything else: fontconfig usually has at least one of these.
    return const <String>[
      'Noto Sans CJK SC',
      'Source Han Sans SC',
      'WenQuanYi Micro Hei',
      'DejaVu Sans',
    ];
  }

  /// The platform face that should lead the stack.
  ///
  /// Only Windows is named explicitly: it is the platform where the default
  /// Roboto-first stack visibly degrades, and Segoe UI Variable is both the
  /// modern Windows face and a close match for Material. Elsewhere the system
  /// default already is the right one, and forcing a name would break on a
  /// release that renamed it.
  static String? get _primaryFontFamily {
    if (Platform.isWindows) {
      return 'Segoe UI Variable';
    }
    return null;
  }

  /// Monospace stack for digests, paths and anything column-aligned.
  ///
  /// A hex digest is only readable when every character has the same advance;
  /// a proportional fallback made the report columns ragged.
  static const List<String> monoFontFallback = <String>[
    'Cascadia Mono',
    'JetBrains Mono',
    'SF Mono',
    'Consolas',
    'Noto Sans Mono',
    'monospace',
  ];

  /// The monospace family to request, or null to let the fallback stack decide.
  static String? get monoFontFamily {
    if (Platform.isWindows) {
      return 'Cascadia Mono';
    }
    if (Platform.isMacOS) {
      return 'SF Mono';
    }
    return null;
  }

  static ThemeData _themeFrom(ColorScheme scheme) {
    final String? family = _primaryFontFamily;
    final List<String> fallback = _cjkFallback;
    // The text theme is rebuilt rather than only setting `fontFamily` on
    // ThemeData: the per-style fields are what actually reach a Text widget, and
    // a family set only at the top is ignored by styles that already carry one.
    final TextTheme base = Typography.material2021(
      platform: defaultTargetPlatform,
      colorScheme: scheme,
    ).black.apply(fontFamily: family, fontFamilyFallback: fallback);
    return ThemeData(
      colorScheme: scheme,
      useMaterial3: true,
      visualDensity: VisualDensity.compact,
      fontFamily: family,
      fontFamilyFallback: fallback,
      textTheme: base.apply(fontFamily: family, fontFamilyFallback: fallback),
    );
  }
}

IconData entryStatusIcon(EntryStatus status) => switch (status) {
  EntryStatus.ok => Icons.check_circle,
  EntryStatus.failed => Icons.error,
  EntryStatus.skipped => Icons.remove_circle_outline,
};

Color entryStatusColor(EntryStatus status, ColorScheme scheme) =>
    switch (status) {
      EntryStatus.ok => scheme.tertiary,
      EntryStatus.failed => scheme.error,
      EntryStatus.skipped => scheme.outline,
    };

IconData diffStatusIcon(DiffStatus status) => switch (status) {
  DiffStatus.unchanged => Icons.check,
  DiffStatus.changed => Icons.change_circle,
  DiffStatus.added => Icons.add_circle,
  DiffStatus.removed => Icons.remove_circle,
  DiffStatus.unknown => Icons.help,
  DiffStatus.ambiguous => Icons.call_split,
  DiffStatus.duplicateKey => Icons.copy_all,
  DiffStatus.outOfRoot => Icons.exit_to_app,
  DiffStatus.corrupt => Icons.warning,
  DiffStatus.unstable => Icons.timeline,
};

Color diffStatusColor(DiffStatus status, ColorScheme scheme) =>
    switch (status) {
      DiffStatus.unchanged => scheme.outline,
      DiffStatus.changed => scheme.error,
      DiffStatus.added => scheme.tertiary,
      DiffStatus.removed => scheme.secondary,
      DiffStatus.unknown => scheme.outline,
      DiffStatus.ambiguous => scheme.tertiary,
      DiffStatus.duplicateKey => scheme.error,
      DiffStatus.outOfRoot => scheme.error,
      DiffStatus.corrupt => scheme.error,
      DiffStatus.unstable => scheme.secondary,
    };

Color diffRowBackground(DiffStatus status, ColorScheme scheme) =>
    status == DiffStatus.changed ? scheme.errorContainer : Colors.transparent;

Color diffRowForeground(DiffStatus status, ColorScheme scheme) =>
    status == DiffStatus.changed ? scheme.onErrorContainer : scheme.onSurface;

IconData renameRowStatusIcon(RenameRowStatus status) => switch (status) {
  RenameRowStatus.ready => Icons.check_circle,
  RenameRowStatus.conflict => Icons.warning,
  RenameRowStatus.skipped => Icons.skip_next,
  RenameRowStatus.missingHash => Icons.help,
  RenameRowStatus.invalidName => Icons.error,
  RenameRowStatus.noPermission => Icons.lock,
  RenameRowStatus.unsupported => Icons.block,
  // A digest that no longer matches is a security signal, not a routine skip.
  RenameRowStatus.hashMismatch => Icons.gpp_bad,
  RenameRowStatus.suffixMismatch => Icons.link_off,
  RenameRowStatus.suffixUnverified => Icons.help_outline,
};

Color renameRowStatusColor(RenameRowStatus status, ColorScheme scheme) =>
    switch (status) {
      RenameRowStatus.ready => scheme.tertiary,
      RenameRowStatus.conflict => scheme.error,
      RenameRowStatus.skipped => scheme.outline,
      RenameRowStatus.missingHash => scheme.error,
      RenameRowStatus.invalidName => scheme.error,
      RenameRowStatus.noPermission => scheme.error,
      RenameRowStatus.unsupported => scheme.outline,
      RenameRowStatus.hashMismatch => scheme.error,
      RenameRowStatus.suffixMismatch => scheme.error,
      RenameRowStatus.suffixUnverified => scheme.tertiary,
    };
