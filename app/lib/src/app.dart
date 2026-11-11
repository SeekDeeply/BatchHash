import 'dart:async';

import 'package:dynamic_color/dynamic_color.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'app_state.dart';
import 'l10n/app_strings.dart';
import 'l10n/app_strings_scope.dart';
import 'l10n/locale_controller.dart';
import 'pages/compare_page.dart';
import 'pages/hash_page.dart';
import 'pages/rename_page.dart';
import 'pages/report_page.dart';
import 'pages/settings_page.dart';
import 'services/batch_hash_service.dart';
import 'services/fake_batch_hash_service.dart';
import 'services/first_run_store.dart';
import 'services/logging_batch_hash_service.dart';
import 'services/operation_log_store.dart';
import 'services/rust_batch_hash_service.dart';
import 'theme.dart';
import 'theme_controller.dart';
import 'widgets/adaptive_scaffold.dart';
import 'widgets/demo_data_banner.dart';
import 'widgets/first_run_dialog.dart';

/// A navigation destination with a translation key instead of a literal label.
class AppSection {
  const AppSection({
    required this.key,
    required this.icon,
    required this.build,
  });

  /// Translation key resolved against [AppStringsScope].
  final String key;
  final IconData icon;
  final Widget Function() build;
}

/// Root of the BatchHash Flutter shell.
///
/// A single [BatchHashService] is injected here. When none is supplied the app
/// tries the native Rust bridge first and falls back to the deterministic fake,
/// which keeps widget tests and platforms without the library working. The shell
/// is Chinese-first ([AppStrings.defaultLocale]) and always surfaces a banner
/// when it is running on demo data, so a packaging mistake can never look like
/// real hashing.
///
/// The real app also gates the first launch behind [FirstRunDialog]. An
/// injected [service] disables that gate, so tests and embedders never get a
/// modal they did not ask for.
class BatchHashApp extends StatefulWidget {
  const BatchHashApp({
    super.key,
    this.service,
    this.enableDynamicColor = true,
    this.locale,
    this.firstRunStore,
    this.onDecline,
  });

  final BatchHashService? service;
  final bool enableDynamicColor;

  /// Initial UI language. Defaults to Chinese; tests pin it to read fixed
  /// copy.
  final String? locale;

  /// Source of the first-run acceptance; injectable so a test can point at a
  /// temporary directory. Defaults to a [FirstRunStore].
  final FirstRunStore? firstRunStore;

  /// Called when the user declines the first-run terms.
  ///
  /// Tests inject a callback here: the default leaves the app through
  /// [SystemNavigator.pop] and would tear the test host down.
  final VoidCallback? onDecline;

  @override
  State<BatchHashApp> createState() => _BatchHashAppState();
}

class _BatchHashAppState extends State<BatchHashApp> {
  late final BatchHashService _service;
  late final BatchHashAppState _appState;
  late final LocaleController _localeController;
  late final ThemeController _themeController;
  late final OperationLogStore _logStore;
  late final List<AppSection> _sections;
  late final FirstRunStore _firstRunStore;
  late final bool _usingDemoData;
  int _selectedIndex = 0;

  @override
  void initState() {
    super.initState();
    _firstRunStore = widget.firstRunStore ?? FirstRunStore();
    final RustBatchHashService? native = widget.service == null
        ? RustBatchHashService.tryCreate()
        : null;
    _usingDemoData = native == null && widget.service == null;
    final BatchHashService core = widget.service ?? native ?? FakeBatchHashService();
    // A test injects its own service and must not have a side effect on the
    // real support directory, so history is only recorded for the app's own
    // wiring. The decorator is transparent: pages still see a plain service.
    _logStore = OperationLogStore();
    _service = widget.service == null
        ? LoggingBatchHashService(inner: core, store: _logStore)
        : core;
    _localeController = LocaleController(initial: widget.locale);
    _themeController = ThemeController();
    _appState = BatchHashAppState(service: _service);
    _sections = <AppSection>[
      AppSection(
        key: 'nav.hash',
        icon: Icons.fingerprint,
        build: () => HashPage(state: _appState),
      ),
      AppSection(
        key: 'nav.rename',
        icon: Icons.drive_file_rename_outline,
        build: () => RenamePage(state: _appState),
      ),
      AppSection(
        key: 'nav.report',
        icon: Icons.description,
        build: () => ReportPage(state: _appState),
      ),
      AppSection(
        key: 'nav.compare',
        icon: Icons.compare_arrows,
        build: () => ComparePage(state: _appState),
      ),
      AppSection(
        key: 'nav.settings',
        icon: Icons.settings,
        build: () => SettingsPage(
          service: _service,
          locale: _localeController.locale,
          onLocaleChanged: _localeController.select,
          theme: _themeController,
          logStore: _logStore,
        ),
      ),
    ];
  }

