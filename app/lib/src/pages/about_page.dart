import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../services/batch_hash_service.dart';
import '../widgets/hardware_panel.dart';

/// Static product information plus the hardware panel.
class AboutPage extends StatelessWidget {
  const AboutPage({super.key, required this.service});

  final BatchHashService service;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String versionLabel = context.strings('about.version');
    return Scaffold(
      appBar: AppBar(title: Text(context.strings('about.title'))),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Row(
              children: <Widget>[
                Icon(Icons.fingerprint, size: 40, color: scheme.primary),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      Text(
                        context.strings('app.title'),
                        style: theme.textTheme.headlineSmall,
                      ),
                      Text(
                        '$versionLabel 0.1.0 (P0 preview)',
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: scheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
            const SizedBox(height: 16),
            Text(
              context.strings('about.tagline'),
              style: theme.textTheme.bodyMedium,
            ),
            const SizedBox(height: 16),
            Card(
              child: Column(
                children: <Widget>[
                  ListTile(
                    leading: const Icon(Icons.speed_outlined),
                    title: Text(context.strings('about.feature1')),
                    subtitle: Text(context.strings('about.feature1Detail')),
                  ),
                  const Divider(height: 1),
                  ListTile(
                    leading: const Icon(Icons.compare_arrows),
                    title: Text(context.strings('about.feature2')),
                    subtitle: Text(context.strings('about.feature2Detail')),
                  ),
                  const Divider(height: 1),
                  ListTile(
                    leading: const Icon(Icons.drive_file_rename_outline),
                    title: Text(context.strings('about.feature3')),
                    subtitle: Text(context.strings('about.feature3Detail')),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            HardwarePanelLoader(service: service),
            const SizedBox(height: 16),
            Text(
              context.strings('about.privacy'),
              style: theme.textTheme.bodySmall?.copyWith(
                color: scheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
