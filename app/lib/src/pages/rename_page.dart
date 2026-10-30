import 'package:flutter/material.dart';

import '../app_state.dart';
import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../theme.dart';
import '../util/formatting.dart';
import '../widgets/empty_state.dart';
import '../widgets/status_chip.dart';

/// Translation key for the label of one [RenameRowStatus].
String _rowStatusKey(RenameRowStatus status) => switch (status) {
  RenameRowStatus.ready => 'rename.status.ready',
  RenameRowStatus.conflict => 'rename.status.conflict',
  RenameRowStatus.skipped => 'rename.status.skipped',
  RenameRowStatus.missingHash => 'rename.status.missingHash',
  RenameRowStatus.invalidName => 'rename.status.invalidName',
  RenameRowStatus.noPermission => 'rename.status.noPermission',
  RenameRowStatus.unsupported => 'rename.status.unsupported',
};

/// Suffix based rename workflow with a pure dry run before execution.
class RenamePage extends StatefulWidget {
  const RenamePage({super.key, required this.state});

  final BatchHashAppState state;

  @override
  State<RenamePage> createState() => _RenamePageState();
}

class _RenamePageState extends State<RenamePage> {
  String _algorithm = 'blake3';
  int _length = 5;
  bool _uppercase = true;
  bool _planning = false;
  bool _executing = false;
  String? _error;
  String? _message;
  RenamePlanModel? _plan;

