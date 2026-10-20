import 'package:flutter/material.dart';

import '../models/models.dart';
import '../theme.dart';
import '../util/formatting.dart';
import 'empty_state.dart';
import 'status_chip.dart';

/// Non-scrolling vertical list of batch entries.
class EntryList extends StatelessWidget {
  const EntryList({super.key, required this.entries});

  final List<BatchEntry> entries;

  @override
  Widget build(BuildContext context) {
    if (entries.isEmpty) {
      return const EmptyState(
        icon: Icons.inbox_outlined,
        title: 'No entries',
        message: 'There is nothing to show yet.',
      );
    }
    return Column(
      children: <Widget>[
        for (final BatchEntry entry in entries)
          EntryTile(
            key: ValueKey<String>('entry-${entry.path}'),
            entry: entry,
          ),
      ],
    );
  }
}

/// One batch entry row with an icon, a text status and a semantics label.
class EntryTile extends StatelessWidget {
  const EntryTile({super.key, required this.entry});

  final BatchEntry entry;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final Color statusColor = entryStatusColor(entry.status, scheme);
    final String semanticsLabel =
        '${entry.path}. Status ${entry.status.label}. ${formatBytes(entry.size)}.';
    return Semantics(
      key: ValueKey<String>('entry-row-${entry.path}'),
      container: true,
      label: semanticsLabel,
      child: ListTile(
        dense: true,
        contentPadding: const EdgeInsets.symmetric(horizontal: 4),
        leading: Icon(
          entryStatusIcon(entry.status),
          key: ValueKey<String>('entry-status-icon-${entry.path}'),
          color: statusColor,
          semanticLabel: 'Status ${entry.status.label}',
        ),
        title: Text(
          entry.path,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        subtitle: Text(
          _subtitle(),
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
        ),
        trailing: StatusChip(
          label: entry.status.label,
          color: statusColor,
          icon: entryStatusIcon(entry.status),
        ),
      ),
    );
  }

  String _subtitle() {
    final List<String> parts = <String>[formatBytes(entry.size)];
    final String? errorKind = entry.errorKind;
    if (errorKind != null) {
      parts.add('error: $errorKind');
    }
    if (entry.hashes.isNotEmpty) {
      parts.add('${entry.hashes.length} hashes');
    }
    final String? suffix = entry.suffixApplied;
    if (suffix != null) {
      parts.add('suffix $suffix');
    }
    final String? renamedTo = entry.renamedTo;
    if (renamedTo != null) {
      parts.add('renamed to $renamedTo');
    }
    return parts.join(' · ');
  }
}
