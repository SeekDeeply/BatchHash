import 'package:flutter/material.dart';

import 'src/app.dart';
import 'src/services/app_settings_store.dart';
import 'src/services/window_state_controller.dart';
import 'src/services/window_state_store.dart';

Future<void> main() async {
  // The window is sized before the first frame, so the platform channel has to
  // be ready; without this the geometry call throws and the window opens at
  // whatever size the runner happened to generate.
  WidgetsFlutterBinding.ensureInitialized();
  final WindowStateController window = WindowStateController(
    store: WindowStateStore(),
  );
  // Awaited so the restored size is applied before the first frame; attach()
  // swallows its own failures, so a missing window manager cannot block start.
  await window.attach();
  // Read the saved preferences before the first frame for the same reason:
  // loading them later made the UI paint the defaults and then swap, and the
  // algorithm selection never converged because that page copies it once.
  // load() degrades to empty settings on any failure, so this cannot block start.
  final AppSettingsStore settingsStore = AppSettingsStore();
  final AppSettings settings = await settingsStore.load();
  runApp(BatchHashApp(settingsStore: settingsStore, initialSettings: settings));
}
