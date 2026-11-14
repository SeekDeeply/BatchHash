import 'dart:convert';
import 'dart:io';

import 'package:flutter/painting.dart' show Color;
import 'package:path_provider/path_provider.dart';

import 'app_config_store.dart' show SupportDirectoryResolver;

/// Preferences the app must remember between launches.
///
/// Kept as one file rather than one file per setting: these are saved together
/// on every change, and a single read at start-up keeps the first frame from
/// waiting on several IO round trips. Every field is optional on purpose, so a
/// file written by an older build still loads what it can instead of being
/// discarded wholesale.
class AppSettings {
  const AppSettings({
    this.locale,
    this.seedArgb,
    this.algorithms,
    this.renameLength,
    this.renameAlgorithms,
    this.renameStrict,
    this.renameUppercase,
  });

  /// UI language code, `zh` or `en`; null means never chosen.
  final String? locale;

  /// Accent seed as 0xAARRGGBB; null means never chosen.
  final int? seedArgb;


  /// Algorithms the user last selected.
  final List<String>? algorithms;

  /// Suffix length the rename page last used, in hex characters.
  ///
  /// Null means never chosen, which the page reads as the default constant
  /// rather than as zero: storing the default as a value would make it
  /// indistinguishable from a deliberate choice of that same number.
  final int? renameLength;

  /// Algorithms the rename page last wrote into suffixes.
  final List<String>? renameAlgorithms;

  /// Whether the rename page last ran in strict-match mode.
  final bool? renameStrict;

  /// Whether the rename suffix was written uppercase.
  final bool? renameUppercase;

  /// There is deliberately no field for wallpaper dynamic colour: it is a
  /// build-time switch (`BatchHashApp.enableDynamicColor`), not a user setting,
  /// and storing a value nothing reads would look like a setting that works.

  /// Accent seed as a [Color], or null when nothing was stored.
  Color? get seed => seedArgb == null ? null : Color(seedArgb!);

  AppSettings copyWith({
    String? locale,
    int? seedArgb,
    List<String>? algorithms,
    int? renameLength,
    List<String>? renameAlgorithms,
    bool? renameStrict,
    bool? renameUppercase,
  }) {
    return AppSettings(
      locale: locale ?? this.locale,
      seedArgb: seedArgb ?? this.seedArgb,
      algorithms: algorithms ?? this.algorithms,
      renameLength: renameLength ?? this.renameLength,
      renameAlgorithms: renameAlgorithms ?? this.renameAlgorithms,
      renameStrict: renameStrict ?? this.renameStrict,
      renameUppercase: renameUppercase ?? this.renameUppercase,
    );
  }

  Map<String, Object?> toJson() => <String, Object?>{
    if (locale != null) 'locale': locale,
    if (seedArgb != null) 'seed_argb': seedArgb,
    if (algorithms != null) 'algorithms': algorithms,
    if (renameLength != null) 'rename_length': renameLength,
    if (renameAlgorithms != null) 'rename_algorithms': renameAlgorithms,
    if (renameStrict != null) 'rename_strict': renameStrict,
    if (renameUppercase != null) 'rename_uppercase': renameUppercase,
  };

  /// Decode a stored payload, ignoring any field that does not parse.
  ///
  /// Partial recovery is deliberate: one corrupt field must not reset the rest,
  /// which is exactly what a user would notice as "my settings reset again".
  static AppSettings fromJson(Object? raw) {
    if (raw is! Map<String, Object?>) {
      return const AppSettings();
    }
    final Object? locale = raw['locale'];
    final Object? seed = raw['seed_argb'];
    final Object? algorithms = raw['algorithms'];
    final Object? renameLength = raw['rename_length'];
    final Object? renameAlgorithms = raw['rename_algorithms'];
    final Object? renameStrict = raw['rename_strict'];
    final Object? renameUppercase = raw['rename_uppercase'];
    return AppSettings(
      locale: locale is String && locale.trim().isNotEmpty
          ? locale.trim()
          : null,
      seedArgb: seed is int ? seed : null,
      algorithms: algorithms is List
          ? <String>[
              for (final Object? entry in algorithms)
                if (entry is String && entry.trim().isNotEmpty) entry.trim(),
            ]
          : null,
      renameLength: renameLength is int ? renameLength : null,
      renameAlgorithms: renameAlgorithms is List
          ? <String>[
              for (final Object? entry in renameAlgorithms)
                if (entry is String && entry.trim().isNotEmpty) entry.trim(),
            ]
          : null,
      renameStrict: renameStrict is bool ? renameStrict : null,
      renameUppercase: renameUppercase is bool ? renameUppercase : null,
    );
  }
}

/// Reads and writes [AppSettings] in the application support directory.
///
/// Every failure degrades to "nothing stored": a settings file is a convenience,
/// and refusing to start because it is unreadable would be worse than starting
/// with defaults.
class AppSettingsStore {
  AppSettingsStore({SupportDirectoryResolver? supportDirectory})
      : _supportDirectory = supportDirectory ?? getApplicationSupportDirectory;

  final SupportDirectoryResolver _supportDirectory;

  File? _cachedFile;

  Future<File> _file() async {
    final File? cached = _cachedFile;
    if (cached != null) {
      return cached;
    }
    final Directory directory = await _supportDirectory();
    final File file = File(
      '${directory.path}${Platform.pathSeparator}settings.json',
    );
    _cachedFile = file;
    return file;
  }

  /// The stored settings, or empty defaults when nothing usable exists.
  Future<AppSettings> load() async {
    try {
      final File file = await _file();
      if (!await file.exists()) {
        return const AppSettings();
      }
      return AppSettings.fromJson(jsonDecode(await file.readAsString()));
    } catch (_) {
      return const AppSettings();
    }
  }

  /// Persist [settings].
  ///
  /// No forced disk sync: a lost preference costs one more trip to the picker,
  /// while a sync on every tap makes the UI feel sticky.
  Future<void> save(AppSettings settings) async {
    try {
      final File file = await _file();
      await file.parent.create(recursive: true);
      await file.writeAsString(jsonEncode(settings.toJson()));
    } catch (_) {
      // Writing settings must never surface an error the user cannot act on.
    }
  }
}
