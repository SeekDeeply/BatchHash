import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
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
    final String id = batch.id;
    final String undoneMessage = context.strings(
      'history.undoneBatch',
      args: <String, String>{'id': id},
    );
    final String rejectedMessage = context.strings(
      'history.undoRejected',
      args: <String, String>{'id': id},
    );
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
        _message = undone ? undoneMessage : rejectedMessage;
        _future = widget.service.loadHistory();
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _busy = false;
        _message = context.strings(
          'history.undoFailed',
          args: <String, String>{'error': '$error'},
        );
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
          Text(
            context.strings('history.title'),
            style: theme.textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(
            context.strings('history.subtitle'),
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
                        child: Text(
                          context.strings(
                            'history.loadFailed',
                            args: <String, String>{'error': '$error'},
                          ),
                        ),
                      );
                    }
                    final List<HistoryBatchModel> batches =
                        snapshot.data ?? const <HistoryBatchModel>[];
                    if (batches.isEmpty) {
                      return EmptyState(
                        icon: Icons.history,
                        title: context.strings('history.noBatches'),
                        message: context.strings('history.noBatchesHint'),
                      );
                    }
                    return ListView.separated(
                      itemCount: batches.length,
                      separatorBuilder: (BuildContext context, int index) =>
                          const SizedBox(height: 8),
                      itemBuilder: (BuildContext context, int index) {
                        final HistoryBatchModel batch = batches[index];
                        final String fileCountLabel = context.strings(
                          'history.fileCount',
                          args: <String, String>{
                            'count': '${batch.fileCount}',
                          },
                        );
                        final String renamedLabel = context.strings(
                          'history.renamedCount',
                          args: <String, String>{
                            'count': '${batch.appliedCount}',
                          },
                        );
                        final String createdLabel = formatTimestamp(
                          batch.createdAt,
                          fallback: context.strings('common.unknown'),
                        );
                        return Card(
                          child: ListTile(
                            leading: Icon(
                              batch.undone ? Icons.undo : Icons.history,
                              color: batch.undone
                                  ? scheme.outline
                                  : scheme.tertiary,
                            ),
                            title: Text(
                              context.strings(
                                'history.batch',
                                args: <String, String>{'id': batch.id},
                              ),
                            ),
                            subtitle: Text(
                              '$fileCountLabel · $renamedLabel · $createdLabel',
                            ),
                            trailing: batch.undone
                                ? StatusChip(
                                    label: context.strings('history.undone'),
                                    color: scheme.outline,
                                  )
                                : TextButton(
                                    onPressed: _busy
                                        ? null
                                        : () => _undo(batch),
                                    child: Text(
                                      context.strings('history.undo'),
                                    ),
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
