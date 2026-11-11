import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../l10n/app_strings_scope.dart';
import '../theme_controller.dart';

/// Settings card that lets the user type a seed color.
///
/// The field accepts the formats [parseColorInput] understands and only applies
/// a value once it parses, so a half-typed `#00` never repaints the whole app or
/// throws. A row of presets covers the common cases without typing at all.
class AccentColorCard extends StatefulWidget {
  const AccentColorCard({super.key, required this.controller});

  /// Holds the active seed; applying a parsed color calls [ThemeController.select].
  final ThemeController controller;

  @override
  State<AccentColorCard> createState() => _AccentColorCardState();
}

class _AccentColorCardState extends State<AccentColorCard> {
  late final TextEditingController _field = TextEditingController(
    text: formatColorInput(widget.controller.seed),
  );
  String? _error;

  /// A few seeds worth offering directly; typed colors cover everything else.
  static const List<({String key, Color color})> _presets = <({String key, Color color})>[
    (key: 'accent.teal', color: Color(0xFF00658F)),
    (key: 'accent.indigo', color: Color(0xFF3F51B5)),
    (key: 'accent.violet', color: Color(0xFF6750A4)),
    (key: 'accent.rose', color: Color(0xFFB3261E)),
    (key: 'accent.amber', color: Color(0xFFB26A00)),
    (key: 'accent.green', color: Color(0xFF2E6B4F)),
  ];

  @override
  void dispose() {
    _field.dispose();
    super.dispose();
  }

  void _apply(String raw) {
    final Color? parsed = parseColorInput(raw);
    if (parsed == null) {
      setState(() {
        _error = raw.trim().isEmpty ? null : context.strings("accent.invalid");
      });
      return;
    }
    setState(() {
      _error = null;
    });
    widget.controller.select(parsed);
  }

  void _selectPreset(Color color) {
    _field.text = formatColorInput(color);
    _apply(_field.text);
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Row(
            children: <Widget>[
              Icon(Icons.palette_outlined, color: scheme.primary),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  context.strings('settings.accent'),
                  style: theme.textTheme.titleMedium,
                ),
              ),
              Container(
                key: const ValueKey<String>("accent-preview"),
                width: 28,
                height: 28,
                decoration: BoxDecoration(
                  color: widget.controller.seed,
                  shape: BoxShape.circle,
                  border: Border.all(color: scheme.outlineVariant),
                ),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text(
            context.strings('settings.accentDetail'),
            style: theme.textTheme.bodySmall?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 12),
          Row(
            children: <Widget>[
              Expanded(
                child: TextField(
                  key: const ValueKey<String>("accent-input"),
                  controller: _field,
                  decoration: InputDecoration(
                    labelText: context.strings('accent.inputLabel'),
                    hintText: '#00658F  ·  rgb(0,101,143)',
                    border: const OutlineInputBorder(),
                    isDense: true,
                    errorText: _error,
                  ),
                  inputFormatters: <TextInputFormatter>[
                    LengthLimitingTextInputFormatter(32),
                  ],
                  onSubmitted: _apply,
                ),
              ),
              const SizedBox(width: 8),
              FilledButton(
                key: const ValueKey<String>("accent-apply"),
                onPressed: () => _apply(_field.text),
                child: Text(context.strings('accent.apply')),
              ),
              const SizedBox(width: 8),
              TextButton(
                key: const ValueKey<String>("accent-reset"),
                onPressed: widget.controller.isDefault
                    ? null
                    : () {
                        widget.controller.reset();
                        _field.text = formatColorInput(
                          widget.controller.seed,
                        );
                        setState(() {
                          _error = null;
                        });
                      },
                child: Text(context.strings('accent.reset')),
              ),
            ],
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: <Widget>[
              for (final ({String key, Color color}) preset in _presets)
                Tooltip(
                  message: context.strings(preset.key),
                  child: InkWell(
                    key: ValueKey<String>("accent-preset-${preset.key}"),
                    borderRadius: BorderRadius.circular(20),
                    onTap: () => _selectPreset(preset.color),
                    child: Container(
                      width: 32,
                      height: 32,
                      decoration: BoxDecoration(
                        color: preset.color,
                        shape: BoxShape.circle,
                        border: Border.all(
                          color: widget.controller.seed.value == preset.color.value
                              ? scheme.onSurface
                              : scheme.outlineVariant,
                          width: widget.controller.seed.value == preset.color.value
                              ? 2
                              : 1,
                        ),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ],
      ),
    );
  }
}