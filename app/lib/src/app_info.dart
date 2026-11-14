import 'dart:io';

/// Version the UI reports.
///
/// A compile-time constant because reading the real package version would need
/// a package_info plugin; bump it together with `version:` in pubspec.yaml so
/// the About page and the first-run record never disagree.
const String kAppVersion = '1.11.1';

/// Project repository the Star buttons point at.
const String kProjectUrl = 'https://github.com/SeekDeeply/BatchHash';

/// Collaborators credited on the About page.
///
/// Only the GitHub handle is stored; the profile URL is derived, so adding a
/// name never means keeping two strings in sync. Roles are i18n keys rather
/// than free text so the credit reads in the same language as the rest of the
/// UI.
const List<({String handle, List<String> roleKeys})> kCollaborators =
    <({String handle, List<String> roleKeys})>[
  (
    handle: 'IPF-Sinon',
    roleKeys: <String>[
      'about.role.devEnv',
      'about.role.beta',
      'about.role.funding',
    ],
  ),
];

/// Profile page for a GitHub [handle].
String githubProfileUrl(String handle) => 'https://github.com/$handle';

/// Open [url] with the platform's default handler.
///
/// [url] defaults to the project repository; the collaborators list passes a
/// profile URL through the same path so both share one fallback story.
///
/// There is deliberately no url_launcher dependency in pubspec.yaml, so desktop
/// platforms shell out to their opener. Mobile has no equivalent command, so the
/// caller gets `false` and can hand the URL over another way instead of
/// showing a button that silently does nothing.
Future<bool> openProjectUrl({String url = kProjectUrl}) async {
  if (!(Platform.isLinux || Platform.isMacOS || Platform.isWindows)) {
    return false;
  }
  try {
    final ProcessResult result;
    if (Platform.isMacOS) {
      result = await Process.run('open', <String>[url]);
    } else if (Platform.isWindows) {
      // The empty argument is `start`'s window-title slot.
      result = await Process.run('cmd', <String>[
        '/c',
        'start',
        '',
        url,
      ]);
    } else {
      result = await Process.run('xdg-open', <String>[url]);
    }
    return result.exitCode == 0;
  } catch (_) {
    // A headless host without a browser cannot open anything; `false` lets the
    // caller fall back to showing the link.
    return false;
  }
}
