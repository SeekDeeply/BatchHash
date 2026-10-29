import 'package:flutter/material.dart';

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
    return Scaffold(
      appBar: AppBar(title: const Text('About BatchHash')),
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
                      Text('BatchHash', style: theme.textTheme.headlineSmall),
                      Text(
                        'Version 0.1.0 (P0 preview)',
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
              'Cross-platform batch file hashing and verification. The Flutter '
              'shell talks to a Rust core through a narrow service interface; '
              'the P0 preview runs against a deterministic fake.',
              style: theme.textTheme.bodyMedium,
            ),
            const SizedBox(height: 16),
            const Card(
              child: Column(
                children: <Widget>[
                  ListTile(
                    leading: Icon(Icons.speed_outlined),
                    title: Text('Single-pass multi-hash engine'),
                    subtitle: Text(
                      'One read per file feeds every selected algorithm.',
                    ),
                  ),
                  Divider(height: 1),
                  ListTile(
                    leading: Icon(Icons.compare_arrows),
                    title: Text('Report comparison and diffing'),
                    subtitle: Text(
                      'Normalized paths, algorithm intersection and explicit '
                      'unknowns.',
                    ),
                  ),
                  Divider(height: 1),
                  ListTile(
                    leading: Icon(Icons.drive_file_rename_outline),
                    title: Text('Hash suffix renaming with undo'),
                    subtitle: Text(
                      'Dry run first, then an undoable rename log.',
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            HardwarePanelLoader(service: service),
            const SizedBox(height: 16),
            Text(
              'Hashing and renaming run locally. No file content leaves the '
              'device.',
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
