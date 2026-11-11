import 'package:flutter/material.dart';

import 'src/app.dart';
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
  runApp(const BatchHashApp());
}
