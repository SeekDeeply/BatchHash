import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';

/// Format an exported log is written in.
enum ExportFormat { json, markdown }

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
  const LogExportDialog({super.key, required this.count});

  /// How many entries the export will contain, shown so the user can tell the
  /// filtered set apart from the whole log.
  final int count;

  @override
  State<LogExportDialog> createState() => _LogExportDialogState();
}

class _LogExportDialogState extends State<LogExportDialog> {
  ExportFormat _format = ExportFormat.json;
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
                segments: const <ButtonSegment<ExportFormat>>[
                  ButtonSegment<ExportFormat>(
                    value: ExportFormat.json,
                    label: Text('JSON'),
                  ),
                  ButtonSegment<ExportFormat>(
                    value: ExportFormat.markdown,
                    label: Text('Markdown'),
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