  Future<void> _preview(List<BatchEntry> entries) async {
    setState(() {
      _planning = true;
      _error = null;
      _message = null;
    });
    try {
      final RenamePlanModel plan = await widget.state.service.planRenames(
        entries: entries,
        algorithm: _algorithm,
        length: _length,
        uppercase: _uppercase,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _plan = plan;
        _planning = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _planning = false;
        _error = context.strings(
          'rename.previewFailed',
          args: <String, String>{'error': '$error'},
        );
      });
    }
  }

  Future<void> _execute() async {
    final RenamePlanModel? plan = _plan;
    if (plan == null) {
      return;
    }
    setState(() {
      _executing = true;
      _error = null;
      _message = null;
    });
    try {
      final RenameExecuteResult result = await widget.state.service
          .executeRenames(plan);
      if (!mounted) {
        return;
      }
      setState(() {
        _executing = false;
        _message = context.strings(
          'rename.resultSummary',
          args: <String, String>{
            'applied': '${result.applied}',
            'skipped': '${result.skipped}',
            'failed': '${result.failed}',
          },
        );
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _executing = false;
        _error = context.strings(
          'rename.executeFailed',
          args: <String, String>{'error': '$error'},
        );
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return ValueListenableBuilder<List<BatchEntry>>(
      valueListenable: widget.state.entries,
      builder: (BuildContext context, List<BatchEntry> entries, Widget? child) {
        final int hashedCount = entries
            .where((BatchEntry entry) => entry.hashes.isNotEmpty)
            .length;
        final int fileCount = hashedCount > 0 ? hashedCount : entries.length;
        final int recommended = recommendedSuffixLength(fileCount);
        final double probability = collisionProbability(fileCount, _length);
        final RenamePlanModel? plan = _plan;
        final String? error = _error;
        final String? message = _message;
        return SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                context.strings('rename.title'),
                style: theme.textTheme.headlineSmall,
              ),
              const SizedBox(height: 4),
              Text(
                context.strings('rename.subtitle'),
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
              if (entries.isEmpty) ...<Widget>[
                const SizedBox(height: 48),
                EmptyState(
                  icon: Icons.drive_file_rename_outline,
                  title: context.strings('rename.noHashed'),
                  message: context.strings('rename.noHashedHint'),
                ),
              ] else ...<Widget>[
                const SizedBox(height: 20),
                Row(
                  children: <Widget>[
                    SizedBox(
                      width: 120,
                      child: Text(
                        context.strings('rename.algorithm'),
                        style: theme.textTheme.bodyMedium,
                      ),
                    ),
                    Expanded(
                      child: InputDecorator(
                        decoration: const InputDecoration(
                          border: OutlineInputBorder(),
                          isDense: true,
                          contentPadding: EdgeInsets.symmetric(
                            horizontal: 12,
                            vertical: 8,
                          ),
                        ),
                        child: DropdownButtonHideUnderline(
                          child: DropdownButton<String>(
                            key: const ValueKey<String>(
                              'rename-algorithm-dropdown',
                            ),
                            value: _algorithm,
                            isDense: true,
                            isExpanded: true,
                            onChanged: (String? value) {
                              if (value == null) {
                                return;
                              }
                              setState(() {
                                _algorithm = value;
                                _plan = null;
                              });
                            },
                            items: <DropdownMenuItem<String>>[
                              for (final String algorithm in kSuffixAlgorithms)
                                DropdownMenuItem<String>(
                                  value: algorithm,
                                  child: Text(algorithm.toUpperCase()),
                                ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                Row(
                  children: <Widget>[
                    SizedBox(
                      width: 120,
                      child: Text(
                        context.strings('rename.length'),
                        style: theme.textTheme.bodyMedium,
                      ),
                    ),
                    Expanded(
                      child: Slider(
                        key: const ValueKey<String>('rename-length-slider'),
                        value: _length.toDouble(),
                        min: 4,
                        max: 16,
                        divisions: 12,
                        label: '$_length',
                        onChanged: (double value) {
                          setState(() {
                            _length = value.round();
                            _plan = null;
                            _message = null;
                          });
                        },
                      ),
                    ),
                    SizedBox(
                      width: 32,
                      child: Text('$_length', textAlign: TextAlign.end),
                    ),
                  ],
                ),
                SwitchListTile(
                  key: const ValueKey<String>('rename-uppercase-switch'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(context.strings('rename.uppercase')),
                  value: _uppercase,
                  onChanged: (bool value) {
                    setState(() {
                      _uppercase = value;
                      _plan = null;
                    });
                  },
                ),
                Text(
                  context.strings(
                    'rename.collisionEstimate',
                    args: <String, String>{
                      'length': '$_length',
                      'files': '$fileCount',
                      'probability': formatProbabilityPercent(probability),
                      'recommended': '$recommended',
                    },
                  ),
                  key: const ValueKey<String>('rename-collision-estimate'),
                  style: theme.textTheme.bodyMedium,
                ),
                const SizedBox(height: 16),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: <Widget>[
                    FilledButton.icon(
                      key: const ValueKey<String>('rename-dry-run-button'),
                      onPressed: _planning || _executing
                          ? null
                          : () => _preview(entries),
                      icon: const Icon(Icons.preview),
                      label: Text(context.strings('rename.preview')),
                    ),
                    FilledButton.tonalIcon(
                      key: const ValueKey<String>('rename-execute-button'),
                      onPressed:
                          plan == null || plan.readyCount == 0 || _executing
                          ? null
                          : _execute,
                      icon: const Icon(Icons.drive_file_rename_outline),
                      label: Text(context.strings('rename.apply')),
                    ),
                  ],
                ),
                if (_planning) ...<Widget>[
                  const SizedBox(height: 12),
                  const LinearProgressIndicator(),
                ],
                if (error != null) ...<Widget>[
                  const SizedBox(height: 12),
                  _InlineMessage(message: error, isError: true),
                ],
                if (message != null) ...<Widget>[
                  const SizedBox(height: 12),
                  _InlineMessage(message: message, isError: false),
                ],
                if (plan != null) ...<Widget>[
                  const SizedBox(height: 20),
                  Row(
                    children: <Widget>[
                      Expanded(
                        child: Text(
                          context.strings('rename.dryRun'),
                          style: theme.textTheme.titleMedium,
                        ),
                      ),
                      Text(
                        context.strings(
                          'rename.planCounts',
                          args: <String, String>{
                            'ready': '${plan.readyCount}',
                            'conflict': '${plan.conflictCount}',
                          },
                        ),
                        style: theme.textTheme.bodySmall,
                      ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  Card(
                    child: Column(
                      children: <Widget>[
                        for (final RenamePlanRowModel row in plan.rows)
                          _RenamePlanTile(row: row),
                      ],
                    ),
                  ),
                ],
              ],
            ],
          ),
        );
      },
    );
  }
}

class _RenamePlanTile extends StatelessWidget {
  const _RenamePlanTile({required this.row});

  final RenamePlanRowModel row;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? target = row.targetPath;
    final String? message = row.message;
    return ListTile(
      dense: true,
      title: Text(
        row.path,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: theme.textTheme.bodyMedium,
      ),
      subtitle: Text(
        target ?? message ?? context.strings('rename.noTarget'),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: theme.textTheme.bodySmall,
      ),
      trailing: StatusChip(
        label: context.strings(_rowStatusKey(row.status)),
        color: renameRowStatusColor(row.status, scheme),
        icon: renameRowStatusIcon(row.status),
      ),
    );
  }
}

class _InlineMessage extends StatelessWidget {
  const _InlineMessage({required this.message, required this.isError});

  final String message;
  final bool isError;

  @override
  Widget build(BuildContext context) {
    final ColorScheme scheme = Theme.of(context).colorScheme;
    final Color background = isError
        ? scheme.errorContainer
        : scheme.secondaryContainer;
    final Color foreground = isError
        ? scheme.onErrorContainer
        : scheme.onSecondaryContainer;
    return Card(
      color: background,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Text(
          message,
          style: Theme.of(context).textTheme.bodySmall
              ?.copyWith(color: foreground),
        ),
      ),
    );
  }
}
