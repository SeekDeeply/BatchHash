import 'package:flutter/material.dart';

import 'models/models.dart';
import 'theme_controller.dart';

/// Material 3 theme configuration with a teal-blue seed fallback.
class AppTheme {
  const AppTheme._();

  /// Seed the app shipped with; kept as an alias for the settings copy.
  static const Color seedColor = AppThemeDefaults.seed;

  /// Build the light theme.
  ///
  /// [seed] comes from the user's color choice and only applies when there is
  /// no dynamic (wallpaper) scheme: an explicit choice must win over the
  /// platform palette, otherwise the setting would look broken on Android.
  static ThemeData light(ColorScheme? dynamicScheme, {Color? seed}) {
    return _themeFrom(
      dynamicScheme ?? ColorScheme.fromSeed(seedColor: seed ?? seedColor),
    );
  }

  /// Build the dark theme, honouring [seed] the same way as [light].
  static ThemeData dark(ColorScheme? dynamicScheme, {Color? seed}) {
    return _themeFrom(
      dynamicScheme ??
          ColorScheme.fromSeed(
            seedColor: seed ?? seedColor,
            brightness: Brightness.dark,
          ),
    );
  }

  static ThemeData _themeFrom(ColorScheme scheme) {
    return ThemeData(
      colorScheme: scheme,
      useMaterial3: true,
      visualDensity: VisualDensity.compact,
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
    };
