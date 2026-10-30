import 'package:flutter/foundation.dart';

import 'app_strings.dart';

/// Holds the active UI language for the whole application.
///
/// The app ships Chinese-first: [AppStrings.defaultLocale] is the initial value
/// regardless of the platform language, so a first run is always Chinese. The
/// settings page switches it at runtime and every listener is rebuilt through
/// [notifyListeners]. Nothing is persisted yet, so the choice lasts for the
/// session only; adding persistence later only touches this class.
class LocaleController extends ChangeNotifier {
  LocaleController({String? initial})
    : _locale = AppStrings.normalize(initial);

  String _locale;

  /// Active language code, either 'zh' or 'en'.
  String get locale => _locale;

  /// Whether the active language is Chinese.
  bool get isChinese => _locale == 'zh';

  /// Switch the active language; unknown codes fall back to Chinese.
  void select(String locale) {
    final String next = AppStrings.normalize(locale);
    if (next == _locale) {
      return;
    }
    _locale = next;
    notifyListeners();
  }
}
