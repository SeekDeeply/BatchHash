import 'package:flutter/material.dart';

import '../services/batch_hash_service.dart';
import '../theme.dart';
import '../widgets/hardware_panel.dart';
import 'about_page.dart';

/// App preferences plus a hardware panel and the About entry point.
class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key, required this.service});

  final BatchHashService service;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text('Settings', style: theme.textTheme.headlineSmall),
          const SizedBox(height: 4),
          Text(
            'Theme, acceleration reporting and app information.',
            style: theme.textTheme.bodyMedium?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 16),
          Card(
            child: Column(
              children: <Widget>[
                ListTile(
                  leading: const Icon(Icons.palette_outlined),
                  title: const Text('Material 3 theme'),
                  subtitle: Text(
                    'Seed color ${AppTheme.seedColor} with dynamic color when '
                    'the platform provides it. Compact visual density.',
                  ),
                ),
                const Divider(height: 1),
                const ListTile(
                  leading: Icon(Icons.memory_outlined),
                  title: Text('Acceleration reporting'),
                  subtitle: Text(
                    'Hash acceleration is reported by the core probe; UI '
                    'rendering acceleration comes from the platform layer. '
                    'Unknown values are shown as unknown, never guessed.',
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          HardwarePanelLoader(service: service),
          const SizedBox(height: 16),
          Card(
            child: ListTile(
              key: const ValueKey<String>('settings-about'),
              leading: const Icon(Icons.info_outline),
              title: const Text('About BatchHash'),
              subtitle: const Text('Version, credits and hardware details.'),
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
