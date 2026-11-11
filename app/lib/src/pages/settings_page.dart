import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../services/batch_hash_service.dart';
import '../services/operation_log_store.dart';
import '../theme.dart';
import '../theme_controller.dart';
import '../widgets/accent_color_card.dart';
import '../widgets/hardware_panel.dart';
import 'about_page.dart';
import 'legal_page.dart';
import 'operation_log_page.dart';

/// App preferences plus a hardware panel and the About entry point.
class SettingsPage extends StatelessWidget {
  const SettingsPage({
    super.key,
    required this.service,
    this.locale,
    this.onLocaleChanged,
    this.theme,
    this.logStore,
  });

  final BatchHashService service;

  /// Active UI language code ('zh' or 'en').
  final String? locale;

  /// Called with the new language code; when null the switch is hidden.
  final ValueChanged<String>? onLocaleChanged;

  /// Holds the seed color; when null the accent picker is hidden.
  final ThemeController? theme;

  /// Source of the operation history; when null a fresh store is used.
  final OperationLogStore? logStore;

  @override
  Widget build(BuildContext context) {
    // A field is not promoted by a null check, so the branch needs a local:
    // `theme == null` on the field would keep the un-promoted static type and
    // the else branch could not pass a ThemeController.
    final ThemeController? themeController = theme;
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final ValueChanged<String>? onLocaleChanged = this.onLocaleChanged;
    final String language = AppStrings.normalize(locale);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            context.strings('settings.title'),
            style: theme.textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(
            context.strings('settings.subtitle'),
            style: theme.textTheme.bodyMedium?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 16),
          Card(
            child: Column(
              children: <Widget>[
                ListTile(
                  leading: const Icon(Icons.translate),
                  title: Text(context.strings('settings.language')),
                  subtitle: Text(context.strings('settings.languageDetail')),
                ),
                if (onLocaleChanged != null)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
                    child: Align(
                      alignment: AlignmentDirectional.centerStart,
                      child: SegmentedButton<String>(
                        key: const ValueKey<String>('settings-language'),
                        segments: <ButtonSegment<String>>[
                          ButtonSegment<String>(
                            value: 'zh',
                            label: Text(context.strings('settings.languageZh')),
                          ),
                          ButtonSegment<String>(
                            value: 'en',
                            label: Text(context.strings('settings.languageEn')),
                          ),
                        ],
                        selected: <String>{language},
                        onSelectionChanged: (Set<String> selection) {
                          onLocaleChanged(selection.first);
                        },
                      ),
                    ),
                  ),
                const Divider(height: 1),
                if (themeController == null)
                  ListTile(
                    leading: const Icon(Icons.palette_outlined),
                    title: Text(context.strings('settings.theme')),
                    subtitle: Text(
                      context.strings(
                        'settings.themeDetail',
                        args: <String, String>{
                          'seed': AppTheme.seedColor.toString(),
                        },
                      ),
                    ),
                  )
                else
                  AccentColorCard(controller: themeController),
                const Divider(height: 1),
                ListTile(
                  leading: const Icon(Icons.memory_outlined),
                  title: Text(context.strings('settings.acceleration')),
                  subtitle: Text(
                    context.strings('settings.accelerationDetail'),
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          HardwarePanelLoader(service: service),
          const SizedBox(height: 16),
          const SizedBox(height: 16),
          Card(
            child: Column(
              children: <Widget>[
                ListTile(
                  key: const ValueKey<String>('settings-history'),
                  leading: const Icon(Icons.history),
                  title: Text(context.strings('settings.history')),
                  subtitle: Text(context.strings('settings.historyDetail')),
                  trailing: const Icon(Icons.chevron_right),
                  onTap: () async {
                    await Navigator.of(context).push<void>(
                      MaterialPageRoute<void>(
                        builder: (BuildContext context) =>
                            OperationLogPage(
                              store: logStore ?? OperationLogStore(),
                            ),
                      ),
                    );
                  },
                ),
                const Divider(height: 1),
                ListTile(
                  key: const ValueKey<String>('settings-legal'),
                  leading: const Icon(Icons.gavel_outlined),
                  title: Text(context.strings('settings.legal')),
                  subtitle: Text(context.strings('settings.legalDetail')),
                  trailing: const Icon(Icons.chevron_right),
                  onTap: () async {
                    await Navigator.of(context).push<void>(
                      MaterialPageRoute<void>(
                        builder: (BuildContext context) => const LegalPage(),
                      ),
                    );
                  },
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),          Card(
            child: ListTile(
              key: const ValueKey<String>('settings-about'),
              leading: const Icon(Icons.info_outline),
              title: Text(context.strings('settings.about')),
              subtitle: Text(context.strings('settings.aboutDetail')),
              trailing: const Icon(Icons.chevron_right),
              onTap: () async {
                await Navigator.of(context).push<void>(
                  MaterialPageRoute<void>(
                    builder: (BuildContext context) =>
                        AboutPage(service: service),
                  ),
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}
