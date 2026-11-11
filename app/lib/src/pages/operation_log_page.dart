import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../models/operation_log.dart';
import '../services/operation_log_store.dart';
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

class _OperationLogPageState extends State<OperationLogPage> {
  late Future<List<OperationLogEntry>> _future;

  @override
  void initState() {
    super.initState();
    _future = widget.store.load();
  }

  void _reload() {
    setState(() {
      _future = widget.store.load();
    });
  }

  Future<void> _clear() async {
    await widget.store.clear();
    if (!mounted) {
      return;
    }
    _reload();
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: Text(context.strings('history.operationsTitle')),
        actions: <Widget>[
          IconButton(
            key: const ValueKey<String>('operation-log-refresh'),
            tooltip: context.strings('history.refresh'),
            onPressed: _reload,
            icon: const Icon(Icons.refresh),
          ),
          IconButton(
            key: const ValueKey<String>('operation-log-clear'),
            tooltip: context.strings('history.clear'),
            onPressed: _clear,
            icon: const Icon(Icons.delete_outline),
          ),
        ],
      ),
      body: FutureBuilder<List<OperationLogEntry>>(
        future: _future,
        builder: (
          BuildContext context,
          AsyncSnapshot<List<OperationLogEntry>> snapshot,
        ) {
          if (snapshot.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snapshot.hasError) {
            return Center(
              child: Text(
                context.strings(
                  'history.operationsFailed',
                  args: <String, String>{'error': '${snapshot.error}'},
                ),
              ),
            );
          }
          final List<OperationLogEntry> entries =
              snapshot.data ?? const <OperationLogEntry>[];
          if (entries.isEmpty) {
            return EmptyState(
              icon: Icons.history_toggle_off,
              title: context.strings('history.operationsEmptyTitle'),
              message: context.strings('history.operationsEmptyBody'),
            );
          }
          return ListView.separated(
            key: const ValueKey<String>('operation-log-list'),
            padding: const EdgeInsets.all(16),
            itemCount: entries.length,
            separatorBuilder: (_, _) => const Divider(height: 1),
            itemBuilder: (BuildContext context, int index) {
              final OperationLogEntry entry = entries[index];
              return ListTile(
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
                        args: <String, String>{
                          'ms': '${entry.durationMs}',
                        },
                      ),
                    if (entry.detail != null) entry.detail!,
                  ].join('  ·  '),
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: entry.status ? scheme.onSurfaceVariant : scheme.error,
                  ),
                ),
              );
            },
          );
        },
      ),
    );
  }
}