  @override
  void dispose() {
    _localeController.dispose();
    _themeController.dispose();
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
    return ListenableBuilder(
      // Both controllers drive the shell: the locale picks copy, the theme
      // controller picks the seed the color scheme is derived from.
      listenable: Listenable.merge(<Listenable>[
        _localeController,
        _themeController,
      ]),
      builder: (BuildContext context, Widget? child) {
        final String locale = _localeController.locale;
        final Color seed = _themeController.seed;
        return MaterialApp(
          title: AppStrings.of('app.title', locale: locale),
          debugShowCheckedModeBanner: false,
          theme: AppTheme.light(lightDynamic, seed: seed),
          darkTheme: AppTheme.dark(darkDynamic, seed: seed),
          themeMode: ThemeMode.system,
          locale: Locale(locale),
          supportedLocales: <Locale>[
            for (final String code in AppStrings.supportedLocales) Locale(code),
          ],
          localizationsDelegates: const <LocalizationsDelegate<dynamic>>[
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          builder: (BuildContext context, Widget? child) {
            return AppStringsScope.wrap(
              locale: locale,
              child: child ?? const SizedBox.shrink(),
            );
          },
          home: _buildHome(),
        );
      },
    );
  }

  /// The home route, wrapped in the first-run gate for the real app only.
  Widget _buildHome() {
    final Widget home = _Home(
      sections: _sections,
      selectedIndex: _selectedIndex,
      usingDemoData: _usingDemoData,
      onDestinationSelected: (int index) {
        setState(() {
          _selectedIndex = index;
        });
      },
    );
    if (widget.service != null) {
      return home;
    }
    return _FirstRunGate(
      store: _firstRunStore,
      onDecline: _declineFirstRun,
      child: home,
    );
  }

  Future<void> _declineFirstRun() async {
    final VoidCallback? onDecline = widget.onDecline;
    if (onDecline != null) {
      onDecline();
      return;
    }
    // The default leaves the app; tests inject onDecline instead.
    await SystemNavigator.pop();
  }
}

/// Runs the first-run gate once, from a context below the Navigator.
///
/// It sits inside `home` on purpose: `MaterialApp.builder` is above the
/// Navigator, where `showDialog` has no navigator to push a route onto.
class _FirstRunGate extends StatefulWidget {
  const _FirstRunGate({
    required this.store,
    required this.onDecline,
    required this.child,
  });

  final FirstRunStore store;
  final VoidCallback onDecline;
  final Widget child;

  @override
  State<_FirstRunGate> createState() => _FirstRunGateState();
}

class _FirstRunGateState extends State<_FirstRunGate> {
  bool _checked = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      unawaited(_maybeShow());
    });
  }

  Future<void> _maybeShow() async {
    if (_checked) {
      return;
    }
    _checked = true;
    // Best effort: an unreadable store reports "not accepted", which is the
    // safe direction for a terms gate.
    final bool accepted = await widget.store.hasAccepted();
    if (!mounted || accepted) {
      return;
    }
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (BuildContext context) => FirstRunDialog(
        onAccept: _accept,
        onDecline: widget.onDecline,
      ),
    );
  }

  void _accept() {
    unawaited(widget.store.markAccepted());
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

/// Themed shell around the adaptive scaffold.
class _Home extends StatelessWidget {
  const _Home({
    required this.sections,
    required this.selectedIndex,
    required this.usingDemoData,
    required this.onDestinationSelected,
  });

  final List<AppSection> sections;
  final int selectedIndex;
  final bool usingDemoData;
  final ValueChanged<int> onDestinationSelected;

  @override
  Widget build(BuildContext context) {
    final List<AppDestination> destinations = <AppDestination>[
      for (final AppSection section in sections)
        AppDestination(
          label: context.strings(section.key),
          icon: section.icon,
          selectedIcon: section.icon,
          page: section.build(),
        ),
    ];
    return AdaptiveScaffold(
      destinations: destinations,
      selectedIndex: selectedIndex,
      onDestinationSelected: onDestinationSelected,
      banner: usingDemoData
          ? DemoDataBanner(
              message: context.strings('demo.banner'),
              actionLabel: context.strings('demo.detail'),
              title: context.strings('demo.dialogTitle'),
              body: context.strings('demo.dialogBody'),
              remedy: context.strings('demo.howToFix'),
            )
          : null,
    );
  }
}
