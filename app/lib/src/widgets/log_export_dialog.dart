import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../models/models.dart';

/// Format an exported log is written in.
enum ExportFormat {
  json,
  markdown,
  /// The formats the report page also offers, so a single hash result can be
  /// exported in the same shape a full report would be. They only apply when
  /// the selection is nothing but hash digests: a log entry that records a
  /// scan or a rename has no digest table to render.
  csv,
  txt,
  md5sum,
  bsd,
}

/// Choices made in [LogExportDialog].
class ExportOptions {
  const ExportOptions({
    required this.format,
    required this.zip,
    required this.password,
    required this.sha256,
  });

  final ExportFormat format;

  /// Whether to pack the payload into a ZIP.
  final bool zip;

  /// Archive password; null or empty means no encryption.
  final String? password;

  /// Whether to append a SHA-256 suffix to the file name.
  final bool sha256;
}

/// Options for exporting the operation log.
///
/// The password field only appears once ZIP is ticked: a password for a file
/// that is not an archive would be silently ignored, and a control that does
/// nothing is worse than a control that is absent.
class LogExportDialog extends StatefulWidget {
  const LogExportDialog({
    super.key,
    required this.count,
    this.hashOnly = false,
  });

  /// Whether every selected entry is a hash result.
  ///
  /// Only then do the report-shaped formats make sense: they render a digest
  /// table, and a selection that includes a scan or rename has no such table.
  final bool hashOnly;

  /// How many entries the export will contain, shown so the user can tell the
  /// filtered set apart from the whole log.
  final int count;

  @override
  State<LogExportDialog> createState() => _LogExportDialogState();
}

class _LogExportDialogState extends State<LogExportDialog> {
  ExportFormat _format = ExportFormat.json;

  /// Report formats that make sense for a digest-only selection.
  ///
  /// Tree is excluded: it renders the folder hierarchy of a scan, which a log
  /// entry does not carry.
  static const Set<ReportFormat> _reportFormats = <ReportFormat>{
    ReportFormat.csv,
    ReportFormat.txt,
    ReportFormat.md5sum,
    ReportFormat.bsd,
  };

  static ExportFormat _toExportFormat(ReportFormat format) {
    switch (format) {
      case ReportFormat.csv:
        return ExportFormat.csv;
      case ReportFormat.txt:
        return ExportFormat.txt;
      case ReportFormat.md5sum:
        return ExportFormat.md5sum;
      case ReportFormat.bsd:
        return ExportFormat.bsd;
      case ReportFormat.json:
      case ReportFormat.markdown:
      case ReportFormat.tree:
        // Not offered through this dialog; the two human formats have their
        // own segments above.
        return ExportFormat.json;
    }
  }
  bool _zip = false;
  bool _sha256 = false;
  final TextEditingController _password = TextEditingController();

  @override
  void dispose() {
    _password.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return AlertDialog(
      key: const ValueKey<String>('operation-log-export-dialog'),
      title: Text(context.strings('log.exportTitle')),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                context.strings(
                  'log.exportCount',
                  args: <String, String>{'count': '${widget.count}'},
                ),
                style: theme.textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(height: 16),
              Text(
                context.strings('log.exportFormat'),
                style: theme.textTheme.labelLarge,
              ),
              const SizedBox(height: 8),
              SegmentedButton<ExportFormat>(
                key: const ValueKey<String>('operation-log-export-format'),
                segments: <ButtonSegment<ExportFormat>>[
                  const ButtonSegment<ExportFormat>(
                    value: ExportFormat.json,
                    label: Text('JSON'),
                  ),
                  const ButtonSegment<ExportFormat>(
                    value: ExportFormat.markdown,
                    label: Text('Markdown'),
                  ),
                  // Offered only for a pure hash selection, in the same set
                  // the report page shows, so the two agree on what a digest
                  // export looks like.
                  if (widget.hashOnly)
                    for (final ReportFormat format
                        in ReportFormat.values)
                      if (_reportFormats.contains(format))
                        ButtonSegment<ExportFormat>(
                          value: _toExportFormat(format),
                          label: Text(format.label),
                        ),
                ],
                selected: <ExportFormat>{_format},
                onSelectionChanged: (Set<ExportFormat> selection) {
                  setState(() => _format = selection.first);
                },
              ),
              const SizedBox(height: 8),
              CheckboxListTile(
                key: const ValueKey<String>('operation-log-export-zip'),
                contentPadding: EdgeInsets.zero,
                value: _zip,
                onChanged: (bool? value) => setState(() => _zip = value ?? false),
                title: Text(context.strings('log.exportZip')),
                controlAffinity: ListTileControlAffinity.leading,
              ),
              if (_zip)
                Padding(
                  padding: const EdgeInsets.only(left: 32, bottom: 8),
                  child: TextField(
                    key: const ValueKey<String>('operation-log-export-password'),
                    controller: _password,
                    obscureText: true,
                    decoration: InputDecoration(
                      labelText: context.strings('log.exportZipPassword'),
                      hintText: context.strings('log.exportPasswordHint'),
                      border: const OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
              CheckboxListTile(
                key: const ValueKey<String>('operation-log-export-sha256'),
                contentPadding: EdgeInsets.zero,
                value: _sha256,
                onChanged: (bool? value) =>
                    setState(() => _sha256 = value ?? false),
                title: Text(context.strings('log.exportSha256')),
                subtitle: Text(context.strings('log.exportSha256Detail')),
                controlAffinity: ListTileControlAffinity.leading,
              ),
            ],
          ),
        ),
      ),
      actions: <Widget>[
        TextButton(
          key: const ValueKey<String>('operation-log-export-cancel'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.strings('common.cancel')),
        ),
        FilledButton(
          key: const ValueKey<String>('operation-log-export-confirm'),
          onPressed: () {
            Navigator.of(context).pop(
              ExportOptions(
                format: _format,
                zip: _zip,
                password: _zip ? _password.text.trim() : null,
                sha256: _sha256,
              ),
            );
          },
          child: Text(context.strings('log.export')),
        ),
      ],
    );
  }
}
