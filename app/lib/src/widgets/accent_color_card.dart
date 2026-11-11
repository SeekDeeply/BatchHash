import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../theme_controller.dart';

/// Settings card that lets the user pick the seed color.
///
/// Presets apply on tap: a color swatch is a direct choice, and making the user
/// confirm it with a second button is friction for no gain. Typing a value is a
/// different case, because a half-typed `#00` is not a decision yet, so the field
/// lives in a dialog where Apply is explicit.
class AccentColorCard extends StatelessWidget {
  const AccentColorCard({super.key, required this.controller});

  /// Holds the active seed; selecting a color calls [ThemeController.select].
  final ThemeController controller;

  /// Presets worth offering directly; typed colors cover everything else.
  ///
  /// The seeds are chosen so their *derived* accents stay distinguishable. The
  /// previous pair (`#00658F` and `#66CCFF`) sat ~3 degrees apart in hue, and
  /// Material 3 pulls primary from a fixed tonal step, so both produced nearly
  /// the same button color. Sky is now `#29B6F6`, which is far enough to read
  /// as a different accent rather than a slightly different patch of blue.
  static const List<({String key, Color color})> presets = <({String key, Color color})>[
    (key: 'accent.teal', color: Color(0xFF00658F)),
    (key: 'accent.sky', color: Color(0xFF29B6F6)),
    (key: 'accent.indigo', color: Color(0xFF3F51B5)),
    (key: 'accent.violet', color: Color(0xFF6750A4)),
    (key: 'accent.rose', color: Color(0xFFB3261E)),
    (key: 'accent.amber', color: Color(0xFFB26A00)),
    (key: 'accent.green', color: Color(0xFF2E6B4F)),
  ];

