import 'dart:async';

import 'package:flutter/material.dart';

import '../app_state.dart';
import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../util/formatting.dart';
import '../util/platform_pickers.dart';
import '../widgets/diff_table.dart';
import '../widgets/empty_state.dart';
import '../widgets/entry_list.dart';
import '../widgets/path_input_dialog.dart';
import '../widgets/summary_card.dart';

/// Filter applied to comparison rows.
///
/// [label] holds the translation key of the chip rather than display copy;
/// resolve it with `context.strings(filter.label)` so the chips follow the
/// active locale.
enum CompareFilter {
  all('common.all'),
  differences('compare.differences'),
  changed('compare.changed'),
  added('compare.added'),
  removed('compare.removed'),
  ambiguous('compare.ambiguous');

  const CompareFilter(this.label);

  /// Translation key for the filter chip label.
  final String label;

  bool matches(DiffStatus status) => switch (this) {
    CompareFilter.all => true,
    CompareFilter.differences => status.isDifference,
    CompareFilter.changed => status == DiffStatus.changed,
    CompareFilter.added => status == DiffStatus.added,
    CompareFilter.removed => status == DiffStatus.removed,
    CompareFilter.ambiguous => status == DiffStatus.ambiguous,
  };
}

/// Pane shown on narrow screens.
enum ComparePane { left, right, differences }

/// Where the two sides of the comparison come from.
enum CompareMode {
  directory('compare.mode.directory'),
  reports('compare.mode.reports');

  const CompareMode(this.label);

  /// Translation key for the mode selector.
  final String label;
}

/// Compares two reports: the current report against a live folder, or two
/// report files imported through the in-app path chooser.
///
/// Every user-visible string, including the diff table headers, comes from the
/// locale string table.
class ComparePage extends StatefulWidget {
  const ComparePage({super.key, required this.state});

  final BatchHashAppState state;

  @override
  State<ComparePage> createState() => _ComparePageState();
}

class _ComparePageState extends State<ComparePage> {
  static const double _narrowBreakpoint = 720;

  List<DiffRowModel> _rows = const <DiffRowModel>[];
  bool _loading = false;
  bool _importing = false;
  String? _error;
  String? _loadedReportKey;
  CompareFilter _filter = CompareFilter.all;
  ComparePane _pane = ComparePane.differences;
  CompareMode _mode = CompareMode.directory;
  ReportModel? _leftReport;
  ReportModel? _rightReport;
  String _leftPath = '';
  String _rightPath = '';
  String? _expandedPath;

  @override
  void initState() {
    super.initState();
    widget.state.report.addListener(_handleReportChanged);
    final ReportModel? report = widget.state.report.value;
    if (report != null) {
      _loadedReportKey = _reportKey(report);
      _loading = true;
      unawaited(_load(report));
    }
  }

  @override
  void dispose() {
    widget.state.report.removeListener(_handleReportChanged);
    // Leaving the page must stop the native comparison, not leave it hashing
    // a directory nobody is looking at.
    unawaited(_cancelDiffStream());
    super.dispose();
  }

  void _handleReportChanged() {
    if (_mode != CompareMode.directory) {
      return;
    }
    final ReportModel? report = widget.state.report.value;
    if (report == null) {
      if (_loadedReportKey != null) {
        setState(() {
          _loadedReportKey = null;
          _rows = const <DiffRowModel>[];
          _loading = false;
          _error = null;
        });
      }
      return;
    }
    final String key = _reportKey(report);
    if (key == _loadedReportKey) {
      return;
    }
    _loadedReportKey = key;
    setState(() {
      _loading = true;
      _error = null;
      _rows = const <DiffRowModel>[];
    });
    unawaited(_load(report));
  }

  String _reportKey(ReportModel report) =>
      '${report.root}|${report.generatedAt.microsecondsSinceEpoch}';

  /// Generation of the comparison currently being streamed.

  ///
  /// A directory comparison hashes every scanned file, so it can outlive the
  /// widget's interest in it: switching mode, importing another report or
  /// starting a new run bumps the generation, and a late sample from an older
  /// stream is dropped instead of overwriting newer rows.
  int _generation = 0;
  StreamSubscription<DiffProgress>? _diffSubscription;
  DiffProgress? _diffProgress;

  Future<void> _load(ReportModel report) async {
    await _startDirectoryDiff(report, report.root);
  }

