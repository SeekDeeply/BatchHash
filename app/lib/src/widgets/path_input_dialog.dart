import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';

/// In-app replacement for a native file picker.
///
/// The dialog collects one absolute path (single line) or several, one per
/// line. The hash engine validates the path when it is used, so the dialog only
/// rejects empty input; [allowEmpty] keeps the "use the app default" option for
/// the export destination.
class PathInputDialog extends StatefulWidget {
  const PathInputDialog({
    super.key,
    required this.title,
    required this.hintText,
    this.helperText,
    this.initialValue = '',
    this.allowMultiple = false,
    this.allowEmpty = false,
    this.suggestions = const <String>[],
  });

  /// Dialog title, already localized by the caller.
  final String title;

  /// Hint shown inside the text field, already localized by the caller.
  final String hintText;

  /// Optional helper line under the field, already localized.
  final String? helperText;

  /// Pre-filled value.
  final String initialValue;

  /// True renders a multiline field for one path per line.
  final bool allowMultiple;

  /// True lets the user confirm an empty value.
  final bool allowEmpty;

  /// Recently used paths offered as one-tap suggestions.
  final List<String> suggestions;

  /// Show the dialog and return the trimmed input, or null when cancelled.
  static Future<String?> show(
    BuildContext context, {
    required String title,
    required String hintText,
    String? helperText,
    String initialValue = '',
    bool allowMultiple = false,
    bool allowEmpty = false,
    List<String> suggestions = const <String>[],
  }) {
    return showDialog<String>(
      context: context,
      builder: (BuildContext dialogContext) => PathInputDialog(
        title: title,
        hintText: hintText,
        helperText: helperText,
        initialValue: initialValue,
        allowMultiple: allowMultiple,
        allowEmpty: allowEmpty,
        suggestions: suggestions,
      ),
    );
  }

  @override
  State<PathInputDialog> createState() => _PathInputDialogState();
}

class _PathInputDialogState extends State<PathInputDialog> {
  late final TextEditingController _controller = TextEditingController(
    text: widget.initialValue,
  );
  bool _missing = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final String value = _controller.text.trim();
    if (value.isEmpty && !widget.allowEmpty) {
      setState(() {
        _missing = true;
      });
      return;
    }
    Navigator.of(context).pop(value);
  }

  void _useSuggestion(String suggestion) {
    _controller.text = suggestion;
    setState(() {
      _missing = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.title),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            if (widget.suggestions.isNotEmpty) ...<Widget>[
              Text(
                context.strings('path.recent'),
                style: Theme.of(context).textTheme.labelMedium,
              ),
              const SizedBox(height: 4),
              for (final String suggestion in widget.suggestions)
                InkWell(
                  key: ValueKey<String>('path-suggestion-$suggestion'),
                  onTap: () => _useSuggestion(suggestion),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 6),
                    child: Text(
                      suggestion,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ),
              const SizedBox(height: 8),
            ],
            TextField(
              key: const ValueKey<String>('path-input-field'),
              controller: _controller,
              autofocus: true,
              minLines: widget.allowMultiple ? 3 : 1,
              maxLines: widget.allowMultiple ? 4 : 1,
              decoration: InputDecoration(
                hintText: widget.hintText,
                helperText: widget.helperText,
                errorText: _missing ? context.strings('path.required') : null,
                border: const OutlineInputBorder(),
              ),
              onSubmitted: widget.allowMultiple
                  ? null
                  : (String value) => _submit(),
            ),
          ],
        ),
      ),
      actions: <Widget>[
        TextButton(
          key: const ValueKey<String>('path-cancel'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.strings('common.cancel')),
        ),
        FilledButton(
          key: const ValueKey<String>('path-confirm'),
          onPressed: _submit,
          child: Text(context.strings('common.confirm')),
        ),
      ],
    );
  }
}
