import 'dart:async';

import 'package:flutter/material.dart';

import '../app_state.dart';
import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../util/formatting.dart';
import '../widgets/empty_state.dart';
import '../widgets/entry_list.dart';
import '../widgets/path_input_dialog.dart';
import '../widgets/summary_card.dart';

/// Batch hashing workflow: choose input, pick algorithms, hash, inspect.
///
/// Input paths come from the in-app path chooser instead of a native picker.
/// The scanned entries, the progress summary, the last error and the selected
/// algorithms are mirrored into [BatchHashAppState] so a tab switch does not
/// throw the batch away.
class HashPage extends StatefulWidget {
  const HashPage({super.key, required this.state});

  final BatchHashAppState state;

  @override
  State<HashPage> createState() => _HashPageState();
}

class _HashPageState extends State<HashPage> {
  late Set<String> _algorithms;
  late List<BatchEntry> _entries;
  late HashProgress? _progress;
  late Duration? _elapsed;
  late String? _error;
  late bool _cancelled;
  bool _scanning = false;
  bool _computing = false;
  StreamSubscription<HashProgress>? _subscription;

  @override
  void initState() {
    super.initState();
    final BatchHashAppState app = widget.state;
    _algorithms = <String>{...app.algorithms.value};
    _entries = app.entries.value;
    _progress = app.progress.value;
    _elapsed = app.elapsed.value;
    _error = app.error.value;
    _cancelled = app.cancelled.value;
  }

  @override
  void dispose() {
    unawaited(_subscription?.cancel());
    super.dispose();
  }

  Future<void> _selectFiles() async {
    final BatchHashAppState app = widget.state;
    final String? input = await PathInputDialog.show(
      context,
      title: context.strings('hash.chooseFiles'),
      hintText: context.strings('path.hint.files'),
      helperText: context.strings('path.filesHelper'),
      allowMultiple: true,
      suggestions: app.recentPaths.value,
    );
    if (!mounted) {
      return;
    }
    if (input == null) {
      return;
    }
    final List<String> paths = <String>[
      for (final String line in input.split('\n'))
        if (line.trim().isNotEmpty) line.trim(),
    ];
    if (paths.isEmpty) {
      return;
    }
    await _scanInputs(paths, recursive: false);
  }

