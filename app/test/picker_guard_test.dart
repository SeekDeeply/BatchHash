import 'package:batchhash/src/util/platform_pickers.dart';
import 'package:flutter_test/flutter_test.dart';

/// The picker guard is what stops the window from accepting a second action
/// while a system dialog is open.
///
/// A desktop file dialog is not modal to Flutter: without this guard the user
/// could open a second dialog, start a run, or switch tabs while the first one
/// was still waiting to return. The OS-level modality comes from
/// lockParentWindow; this guard covers the Dart side.
///
/// Note what the guard does *not* cover: the app's own fallback dialog runs with
/// the guard deliberately released, because that dialog is a route which already
/// blocks the page underneath it, and holding the guard would block the dialog
/// own buttons.
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  tearDown(() {
    PlatformPickers.picking.value = false;
  });

  test('no picker is open before anything is asked for', () {
    expect(PlatformPickers.picking.value, isFalse);
  });

  test('a cancelled picker still clears the guard', () async {
    // A cancel returns null rather than throwing, and must not leave the app
    // permanently blocked.
    final String? picked = await PlatformPickers.directory(() async => null);
    expect(picked, isNull);
    // The release is deferred by a frame to drain the mouse-up that dismissed
    // the dialog, so it is not observable on the same microtask as the return.
    await Future<void>.delayed(Duration.zero);
    expect(PlatformPickers.picking.value, isFalse);
  });

  test('the guard is released before the fallback dialog runs', () async {
    // The fallback is an in-app dialog, not a system one. Leaving the guard up
    // would block the dialog's own buttons, which is worse than the problem the
    // guard solves.
    bool? guardDuringFallback;
    await PlatformPickers.directory(() async {
      guardDuringFallback = PlatformPickers.picking.value;
      return null;
    });
    expect(guardDuringFallback, isFalse);
    await Future<void>.delayed(Duration.zero);
    // And it is restored to "off" once everything unwinds.
    expect(PlatformPickers.picking.value, isFalse);
  });
}
