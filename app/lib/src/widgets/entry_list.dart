import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../theme.dart';
import '../util/formatting.dart';
import 'empty_state.dart';
import 'status_chip.dart';

/// Translation key for the label of a per-entry [EntryStatus].
String _statusLabelKey(EntryStatus status) => switch (status) {
  EntryStatus.ok => 'common.statusOk',
  EntryStatus.failed => 'common.statusFailed',
  EntryStatus.skipped => 'common.statusSkipped',
};

/// Non-scrolling vertical list of batch entries.
class EntryList extends StatelessWidget {
  const EntryList({super.key, required this.entries});

  final List<BatchEntry> entries;

  @override
  Widget build(BuildContext context) {
    if (entries.isEmpty) {
      return EmptyState(
        icon: Icons.inbox_outlined,
        title: context.strings('empty.noEntries'),
        message: context.strings('empty.noEntriesHint'),
      );
    }
    return Column(
      children: <Widget>[
        for (final BatchEntry entry in entries)
          EntryTile(key: ValueKey<String>('entry-${entry.path}'), entry: entry),
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
    final String statusLabel = context.strings(_statusLabelKey(entry.status));
    final String statusSemantics = context.strings(
      'entry.statusSemantics',
      args: <String, String>{'status': statusLabel},
    );
    final String semanticsLabel =
        '${entry.path}. $statusSemantics. ${formatBytes(entry.size)}.';
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
          semanticLabel: statusSemantics,
        ),
        title: Text(entry.path, maxLines: 1, overflow: TextOverflow.ellipsis),
        subtitle: Text(
          _subtitle(context),
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
        ),
        trailing: StatusChip(
          label: statusLabel,
          color: statusColor,
          icon: entryStatusIcon(entry.status),
        ),
      ),
    );
  }

  String _subtitle(BuildContext context) {
    final List<String> parts = <String>[formatBytes(entry.size)];
    final String? errorKind = entry.errorKind;
    if (errorKind != null) {
      parts.add(
        context.strings(
          'entry.errorDetail',
          args: <String, String>{'kind': errorKind},
        ),
      );
    }
    if (entry.hashes.isNotEmpty) {
      parts.add(
        context.strings(
          'entry.hashCount',
          args: <String, String>{'count': '${entry.hashes.length}'},
        ),
      );
    }
    final String? suffix = entry.suffixApplied;
    if (suffix != null) {
      parts.add(
        context.strings(
          'entry.suffix',
          args: <String, String>{'value': suffix},
        ),
      );
    }
    final String? renamedTo = entry.renamedTo;
    if (renamedTo != null) {
      parts.add(
        context.strings(
          'entry.renamedTo',
          args: <String, String>{'path': renamedTo},
        ),
      );
    }
    return parts.join(' · ');
  }
}