  Future<void> _startDirectoryDiff(
    ReportModel report,
    String directory,
  ) async {
    await _cancelDiffStream();
    final int generation = ++_generation;
    _diffSubscription = widget.state.service
        .compareReportWithDirectory(report: report, directory: directory)
        .listen(
          (DiffProgress progress) {
            if (!mounted || generation != _generation) {
              return;
            }
            setState(() {
              _diffProgress = progress;
              if (progress.done) {
                _loading = false;
                if (progress.cancelled) {
                  _error = null;
                } else {
                  _rows = progress.rows;
                }
              }
            });
          },
          onError: (Object error) {
            if (!mounted || generation != _generation) {
              return;
            }
            final String failure = context.strings(
              'compare.compareFailed',
              args: <String, String>{'error': '$error'},
            );
            setState(() {
              _loading = false;
              _error = failure;
            });
          },
        );
  }

  /// Stop the running comparison and forget its progress.
  Future<void> _cancelDiffStream({bool cancelJob = true}) async {
    final StreamSubscription<DiffProgress>? subscription = _diffSubscription;
    _diffSubscription = null;
    _diffProgress = null;
    if (subscription != null) {
      await subscription.cancel();
      if (cancelJob) {
        // Dropping the subscription alone would leave the native worker
        // hashing the directory in the background.
        await widget.state.service.cancelCurrentOperation();
      }
    }
  }

  Future<void> _cancelComparison() async {
    await _cancelDiffStream();
    if (!mounted) {
      return;
    }
    _generation += 1;
    setState(() {
      _loading = false;
      _error = null,
      _rows = const <DiffRowModel>[],
    });
  }

  void _changeMode(CompareMode mode) {
    if (mode == _mode) {
      return;
    }
    unawaited(_cancelDiffStream());
    _generation += 1;
    setState(() {
      _mode = mode;
      _rows = const <DiffRowModel>[];
      _error = null;
      _loading = false;
      _loadedReportKey = null;
      _diffProgress = null;
    });
    if (mode == CompareMode.directory) {
      _handleReportChanged();
    } else if (_leftReport != null && _rightReport != null) {
      unawaited(_diffImported());
    }
  }

  Future<void> _importReport({required bool left}) async {
    final BatchHashAppState app = widget.state;
    final String titleKey = left
        ? 'compare.importLeft'
        : 'compare.importRight';
    final List<String> picked = await PlatformPickers.files(() async {
      final String? typed = await PathInputDialog.show(
        context,
        title: context.strings(titleKey),
        hintText: context.strings('path.hint.report'),
        suggestions: app.recentPaths.value,
      );
      return typed == null || typed.trim().isEmpty
          ? const <String>[]
          : <String>[typed.trim()];
    });
    final String? input = picked.isEmpty ? null : picked.first;
    if (!mounted) {
      return;
    }
    if (input == null || input.trim().isEmpty) {
      return;
    }
    final String path = input.trim();
    setState(() {
      _importing = true;
      _error = null;
    });
    try {
      final ReportModel report = await widget.state.service.importReport(path);
      if (!mounted) {
        return;
      }
      app.rememberPath(path);
      setState(() {
        _importing = false;
        if (left) {
          _leftReport = report;
          _leftPath = path;
        } else {
          _rightReport = report;
          _rightPath = path;
        }
      });
      await _diffImported();
    } catch (error) {
      if (!mounted) {
        return;
      }
      final String failure = context.strings(
        'compare.importFailed',
        args: <String, String>{'error': '$error'},
      );
      setState(() {
        _importing = false;
        _error = failure;
      });
    }
  }

  Future<void> _diffImported() async {
    final ReportModel? left = _leftReport;
    final ReportModel? right = _rightReport;
    if (left == null || right == null) {
      return;
    }
    // Comparing two stored reports is pure but still proportional to their
    // size, so it runs off the UI isolate and honours the same generation
    // guard as the directory comparison.
    await _cancelDiffStream();
    final int generation = ++_generation;
    if (!mounted) {
      return;
    }
    setState(() {
      _loading = true;
      _error = null;
      _diffProgress = null;
    });
    try {
      final List<DiffRowModel> rows = await widget.state.service.diffReports(
        left: left,
        right: right,
      );
      if (!mounted || generation != _generation) {
        return;
      }
      setState(() {
        _rows = rows;
        _loading = false;
      });
    } catch (error) {
      if (!mounted || generation != _generation) {
        return;
      }
      final String failure = context.strings(
        'compare.compareFailed',
        args: <String, String>{'error': '$error'},
      );
      setState(() {
        _loading = false;
        _error = failure;
      });
    }
  }

