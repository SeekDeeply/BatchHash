import 'package:flutter/widgets.dart';

import 'app_strings.dart';

/// Resolves application copy for the active locale.
///
/// Install it once with [AppStringsScope.wrap] and read strings anywhere below
/// with [AppStringsScope.of]. Lookups go through the nearest [Locale] provided
/// by the platform, defaulting to Chinese when the platform asks for a language
/// the app does not ship.
class AppStringsScope extends InheritedWidget {
  const AppStringsScope({
    super.key,
    required this.locale,
    required super.child,
  });

  /// Locale used for lookups, for example \`zh\` or \`en_US\`.
  final String locale;

  /// Wrap a widget tree so descendants can resolve copy.
  static Widget wrap({
    required String locale,
    required Widget child,
  }) {
    return AppStringsScope(locale: locale, child: child);
  }

  /// The copy resolver for the nearest scope.
  static AppStringsScope of(BuildContext context) {
    final AppStringsScope? scope =
        context.dependOnInheritedWidgetOfExactType<AppStringsScope>();
    assert(scope != null, 'AppStringsScope is missing above this widget');
    return scope!;
  }

  /// Resolve one key, with optional placeholders.
  String call(String key, {Map<String, String>? args}) {
    return AppStrings.of(key, locale: locale, args: args);
  }

  @override
  bool updateShouldNotify(AppStringsScope oldWidget) {
    return oldWidget.locale != locale;
  }
}

/// Convenience accessor: \`context.strings('hash.start')\`.
extension AppStringsContext on BuildContext {
  /// Resolve a key against the nearest [AppStringsScope].
  String strings(String key, {Map<String, String>? args}) {
    return AppStringsScope.of(this).call(key, args: args);
  }
}
