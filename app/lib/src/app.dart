import 'package:dynamic_color/dynamic_color.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'app_state.dart';
import 'l10n/app_strings.dart';
import 'l10n/app_strings_scope.dart';
import 'l10n/locale_controller.dart';
import 'pages/compare_page.dart';
import 'pages/hash_page.dart';
import 'pages/history_page.dart';
import 'pages/rename_page.dart';
import 'pages/report_page.dart';
import 'pages/settings_page.dart';
import 'services/batch_hash_service.dart';
import 'services/fake_batch_hash_service.dart';
import 'services/rust_batch_hash_service.dart';
import 'theme.dart';
import 'widgets/adaptive_scaffold.dart';
import 'widgets/demo_data_banner.dart';

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
class BatchHashApp extends StatefulWidget {
  const BatchHashApp({
    super.key,
    this.service,
    this.enableDynamicColor = true,
    this.locale,
  });

  final BatchHashService? service;
  final bool enableDynamicColor;

  /// Initial UI language. Defaults to Chinese; tests pin it to read fixed
  /// copy.
  final String? locale;

  @override
  State<BatchHashApp> createState() => _BatchHashAppState();
}

class _BatchHashAppState extends State<BatchHashApp> {
  late final BatchHashService _service;
  late final BatchHashAppState _appState;
  late final LocaleController _localeController;
  late final List<AppSection> _sections;
  late final bool _usingDemoData;
  int _selectedIndex = 0;

  @override
  void initState() {
    super.initState();
    final RustBatchHashService? native = widget.service == null
        ? RustBatchHashService.tryCreate()
        : null;
    _service = widget.service ?? native ?? FakeBatchHashService();
    _usingDemoData = native == null && widget.service == null;
    _localeController = LocaleController(initial: widget.locale);
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
        key: 'nav.history',
        icon: Icons.history,
        build: () => HistoryPage(service: _service),
      ),
      AppSection(
        key: 'nav.settings',
        icon: Icons.settings,
        build: () => SettingsPage(
          service: _service,
          locale: _localeController.locale,
          onLocaleChanged: _localeController.select,
        ),
      ),
    ];
  }

  @override
  void dispose() {
    _localeController.dispose();
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
      listenable: _localeController,
      builder: (BuildContext context, Widget? child) {
        final String locale = _localeController.locale;
        return MaterialApp(
          title: AppStrings.of('app.title', locale: locale),
          debugShowCheckedModeBanner: false,
          theme: AppTheme.light(lightDynamic),
          darkTheme: AppTheme.dark(darkDynamic),
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
          home: _Home(
            sections: _sections,
            selectedIndex: _selectedIndex,
            usingDemoData: _usingDemoData,
            onDestinationSelected: (int index) {
              setState(() {
                _selectedIndex = index;
              });
            },
          ),
        );
      },
    );
  }
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