  void _toggleRow(DiffRowModel row) {
    setState(() {
      _expandedPath = _expandedPath == row.path ? null : row.path;
    });
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final double width = MediaQuery.sizeOf(context).width;
    final bool narrow = width < _narrowBreakpoint;
    final String? error = _error;
    final List<DiffRowModel> visibleRows = _rows
        .where((DiffRowModel row) => _filter.matches(row.status))
        .toList();
    final int changedCount = _rows
        .where((DiffRowModel row) => row.status == DiffStatus.changed)
        .length;
    final int addedCount = _rows
        .where((DiffRowModel row) => row.status == DiffStatus.added)
        .length;
    final int removedCount = _rows
        .where((DiffRowModel row) => row.status == DiffStatus.removed)
        .length;
    final int ambiguousCount = _rows
        .where((DiffRowModel row) => row.status == DiffStatus.ambiguous)
        .length;
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            context.strings('compare.title'),
            style: theme.textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(
            context.strings('compare.subtitle'),
            style: theme.textTheme.bodyMedium?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 16),
          _buildModeSelector(),
          const SizedBox(height: 12),
          const _PolicyBanner(),
          if (_mode == CompareMode.reports) ...<Widget>[
            const SizedBox(height: 12),
            _buildReportInputs(),
          ],
          const SizedBox(height: 12),
          SummaryCard(
            title: context.strings('compare.summary'),
            metrics: <SummaryMetric>[
              SummaryMetric(
                label: context.strings('compare.rows'),
                value: '${_rows.length}',
              ),
              SummaryMetric(
                label: context.strings('compare.changed'),
                value: '$changedCount',
                color: scheme.error,
              ),
              SummaryMetric(
                label: context.strings('compare.added'),
                value: '$addedCount',
                color: scheme.tertiary,
              ),
              SummaryMetric(
                label: context.strings('compare.removed'),
                value: '$removedCount',
                color: scheme.secondary,
              ),
              SummaryMetric(
                label: context.strings('compare.ambiguous'),
                value: '$ambiguousCount',
                color: scheme.tertiary,
              ),
            ],
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: <Widget>[
              for (final CompareFilter filter in CompareFilter.values)
                FilterChip(
                  key: ValueKey<String>('compare-filter-${filter.name}'),
                  label: Text(context.strings(filter.label)),
                  selected: _filter == filter,
                  onSelected: (bool selected) {
                    setState(() {
                      _filter = filter;
                    });
                  },
                ),
            ],
          ),
          if (narrow) ...<Widget>[
            const SizedBox(height: 12),
            _buildPaneSelector(),
          ],
          const SizedBox(height: 12),
          if (_loading) ...<Widget>[
            _buildLoadingProgress(),
          ] else if (error != null)
            _MessageCard(message: error, isError: true)
          else if (_rows.isEmpty)
            EmptyState(
              icon: Icons.compare_arrows,
              title: context.strings('compare.nothing'),
              message: context.strings('compare.nothingHint'),
            )
          else if (narrow)
            _buildNarrowContent(visibleRows)
          else
            _buildDiffTable(visibleRows, leftKey: 'compare.baseline'),
        ],
      ),
    );
  }


  /// Progress area shown while a comparison runs.
  ///
  /// A determinate bar only appears once the native side reports a total; an
  /// indeterminate one before that would claim knowledge the app does not have.
  Widget _buildLoadingProgress() {
    final ThemeData theme = Theme.of(context);
    final DiffProgress? progress = _diffProgress;
    final double? value =
        progress != null && progress.isDeterminate ? progress.fraction : null;
    final String phaseKey = switch (progress?.phase) {
      HashPhase.scanning => 'compare.phaseScanning',
      HashPhase.hashing => 'compare.phaseHashing',
      HashPhase.comparing => 'compare.phaseComparing',
      HashPhase.completed => 'compare.phaseComparing',
      _ => 'compare.phaseHashing',
    };
    return Column(
      key: const ValueKey<String>('compare-loading-progress'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        LinearProgressIndicator(
          value: value,
          semanticsLabel: context.strings('compare.title'),
        ),
        const SizedBox(height: 6),
        Text(context.strings(phaseKey), style: theme.textTheme.bodyMedium),
        if (progress != null && progress.bytesTotal > 0) ...<Widget>[
          const SizedBox(height: 2),
          Text(
            context.strings(
              'compare.progressBytes',
              args: <String, String>{
                'done': formatBytes(progress.bytesDone),
                'total': formatBytes(progress.bytesTotal),
              },
            ),
            style: theme.textTheme.bodySmall,
          ),
        ],
        if (progress != null && progress.currentPath.isNotEmpty) ...<Widget>[
          const SizedBox(height: 2),
          Text(
            context.strings(
              'compare.progressCurrent',
              args: <String, String>{'path': progress.currentPath},
            ),
            style: theme.textTheme.bodySmall,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
          ),
        ],
        const SizedBox(height: 8),
        OutlinedButton.icon(
          key: const ValueKey<String>('compare-cancel-loading'),
          onPressed: _cancelComparison,
          icon: const Icon(Icons.stop),
          label: Text(context.strings('compare.cancelLoading')),
        ),
      ],
    );
  }
  Widget _buildDiffTable(
    List<DiffRowModel> rows, {
    required String leftKey,
  }) {
    return DiffTable(
      rows: rows,
      expandedPath: _expandedPath,
      onRowTap: _toggleRow,
      leftLabel: context.strings(leftKey),
      rightLabel: context.strings('compare.current'),
    );
  }

  Widget _buildModeSelector() {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: SegmentedButton<CompareMode>(
        key: const ValueKey<String>('compare-mode-selector'),
        showSelectedIcon: false,
        segments: <ButtonSegment<CompareMode>>[
          for (final CompareMode mode in CompareMode.values)
            ButtonSegment<CompareMode>(
              value: mode,
              label: Text(context.strings(mode.label)),
            ),
        ],
        selected: <CompareMode>{_mode},
        onSelectionChanged: (Set<CompareMode> selection) {
          _changeMode(selection.first);
        },
      ),
    );
  }

  Widget _buildReportInputs() {
    final String emptyLabel = context.strings('compare.noReportChosen');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _ReportSlot(
          slot: 'left',
          label: context.strings('compare.baseline'),
          path: _leftPath,
          emptyLabel: emptyLabel,
          chooseLabel: context.strings('compare.importLeft'),
          onPressed: _importing ? null : () => _importReport(left: true),
        ),
        const SizedBox(height: 8),
        _ReportSlot(
          slot: 'right',
          label: context.strings('compare.current'),
          path: _rightPath,
          emptyLabel: emptyLabel,
          chooseLabel: context.strings('compare.importRight'),
          onPressed: _importing ? null : () => _importReport(left: false),
        ),
      ],
    );
  }

  Widget _buildPaneSelector() {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: SegmentedButton<ComparePane>(
        key: const ValueKey<String>('compare-pane-selector'),
        showSelectedIcon: false,
        segments: <ButtonSegment<ComparePane>>[
          ButtonSegment<ComparePane>(
            value: ComparePane.left,
            label: Text(context.strings('compare.left')),
            icon: const Icon(Icons.view_sidebar_outlined),
          ),
          ButtonSegment<ComparePane>(
            value: ComparePane.right,
            label: Text(context.strings('compare.right')),
            icon: const Icon(Icons.view_sidebar),
          ),
          ButtonSegment<ComparePane>(
            value: ComparePane.differences,
            label: Text(context.strings('compare.differences')),
            icon: const Icon(Icons.compare_arrows),
          ),
        ],
        selected: <ComparePane>{_pane},
        onSelectionChanged: (Set<ComparePane> selection) {
          setState(() {
            _pane = selection.first;
          });
        },
      ),
    );
  }

  Widget _buildNarrowContent(List<DiffRowModel> rows) {
    return switch (_pane) {
      ComparePane.left => EntryList(
        entries: <BatchEntry>[
          for (final DiffRowModel row in rows)
            if (row.left != null) row.left!,
        ],
      ),
      ComparePane.right => EntryList(
        entries: <BatchEntry>[
          for (final DiffRowModel row in rows)
            if (row.right != null) row.right!,
        ],
      ),
      ComparePane.differences => _buildDiffTable(
        rows,
        leftKey: 'compare.baseline',
      ),
    };
  }
}

