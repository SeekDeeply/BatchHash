import 'package:batchhash/src/util/platform_pickers.dart';
import 'package:flutter_test/flutter_test.dart';

/// The picker guard is what stops the window from accepting a second action
/// while a system dialog is open.
///
/// A desktop file dialog is not modal to Flutter: without this guard the user
/// could open a second dialog, start a run, or switch tabs while the first one
/// was still waiting to return.
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  tearDown(() {
    PlatformPickers.picking.value = false;
  });

  test('no picker is open before anything is asked for', () {
    expect(PlatformPickers.picking.value, isFalse);
  });

  test('a cancelled picker still clears the guard', () async {
    // The guard is set and cleared inside the picker, so a cancel (which returns
    // null rather than throwing) must not leave the app permanently blocked.
    final String? picked = await PlatformPickers.directory(() async => null);
    expect(picked, isNull);
    // The release is deferred by a frame to drain the mouse-up that dismissed
    // the dialog, so it is not observable on the same microtask as the return.
    await Future<void>.delayed(Duration.zero);
    // Blocked input after a cancel would be worse than the double-dialog the
    // guard exists to prevent.
    expect(PlatformPickers.picking.value, isFalse);
  });

  test('the guard is held while the picker runs', () async {
    // Observed from inside the picker, which is the only moment the guard has to
    // be up: a page that took a tap between "dialog opened" and "dialog closed"
    // is the failure this exists to prevent.
    bool? duringPick;
    await PlatformPickers.directory(() async {
      duringPick = PlatformPickers.picking.value;
      return null;
    });
    expect(duringPick, isTrue);
    await Future<void>.delayed(Duration.zero);
    expect(PlatformPickers.picking.value, isFalse);
  });

  test('a nested picker does not clear the guard early', () async {
    // The directory picker can fall back to the app's own dialog, which runs a
    // second picker nested inside the first. A plain boolean would flip back to
    // false when the inner one finished, unblocking input while the outer was
    // still open; the counter is what makes that impossible.
    bool? innerSawGuard;
    await PlatformPickers.directory(() async {
      await PlatformPickers.directory(() async {
        innerSawGuard = PlatformPickers.picking.value;
        return null;
      });
      return null;
    });
    expect(innerSawGuard, isTrue);
    await Future<void>.delayed(Duration.zero);
    expect(PlatformPickers.picking.value, isFalse);
  });
}
