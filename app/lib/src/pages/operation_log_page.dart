import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart' show FilePicker;

import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../models/operation_log.dart';
import '../models/operation_log_filter.dart';
import '../services/archive_export.dart';
import '../services/operation_log_export.dart';
import '../services/operation_log_store.dart';
import '../widgets/log_export_dialog.dart';
import '../util/formatting.dart';
import '../widgets/empty_state.dart';

/// Audit trail of every operation the app performed.
///
/// Opened from Settings. Unlike the rename history on the rename page, which is
/// about undoable batches, this lists what the app did and when, including
/// failures, so a user can tell why a scan or export did not happen.
class OperationLogPage extends StatefulWidget {
  const OperationLogPage({super.key, required this.store});

  /// Source of the recorded entries.
  final OperationLogStore store;

  @override
  State<OperationLogPage> createState() => _OperationLogPageState();
}

/// Retention choices offered by the page.
const List<int> _retentionChoices = <int>[200, 500, 1000, 5000];

// Empty strings stand in for "no filter" instead of null: a DropdownButton with
// a null value renders the hint rather than a selectable "All" row, and the row
// has to stay visible so the filter can be reset.
const String _allTypes = '';
const String _statusAll = 'all';
const String _statusOk = 'ok';
const String _statusFailed = 'failed';

class _OperationLogPageState extends State<OperationLogPage> {
  List<OperationLogEntry>? _entries;
  Object? _error;
  int _retention = OperationLogStore.defaultMaxEntries;
  String _query = '';
  String _typeFilter = _allTypes;
  String _statusFilter = _statusAll;
  bool _newestFirst = true;

  @override
  void initState() {
    super.initState();
    unawaited(_reload());
    unawaited(_loadRetention());
  }

  Future<void> _loadRetention() async {
    final int value = await widget.store.loadRetention();
    if (!mounted) {
      return;
    }
    setState(() {
      _retention = value;
    });
  }

  Future<void> _reload() async {
    try {
      final List<OperationLogEntry> entries = await widget.store.load();
      if (!mounted) {
        return;
      }
      setState(() {
        _entries = entries;
        _error = null;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _error = error;
      });
    }
  }

  Future<void> _setRetention(int? value) async {
    if (value == null) {
      return;
    }
    setState(() {
      _retention = value;
    });
    await widget.store.saveRetention(value);
  }

