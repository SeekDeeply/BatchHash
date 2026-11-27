import 'package:batchhash/src/util/platform_pickers.dart';
import 'package:flutter_test/flutter_test.dart';

/// The picker guard is what stops the window from accepting a second action
/// while a system dialog is open.
///
/// A desktop file dialog is not modal to Flutter: without this guard the user
/// could open a second dialog, start a run, or switch tabs while the first one
/// was still waiting to return.
void main() {
  tearDown(() {
    PlatformPickers.picking.value = false;
  });

  test('no picker is open before anything is asked for', () {
    expect(PlatformPickers.picking.value, isFalse);
  });

  test('a cancelled picker still clears the guard', () async {
    // The guard is set and cleared with try/finally inside the picker, so a
    // cancel (which returns null rather than throwing) must not leave the app
    // permanently blocked. This drives the real entry point on a platform with
    // no plugin, where it resolves through the fallback path.
    final String? picked = await PlatformPickers.directory(
      () async => null,
    );
    expect(picked, isNull);
    // Blocked input after a cancel would be worse than the double-dialog the
    // guard exists to prevent.
    expect(PlatformPickers.picking.value, isFalse);
  });
}
