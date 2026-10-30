import 'package:flutter/material.dart';

/// Centered placeholder for empty lists and pages without data yet.
///
/// [title] and [message] are rendered verbatim, so callers must resolve them
/// through the active locale (`context.strings(...)`) before passing them in.
class EmptyState extends StatelessWidget {
  const EmptyState({
    super.key,
    required this.icon,
    required this.title,
    required this.message,
    this.action,
  });

  final IconData icon;
  final String title;
  final String message;
  final Widget? action;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final Widget? actionWidget = action;
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 420),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Icon(icon, size: 48, color: scheme.outline),
              const SizedBox(height: 12),
              Text(
                title,
                style: theme.textTheme.titleMedium,
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 4),
              Text(
                message,
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
                textAlign: TextAlign.center,
              ),
              if (actionWidget != null) ...<Widget>[
                const SizedBox(height: 16),
                actionWidget,
              ],
            ],
          ),
        ),
      ),
    );
  }
}
