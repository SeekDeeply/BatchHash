import 'package:flutter/material.dart';

import '../app_info.dart';
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
                        '$versionLabel $kAppVersion',
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
            // Credits sit between the feature list and the hardware details:
            // close enough to the product description to be read, without
            // pushing the technical panel off the first screen.
            Card(
              key: const ValueKey<String>('about-collaborators'),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  ListTile(
                    leading: const Icon(Icons.group_outlined),
                    title: Text(context.strings('about.collaborators')),
                    subtitle: Text(
                      context.strings('about.collaboratorsDetail'),
                    ),
                  ),
                  for (final ({String handle, List<String> roleKeys}) person
                      in kCollaborators)
                    ListTile(
                      key: ValueKey<String>(
                        'about-collaborator-${person.handle}',
                      ),
                      leading: const Icon(Icons.person_outline, size: 20),
                      title: Text(person.handle),
                      // Roles come from the string table: the same person may
                      // be credited differently per language without editing
                      // code.
                      subtitle: Text(
                        <String>[
                          for (final String key in person.roleKeys)
                            context.strings(key),
                        ].join(' · '),
                      ),
                      trailing: const Icon(Icons.open_in_new, size: 18),
                      onTap: () async {
                        final bool opened = await openProjectUrl(
                          url: githubProfileUrl(person.handle),
                        );
                        if (!context.mounted) {
                          return;
                        }
                        // Some hosts have no browser to hand the link to; show
                        // it rather than leaving a row that looks interactive
                        // but does nothing.
                        if (!opened) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                              content: Text(githubProfileUrl(person.handle)),
                            ),
                          );
                        }
                      },
                    ),
                  const SizedBox(height: 8),
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
