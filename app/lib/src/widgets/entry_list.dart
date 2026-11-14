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

/// Collapsible digest list for one entry.
///
/// Collapsed by default: only the first digest is visible, so a long digest
/// list cannot push the rest of the results off screen. Expanded it shows every
/// digest with its copy button. The expanded flag lives in the [State] and the
/// widget is keyed by entry path, so a progress refresh that replaces the
/// [BatchEntry] objects keeps the state with the same file instead of moving it
/// to whichever row happens to occupy the same position.
class _DigestSection extends StatefulWidget {
  const _DigestSection({
    super.key,
    required this.entryPath,
    required this.hashes,
  });

  final String entryPath;
  final Map<String, String> hashes;

  @override
  State<_DigestSection> createState() => _DigestSectionState();
}

class _DigestSectionState extends State<_DigestSection> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final List<MapEntry<String, String>> digests = widget.hashes.entries
        .toList(growable: false);
    if (digests.isEmpty) {
      return const SizedBox.shrink();
    }
    final int total = digests.length;
    // One algorithm needs no toggle: everything is already on screen, and a
    // button that expands nothing is just noise on every row.
    if (total < 2) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          const Divider(height: 16),
          _DigestLine(
            algorithm: digests.first.key,
            digest: digests.first.value,
          ),
        ],
      );
    }
    final bool expanded = _expanded;
    final List<MapEntry<String, String>> visible = expanded
        ? digests
        : digests.take(1).toList(growable: false);
    // The count on the button is what the reader cannot currently see, not how
    // many algorithms exist: "expand (3)" read as three digests to reveal, which
    // was wrong by one whenever one line was already shown.
    final int hiddenCount = expanded ? 0 : digests.length - visible.length;
    final String toggleLabel = expanded
        ? context.strings('entry.collapseDigests')
        : context.strings(
            'entry.expandDigests',
            args: <String, String>{'count': '$hiddenCount'},
          );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        const Divider(height: 16),
        for (final MapEntry<String, String> digest in visible)
          _DigestLine(algorithm: digest.key, digest: digest.value),
        if (!expanded && hiddenCount > 0)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Text(
              context.strings(
                'entry.collapsedCount',
                args: <String, String>{'count': '$hiddenCount'},
              ),
              style: theme.textTheme.bodySmall,
            ),
          ),
        const SizedBox(height: 2),
        Tooltip(
          message: toggleLabel,
          child: TextButton.icon(
            key: ValueKey<String>(
              'entry-digest-toggle-${widget.entryPath}',
            ),
            onPressed: () => setState(() => _expanded = !_expanded),
            icon: Icon(expanded ? Icons.expand_less : Icons.expand_more),
            label: Text(toggleLabel),
          ),
        ),
      ],
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
              if (entry.hashes.isNotEmpty)
                _DigestSection(
                  key: ValueKey<String>('entry-digests-${entry.path}'),
                  entryPath: entry.path,
                  hashes: entry.hashes,
                ),
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
