import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import 'text_diff.dart';

/// Side-by-side line comparison of two pasted texts.
///
/// Kept separate from the report comparison on the same page: report diffing
/// compares digests of files, while this compares arbitrary text a user pastes,
/// so the two share a page but not a data model.
class TextComparePanel extends StatefulWidget {
  const TextComparePanel({super.key});

  @override
  State<TextComparePanel> createState() => _TextComparePanelState();
}

class _TextComparePanelState extends State<TextComparePanel> {
  final TextEditingController _left = TextEditingController();
  final TextEditingController _right = TextEditingController();
  List<TextDiffRow> _rows = const <TextDiffRow>[];
  bool _compared = false;

  @override
  void initState() {
    super.initState();
    // The compare button is enabled only once something is typed. TextField
    // alone does not rebuild this widget, so the controllers have to notify:
    // without this the button stays disabled because build() only ever saw the
    // initial empty text.
    _left.addListener(_onTextChanged);
    _right.addListener(_onTextChanged);
  }

  @override
  void dispose() {
    _left.removeListener(_onTextChanged);
    _right.removeListener(_onTextChanged);
    _left.dispose();
    _right.dispose();
    super.dispose();
  }

  void _onTextChanged() {
    // Any edit invalidates a previous result: keep the button state live and
    // avoid showing a comparison of text that is no longer in the fields.
    setState(() {
      _compared = false;
    });
  }

  void _compare() {
    setState(() {
      _rows = diffLines(_left.text, _right.text);
      _compared = true;
    });
  }

  Color? _rowColor(BuildContext context, TextDiffKind kind) {
    final ColorScheme scheme = Theme.of(context).colorScheme;
    return switch (kind) {
      TextDiffKind.added => scheme.primaryContainer.withOpacity(0.45),
      TextDiffKind.removed => scheme.errorContainer.withOpacity(0.45),
      TextDiffKind.changed => scheme.tertiaryContainer.withOpacity(0.45),
      TextDiffKind.same => null,
    };
  }

  String _kindLabel(BuildContext context, TextDiffKind kind) {
    return switch (kind) {
      TextDiffKind.added => context.strings('compare.lineAdded'),
      TextDiffKind.removed => context.strings('compare.lineRemoved'),
      TextDiffKind.changed => context.strings('compare.lineChanged'),
      TextDiffKind.same => context.strings('compare.lineSame'),
    };
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    // Enabled as soon as either side has content: comparing one filled pane
    // against an empty one is a legitimate use (everything shows as added or
    // removed), and requiring both would leave the button dead in that case.
    final bool bothEmpty =
        _left.text.trim().isEmpty && _right.text.trim().isEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Expanded(
              child: TextField(
                key: const ValueKey<String>('text-compare-left'),
                controller: _left,
                minLines: 4,
                maxLines: 8,
                decoration: InputDecoration(
                  labelText: context.strings('compare.textLeft'),
                  border: const OutlineInputBorder(),
                ),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: TextField(
                key: const ValueKey<String>('text-compare-right'),
                controller: _right,
                minLines: 4,
                maxLines: 8,
                decoration: InputDecoration(
                  labelText: context.strings('compare.textRight'),
                  border: const OutlineInputBorder(),
                ),
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        Row(
          children: <Widget>[
            FilledButton.icon(
              key: const ValueKey<String>('text-compare-run'),
              onPressed: bothEmpty ? null : _compare,
              icon: const Icon(Icons.compare_arrows),
              label: Text(context.strings('compare.textRun')),
            ),
            const SizedBox(width: 12),
            if (_compared)
              Expanded(
                child: Text(
                  TextDiffSummary.of(_rows).isIdentical
                      ? context.strings('compare.textIdentical')
                      : context.strings(
                          'compare.textSummary',
                          args: <String, String>{
                            'added': '${TextDiffSummary.of(_rows).added}',
                            'removed': '${TextDiffSummary.of(_rows).removed}',
                            'changed': '${TextDiffSummary.of(_rows).changed}',
                          },
                        ),
                  key: const ValueKey<String>('text-compare-summary'),
                  style: theme.textTheme.bodyMedium,
                ),
              ),
          ],
        ),
        const SizedBox(height: 12),
        Expanded(
          child: !_compared
              ? Center(
                  child: Text(
                    context.strings('compare.textEmpty'),
                    style: theme.textTheme.bodyMedium,
                  ),
                )
              : ListView.builder(
                  key: const ValueKey<String>('text-compare-rows'),
                  itemCount: _rows.length,
                  itemBuilder: (BuildContext context, int index) {
                    final TextDiffRow row = _rows[index];
                    return Container(
                      color: _rowColor(context, row.kind),
                      padding: const EdgeInsets.symmetric(
                        horizontal: 8,
                        vertical: 2,
                      ),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: <Widget>[
                          SizedBox(
                            width: 40,
                            child: Text(
                              row.leftLine?.toString() ?? '',
                              style: theme.textTheme.bodySmall,
                            ),
                          ),
                          Expanded(
                            child: Text(
                              row.left ?? '',
                              style: theme.textTheme.bodyMedium,
                            ),
                          ),
                          SizedBox(
                            width: 40,
                            child: Text(
                              row.rightLine?.toString() ?? '',
                              style: theme.textTheme.bodySmall,
                            ),
                          ),
                          Expanded(
                            child: Text(
                              row.right ?? '',
                              style: theme.textTheme.bodyMedium,
                            ),
                          ),
                          SizedBox(
                            width: 64,
                            child: Text(
                              _kindLabel(context, row.kind),
                              style: theme.textTheme.labelSmall,
                            ),
                          ),
                        ],
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }
}