import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app_info.dart';
import '../l10n/app_strings_scope.dart';

/// First-launch terms gate.
///
/// The terms are rendered inline instead of as a pushed route, because a route
/// the user can navigate away from would let them accept copy they never saw.
/// The body scrolls: the legal text is longer than any phone screen.
class FirstRunDialog extends StatelessWidget {
  const FirstRunDialog({
    super.key,
    required this.onAccept,
    required this.onDecline,
  });

  /// Called when the user accepts, right before the dialog closes. The caller
  /// owns persisting the acceptance.
  final VoidCallback onAccept;

  /// Called when the user declines; the default wiring leaves the app.
  final VoidCallback onDecline;

  /// Width cap for the scrolling body, so the text keeps a readable measure on
  /// a wide desktop window.
  static const double maxWidth = 560;

  Future<void> _openProject(BuildContext context) async {
    if (await openProjectUrl()) {
      return;
    }
    // No desktop opener (mobile, or a host without a browser): hand the link
    // over instead of leaving a button that appears to do nothing.
    await Clipboard.setData(const ClipboardData(text: kProjectUrl));
    if (!context.mounted) {
      return;
    }
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text(kProjectUrl)),
    );
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return AlertDialog(
      key: const ValueKey<String>('first-run-dialog'),
      title: Text(context.strings('firstRun.title')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: maxWidth),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                context.strings('firstRun.lead'),
                style: theme.textTheme.bodyMedium,
              ),
              const SizedBox(height: 16),
              _InfoCard(
                background: scheme.secondaryContainer,
                foreground: scheme.onSecondaryContainer,
                title: context.strings('firstRun.projectTitle'),
                body: context.strings('firstRun.projectBody'),
              ),
              const SizedBox(height: 12),
              _InfoCard(
                background: scheme.tertiaryContainer,
                foreground: scheme.onTertiaryContainer,
                title: context.strings('firstRun.starTitle'),
                body: context.strings('firstRun.starBody'),
                action: Align(
                  alignment: Alignment.centerLeft,
                  child: FilledButton.icon(
                    key: const ValueKey<String>('first-run-open-github'),
                    onPressed: () => _openProject(context),
                    icon: const Icon(Icons.star_border),
                    label: Text(context.strings('firstRun.openGithub')),
                  ),
                ),
              ),
              const SizedBox(height: 20),
              Text(
                context.strings('firstRun.scrollHint'),
                style: theme.textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(height: 12),
              const _TermsSection(
                titleKey: 'legal.disclaimerTitle',
                bodyKeys: <String>[
                  'legal.disclaimerBody1',
                  'legal.disclaimerBody2',
                  'legal.disclaimerBody3',
                ],
              ),
              const _TermsSection(
                titleKey: 'legal.termsTitle',
                bodyKeys: <String>[
                  'legal.termsBody1',
                  'legal.termsBody2',
                  'legal.termsBody3',
                  'legal.termsBody4',
                ],
              ),
              const _TermsSection(
                titleKey: 'legal.privacyTitle',
                bodyKeys: <String>[
                  'legal.privacyBody1',
                  'legal.privacyBody2',
                ],
              ),
              const _TermsSection(
                titleKey: 'legal.licenseTitle',
                bodyKeys: <String>[
                  'legal.licenseBody1',
                  'legal.licenseBody2',
                ],
              ),
            ],
          ),
        ),
      ),
      actions: <Widget>[
        TextButton(
          key: const ValueKey<String>('first-run-decline'),
          onPressed: () {
            Navigator.of(context).pop();
            onDecline();
          },
          child: Text(context.strings('firstRun.decline')),
        ),
        FilledButton(
          key: const ValueKey<String>('first-run-accept'),
          onPressed: () {
            onAccept();
            Navigator.of(context).pop();
          },
          child: Text(context.strings('firstRun.accept')),
        ),
      ],
    );
  }
}

/// Rounded callout used for the project and Star blocks.
class _InfoCard extends StatelessWidget {
  const _InfoCard({
    required this.background,
    required this.foreground,
    required this.title,
    required this.body,
    this.action,
  });

  final Color background;
  final Color foreground;
  final String title;
  final String body;
  final Widget? action;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final Widget? actionWidget = action;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            title,
            style: theme.textTheme.titleSmall?.copyWith(color: foreground),
          ),
          const SizedBox(height: 6),
          Text(
            body,
            style: theme.textTheme.bodySmall?.copyWith(
              color: foreground,
              height: 1.5,
            ),
          ),
          if (actionWidget != null) ...<Widget>[
            const SizedBox(height: 10),
            actionWidget,
          ],
        ],
      ),
    );
  }
}

/// One titled block of the legal copy, reusing LegalPage's translation keys
/// without embedding the whole page.
class _TermsSection extends StatelessWidget {
  const _TermsSection({required this.titleKey, required this.bodyKeys});

  final String titleKey;
  final List<String> bodyKeys;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(context.strings(titleKey), style: theme.textTheme.titleSmall),
          const SizedBox(height: 6),
          for (final String key in bodyKeys)
            Padding(
              padding: const EdgeInsets.only(bottom: 6),
              child: Text(
                context.strings(key),
                style: theme.textTheme.bodySmall?.copyWith(height: 1.5),
              ),
            ),
        ],
      ),
    );
  }
}
