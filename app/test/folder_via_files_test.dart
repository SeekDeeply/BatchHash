import 'package:batchhash/src/util/platform_pickers.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  // Android cannot return an openable folder from its picker, so the app asks
  // for files and derives the folder. These cases pin that derivation, because
  // getting it wrong means scanning a path that does not exist and silently
  // showing an empty list — the exact symptom being fixed.
  group('commonParent', () {
    test('files in one directory resolve to that directory', () {
      expect(
        PlatformPickers.commonParent(<String>[
          '/storage/emulated/0/Download/a.txt',
          '/storage/emulated/0/Download/b.txt',
        ]),
        '/storage/emulated/0/Download',
      );
    });

    test('files in different subdirectories resolve to the shared root', () {
      expect(
        PlatformPickers.commonParent(<String>[
          '/storage/emulated/0/Download/x/a.txt',
          '/storage/emulated/0/Download/y/b.txt',
        ]),
        '/storage/emulated/0/Download',
      );
    });

    test('a single file resolves to its own directory', () {
      expect(
        PlatformPickers.commonParent(<String>['/sdcard/movie.mkv']),
        '/sdcard',
      );
    });

    test('the same file twice still resolves to its directory', () {
      expect(
        PlatformPickers.commonParent(<String>[
          '/sdcard/clip.mp4',
          '/sdcard/clip.mp4',
        ]),
        '/sdcard',
      );
    });

    test('paths with no shared directory yield an empty string', () {
      // The caller treats this as "no usable folder" rather than scanning the
      // filesystem root, which would be a very expensive surprise.
      expect(
        PlatformPickers.commonParent(<String>[
          '/sdcard/a.txt',
          '/storage/b.txt',
        ]),
        isEmpty,
      );
    });

    test('an empty list yields an empty string', () {
      expect(PlatformPickers.commonParent(const <String>[]), isEmpty);
    });
  });
}
