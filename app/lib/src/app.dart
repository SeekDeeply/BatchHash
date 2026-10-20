import 'package:dynamic_color/dynamic_color.dart';
import 'package:flutter/material.dart';

import 'app_state.dart';
import 'pages/compare_page.dart';
import 'pages/hash_page.dart';
import 'pages/history_page.dart';
import 'pages/rename_page.dart';
import 'pages/report_page.dart';
import 'pages/settings_page.dart';
import 'services/batch_hash_service.dart';
import 'services/fake_batch_hash_service.dart';
import 'theme.dart';
import 'widgets/adaptive_scaffold.dart';

/// Root of the BatchHash Flutter shell.
///
/// A single [BatchHashService] is injected here. When none is supplied the
/// deterministic fake is used, which keeps the P0 preview and the widget tests
/// independent of native code. Dynamic color always falls back to a seeded
/// Material 3 scheme.
class BatchHashApp extends StatefulWidget {
  const BatchHashApp({
    super.key,
    this.service,
    this.enableDynamicColor = true,
  });

  final BatchHashService? service;
  final bool enableDynamicColor;

  @override
  State<BatchHashApp> createState() => _BatchHashAppState();
}

class _BatchHashAppState extends State<BatchHashApp> {
  late final BatchHashService _service;
  late final BatchHashAppState _appState;
  late final List<AppDestination> _destinations;
  int _selectedIndex = 0;

  @override
  void initState() {
    super.initState();
    _service = widget.service ?? FakeBatchHashService();
    _appState = BatchHashAppState(service: _service);
    _destinations = <AppDestination>[
      AppDestination(
        label: 'Hash',
        icon: Icons.fingerprint,
        selectedIcon: Icons.fingerprint,
        page: HashPage(state: _appState),
      ),
      AppDestination(
        label: 'Rename',
        icon: Icons.drive_file_rename_outline,
        selectedIcon: Icons.drive_file_rename_outline,
        page: RenamePage(state: _appState),
      ),
      AppDestination(
        label: 'Report',
        icon: Icons.description,
        selectedIcon: Icons.description,
        page: ReportPage(state: _appState),
      ),
      AppDestination(
        label: 'Compare',
        icon: Icons.compare_arrows,
        selectedIcon: Icons.compare_arrows,
        page: ComparePage(state: _appState),
      ),
      AppDestination(
        label: 'History',
        icon: Icons.history,
        selectedIcon: Icons.history,
        page: HistoryPage(service: _service),
      ),
      AppDestination(
        label: 'Settings',
        icon: Icons.settings,
        selectedIcon: Icons.settings,
        page: SettingsPage(service: _service),
      ),
    ];
  }

  @override
  void dispose() {
    _appState.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.enableDynamicColor) {
      return _buildApp(null, null);
    }
    return DynamicColorBuilder(
      builder: (ColorScheme? lightDynamic, ColorScheme? darkDynamic) {
        return _buildApp(lightDynamic, darkDynamic);
      },
    );
  }

  Widget _buildApp(ColorScheme? lightDynamic, ColorScheme? darkDynamic) {
    return MaterialApp(
      title: 'BatchHash',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(lightDynamic),
      darkTheme: AppTheme.dark(darkDynamic),
      themeMode: ThemeMode.system,
      home: AdaptiveScaffold(
        destinations: _destinations,
        selectedIndex: _selectedIndex,
        onDestinationSelected: (int index) {
          setState(() {
            _selectedIndex = index;
          });
        },
      ),
    );
  }
}
