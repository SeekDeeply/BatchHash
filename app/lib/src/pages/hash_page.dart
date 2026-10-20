import 'dart:async';

import 'package:flutter/material.dart';

import '../app_state.dart';
import '../models/models.dart';
import '../theme.dart';
import '../util/formatting.dart';
import '../widgets/empty_state.dart';
import '../widgets/entry_list.dart';
import '../widgets/summary_card.dart';

/// Batch hashing workflow: choose input, pick algorithms, hash, inspect.
class HashPage extends StatefulWidget {
  const HashPage({super.key, required this.state});

  final BatchHashAppState state;

  @override
  State<HashPage> createState() => _HashPageState();
}

class _HashPageState extends State<HashPage> {
  static const String _sampleFolder = '/home/user/Documents';
  static const List<String> _defaultAlgorithms = <String>['blake3', 'sha256'];

  final Set<String> _algorithms = <String>{..._defaultAlgorithms};
  List<BatchEntry> _entries = const <BatchEntry>[];
  bool _scanning = false;
  bool _computing = false;
  bool _cancelled = false;
  String? _error;
  HashProgress? _progress;
  StreamSubscription<HashProgress>? _subscription;
  Duration? _elapsed;

  @override
  void dispose() {
    unawaited(_subscription?.cancel());
    super.dispose();
  }

  Future<void> _selectFiles() async {
    await _scanFolder(recursive: false);
  }

