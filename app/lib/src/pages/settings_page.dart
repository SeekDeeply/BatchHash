import 'package:flutter/material.dart';

import '../l10n/app_strings.dart';
import '../l10n/app_strings_scope.dart';
import '../app_info.dart';
import '../services/batch_hash_service.dart';
import '../services/operation_log_store.dart';
import '../theme.dart';
import '../theme_controller.dart';
import '../widgets/accent_color_card.dart';
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
    // The card needs a promoted ThemeController, and a nullable field is never
    // promoted by a null check, so read it into a local first.
    final ThemeController? themeController = theme;
    final ThemeData themeData = Theme.of(context);
    final ColorScheme scheme = themeData.colorScheme;
    final ValueChanged<String>? onLocaleChanged = this.onLocaleChanged;
    final String language = AppStrings.normalize(locale);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            context.strings('settings.title'),
            style: themeData.textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(
            context.strings('settings.subtitle'),
            style: themeData.textTheme.bodyMedium?.copyWith(
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
              ],
            ),
          ),
          const SizedBox(height: 16),
          // The hardware panel lives on the About page only. It was rendered in
          // both places, together with a one-line "acceleration" description
          // that said the same thing as the panel itself, so Settings showed the
          // same facts twice and About showed them again.
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
          const SizedBox(height: 16),
          Card(
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
          const SizedBox(height: 24),
          // Project identity lives at the bottom of Settings, not buried in
          // About: the version and the Star ask are what people look for after
          // using the app for a while, and About needs a tap to reach.
          const _ProjectFooter(),
        ],
      ),
    );
  }
}

/// Version, one-line pitch and the Star call to action.
class _ProjectFooter extends StatelessWidget {
  const _ProjectFooter();

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return Card(
      key: const ValueKey<String>('settings-project-footer'),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Row(
              children: <Widget>[
                Icon(Icons.fingerprint, color: scheme.primary),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    context.strings('settings.projectTitle'),
                    style: theme.textTheme.titleMedium,
                  ),
                ),
                Text(
                  '${context.strings('settings.version')} $kAppVersion',
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(
              context.strings('settings.projectBody'),
              style: theme.textTheme.bodySmall?.copyWith(
                color: scheme.onSurfaceVariant,
              ),
            ),
            const SizedBox(height: 12),
            Text(
              context.strings('settings.starDetail'),
              style: theme.textTheme.bodyMedium,
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: <Widget>[
                FilledButton.tonalIcon(
                  key: const ValueKey<String>('settings-star'),
                  onPressed: () async {
                    final bool opened = await openProjectUrl();
                    if (!context.mounted) {
                      return;
                    }
                    // When no handler exists, show the URL instead of leaving
                    // the button looking broken.
                    if (!opened) {
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text(kProjectUrl)),
                      );
                    }
                  },
                  icon: const Icon(Icons.star_border, size: 18),
                  label: Text(context.strings('settings.starAction')),
                ),
                SelectableText(
                  kProjectUrl,
                  style: theme.textTheme.bodySmall?.copyWith(
                    fontFamily: 'monospace',
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