/// One imported-report slot in the report-versus-report mode.
class _ReportSlot extends StatelessWidget {
  const _ReportSlot({
    required this.slot,
    required this.label,
    required this.path,
    required this.emptyLabel,
    required this.chooseLabel,
    required this.onPressed,
  });

  /// Stable identifier used for the widget keys, 'left' or 'right'.
  final String slot;
  final String label;
  final String path;
  final String emptyLabel;
  final String chooseLabel;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Card(
      key: ValueKey<String>('compare-import-$slot'),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(label, style: theme.textTheme.labelLarge),
            const SizedBox(height: 4),
            Text(
              path.isEmpty ? emptyLabel : path,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall,
            ),
            const SizedBox(height: 8),
            Align(
              alignment: Alignment.centerLeft,
              child: OutlinedButton.icon(
                key: ValueKey<String>('compare-import-$slot-button'),
                onPressed: onPressed,
                icon: const Icon(Icons.description_outlined),
                label: Text(chooseLabel),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _PolicyBanner extends StatelessWidget {
  const _PolicyBanner();

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return Container(
      key: const ValueKey<String>('compare-policy-banner'),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: scheme.secondaryContainer,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Icon(Icons.info_outline, color: scheme.onSecondaryContainer),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              context.strings('compare.policy'),
              style: theme.textTheme.bodySmall?.copyWith(
                color: scheme.onSecondaryContainer,
              ),
            ),
          ),
        ],
      ),
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
        child: Text(
          message,
          style: Theme.of(context).textTheme.bodySmall
              ?.copyWith(color: foreground),
        ),
      ),
    );
  }
}