  Future<void> _selectFolder() async {
    final bool? recursive = await showDialog<bool>(
      context: context,
      builder: (BuildContext dialogContext) => AlertDialog(
        title: const Text('Scan folder'),
        content: const Text('Include files inside subfolders?'),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: const Text('This folder only'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: const Text('Include subfolders'),
          ),
        ],
      ),
    );
    if (recursive == null) {
      return;
    }
    await _scanFolder(recursive: recursive);
  }

  Future<void> _scanFolder({required bool recursive}) async {
    setState(() {
      _scanning = true;
      _error = null;
      _cancelled = false;
    });
    try {
      final List<BatchEntry> entries = await widget.state.service
          .scanDirectory(_sampleFolder, recursive: recursive);
      if (!mounted) {
        return;
      }
      setState(() {
        _entries = entries;
        _progress = null;
        _elapsed = null;
        _scanning = false;
      });
      widget.state.entries.value = entries;
      widget.state.report.value = null;
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _scanning = false;
        _error = 'Could not scan the selected input: $error';
      });
    }
  }

  void _clear() {
    setState(() {
      _entries = const <BatchEntry>[];
      _progress = null;
      _elapsed = null;
      _cancelled = false;
      _error = null;
    });
    widget.state.entries.value = const <BatchEntry>[];
    widget.state.report.value = null;
  }

  void _toggleAlgorithm(String algorithm, bool selected) {
    setState(() {
      if (selected) {
        _algorithms.add(algorithm);
      } else if (_algorithms.length > 1) {
        _algorithms.remove(algorithm);
      }
    });
  }

  void _startHashing() {
    if (_entries.isEmpty || _algorithms.isEmpty) {
      return;
    }
    setState(() {
      _computing = true;
      _cancelled = false;
      _error = null;
      _progress = null;
      _elapsed = null;
    });
    final Stopwatch stopwatch = Stopwatch()..start();
    final List<String> algorithms = _algorithms.toList()..sort();
    final Stream<HashProgress> stream = widget.state.service.computeHashes(
      entries: _entries,
      algorithms: algorithms,
    );
    _subscription = stream.listen(
      (HashProgress progress) {
        if (!mounted) {
          return;
        }
        setState(() {
          _progress = progress;
          if (progress.entries.isNotEmpty) {
            _entries = progress.entries;
          }
        });
        if (progress.done) {
          stopwatch.stop();
          _finishHashing(progress, stopwatch.elapsed, algorithms);
        }
      },
      onError: (Object error) {
        if (!mounted) {
          return;
        }
        setState(() {
          _computing = false;
          _error = 'Hashing failed: $error';
        });
      },
      onDone: () {
        if (!mounted) {
          return;
        }
        if (_computing && !_cancelled) {
          setState(() {
            _computing = false;
          });
        }
      },
    );
  }

  void _finishHashing(
    HashProgress progress,
    Duration elapsed,
    List<String> algorithms,
  ) {
    final ReportModel report = ReportModel(
      root: _sampleFolder,
      generatedAt: DateTime.now(),
      algorithms: algorithms,
      entries: progress.entries,
    );
    _subscription = null;
    setState(() {
      _computing = false;
      _progress = progress;
      _entries = progress.entries;
      _elapsed = elapsed;
    });
    widget.state.entries.value = progress.entries;
    widget.state.report.value = report;
  }

  Future<void> _cancelHashing() async {
    final StreamSubscription<HashProgress>? subscription = _subscription;
    _subscription = null;
    await subscription?.cancel();
    if (!mounted) {
      return;
    }
    setState(() {
      _computing = false;
      _cancelled = true;
      _progress = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? error = _error;
    final HashProgress? progress = _progress;
    final Duration? elapsed = _elapsed;
    final int okCount =
        _entries.where((BatchEntry entry) => entry.status == EntryStatus.ok).length;
    final int failedCount = _entries
        .where((BatchEntry entry) => entry.status == EntryStatus.failed)
        .length;
    final int skippedCount = _entries
        .where((BatchEntry entry) => entry.status == EntryStatus.skipped)
        .length;
    final int totalBytes = _entries.fold<int>(
      0,
      (int sum, BatchEntry entry) => sum + entry.size,
    );
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text('Batch hashing', style: theme.textTheme.headlineSmall),
          const SizedBox(height: 4),
          Text(
            'Pick files or a folder, choose the algorithms, then hash everything '
            'in a single pass. Files never leave this device.',
            style: theme.textTheme.bodyMedium
                ?.copyWith(color: scheme.onSurfaceVariant),
          ),
          const SizedBox(height: 16),
          _buildToolbar(),
          if (error != null) ...<Widget>[
            const SizedBox(height: 12),
            _MessageCard(message: error, isError: true),
          ],
          if (_cancelled) ...<Widget>[
            const SizedBox(height: 12),
            const _MessageCard(
              message: 'Hashing was cancelled. Partial results are shown.',
              isError: false,
            ),
          ],
          const SizedBox(height: 16),
          Text('Algorithms', style: theme.textTheme.titleSmall),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: <Widget>[
              for (final String algorithm in kAvailableAlgorithms)
                FilterChip(
                  key: ValueKey<String>('hash-algorithm-$algorithm'),
                  label: Text(algorithm),
                  selected: _algorithms.contains(algorithm),
                  onSelected: _computing
                      ? null
                      : (bool selected) =>
                          _toggleAlgorithm(algorithm, selected),
                ),
            ],
          ),
          const SizedBox(height: 16),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: <Widget>[
              FilledButton.icon(
                key: const ValueKey<String>('hash-start'),
                onPressed: _entries.isEmpty || _computing || _scanning
                    ? null
                    : _startHashing,
                icon: const Icon(Icons.play_arrow),
                label: Text(_computing ? 'Hashing...' : 'Start hashing'),
              ),
              if (_computing)
                OutlinedButton.icon(
                  key: const ValueKey<String>('hash-cancel'),
                  onPressed: _cancelHashing,
                  icon: const Icon(Icons.stop),
                  label: const Text('Cancel'),
                ),
            ],
          ),
          if (_scanning) ...<Widget>[
            const SizedBox(height: 12),
            const LinearProgressIndicator(),
          ],
          if (progress != null && _computing) ...<Widget>[
            const SizedBox(height: 12),
            LinearProgressIndicator(
              value: progress.fraction,
              semanticsLabel: 'Hash progress',
            ),
            const SizedBox(height: 4),
            Text(
              'Hashing file ${progress.completed} of ${progress.total}...',
              style: theme.textTheme.bodySmall,
            ),
          ],
          if (_entries.isNotEmpty) ...<Widget>[
            const SizedBox(height: 16),
            SummaryCard(
              title: 'Batch summary',
              metrics: <SummaryMetric>[
                SummaryMetric(label: 'Files', value: '${_entries.length}'),
                SummaryMetric(
                  label: 'OK',
                  value: '$okCount',
                  color: scheme.tertiary,
                ),
                SummaryMetric(
                  label: 'Failed',
                  value: '$failedCount',
                  color: scheme.error,
                ),
                SummaryMetric(
                  label: 'Skipped',
                  value: '$skippedCount',
                  color: scheme.outline,
                ),
                SummaryMetric(
                  label: 'Total size',
                  value: formatBytes(totalBytes),
                ),
                if (elapsed != null)
                  SummaryMetric(
                    label: 'Elapsed',
                    value: '${elapsed.inMilliseconds} ms',
                  ),
              ],
            ),
            const SizedBox(height: 16),
            Row(
              children: <Widget>[
                Expanded(
                  child: Text('Results', style: theme.textTheme.titleMedium),
                ),
                Text(
                  '${_entries.length} files',
                  style: theme.textTheme.bodySmall,
                ),
              ],
            ),
            const SizedBox(height: 8),
            EntryList(entries: _entries),
          ] else if (!_scanning) ...<Widget>[
            const SizedBox(height: 48),
            const EmptyState(
              icon: Icons.fingerprint,
              title: 'No files selected',
              message: 'Choose files or a folder to start hashing. '
                  'Everything stays on this device.',
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildToolbar() {
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: <Widget>[
        FilledButton.icon(
          key: const ValueKey<String>('hash-choose-files'),
          onPressed: _computing || _scanning ? null : _selectFiles,
          icon: const Icon(Icons.insert_drive_file_outlined),
          label: const Text('Choose files'),
        ),
        OutlinedButton.icon(
          key: const ValueKey<String>('hash-choose-folder'),
          onPressed: _computing || _scanning ? null : _selectFolder,
          icon: const Icon(Icons.folder_open_outlined),
          label: const Text('Choose folder'),
        ),
        if (_entries.isNotEmpty)
          TextButton.icon(
            onPressed: _computing || _scanning ? null : _clear,
            icon: const Icon(Icons.clear_all),
            label: const Text('Clear'),
          ),
      ],
    );
  }
}

class _MessageCard extends StatelessWidget {
  const _MessageCard({required this.message, required this.isError});

  final String message;
  final bool isError;

  @override
  Widget build(BuildContext context) {
    final ColorScheme scheme = Theme.of(context).colorScheme;
    final Color background =
        isError ? scheme.errorContainer : scheme.secondaryContainer;
    final Color foreground =
        isError ? scheme.onErrorContainer : scheme.onSecondaryContainer;
    return Card(
      color: background,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Icon(
              isError ? Icons.error_outline : Icons.info_outline,
              color: foreground,
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                message,
                style: Theme.of(context)
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: foreground),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
