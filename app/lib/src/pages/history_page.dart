import 'package:flutter/material.dart';

import '../models/models.dart';
import '../services/batch_hash_service.dart';
import '../util/formatting.dart';
import '../widgets/empty_state.dart';
import '../widgets/status_chip.dart';

/// Lists recorded rename batches and offers undo.
class HistoryPage extends StatefulWidget {
  const HistoryPage({super.key, required this.service});

  final BatchHashService service;

  @override
  State<HistoryPage> createState() => _HistoryPageState();
}

class _HistoryPageState extends State<HistoryPage> {
  late Future<List<HistoryBatchModel>> _future;
  bool _busy = false;
  String? _message;

  @override
  void initState() {
    super.initState();
    _future = widget.service.loadHistory();
  }

  Future<void> _undo(HistoryBatchModel batch) async {
    setState(() {
      _busy = true;
      _message = null;
    });
    try {
      final bool undone = await widget.service.undoBatch(batch.id);
      if (!mounted) {
        return;
      }
      setState(() {
        _busy = false;
        _message = undone
            ? 'Batch ${batch.id} was undone.'
            : 'Batch ${batch.id} could not be undone.';
        _future = widget.service.loadHistory();
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _busy = false;
        _message = 'Undo failed: $error';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? message = _message;
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text('Batch history', style: theme.textTheme.headlineSmall),
          const SizedBox(height: 4),
          Text(
            'Every rename batch is recorded so it can be undone later. '
            'Batches are listed newest first.',
            style: theme.textTheme.bodyMedium?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
          if (message != null) ...<Widget>[
            const SizedBox(height: 12),
            Card(
              color: scheme.secondaryContainer,
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Text(
                  message,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: scheme.onSecondaryContainer,
                  ),
                ),
              ),
            ),
          ],
          const SizedBox(height: 12),
          Expanded(
            child: FutureBuilder<List<HistoryBatchModel>>(
              future: _future,
              builder:
                  (
                    BuildContext context,
                    AsyncSnapshot<List<HistoryBatchModel>> snapshot,
                  ) {
                    if (snapshot.connectionState != ConnectionState.done) {
                      return const Center(child: CircularProgressIndicator());
                    }
                    final Object? error = snapshot.error;
                    if (error != null) {
                      return Center(
                        child: Text('Could not load history: $error'),
                      );
                    }
                    final List<HistoryBatchModel> batches =
                        snapshot.data ?? const <HistoryBatchModel>[];
                    if (batches.isEmpty) {
                      return const EmptyState(
                        icon: Icons.history,
                        title: 'No batches yet',
                        message:
                            'Renamed batches will appear here and can be '
                            'undone from this page.',
                      );
                    }
                    return ListView.separated(
                      itemCount: batches.length,
                      separatorBuilder: (BuildContext context, int index) =>
                          const SizedBox(height: 8),
                      itemBuilder: (BuildContext context, int index) {
                        final HistoryBatchModel batch = batches[index];
                        return Card(
                          child: ListTile(
                            leading: Icon(
                              batch.undone ? Icons.undo : Icons.history,
                              color: batch.undone
                                  ? scheme.outline
                                  : scheme.tertiary,
                            ),
                            title: Text('Batch ${batch.id}'),
                            subtitle: Text(
                              '${batch.fileCount} files · '
                              '${batch.appliedCount} renamed · '
                              '${formatTimestamp(batch.createdAt)}',
                            ),
                            trailing: batch.undone
                                ? StatusChip(
                                    label: 'undone',
                                    color: scheme.outline,
                                  )
                                : TextButton(
                                    onPressed: _busy
                                        ? null
                                        : () => _undo(batch),
                                    child: const Text('Undo'),
                                  ),
                          ),
                        );
                      },
                    );
                  },
            ),
          ),
        ],
      ),
    );
  }
}
