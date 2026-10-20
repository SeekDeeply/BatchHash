import 'package:flutter/material.dart';

/// One labeled value rendered inside a [SummaryCard].
class SummaryMetric {
  const SummaryMetric({
    required this.label,
    required this.value,
    this.icon,
    this.color,
  });

  final String label;
  final String value;
  final IconData? icon;
  final Color? color;
}

/// Compact summary card used by the hash and compare pages.
class SummaryCard extends StatelessWidget {
  const SummaryCard({
    super.key,
    required this.title,
    required this.metrics,
    this.trailing,
  });

  final String title;
  final List<SummaryMetric> metrics;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final Widget? trailingWidget = trailing;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Row(
              children: <Widget>[
                Expanded(child: Text(title, style: theme.textTheme.titleMedium)),
                if (trailingWidget != null) trailingWidget,
              ],
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 24,
              runSpacing: 12,
              children: <Widget>[
                for (final SummaryMetric metric in metrics)
                  _SummaryMetricTile(metric: metric),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _SummaryMetricTile extends StatelessWidget {
  const _SummaryMetricTile({required this.metric});

  final SummaryMetric metric;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final Color color = metric.color ?? scheme.onSurface;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Text(
          metric.label,
          style: theme.textTheme.labelSmall
              ?.copyWith(color: scheme.onSurfaceVariant),
        ),
        const SizedBox(height: 2),
        Row(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            if (metric.icon != null) ...<Widget>[
              Icon(metric.icon, size: 16, color: color),
              const SizedBox(width: 4),
            ],
            Text(
              metric.value,
              style: theme.textTheme.titleMedium?.copyWith(color: color),
            ),
          ],
        ),
      ],
    );
  }
}