  Future<void> _openManualDialog(BuildContext context) async {
    final Color? picked = await showDialog<Color>(
      context: context,
      builder: (BuildContext context) =>
          AccentColorInputDialog(controller: controller),
    );
    if (picked != null) {
      controller.select(picked);
    }
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    // Rebuilt through the controller so the active swatch and preview follow a
    // selection made here or by any other surface.
    return ListenableBuilder(
      listenable: controller,
      builder: (BuildContext context, Widget? child) {
        final Color seed = controller.seed;
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
                  Tooltip(
                    message: context.strings('accent.derivedHint'),
                    child: Container(
                      key: const ValueKey<String>('accent-preview'),
                      width: 28,
                      height: 28,
                      decoration: BoxDecoration(
                        // The derived accent, not the seed: this is what the
                        // UI will actually paint.
                        color: derivedPrimary(seed),
                        shape: BoxShape.circle,
                        border: Border.all(color: scheme.outlineVariant),
                      ),
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
              Wrap(
                spacing: 10,
                runSpacing: 10,
                children: <Widget>[
                  for (final ({String key, Color color}) preset in presets)
                    Tooltip(
                      message: context.strings(preset.key),
                      child: InkWell(
                        key: ValueKey<String>(
                          'accent-preset-${preset.key}',
                        ),
                        borderRadius: BorderRadius.circular(20),
                        onTap: () => controller.select(preset.color),
                        // Two rings on purpose: the outer edge is the seed the
                        // user types, the inner disc is the accent that will
                        // actually be painted. Showing only the seed is what
                        // made two presets look identical before.
                        child: Container(
                          width: 34,
                          height: 34,
                          padding: const EdgeInsets.all(4),
                          decoration: BoxDecoration(
                            color: preset.color,
                            shape: BoxShape.circle,
                            border: Border.all(
                              color: seed.value == preset.color.value
                                  ? scheme.onSurface
                                  : scheme.outlineVariant,
                              width: seed.value == preset.color.value ? 3 : 1,
                            ),
                          ),
                          child: Container(
                            key: ValueKey<String>(
                              'accent-preset-inner-${preset.key}',
                            ),
                            decoration: BoxDecoration(
                              color: derivedPrimary(preset.color),
                              shape: BoxShape.circle,
                            ),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
              const SizedBox(height: 8),
              Row(
                children: <Widget>[
                  TextButton.icon(
                    key: const ValueKey<String>('accent-open-manual'),
                    onPressed: () => _openManualDialog(context),
                    icon: const Icon(Icons.edit_outlined, size: 18),
                    label: Text(context.strings('accent.manualEntry')),
                  ),
                  const SizedBox(width: 4),
                  TextButton(
                    key: const ValueKey<String>('accent-reset'),
                    onPressed: controller.isDefault ? null : controller.reset,
                    child: Text(context.strings('accent.resetToDefault')),
                  ),
                ],
              ),
            ],
          ),
        );
      },
    );
  }
}

/// Dialog for entering a color by value.
///
/// Apply is explicit here because arbitrary input has to be validated first:
/// this is the only place a value is worth confirming rather than applying on
/// the spot.
class AccentColorInputDialog extends StatefulWidget {
  const AccentColorInputDialog({super.key, required this.controller});

  /// Read for the initial value and used to render the live preview.
  final ThemeController controller;

  @override
  State<AccentColorInputDialog> createState() => _AccentColorInputDialogState();
}

class _AccentColorInputDialogState extends State<AccentColorInputDialog> {
  late final TextEditingController _field = TextEditingController(
    text: formatColorInput(widget.controller.seed),
  );
  String? _error;

  @override
  void dispose() {
    _field.dispose();
    super.dispose();
  }

  /// Parse the current text, returning null and setting an error when invalid.
  Color? _parsed() {
    final Color? parsed = parseColorInput(_field.text);
    setState(() {
      _error = parsed == null ? context.strings('accent.invalid') : null;
    });
    return parsed;
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    // The preview follows the text as long as it parses, so the user sees the
    // color before committing to it.
    final Color? live = parseColorInput(_field.text);
    return AlertDialog(
      key: const ValueKey<String>('accent-input-dialog'),
      title: Text(context.strings('accent.manualTitle')),
      content: SizedBox(
        width: 360,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            TextField(
              key: const ValueKey<String>('accent-input'),
              controller: _field,
              autofocus: true,
              decoration: InputDecoration(
                labelText: context.strings('accent.inputLabel'),
                hintText: '#66CCFF  ·  rgb(102,204,255)',
                border: const OutlineInputBorder(),
                errorText: _error,
              ),
              onChanged: (_) => setState(() {
                _error = null;
              }),
              onSubmitted: (_) {
                final Color? parsed = _parsed();
                if (parsed != null) {
                  Navigator.of(context).pop(parsed);
                }
              },
            ),
            const SizedBox(height: 16),
            Row(
              children: <Widget>[
                // The preview paints the derived accent, and the seed ring
                // around it keeps the typed value visible: what you type and
                // what you get are both on screen.
                Container(
                  key: const ValueKey<String>('accent-dialog-preview'),
                  width: 44,
                  height: 44,
                  padding: const EdgeInsets.all(5),
                  decoration: BoxDecoration(
                    color: live ?? scheme.surfaceContainerHighest,
                    shape: BoxShape.circle,
                    border: Border.all(color: scheme.outlineVariant),
                  ),
                  child: Container(
                    decoration: BoxDecoration(
                      color: live == null
                          ? scheme.surfaceContainerHighest
                          : derivedPrimary(live),
                      shape: BoxShape.circle,
                    ),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      Text(
                        live == null
                            ? context.strings('accent.previewInvalid')
                            : formatColorInput(live),
                        style: theme.textTheme.bodyMedium?.copyWith(
                          fontFamily: 'monospace',
                        ),
                      ),
                      if (live != null)
                        Text(
                          context.strings('accent.derivedHint'),
                          style: theme.textTheme.bodySmall?.copyWith(
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
      actions: <Widget>[
        TextButton(
          key: const ValueKey<String>('accent-cancel'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.strings('common.cancel')),
        ),
        FilledButton(
          key: const ValueKey<String>('accent-apply'),
          onPressed: () {
            final Color? parsed = _parsed();
            if (parsed != null) {
              Navigator.of(context).pop(parsed);
            }
          },
          child: Text(context.strings('accent.apply')),
        ),
      ],
    );
  }
}
