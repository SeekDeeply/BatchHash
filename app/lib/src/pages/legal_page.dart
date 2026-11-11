import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';

/// Disclaimer and terms of service, shown from Settings.
///
/// Kept in the app rather than behind a link so the text is available offline
/// and cannot rot: a batch tool renames files, and the text a user agreed to
/// must not depend on a network round trip.
class LegalPage extends StatelessWidget {
  const LegalPage({super.key});

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(context.strings('legal.title'))),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(20, 16, 20, 40),
        children: <Widget>[
          Text(
            context.strings('legal.updated'),
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 20),
          _Section(
            titleKey: 'legal.disclaimerTitle',
            body: <String>[
              context.strings('legal.disclaimerBody1'),
              context.strings('legal.disclaimerBody2'),
              context.strings('legal.disclaimerBody3'),
            ],
          ),
          _Section(
            titleKey: 'legal.termsTitle',
            body: <String>[
              context.strings('legal.termsBody1'),
              context.strings('legal.termsBody2'),
              context.strings('legal.termsBody3'),
              context.strings('legal.termsBody4'),
            ],
          ),
          _Section(
            titleKey: 'legal.privacyTitle',
            body: <String>[
              context.strings('legal.privacyBody1'),
              context.strings('legal.privacyBody2'),
            ],
          ),
          _Section(
            titleKey: 'legal.licenseTitle',
            body: <String>[
              context.strings('legal.licenseBody1'),
              context.strings('legal.licenseBody2'),
            ],
          ),
        ],
      ),
    );
  }
}

/// One titled block of legal copy.
class _Section extends StatelessWidget {
  const _Section({required this.titleKey, required this.body});

  final String titleKey;
  final List<String> body;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(context.strings(titleKey), style: theme.textTheme.titleMedium),
          const SizedBox(height: 8),
          for (final String paragraph in body)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: SelectableText(
                paragraph,
                style: theme.textTheme.bodyMedium?.copyWith(height: 1.6),
              ),
            ),
        ],
      ),
    );
  }
}
