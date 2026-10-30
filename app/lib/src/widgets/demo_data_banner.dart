import 'package:flutter/material.dart';

/// Persistent warning that the app is running on bundled demo data.
///
/// This is the guard against the worst failure mode of a packaging mistake: the
/// UI keeps working while every hash, diff and rename preview is fabricated.
/// The banner is deliberately loud and cannot be dismissed.
class DemoDataBanner extends StatelessWidget {
  const DemoDataBanner({
    super.key,
    required this.message,
    required this.actionLabel,
    required this.title,
    required this.body,
    required this.remedy,
  });

  /// One line summary shown in the bar.
  final String message;

  /// Label of the details button.
  final String actionLabel;

  /// Dialog title with the full explanation.
  final String title;

  /// Dialog body explaining what is fake.
  final String body;

  /// Dialog line describing how to fix a broken installation.
  final String remedy;

  Future<void> _showDetails(BuildContext context) {
    return showDialog<void>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        key: const ValueKey<String>('demo-data-dialog'),
        icon: const Icon(Icons.warning_amber_rounded),
        title: Text(title),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(body),
            const SizedBox(height: 12),
            Text(remedy, style: Theme.of(context).textTheme.bodySmall),
          ],
        ),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(MaterialLocalizations.of(context).closeButtonLabel),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final ColorScheme scheme = Theme.of(context).colorScheme;
    return Material(
      key: const ValueKey<String>('demo-data-banner'),
      color: scheme.errorContainer,
      child: SafeArea(
        bottom: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 8, 8),
          child: Row(
            children: <Widget>[
              Icon(
                Icons.warning_amber_rounded,
                size: 20,
                color: scheme.onErrorContainer,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  message,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onErrorContainer,
                  ),
                ),
              ),
              TextButton(
                key: const ValueKey<String>('demo-data-details'),
                onPressed: () => _showDetails(context),
                style: TextButton.styleFrom(
                  foregroundColor: scheme.onErrorContainer,
                ),
                child: Text(actionLabel),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
