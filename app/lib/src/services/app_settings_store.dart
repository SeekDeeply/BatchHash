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
    this.dynamicColor,
    this.algorithms,
  });

  /// UI language code, `zh` or `en`; null means never chosen.
  final String? locale;

  /// Accent seed as 0xAARRGGBB; null means never chosen.
  final int? seedArgb;

  /// Whether wallpaper dynamic colour is allowed when no accent was chosen.
  final bool? dynamicColor;

  /// Algorithms the user last selected.
  final List<String>? algorithms;

  /// Accent seed as a [Color], or null when nothing was stored.
  Color? get seed => seedArgb == null ? null : Color(seedArgb!);

  AppSettings copyWith({
    String? locale,
    int? seedArgb,
    bool? dynamicColor,
    List<String>? algorithms,
  }) {
    return AppSettings(
      locale: locale ?? this.locale,
      seedArgb: seedArgb ?? this.seedArgb,
      dynamicColor: dynamicColor ?? this.dynamicColor,
      algorithms: algorithms ?? this.algorithms,
    );
  }

  Map<String, Object?> toJson() => <String, Object?>{
    if (locale != null) 'locale': locale,
    if (seedArgb != null) 'seed_argb': seedArgb,
    if (dynamicColor != null) 'dynamic_color': dynamicColor,
    if (algorithms != null) 'algorithms': algorithms,
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
    final Object? dynamic = raw['dynamic_color'];
    final Object? algorithms = raw['algorithms'];
    return AppSettings(
      locale: locale is String && locale.trim().isNotEmpty
          ? locale.trim()
          : null,
      seedArgb: seed is int ? seed : null,
      dynamicColor: dynamic is bool ? dynamic : null,
      algorithms: algorithms is List
          ? <String>[
              for (final Object? entry in algorithms)
                if (entry is String && entry.trim().isNotEmpty) entry.trim(),
            ]
          : null,
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
