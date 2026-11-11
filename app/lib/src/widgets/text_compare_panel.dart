import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../services/batch_hash_service.dart';
import 'text_diff.dart';

/// Side-by-side line comparison of two pasted texts.
///
/// Kept separate from the report comparison on the same page: report diffing
/// compares digests of files, while this compares arbitrary text a user pastes,
/// so the two share a page but not a data model.
class TextComparePanel extends StatefulWidget {
  const TextComparePanel({super.key, this.service, this.algorithms});

  /// Hasher used to show each side's digest; when null only the diff renders.
  final BatchHashService? service;

  /// Algorithms to report digests for; defaults to one common digest pair.
  final List<String>? algorithms;

  @override
  State<TextComparePanel> createState() => _TextComparePanelState();
}

class _TextComparePanelState extends State<TextComparePanel> {
  final TextEditingController _left = TextEditingController();
  final TextEditingController _right = TextEditingController();
  List<TextDiffRow> _rows = const <TextDiffRow>[];
  bool _compared = false;

  /// Digests of the two panes, shown under the summary once a compare ran.
  List<TextHashResult> _leftHashes = const <TextHashResult>[];
  List<TextHashResult> _rightHashes = const <TextHashResult>[];
  bool _hashing = false;
  String? _hashError;

  /// Algorithms used for the digest rows when the caller does not choose.
  static const List<String> _defaultHashAlgorithms = <String>[
    'sha256',
    'blake3',
  ];

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
      // Stale digests would claim the old text still matches the fields.
      _leftHashes = const <TextHashResult>[];
      _rightHashes = const <TextHashResult>[];
      _hashError = null;
    });
  }

  Future<void> _compare() async {
    final List<String> algorithms =
        widget.algorithms ?? _defaultHashAlgorithms;
    setState(() {
      _rows = diffLines(_left.text, _right.text);
      _compared = true;
      _hashing = widget.service != null;
      _hashError = null;
      _leftHashes = const <TextHashResult>[];
      _rightHashes = const <TextHashResult>[];
    });
    final BatchHashService? service = widget.service;
    if (service == null) {
      return;
    }
    try {
      // Hash exactly what the user pasted: no trimming or newline rewriting, or
      // the digest would not match hashing the same text as a file.
      final List<TextHashResult> left = await service.hashText(
        text: _left.text,
        algorithms: algorithms,
      );
      final List<TextHashResult> right = await service.hashText(
        text: _right.text,
        algorithms: algorithms,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _leftHashes = left;
        _rightHashes = right;
        _hashing = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _hashing = false;
        _hashError = '$error';
      });
    }
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
        if (_compared && widget.service != null) ...<Widget>[
          const SizedBox(height: 12),
          _HashRows(
            leftHashes: _leftHashes,
            rightHashes: _rightHashes,
            hashing: _hashing,
            error: _hashError,
          ),
        ],
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
/// Digests of the two compared texts, side by side.
///
/// Shown under the diff summary so a user can confirm integrity without
/// reading the diff: equal digests mean the two texts are byte identical, which
/// a line diff alone does not state.
class _HashRows extends StatelessWidget {
  const _HashRows({
    required this.leftHashes,
    required this.rightHashes,
    required this.hashing,
    required this.error,
  });

  final List<TextHashResult> leftHashes;
  final List<TextHashResult> rightHashes;
  final bool hashing;
  final String? error;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? error = this.error;
    if (hashing) {
      return Row(
        children: <Widget>[
          const SizedBox(
            width: 16,
            height: 16,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          const SizedBox(width: 8),
          Text(
            context.strings('compare.textHashing'),
            style: theme.textTheme.bodySmall,
          ),
        ],
      );
    }
    if (error != null) {
      return Text(
        context.strings(
          'compare.textHashFailed',
          args: <String, String>{'error': error},
        ),
        key: const ValueKey<String>('text-compare-hash-error'),
        style: theme.textTheme.bodySmall?.copyWith(color: scheme.error),
      );
    }
    if (leftHashes.isEmpty && rightHashes.isEmpty) {
      return const SizedBox.shrink();
    }
    final Map<String, String> rightByAlgorithm = <String, String>{
      for (final TextHashResult result in rightHashes)
        result.algorithmId: result.digest,
    };
    return Card(
      key: const ValueKey<String>('text-compare-hashes'),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(
              context.strings('compare.textHashes'),
              style: theme.textTheme.labelLarge,
            ),
            const SizedBox(height: 6),
            for (final TextHashResult left in leftHashes)
              _HashLine(
                result: left,
                other: rightByAlgorithm[left.algorithmId],
              ),
          ],
        ),
      ),
    );
  }
}

/// One algorithm's digests for both sides, marking whether they match.
class _HashLine extends StatelessWidget {
  const _HashLine({required this.result, required this.other});

  final TextHashResult result;
  final String? other;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? other = this.other;
    final bool matches = other != null && other == result.digest;
    final Color color = matches ? scheme.tertiary : scheme.error;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Row(
            children: <Widget>[
              Icon(
                matches ? Icons.check_circle : Icons.error_outline,
                size: 14,
                color: color,
              ),
              const SizedBox(width: 4),
              Text(result.displayName, style: theme.textTheme.labelMedium),
            ],
          ),
          SelectableText(
            result.digest,
            style: theme.textTheme.bodySmall?.copyWith(
              fontFamily: 'monospace',
              color: scheme.onSurfaceVariant,
            ),
          ),
          if (other != null)
            SelectableText(
              other,
              style: theme.textTheme.bodySmall?.copyWith(
                fontFamily: 'monospace',
                color: scheme.onSurfaceVariant,
              ),
            ),
        ],
      ),
    );
  }
}