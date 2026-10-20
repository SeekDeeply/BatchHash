import 'package:flutter/material.dart';

import '../models/models.dart';
import '../theme.dart';
import '../util/formatting.dart';
import 'empty_state.dart';
import 'status_chip.dart';

/// Dual-column comparison table. Each row is a synchronized pair of columns
/// with a 4px status bar on the leading edge; tapping a row expands details.
class DiffTable extends StatelessWidget {
  const DiffTable({
    super.key,
    required this.rows,
    this.expandedPath,
    this.onRowTap,
    this.leftLabel = 'Baseline',
    this.rightLabel = 'Current',
  });

  final List<DiffRowModel> rows;
  final String? expandedPath;
  final ValueChanged<DiffRowModel>? onRowTap;
  final String leftLabel;
  final String rightLabel;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    if (rows.isEmpty) {
      return const EmptyState(
        icon: Icons.compare_arrows,
        title: 'No rows match',
        message: 'Adjust the filters to see comparison rows.',
      );
    }
    final String? expanded = expandedPath;
    final ValueChanged<DiffRowModel>? tap = onRowTap;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
          child: Row(
            children: <Widget>[
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  leftLabel,
                  style: theme.textTheme.labelSmall
                      ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  rightLabel,
                  style: theme.textTheme.labelSmall
                      ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                ),
              ),
            ],
          ),
        ),
        for (final DiffRowModel row in rows)
          DiffRowTile(
            key: ValueKey<String>('diff-row-tile-${row.path}'),
            row: row,
            expanded: expanded == row.path,
            onTap: tap == null ? null : () => tap(row),
          ),
      ],
    );
  }
}

/// One expandable comparison row.
class DiffRowTile extends StatelessWidget {
  const DiffRowTile({
    super.key,
    required this.row,
    required this.expanded,
    this.onTap,
  });

  static const double statusBarWidth = 4;
  static const double rowHeight = 56;

  final DiffRowModel row;
  final bool expanded;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final Color barColor = diffStatusColor(row.status, scheme);
    final Color background = diffRowBackground(row.status, scheme);
    final Color foreground = diffRowForeground(row.status, scheme);
    final bool isChanged = row.status == DiffStatus.changed;
    return Material(
      key: ValueKey<String>('diff-row-bg-${row.path}'),
      color: background,
      child: InkWell(
        key: ValueKey<String>('diff-row-${row.path}'),
        onTap: onTap,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            SizedBox(
              height: rowHeight,
              child: Row(
                children: <Widget>[
                  Container(
                    key: ValueKey<String>('diff-status-bar-${row.path}'),
                    width: statusBarWidth,
                    height: rowHeight,
                    color: barColor,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: _DiffCell(
                      entry: row.left,
                      isChanged: isChanged,
                      foreground: foreground,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: _DiffCell(
                      entry: row.right,
                      isChanged: isChanged,
                      foreground: foreground,
                    ),
                  ),
                  const SizedBox(width: 8),
                  StatusChip(
                    label: row.status.label,
                    color: barColor,
                    icon: diffStatusIcon(row.status),
                  ),
                  const SizedBox(width: 4),
                  Icon(
                    expanded ? Icons.expand_less : Icons.expand_more,
                    size: 18,
                    semanticLabel: expanded
                        ? 'Collapse row details'
                        : 'Expand row details',
                  ),
                  const SizedBox(width: 8),
                ],
              ),
            ),
            if (expanded)
              _DiffDetails(row: row, foreground: foreground),
          ],
        ),
      ),
    );
  }
}

class _DiffCell extends StatelessWidget {
  const _DiffCell({
    required this.entry,
    required this.isChanged,
    required this.foreground,
  });

  final BatchEntry? entry;
  final bool isChanged;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final BatchEntry? value = entry;
    if (value == null) {
      return Text(
        'not present',
        style: theme.textTheme.bodySmall
            ?.copyWith(color: theme.colorScheme.outline),
      );
    }
    final String summary = value.hashes.isEmpty
        ? formatBytes(value.size)
        : '${formatBytes(value.size)} · ${value.hashes.length} hashes';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisAlignment: MainAxisAlignment.center,
      children: <Widget>[
        Text(
          value.path,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: theme.textTheme.bodyMedium?.copyWith(color: foreground),
        ),
        Text(
          summary,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: theme.textTheme.bodySmall
              ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
        ),
      ],
    );
  }
}

class _DiffDetails extends StatelessWidget {
  const _DiffDetails({required this.row, required this.foreground});

  final DiffRowModel row;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final String? note = row.note;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          if (note != null)
            Text(
              note,
              style: theme.textTheme.bodySmall?.copyWith(color: foreground),
            ),
          _detailLine(context, 'Baseline', row.left),
          _detailLine(context, 'Current', row.right),
        ],
      ),
    );
  }

  Widget _detailLine(BuildContext context, String label, BatchEntry? entry) {
    final ThemeData theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Text(
        '$label: ${_describe(entry)}',
        style: theme.textTheme.bodySmall?.copyWith(color: foreground),
      ),
    );
  }

  String _describe(BatchEntry? entry) {
    if (entry == null) {
      return 'not present';
    }
    if (entry.hashes.isEmpty) {
      return '${formatBytes(entry.size)}, no hashes';
    }
    final List<String> digests = <String>[];
    for (final MapEntry<String, String> digest in entry.hashes.entries) {
      final String value = digest.value.length > 16
          ? '${digest.value.substring(0, 16)}…'
          : digest.value;
      digests.add('${digest.key}=$value');
    }
    return '${formatBytes(entry.size)}, ${digests.join(', ')}';
  }
}