  Future<void> _confirmClear(int total) async {
    final bool? confirmed = await showDialog<bool>(
      context: context,
      builder: (BuildContext dialogContext) => AlertDialog(
        title: Text(dialogContext.strings('log.clearConfirmTitle')),
        content: Text(
          dialogContext.strings(
            'log.clearConfirmBody',
            args: <String, String>{'count': '$total'},
          ),
        ),
        actions: <Widget>[
          TextButton(
            key: const ValueKey<String>('operation-log-clear-cancel'),
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text(dialogContext.strings('common.cancel')),
          ),
          // The destructive button reuses the toolbar's own "clear" label so
          // the dialog copy cannot drift from the action that opened it.
          TextButton(
            key: const ValueKey<String>('operation-log-clear-confirm'),
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: Text(dialogContext.strings('history.clear')),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) {
      return;
    }
    await widget.store.clear();
    if (!mounted) {
      return;
    }
    await _reload();
  }

  @override
  Widget build(BuildContext context) {
    final List<OperationLogEntry>? entries = _entries;
    return Scaffold(
      appBar: AppBar(
        title: Text(context.strings('log.title')),
        actions: <Widget>[
          IconButton(
            key: const ValueKey<String>('operation-log-refresh'),
            tooltip: context.strings('history.refresh'),
            onPressed: _reload,
            icon: const Icon(Icons.refresh),
          ),
          IconButton(
            key: const ValueKey<String>('operation-log-export'),
            tooltip: context.strings('log.export'),
            onPressed: entries == null || entries.isEmpty
                ? null
                : () => _export(entries),
            icon: const Icon(Icons.ios_share),
          ),
          IconButton(
            key: const ValueKey<String>('operation-log-clear'),
            tooltip: context.strings('history.clear'),
            onPressed: entries == null || entries.isEmpty
                ? null
                : () => _confirmClear(entries.length),
            icon: const Icon(Icons.delete_outline),
          ),
        ],
      ),
      body: _buildBody(context),
    );
  }

  /// Entries that survive the current filters, newest-first as configured.
  ///
  /// Shared with the export so the file always matches what the list shows:
  /// exporting more than the user can see is a way to leak records they
  /// filtered out on purpose.
  List<OperationLogEntry> _visibleEntries(
    BuildContext context,
    List<OperationLogEntry> entries,
  ) {
    return filterLogEntries(
      entries,
      query: _query,
      kind: _typeFilter == _allTypes ? null : _typeFilter,
      ok: _statusFilter == _statusAll ? null : _statusFilter == _statusOk,
      newestFirst: _newestFirst,
      kindLabel: (String kind) => context.strings(kind),
    );
  }

  /// Ask for the export options, then write the file.
  Future<void> _export(List<OperationLogEntry> entries) async {
    final List<OperationLogEntry> visible = _visibleEntries(context, entries);
    final ExportOptions? options = await showDialog<ExportOptions>(
      context: context,
      builder: (BuildContext context) =>
          LogExportDialog(count: visible.length),
    );
    if (options == null || !mounted) {
      return;
    }
    if (visible.isEmpty) {
      _snack(context.strings('log.exportNoEntries'));
      return;
    }
    final String? directory = await _pickDirectory();
    if (directory == null || !mounted) {
      return;
    }
    try {
      final String stamp = exportTimestamp();
      final String body = options.format == ExportFormat.json
          ? encodeLogAsJson(visible)
          : encodeLogAsMarkdown(visible);
      final String extension = options.format == ExportFormat.json
          ? '.json'
          : '.md';
      final String base = 'batchhash-operation-log-$stamp';

      // The digest belongs to whatever file the user ends up with, so it has to
      // be computed after packing, not on the raw text: a ZIP named with the
      // digest of its contents cannot be checked with a stock hashing tool.
      final Uint8List? zipped = options.zip
          ? zipBytes(
              <String, List<int>>{
                '$base$extension': utf8.encode(body),
              },
              password: options.password,
            )
          : null;
      final List<int> payload = zipped ?? utf8.encode(body);
      final String? digest = options.sha256
          ? sha256Hex(payload)
          : null;
      final String name = exportFileName(
        base,
        digest: digest,
        extension: zipped == null ? extension : '.zip',
      );
      final File file = File('$directory/$name');
      await file.writeAsBytes(payload, flush: true);
      if (!mounted) {
        return;
      }
      _snack(
        context.strings('log.exportDone', args: <String, String>{'name': name}),
      );
    } catch (error) {
      if (!mounted) {
        return;
      }
      _snack(
        context.strings(
          'log.exportFailed',
          args: <String, String>{'error': '$error'},
        ),
      );
    }
  }

  Future<String?> _pickDirectory() async {
    try {
      final String? picked = await FilePicker.platform.getDirectoryPath();
      if (picked == null || picked.trim().isEmpty) {
      return null;
      }
      return picked.trim();
    } catch (_) {
      return null;
    }
  }

  void _snack(String message) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(message)));
  }
  Widget _buildBody(BuildContext context) {
    final Object? error = _error;
    if (error != null) {
      return Center(
        child: Text(
          context.strings(
            'history.operationsFailed',
            args: <String, String>{'error': '$error'},
          ),
        ),
      );
    }
    final List<OperationLogEntry>? entries = _entries;
    if (entries == null) {
      return const Center(child: CircularProgressIndicator());
    }
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final List<OperationLogEntry> visible = _visibleEntries(context, entries);
    final List<String> kinds = <String>[];
    for (final OperationLogEntry entry in entries) {
      if (!kinds.contains(entry.kind)) {
        kinds.add(entry.kind);
      }
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                context.strings('log.subtitle'),
                style: theme.textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                key: const ValueKey<String>('operation-log-search'),
                onChanged: (String value) => setState(() => _query = value),
                decoration: InputDecoration(
                  hintText: context.strings('log.searchHint'),
                  prefixIcon: const Icon(Icons.search),
                  isDense: true,
                  border: const OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: 8),
              Wrap(
                spacing: 16,
                runSpacing: 8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: <Widget>[
                  _typeControl(context, kinds),
                  _statusControl(context),
                  OutlinedButton.icon(
                    key: const ValueKey<String>('operation-log-sort'),
                    onPressed: () =>
                        setState(() => _newestFirst = !_newestFirst),
                    icon: const Icon(Icons.swap_vert),
                    label: Text(
                      context.strings(
                        _newestFirst ? 'log.sortNewest' : 'log.sortOldest',
                      ),
                    ),
                  ),
                  _retentionControl(context),
                ],
              ),
              const SizedBox(height: 8),
              Text(
                context.strings(
                  'log.matchCount',
                  args: <String, String>{
                    'count': '${visible.length}',
                    'total': '${entries.length}',
                  },
                ),
                style: theme.textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 8),
        Expanded(child: _list(context, entries, visible)),
      ],
    );
  }

  Widget _typeControl(BuildContext context, List<String> kinds) {
    // The active filter is re-added when the loaded entries no longer contain
    // it, otherwise the DropdownButton would assert after a clear or reload.
    final List<String> values = <String>[_allTypes, ...kinds];
    if (!values.contains(_typeFilter)) {
      values.add(_typeFilter);
    }
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Text(context.strings('log.filterType')),
        const SizedBox(width: 8),
        DropdownButton<String>(
          key: const ValueKey<String>('operation-log-type-filter'),
          value: _typeFilter,
          items: <DropdownMenuItem<String>>[
            for (final String value in values)
              DropdownMenuItem<String>(
                value: value,
                child: Text(
                  value == _allTypes
                      ? context.strings('log.filterAll')
                      : context.strings(value),
                ),
              ),
          ],
          onChanged: (String? value) {
            if (value != null) {
              setState(() => _typeFilter = value);
            }
          },
        ),
      ],
    );
  }

  Widget _statusControl(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Text(context.strings('log.filterStatus')),
        const SizedBox(width: 8),
        DropdownButton<String>(
          key: const ValueKey<String>('operation-log-status-filter'),
          value: _statusFilter,
          items: <DropdownMenuItem<String>>[
            DropdownMenuItem<String>(
              value: _statusAll,
              child: Text(context.strings('log.filterAll')),
            ),
            DropdownMenuItem<String>(
              value: _statusOk,
              child: Text(context.strings('log.statusOk')),
            ),
            DropdownMenuItem<String>(
              value: _statusFailed,
              child: Text(context.strings('log.statusFailed')),
            ),
          ],
          onChanged: (String? value) {
            if (value != null) {
              setState(() => _statusFilter = value);
            }
          },
        ),
      ],
    );
  }

  Widget _retentionControl(BuildContext context) {
    // A hand-edited retention.json may hold a value that is not in the menu;
    // adding it keeps the dropdown truthful instead of asserting.
    final Set<int> values = <int>{..._retentionChoices, _retention};
    final List<int> choices = values.toList()..sort();
    return Tooltip(
      message: context.strings('log.retentionDetail'),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Text(context.strings('log.retention')),
          const SizedBox(width: 8),
          DropdownButton<int>(
            key: const ValueKey<String>('operation-log-retention'),
            value: _retention,
            items: <DropdownMenuItem<int>>[
              for (final int value in choices)
                DropdownMenuItem<int>(
                  value: value,
                  child: Text(
                    context.strings(
                      'log.retentionOption',
                      args: <String, String>{'count': '$value'},
                    ),
                  ),
                ),
            ],
            onChanged: _setRetention,
          ),
        ],
      ),
    );
  }

  Widget _list(
    BuildContext context,
    List<OperationLogEntry> entries,
    List<OperationLogEntry> visible,
  ) {
    if (entries.isEmpty) {
      return EmptyState(
        icon: Icons.history_toggle_off,
        title: context.strings('history.operationsEmptyTitle'),
        message: context.strings('history.operationsEmptyBody'),
      );
    }
    if (visible.isEmpty) {
      return EmptyState(
        icon: Icons.search_off,
        title: context.strings('log.noMatches'),
        message: context.strings('log.noMatchesBody'),
      );
    }
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return ListView.separated(
      key: const ValueKey<String>('operation-log-list'),
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
      itemCount: visible.length,
      separatorBuilder: (BuildContext context, int index) =>
          const Divider(height: 1),
      itemBuilder: (BuildContext context, int index) {
        final OperationLogEntry entry = visible[index];
        final Widget tile = ListTile(
          dense: true,
          leading: Icon(
            entry.status ? Icons.check_circle : Icons.error,
            color: entry.status ? scheme.tertiary : scheme.error,
            size: 20,
          ),
          title: Text(
            context.strings(entry.kind),
            style: theme.textTheme.bodyMedium,
          ),
          subtitle: Text(
            <String>[
              formatTimestamp(entry.startedAt),
              if (entry.durationMs != null)
                context.strings(
                  'history.operationDuration',
                  args: <String, String>{'ms': '${entry.durationMs}'},
                ),
              if (entry.detail != null) entry.detail!,
            ].join('  ·  '),
            style: theme.textTheme.bodySmall?.copyWith(
              color: entry.status ? scheme.onSurfaceVariant : scheme.error,
            ),
          ),
        );
        if (entry.hashes.isEmpty) {
          return tile;
        }
        // Digests are the verifiable payload, so they stay collapsed: a batch
        // can hold hundreds of them. The key mixes content with the row index
        // so filtering or reordering cannot move an expanded section onto a
        // different operation.
        return ExpansionTile(
          key: ValueKey<String>(
            'operation-log-hashes-${entry.kind}-'
            '${entry.startedAt.microsecondsSinceEpoch}-$index',
          ),
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsets.only(left: 40, bottom: 8),
          expandedCrossAxisAlignment: CrossAxisAlignment.start,
          title: tile,
          children: <Widget>[
            for (final HashDigestRecord record in entry.hashes)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    SizedBox(
                      width: 90,
                      child: Text(
                        record.algorithm,
                        style: theme.textTheme.labelSmall,
                      ),
                    ),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: <Widget>[
                          Text(
                            record.path,
                            style: theme.textTheme.bodySmall,
                          ),
                          SelectableText(
                            record.digest,
                            style: theme.textTheme.bodySmall?.copyWith(
                              fontFamily: 'monospace',
                              color: scheme.onSurfaceVariant,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            if (entry.hashesTruncated > 0)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(
                  context.strings(
                    'history.operationHashesTruncated',
                    args: <String, String>{
                      'count': '${entry.hashesTruncated}',
                    },
                  ),
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
          ],
        );
      },
    );
  }
}
