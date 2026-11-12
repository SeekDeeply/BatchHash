import 'dart:async';

import 'package:dynamic_color/dynamic_color.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'app_state.dart';
import 'l10n/app_strings.dart';
import 'l10n/app_strings_scope.dart';
import 'l10n/locale_controller.dart';
import 'models/models.dart';
import 'pages/compare_page.dart';
import 'pages/hash_page.dart';
import 'pages/rename_page.dart';
import 'pages/report_page.dart';
import 'pages/settings_page.dart';
import 'services/app_settings_store.dart';
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
    this.settingsStore,
  });

  final BatchHashService? service;
  final bool enableDynamicColor;

  /// Initial UI language. Defaults to Chinese; tests pin it to read fixed
  /// copy.
  final String? locale;

  /// Source of the first-run acceptance; injectable so a test can point at a
  /// temporary directory. Defaults to a [FirstRunStore].
  final FirstRunStore? firstRunStore;

  /// Where language and accent are remembered; injectable for tests.
  final AppSettingsStore? settingsStore;

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
  late final AppSettingsStore _settingsStore;
  late final bool _usingDemoData;

  /// Tail of the settings write chain; see [_persistSettings].
  Future<void> _writeQueue = Future<void>.value();

  /// True while the stored payload is being applied.
  ///
  /// Restoring the algorithm selection notifies its listeners, which would fire
  /// a write of the very values just read. Skipping saves during restoration
  /// keeps start-up from rewriting the file it only just read.
  bool _restoring = false;
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
    _settingsStore = widget.settingsStore ?? AppSettingsStore();
    _localeController = LocaleController(initial: widget.locale);
    _themeController = ThemeController();
    // _appState must exist before anything below reads its notifiers; touching
    // it earlier threw a LateInitializationError at start-up.
    _appState = BatchHashAppState(service: _service);
    // Load persisted choices, then mirror every later change back to disk.
    unawaited(_restoreSettings());
    _localeController.addListener(_persistSettings);
    _themeController.addListener(_persistSettings);
    // The picker writes straight into this notifier, so persistence has to hang
    // off the notifier rather than off the picker: a future caller that changes
    // the selection another way must still be remembered.
    _appState.algorithms.addListener(_persistSettings);
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

  /// Apply the stored language and accent once at start-up.
  ///
  /// An explicitly injected [BatchHashApp.locale] wins over the stored value:
  /// tests and embedders pin the language, and a leftover file must not
  /// override that.
  Future<void> _restoreSettings() async {
    final AppSettings stored = await _settingsStore.load();
    if (!mounted) {
      return;
    }
    _restoring = true;
    if (widget.locale == null) {
      _localeController.restore(stored.locale);
    }
    _themeController.restore(stored.seed);
    final List<String>? storedAlgorithms = stored.algorithms;
    if (storedAlgorithms != null && storedAlgorithms.isNotEmpty) {
      // Filtered against the live catalogue: an id that a previous version
      // offered but this build no longer has would otherwise fail the whole
      // hashing call instead of just being dropped.
      final Set<String> available = <String>{
        for (final AlgorithmInfo algorithm in kAlgorithmCatalogue)
          algorithm.id,
      };
      final Set<String> kept = <String>{
        for (final String id in storedAlgorithms)
          if (available.contains(id)) id,
      };
      if (kept.isNotEmpty) {
        _appState.algorithms.value = kept;
      }
    }
    _restoring = false;
    // Rebuild once so the first painted frame already uses the stored values.
    setState(() {});
  }

  /// Write the current language, accent and selection back to disk.
  ///
  /// Read-modify-write on purpose: this file also holds fields this widget does
  /// not own, and rewriting the payload from a stale snapshot would erase them.
  ///
  /// Chained onto [_writeQueue] so two quick changes cannot interleave:
  /// switching the language and the accent in the same second used to start two
  /// overlapping cycles on one file.
  Future<void> _persistSettings() {
    if (_restoring) {
      return _writeQueue;
    }
    _writeQueue = _writeQueue.then((_) async {
      final AppSettings stored = await _settingsStore.load();
      await _settingsStore.save(
        stored.copyWith(
          locale: _localeController.locale,
          seedArgb: _themeController.seed.value,
          algorithms: _appState.algorithms.value.toList()..sort(),
        ),
      );
    });
    return _writeQueue;
  }

  @override
  void dispose() {
    _localeController.removeListener(_persistSettings);
    _themeController.removeListener(_persistSettings);
    _appState.algorithms.removeListener(_persistSettings);
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
    return _FirstRunGate(store: _firstRunStore, child: home);
  }

}

/// Runs the first-run gate once, from a context below the Navigator.
///
/// It sits inside `home` on purpose: `MaterialApp.builder` is above the
/// Navigator, where `showDialog` has no navigator to push a route onto.
class _FirstRunGate extends StatefulWidget {
  const _FirstRunGate({required this.store, required this.child});

  final FirstRunStore store;
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
      builder: (BuildContext context) => FirstRunDialog(onAccept: _accept),
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
