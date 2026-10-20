import 'dart:async';

import 'package:flutter/material.dart';

import '../app_state.dart';
import '../models/models.dart';
import '../widgets/diff_table.dart';
import '../widgets/empty_state.dart';
import '../widgets/entry_list.dart';
import '../widgets/summary_card.dart';

/// Filter applied to comparison rows.
enum CompareFilter {
  all('All'),
  differences('Differences'),
  changed('Changed'),
  added('Added'),
  removed('Removed'),
  ambiguous('Ambiguous');

  const CompareFilter(this.label);

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

/// Compares the current report against the live contents of its root.
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
  String? _error;
  String? _loadedReportKey;
  CompareFilter _filter = CompareFilter.all;
  ComparePane _pane = ComparePane.differences;
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
    super.dispose();
  }

  void _handleReportChanged() {
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

  Future<void> _load(ReportModel report) async {
    try {
      final List<DiffRowModel> rows =
          await widget.state.service.diffReportAgainstDirectory(
        report: report,
        directory: report.root,
        recursive: true,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _rows = rows;
        _loading = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _loading = false;
        _error = 'Comparison failed: $error';
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
    final int changedCount =
        _rows.where((DiffRowModel row) => row.status == DiffStatus.changed).length;
    final int addedCount =
        _rows.where((DiffRowModel row) => row.status == DiffStatus.added).length;
    final int removedCount =
        _rows.where((DiffRowModel row) => row.status == DiffStatus.removed).length;
    final int ambiguousCount = _rows
        .where((DiffRowModel row) => row.status == DiffStatus.ambiguous)
        .length;
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text('Compare reports', style: theme.textTheme.headlineSmall),
          const SizedBox(height: 4),
          Text(
            'Match a baseline report against the current state of a folder and '
            'review what changed.',
            style: theme.textTheme.bodyMedium
                ?.copyWith(color: scheme.onSurfaceVariant),
          ),
          const SizedBox(height: 16),
          const _PolicyBanner(),
          const SizedBox(height: 12),
          SummaryCard(
            title: 'Comparison summary',
            metrics: <SummaryMetric>[
              SummaryMetric(label: 'Rows', value: '${_rows.length}'),
              SummaryMetric(
                label: 'Changed',
                value: '$changedCount',
                color: scheme.error,
              ),
              SummaryMetric(
                label: 'Added',
                value: '$addedCount',
                color: scheme.tertiary,
              ),
              SummaryMetric(
                label: 'Removed',
                value: '$removedCount',
                color: scheme.secondary,
              ),
              SummaryMetric(
                label: 'Ambiguous',
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
                  label: Text(filter.label),
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
          if (_loading)
            const LinearProgressIndicator()
          else if (error != null)
            _MessageCard(message: error, isError: true)
          else if (_rows.isEmpty)
            const EmptyState(
              icon: Icons.compare_arrows,
              title: 'Nothing to compare',
              message: 'Hash a folder first, then compare the report against '
                  'the current contents.',
            )
          else if (narrow)
            _buildNarrowContent(visibleRows)
          else
            DiffTable(
              rows: visibleRows,
              expandedPath: _expandedPath,
              onRowTap: _toggleRow,
            ),
        ],
      ),
    );
  }

  Widget _buildPaneSelector() {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: SegmentedButton<ComparePane>(
        key: const ValueKey<String>('compare-pane-selector'),
        showSelectedIcon: false,
        segments: const <ButtonSegment<ComparePane>>[
          ButtonSegment<ComparePane>(
            value: ComparePane.left,
            label: Text('Left'),
            icon: Icon(Icons.view_sidebar_outlined),
          ),
          ButtonSegment<ComparePane>(
            value: ComparePane.right,
            label: Text('Right'),
            icon: Icon(Icons.view_sidebar),
          ),
          ButtonSegment<ComparePane>(
            value: ComparePane.differences,
            label: Text('Differences'),
            icon: Icon(Icons.compare_arrows),
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
      ComparePane.differences => DiffTable(
          rows: rows,
          expandedPath: _expandedPath,
          onRowTap: _toggleRow,
        ),
    };
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
              'Comparison policy: only algorithms present in both reports are '
              'compared. Files without a shared digest fall back to path and '
              'size. Paths are normalized and compared case-sensitively.',
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: scheme.onSecondaryContainer),
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
    final Color background =
        isError ? scheme.errorContainer : scheme.secondaryContainer;
    final Color foreground =
        isError ? scheme.onErrorContainer : scheme.onSecondaryContainer;
    return Card(
      color: background,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Text(
          message,
          style: Theme.of(context)
              .textTheme
              .bodySmall
              ?.copyWith(color: foreground),
        ),
      ),
    );
  }
}