  Future<void> _selectFolder() async {
    final BatchHashAppState app = widget.state;
    final String? input = await PathInputDialog.show(
      context,
      title: context.strings('hash.chooseFolder'),
      hintText: context.strings('path.hint.folder'),
      helperText: context.strings('path.folderHelper'),
      initialValue: app.recentPaths.value.isEmpty
          ? ''
          : app.recentPaths.value.first,
      suggestions: app.recentPaths.value,
    );
    if (!mounted) {
      return;
    }
    if (input == null || input.trim().isEmpty) {
      return;
    }
    final String folder = input.trim();
    final bool? recursive = await showDialog<bool>(
      context: context,
      builder: (BuildContext dialogContext) => AlertDialog(
        title: Text(context.strings('hash.recursiveTitle')),
        content: Text(context.strings('hash.recursiveQuestion')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text(context.strings('hash.recursiveNo')),
          ),
          FilledButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: Text(context.strings('hash.recursiveYes')),
          ),
        ],
      ),
    );
    if (!mounted) {
      return;
    }
    if (recursive == null) {
      return;
    }
    await _scanInputs(<String>[folder], recursive: recursive);
  }

  /// Scan every chosen path; one file is passed straight to the service.
  Future<void> _scanInputs(
    List<String> paths, {
    required bool recursive,
  }) async {
    setState(() {
      _scanning = true;
      _error = null;
      _cancelled = false;
    });
    try {
      final List<BatchEntry> scanned = <BatchEntry>[];
      for (final String path in paths) {
        final List<BatchEntry> entries = await widget.state.service
            .scanDirectory(path, recursive: recursive);
        scanned.addAll(entries);
      }
      if (!mounted) {
        return;
      }
      final BatchHashAppState app = widget.state;
      for (final String path in paths.reversed) {
        app.rememberPath(path);
      }
      setState(() {
        _entries = scanned;
        _progress = null;
        _elapsed = null;
        _scanning = false;
      });
      app.entries.value = scanned;
      app.report.value = null;
      app.progress.value = null;
      app.elapsed.value = null;
      app.error.value = null;
      app.cancelled.value = false;
      app.scanRoot.value = paths.first;
    } catch (error) {
      if (!mounted) {
        return;
      }
      final String failure = context.strings(
        'hash.scanFailed',
        args: <String, String>{'error': '$error'},
      );
      setState(() {
        _scanning = false;
        _error = failure;
      });
      widget.state.error.value = failure;
    }
  }

  void _clear() {
    final BatchHashAppState app = widget.state;
    setState(() {
      _entries = const <BatchEntry>[];
      _progress = null;
      _elapsed = null;
      _cancelled = false;
      _error = null;
    });
    app.entries.value = const <BatchEntry>[];
    app.report.value = null;
    app.progress.value = null;
    app.elapsed.value = null;
    app.error.value = null;
    app.cancelled.value = false;
    app.scanRoot.value = '';
  }

  void _toggleAlgorithm(String algorithm, bool selected) {
    setState(() {
      if (selected) {
        _algorithms.add(algorithm);
      } else if (_algorithms.length > 1) {
        _algorithms.remove(algorithm);
      }
    });
    widget.state.algorithms.value = <String>{..._algorithms};
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
    widget.state.error.value = null;
    widget.state.cancelled.value = false;
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
        widget.state.progress.value = progress;
        if (progress.entries.isNotEmpty) {
          widget.state.entries.value = progress.entries;
        }
        if (progress.done) {
          stopwatch.stop();
          _finishHashing(progress, stopwatch.elapsed, algorithms);
        }
      },
      onError: (Object error) {
        if (!mounted) {
          return;
        }
        final String failure = context.strings(
          'hash.hashFailed',
          args: <String, String>{'error': '$error'},
        );
        setState(() {
          _computing = false;
          _error = failure;
        });
        widget.state.error.value = failure;
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
    final BatchHashAppState app = widget.state;
    final String root = app.scanRoot.value.trim();
    final String reportRoot = root.isNotEmpty
        ? root
        : (progress.entries.isEmpty ? '' : progress.entries.first.path);
    final ReportModel report = ReportModel(
      root: reportRoot,
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
    app.entries.value = progress.entries;
    app.report.value = report;
    app.progress.value = progress;
    app.elapsed.value = elapsed;
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
    widget.state.cancelled.value = true;
    widget.state.progress.value = null;
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? error = _error;
    final HashProgress? progress = _progress;
    final Duration? elapsed = _elapsed;
    final String progressText = progress == null
        ? ''
        : context.strings(
            'hash.progressText',
            args: <String, String>{
              'done': '${progress.completed}',
              'total': '${progress.total}',
            },
          );
    final int okCount = _entries
        .where((BatchEntry entry) => entry.status == EntryStatus.ok)
        .length;
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
          Text(
            context.strings('hash.title'),
            style: theme.textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(
            context.strings('hash.subtitle'),
            style: theme.textTheme.bodyMedium?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 16),
          _buildToolbar(),
          if (error != null) ...<Widget>[
            const SizedBox(height: 12),
            _MessageCard(message: error, isError: true),
          ],
          if (_cancelled) ...<Widget>[
            const SizedBox(height: 12),
            _MessageCard(
              message: context.strings('hash.cancelled'),
              isError: false,
            ),
          ],
          const SizedBox(height: 16),
          Text(
            context.strings('hash.algorithms'),
            style: theme.textTheme.titleSmall,
          ),
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
                label: Text(
                  context.strings(_computing ? 'hash.hashing' : 'hash.start'),
                ),
              ),
              if (_computing)
                OutlinedButton.icon(
                  key: const ValueKey<String>('hash-cancel'),
                  onPressed: _cancelHashing,
                  icon: const Icon(Icons.stop),
                  label: Text(context.strings('hash.cancel')),
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
              semanticsLabel: context.strings('hash.progressLabel'),
            ),
            const SizedBox(height: 4),
            Text(progressText, style: theme.textTheme.bodySmall),
          ],
          if (_entries.isNotEmpty) ...<Widget>[
            const SizedBox(height: 16),
            SummaryCard(
              title: context.strings('hash.summary'),
              metrics: <SummaryMetric>[
                SummaryMetric(
                  label: context.strings('metric.files'),
                  value: '${_entries.length}',
                ),
                SummaryMetric(
                  label: context.strings('metric.ok'),
                  value: '$okCount',
                  color: scheme.tertiary,
                ),
                SummaryMetric(
                  label: context.strings('metric.failed'),
                  value: '$failedCount',
                  color: scheme.error,
                ),
                SummaryMetric(
                  label: context.strings('metric.skipped'),
                  value: '$skippedCount',
                  color: scheme.outline,
                ),
                SummaryMetric(
                  label: context.strings('metric.totalSize'),
                  value: formatBytes(totalBytes),
                ),
                if (elapsed != null)
                  SummaryMetric(
                    label: context.strings('metric.elapsed'),
                    value: '${elapsed.inMilliseconds} ms',
                  ),
              ],
            ),
            const SizedBox(height: 16),
            Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    context.strings('hash.results'),
                    style: theme.textTheme.titleMedium,
                  ),
                ),
                Text(
                  context.strings(
                    'common.fileCount',
                    args: <String, String>{'count': '${_entries.length}'},
                  ),
                  style: theme.textTheme.bodySmall,
                ),
              ],
            ),
            const SizedBox(height: 8),
            EntryList(entries: _entries),
          ] else if (!_scanning) ...<Widget>[
            const SizedBox(height: 48),
            EmptyState(
              icon: Icons.fingerprint,
              title: context.strings('hash.noFiles'),
              message: context.strings('hash.noFilesHint'),
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
          label: Text(context.strings('hash.chooseFiles')),
        ),
        OutlinedButton.icon(
          key: const ValueKey<String>('hash-choose-folder'),
          onPressed: _computing || _scanning ? null : _selectFolder,
          icon: const Icon(Icons.folder_open_outlined),
          label: Text(context.strings('hash.chooseFolder')),
        ),
        if (_entries.isNotEmpty)
          TextButton.icon(
            onPressed: _computing || _scanning ? null : _clear,
            icon: const Icon(Icons.clear_all),
            label: Text(context.strings('hash.clear')),
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
                style: Theme.of(context).textTheme.bodySmall
                    ?.copyWith(color: foreground),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
