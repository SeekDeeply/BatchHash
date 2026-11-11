import 'dart:io';

/// Version the UI reports.
///
/// A compile-time constant because reading the real package version would need
/// a package_info plugin; bump it together with `version:` in pubspec.yaml so
/// the About page and the first-run record never disagree.
const String kAppVersion = '1.4.2';

/// Project repository the Star buttons point at.
const String kProjectUrl = 'https://github.com/SeekDeeply/BatchHash';

/// Open [kProjectUrl] with the platform's default handler.
///
/// There is deliberately no url_launcher dependency in pubspec.yaml, so desktop
/// platforms shell out to their opener. Mobile has no equivalent command, so the
/// caller gets `false` and can hand the URL over another way instead of
/// showing a button that silently does nothing.
Future<bool> openProjectUrl() async {
  if (!(Platform.isLinux || Platform.isMacOS || Platform.isWindows)) {
    return false;
  }
  try {
    final ProcessResult result;
    if (Platform.isMacOS) {
      result = await Process.run('open', <String>[kProjectUrl]);
    } else if (Platform.isWindows) {
      // The empty argument is `start`'s window-title slot.
      result = await Process.run('cmd', <String>[
        '/c',
        'start',
        '',
        kProjectUrl,
      ]);
    } else {
      result = await Process.run('xdg-open', <String>[kProjectUrl]);
    }
    return result.exitCode == 0;
  } catch (_) {
    // A headless host without a browser cannot open anything; `false` lets the
    // caller fall back to showing the link.
    return false;
  }
}
