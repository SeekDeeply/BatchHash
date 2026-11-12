import 'dart:io';

import 'package:batchhash/src/services/app_settings_store.dart';
import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late Directory temp;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('batchhash-settings-test');
  });

  tearDown(() async {
    if (await temp.exists()) {
      await temp.delete(recursive: true);
    }
  });

  AppSettingsStore store() =>
      AppSettingsStore(supportDirectory: () async => temp);

  group('AppSettings', () {
    test('round-trips every field through JSON', () {
      const AppSettings settings = AppSettings(
        locale: 'en',
        seedArgb: 0xFF29B6F6,
        dynamicColor: false,
        algorithms: <String>['blake3', 'sha256'],
      );
      final AppSettings decoded = AppSettings.fromJson(
        settings.toJson(),
      );
      expect(decoded.locale, 'en');
      expect(decoded.seedArgb, 0xFF29B6F6);
      expect(decoded.dynamicColor, isFalse);
      expect(decoded.algorithms, <String>['blake3', 'sha256']);
      expect(decoded.seed, const Color(0xFF29B6F6));
    });

    test('omits null fields so a partial file stays partial', () {
      expect(const AppSettings().toJson(), isEmpty);
      expect(const AppSettings(locale: 'zh').toJson(), <String, Object?>{
        'locale': 'zh',
      });
    });

    test('a corrupt field does not discard the others', () {
      // Losing the whole file because one field is the wrong type is what a
      // user would experience as "my settings reset again".
      final AppSettings decoded = AppSettings.fromJson(<String, Object?>{
        'locale': 'en',
        'seed_argb': 'not-a-number',
        'dynamic_color': 42,
        'algorithms': <Object?>['blake3', 7, ''],
      });
      expect(decoded.locale, 'en');
      expect(decoded.seedArgb, isNull);
      expect(decoded.dynamicColor, isNull);
      expect(decoded.algorithms, <String>['blake3']);
    });

    test('a non-map payload falls back to defaults', () {
      expect(AppSettings.fromJson(null).locale, isNull);
      expect(AppSettings.fromJson('garbage').seedArgb, isNull);
    });
  });

  group('AppSettingsStore', () {
    test('returns defaults before anything was saved', () async {
      final AppSettings loaded = await store().load();
      expect(loaded.locale, isNull);
      expect(loaded.seedArgb, isNull);
    });

    test('persists across store instances', () async {
      await store().save(
        const AppSettings(locale: 'en', seedArgb: 0xFFB3261E),
      );
      final AppSettings loaded = await store().load();
      expect(loaded.locale, 'en');
      expect(loaded.seedArgb, 0xFFB3261E);
    });

    test('a corrupt file reads as defaults instead of throwing', () async {
      await File('${temp.path}/settings.json').writeAsString('{not json');
      final AppSettings loaded = await store().load();
      expect(loaded.locale, isNull);
    });

    test('copyWith keeps fields it was not given', () {
      const AppSettings base = AppSettings(
        locale: 'zh',
        seedArgb: 0xFF00658F,
        algorithms: <String>['sha1'],
      );
      final AppSettings updated = base.copyWith(seedArgb: 0xFF3F51B5);
      expect(updated.locale, 'zh');
      expect(updated.seedArgb, 0xFF3F51B5);
      expect(updated.algorithms, <String>['sha1']);
    });
  });
}
