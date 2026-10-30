import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../theme.dart';
import '../util/formatting.dart';
import 'empty_state.dart';
import 'status_chip.dart';

/// Dual-column comparison table. Each row is a synchronized pair of columns
/// with a 4px status bar on the leading edge; tapping a row expands details.
///
/// [leftLabel] and [rightLabel] override the column headers. When they are
/// empty the localized `compare.baseline` and `compare.current` copy is used.
class DiffTable extends StatelessWidget {
  const DiffTable({
    super.key,
    required this.rows,
    this.expandedPath,
    this.onRowTap,
    this.leftLabel = '',
    this.rightLabel = '',
  });

  final List<DiffRowModel> rows;
  final String? expandedPath;
  final ValueChanged<DiffRowModel>? onRowTap;
  final String leftLabel;
  final String rightLabel;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final String leftText = leftLabel.isEmpty
        ? context.strings('compare.baseline')
        : leftLabel;
    final String rightText = rightLabel.isEmpty
        ? context.strings('compare.current')
        : rightLabel;
    if (rows.isEmpty) {
      return EmptyState(
        icon: Icons.compare_arrows,
        title: context.strings('compare.nothing'),
        message: context.strings('compare.nothingHint'),
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
                  leftText,
                  style: theme.textTheme.labelSmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  rightText,
                  style: theme.textTheme.labelSmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
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
                    child: _DiffCell(entry: row.left, foreground: foreground),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: _DiffCell(entry: row.right, foreground: foreground),
                  ),
                  const SizedBox(width: 8),
                  StatusChip(
                    label: _diffStatusLabel(context, row.status),
                    color: barColor,
                    icon: diffStatusIcon(row.status),
                  ),
                  const SizedBox(width: 4),
                  Icon(
                    expanded ? Icons.expand_less : Icons.expand_more,
                    size: 18,
                    semanticLabel: context.strings(
                      expanded ? 'common.collapseDetails' : 'common.details',
                    ),
                  ),
                  const SizedBox(width: 8),
                ],
              ),
            ),
            if (expanded) _DiffDetails(row: row, foreground: foreground),
          ],
        ),
      ),
    );
  }
}

/// Localized label for one diff status.
///
/// The status text always comes from the string table, never from the raw enum
/// label, so the Chinese UI never shows an English identifier.
String _diffStatusLabel(BuildContext context, DiffStatus status) {
  return switch (status) {
    DiffStatus.unchanged => context.strings('compare.unchanged'),
    DiffStatus.changed => context.strings('compare.changed'),
    DiffStatus.added => context.strings('compare.added'),
    DiffStatus.removed => context.strings('compare.removed'),
    DiffStatus.ambiguous => context.strings('compare.ambiguous'),
    DiffStatus.unknown => context.strings('common.unknown'),
    DiffStatus.duplicateKey => context.strings('compare.duplicateKey'),
    DiffStatus.outOfRoot => context.strings('compare.outOfRoot'),
    DiffStatus.corrupt => context.strings('compare.corrupt'),
    DiffStatus.unstable => context.strings('compare.unstable'),
  };
}

class _DiffCell extends StatelessWidget {
  const _DiffCell({required this.entry, required this.foreground});

  final BatchEntry? entry;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final BatchEntry? value = entry;
    if (value == null) {
      return Text(
        context.strings('common.notPresent'),
        style: theme.textTheme.bodySmall?.copyWith(
          color: theme.colorScheme.outline,
        ),
      );
    }
    final String hashCountLabel = context.strings(
      'diff.hashesCount',
      args: <String, String>{'count': '${value.hashes.length}'},
    );
    final String summary = value.hashes.isEmpty
        ? formatBytes(value.size)
        : '${formatBytes(value.size)} · $hashCountLabel';
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
          style: theme.textTheme.bodySmall?.copyWith(
            color: theme.colorScheme.onSurfaceVariant,
          ),
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
          _detailLine(context, context.strings('compare.baseline'), row.left),
          _detailLine(context, context.strings('compare.current'), row.right),
        ],
      ),
    );
  }

  Widget _detailLine(BuildContext context, String label, BatchEntry? entry) {
    final ThemeData theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Text(
        '$label: ${_describe(context, entry)}',
        style: theme.textTheme.bodySmall?.copyWith(color: foreground),
      ),
    );
  }

  String _describe(BuildContext context, BatchEntry? entry) {
    if (entry == null) {
      return context.strings('common.notPresent');
    }
    if (entry.hashes.isEmpty) {
      final String noHashes = context.strings('common.noHashes');
      return '${formatBytes(entry.size)}, $noHashes';
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
