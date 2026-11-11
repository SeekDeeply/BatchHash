import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../app_info.dart';
import 'app_config_store.dart' show SupportDirectoryResolver;

/// Remembers whether the user accepted the first-run terms.
///
/// The acceptance is a small JSON file in the application support directory, so
/// a normal restart does not ask again. Every failure degrades to "not
/// accepted": asking twice is a small annoyance, silently skipping the terms
/// once is a legal problem.
class FirstRunStore {
  FirstRunStore({
    SupportDirectoryResolver? supportDirectory,
    this.fileName = 'first_run.json',
  }) : _supportDirectory = supportDirectory;

  /// Where the support directory comes from; overridable so tests can point at
  /// a temporary directory without the path_provider platform channel.
  final SupportDirectoryResolver? _supportDirectory;

  /// Name of the JSON file inside the support directory.
  final String fileName;

  /// The store file, or null when the platform cannot provide a directory.
  Future<File?> _file() async {
    try {
      final SupportDirectoryResolver resolver =
          _supportDirectory ?? getApplicationSupportDirectory;
      final Directory directory = await resolver();
      if (!directory.existsSync()) {
        directory.createSync(recursive: true);
      }
      return File('${directory.path}/$fileName');
    } on Exception catch (_) {
      return null;
    } catch (_) {
      // path_provider throws an Error, not an Exception, before any binding
      // exists; a pure Dart test would otherwise crash on an optional feature.
      return null;
    }
  }

  /// True only when a well-formed acceptance record is present.
  Future<bool> hasAccepted() async {
    final File? file = await _file();
    if (file == null || !file.existsSync()) {
      return false;
    }
    try {
      final Object? decoded = jsonDecode(await file.readAsString());
      if (decoded is! Map<String, Object?>) {
        return false;
      }
      return decoded['accepted'] == true;
    } on Exception catch (_) {
      // A truncated or hand-edited file means "ask again".
      return false;
    } catch (_) {
      return false;
    }
  }

  /// Record the acceptance with the version the user agreed to.
  Future<void> markAccepted() async {
    final File? file = await _file();
    if (file == null) {
      return;
    }
    final Map<String, Object?> payload = <String, Object?>{
      'accepted': true,
      'accepted_at': DateTime.now().toUtc().toIso8601String(),
      // Kept for auditing: which release's terms were accepted.
      'version': kAppVersion,
    };
    try {
      await file.writeAsString(jsonEncode(payload), flush: true);
    } on Exception catch (_) {
      // Best effort; the next launch simply asks again.
    } catch (_) {
      // Same reason as _file(): platform failures surface as Errors.
    }
  }
}
