import 'package:flutter/foundation.dart';

import 'app_strings.dart';

/// Holds the active UI language for the whole application.
///
/// The app ships Chinese-first: [AppStrings.defaultLocale] is the initial value
/// regardless of the platform language, so a first run is always Chinese. The
/// settings page switches it at runtime and every listener is rebuilt through
/// [notifyListeners].
///
/// Persistence lives in the app shell rather than here: the shell already owns
/// the settings file for the accent, and keeping one writer for the whole
/// settings payload avoids two objects racing to rewrite the same JSON.
class LocaleController extends ChangeNotifier {
  LocaleController({String? initial})
    : _locale = AppStrings.normalize(initial);

  String _locale;

  /// Active language code, either 'zh' or 'en'.
  String get locale => _locale;

  /// Whether the active language is Chinese.
  bool get isChinese => _locale == 'zh';

  /// Apply a stored language without notifying.
  ///
  /// Used once during start-up, before anything listens: notifying there would
  /// schedule a rebuild of a tree that has not been built yet.
  void restore(String? locale) {
    _locale = AppStrings.normalize(locale);
  }

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
