import 'package:flutter/material.dart';

import '../app_state.dart';
import '../models/models.dart';
import '../util/formatting.dart';
import '../widgets/empty_state.dart';
import '../widgets/summary_card.dart';

/// Report export page. Formats are chosen with checkboxes and exported on
/// demand; the page stays read only until the user taps Export.
class ReportPage extends StatefulWidget {
  const ReportPage({super.key, required this.state});

  final BatchHashAppState state;

  @override
  State<ReportPage> createState() => _ReportPageState();
}

class _ReportPageState extends State<ReportPage> {
  final Set<ReportFormat> _formats = <ReportFormat>{
    ReportFormat.json,
    ReportFormat.csv,
  };
  bool _exporting = false;
  String? _error;
  String? _message;
  List<ExportedReport> _exported = const <ExportedReport>[];

  Future<void> _export(ReportModel report) async {
    setState(() {
      _exporting = true;
      _error = null;
      _message = null;
    });
    try {
      final List<ExportedReport> files = await widget.state.service
          .exportReport(
            report: report,
            formats: Set<ReportFormat>.of(_formats),
          );
      if (!mounted) {
        return;
      }
      setState(() {
        _exporting = false;
        _exported = files;
        _message = 'Exported ${files.length} file(s).';
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _exporting = false;
        _error = 'Export failed: $error';
      });
    }
  }

  void _toggleFormat(ReportFormat format, bool selected) {
    setState(() {
      if (selected) {
        _formats.add(format);
      } else {
        _formats.remove(format);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return ValueListenableBuilder<ReportModel?>(
      valueListenable: widget.state.report,
      builder: (BuildContext context, ReportModel? report, Widget? child) {
        final String? error = _error;
        final String? message = _message;
        return SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text('Report export', style: theme.textTheme.headlineSmall),
              const SizedBox(height: 4),
              Text(
                'Write the current batch out in one or more formats. '
                'JSON and CSV are lossless and can be imported again.',
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
              if (report == null) ...<Widget>[
                const SizedBox(height: 48),
                const EmptyState(
                  icon: Icons.description_outlined,
                  title: 'No report yet',
                  message:
                      'Run a hash first to build a report, then choose the '
                      'formats to export.',
                ),
              ] else ...<Widget>[
                const SizedBox(height: 16),
                SummaryCard(
                  title: 'Report',
                  metrics: <SummaryMetric>[
                    SummaryMetric(
                      label: 'Schema version',
                      value: '${report.schemaVersion}',
                    ),
                    SummaryMetric(
                      label: 'Files',
                      value: '${report.entries.length}',
                    ),
                    SummaryMetric(
                      label: 'Algorithms',
                      value: report.algorithms.isEmpty
                          ? 'none'
                          : report.algorithms.join(', '),
                    ),
                    SummaryMetric(
                      label: 'Generated',
                      value: formatTimestamp(report.generatedAt),
                    ),
                    SummaryMetric(label: 'Root', value: report.root),
                  ],
                ),
                const SizedBox(height: 20),
                Text('Formats', style: theme.textTheme.titleMedium),
                const SizedBox(height: 8),
                for (final ReportFormat format in ReportFormat.values)
                  CheckboxListTile(
                    key: ValueKey<String>('report-format-${format.name}'),
                    value: _formats.contains(format),
                    onChanged: _exporting
                        ? null
                        : (bool? selected) =>
                              _toggleFormat(format, selected ?? false),
                    title: Text(format.label),
                    subtitle: Text(format.description),
                    secondary: Icon(_iconFor(format)),
                    controlAffinity: ListTileControlAffinity.leading,
                    contentPadding: EdgeInsets.zero,
                  ),
                const SizedBox(height: 16),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: <Widget>[
                    FilledButton.icon(
                      key: const ValueKey<String>('report-export-button'),
                      onPressed: _formats.isEmpty || _exporting
                          ? null
                          : () => _export(report),
                      icon: const Icon(Icons.download),
                      label: Text(
                        _exporting ? 'Exporting...' : 'Export report',
                      ),
                    ),
                    if (_formats.isNotEmpty)
                      Text(
                        '${_formats.length} format(s) selected',
                        style: theme.textTheme.bodySmall,
                      ),
                  ],
                ),
                if (error != null) ...<Widget>[
                  const SizedBox(height: 12),
                  _InlineMessage(message: error, isError: true),
                ],
                if (message != null) ...<Widget>[
                  const SizedBox(height: 12),
                  _InlineMessage(message: message, isError: false),
                ],
                if (_exported.isNotEmpty) ...<Widget>[
                  const SizedBox(height: 16),
                  Card(
                    child: Column(
                      children: <Widget>[
                        for (final ExportedReport exported in _exported)
                          ListTile(
                            dense: true,
                            leading: Icon(_iconFor(exported.format)),
                            title: Text(exported.path),
                            subtitle: Text(exported.format.label),
                          ),
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

  IconData _iconFor(ReportFormat format) => switch (format) {
    ReportFormat.json => Icons.data_object,
    ReportFormat.csv => Icons.table_chart,
    ReportFormat.markdown => Icons.notes,
    ReportFormat.txt => Icons.text_snippet,
    ReportFormat.md5sum => Icons.tag,
    ReportFormat.bsd => Icons.checklist,
  };
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
