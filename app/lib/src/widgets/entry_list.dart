import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

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

/// One algorithm/digest pair, selectable and copyable.
class _DigestLine extends StatelessWidget {
  const _DigestLine({required this.algorithm, required this.digest});

  final String algorithm;
  final String digest;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          SizedBox(
            width: 82,
            child: Text(algorithm, style: theme.textTheme.labelMedium),
          ),
          Expanded(
            child: SelectableText(
              digest,
              key: ValueKey<String>('digest-$algorithm-$digest'),
              style: theme.textTheme.bodySmall?.copyWith(
                fontFamily: 'monospace',
              ),
            ),
          ),
          Tooltip(
            message: context.strings(
              'entry.copyDigest',
              args: <String, String>{'algorithm': algorithm},
            ),
            child: IconButton(
              visualDensity: VisualDensity.compact,
              constraints: const BoxConstraints.tightFor(width: 36, height: 36),
              iconSize: 18,
              icon: const Icon(Icons.copy_outlined),
              onPressed: () => Clipboard.setData(ClipboardData(text: digest)),
            ),
          ),
        ],
      ),
    );
  }
}

/// One batch entry row: status, path, size and every computed digest.
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
      child: Card(
        margin: const EdgeInsets.symmetric(vertical: 3),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 8, 10),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Row(
                children: <Widget>[
                  Icon(
                    entryStatusIcon(entry.status),
                    key: ValueKey<String>('entry-status-icon-${entry.path}'),
                    color: statusColor,
                    semanticLabel: statusSemantics,
                    size: 20,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      entry.path,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.titleSmall,
                    ),
                  ),
                  const SizedBox(width: 8),
                  StatusChip(
                    label: statusLabel,
                    color: statusColor,
                    icon: entryStatusIcon(entry.status),
                  ),
                ],
              ),
              const SizedBox(height: 4),
              Text(_subtitle(context), style: theme.textTheme.bodySmall),
              if (entry.hashes.isNotEmpty) ...<Widget>[
                const Divider(height: 16),
                for (final MapEntry<String, String> digest
                    in entry.hashes.entries)
                  _DigestLine(algorithm: digest.key, digest: digest.value),
              ],
            ],
          ),
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
