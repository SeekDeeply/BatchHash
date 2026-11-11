import 'package:flutter/material.dart';

import '../l10n/app_strings_scope.dart';
import '../models/models.dart';

/// Modal picker for the algorithms a run should compute.
///
/// The catalogue comes from the core ([AlgorithmInfo] rows served by
/// `BatchHashService.listAlgorithms`), so this widget never hardcodes which
/// algorithms exist. It only decides how they are presented: a search field
/// over the display name, stable id and category, and one section per
/// [AlgorithmCategory].
class AlgorithmPickerDialog extends StatefulWidget {
  const AlgorithmPickerDialog({
    super.key,
    required this.catalogue,
    required this.selected,
    this.title,
  });

  /// Every algorithm the core advertises.
  final List<AlgorithmInfo> catalogue;

  /// Stable ids currently selected.
  final Set<String> selected;

  /// Optional heading; defaults to the step-2 string.
  final String? title;

  /// Show the picker and return the chosen ids, or null when dismissed.
  static Future<Set<String>?> show(
    BuildContext context, {
    required List<AlgorithmInfo> catalogue,
    required Set<String> selected,
  }) {
    return showDialog<Set<String>>(
      context: context,
      builder: (BuildContext context) => AlgorithmPickerDialog(
        catalogue: catalogue,
        selected: selected,
      ),
    );
  }

  @override
  State<AlgorithmPickerDialog> createState() => _AlgorithmPickerDialogState();
}

class _AlgorithmPickerDialogState extends State<AlgorithmPickerDialog> {
  late Set<String> _selected = <String>{...widget.selected};
  final TextEditingController _search = TextEditingController();
  String _query = '';

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  List<AlgorithmInfo> get _visible {
    final String needle = _query.trim().toLowerCase();
    if (needle.isEmpty) {
      return widget.catalogue;
    }
    return <AlgorithmInfo>[
      for (final AlgorithmInfo info in widget.catalogue)
        if (info.displayName.toLowerCase().contains(needle) ||
            info.id.toLowerCase().contains(needle) ||
            info.category.wireName.contains(needle))
          info,
    ];
  }

  /// Select every algorithm the catalogue offers.
  ///
  /// Only the algorithms currently visible are selected when a search is
  /// active, so "select all" means "all of these results" rather than silently
  /// adding algorithms the user filtered away.
  void _selectAll() {
    setState(() {
      for (final AlgorithmInfo info in _visible) {
        _selected.add(info.id);
      }
    });
  }

  /// Drop back to a single algorithm: the run needs at least one.
  void _resetSelection() {
    setState(() {
      final String keep = _selected.isEmpty
          ? widget.catalogue.first.id
          : _selected.first;
      _selected = <String>{keep};
    });
  }
  void _toggle(String id) {
    setState(() {
      if (_selected.contains(id)) {
        // The run must keep at least one algorithm: clearing the set would
        // produce a report with no digests at all.
        if (_selected.length > 1) {
          _selected.remove(id);
        }
      } else {
        _selected.add(id);
      }
    });
  }

  String _categoryLabel(BuildContext context, AlgorithmCategory category) {
    return context.strings('algorithm.category.${category.wireName}');
  }

  @override
  Widget build(BuildContext context) {
    final List<AlgorithmInfo> visible = _visible;
    final Map<AlgorithmCategory, List<AlgorithmInfo>> grouped =
        groupAlgorithmsByCategory(visible);
    final ThemeData theme = Theme.of(context);
    return AlertDialog(
      key: const ValueKey<String>('algorithm-picker'),
      title: Text(widget.title ?? context.strings('hash.stepAlgorithms')),
      contentPadding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
      content: SizedBox(
        width: 520,
        height: 460,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            TextField(
              key: const ValueKey<String>('algorithm-search'),
              controller: _search,
              autofocus: true,
              decoration: InputDecoration(
                prefixIcon: const Icon(Icons.search),
                hintText: context.strings('algorithm.searchHint'),
                border: const OutlineInputBorder(),
                isDense: true,
              ),
              onChanged: (String value) => setState(() => _query = value),
            ),
            const SizedBox(height: 8),
            Row(
              children: <Widget>[
                TextButton.icon(
                  key: const ValueKey<String>('algorithm-select-all'),
                  onPressed: _selectAll,
                  icon: const Icon(Icons.done_all, size: 18),
                  label: Text(context.strings('algorithm.selectAll')),
                ),
                TextButton.icon(
                  key: const ValueKey<String>('algorithm-select-none'),
                  onPressed: _selected.length > 1 ? _resetSelection : null,
                  icon: const Icon(Icons.remove_done, size: 18),
                  label: Text(context.strings('algorithm.selectNone')),
                ),
                const Spacer(),
                Text(
                  context.strings(
                    'algorithm.selectedCount',
                    args: <String, String>{
                      'count': '${_selected.length}',
                      'total': '${widget.catalogue.length}',
                    },
                  ),
                  style: theme.textTheme.bodySmall,
                ),
              ],
            ),
            const SizedBox(height: 12),
            Expanded(
              child: visible.isEmpty
                  ? Center(
                      child: Text(
                        context.strings('algorithm.noMatch'),
                        style: theme.textTheme.bodyMedium,
                      ),
                    )
                  : ListView(
                      children: <Widget>[
                        for (final MapEntry<AlgorithmCategory, List<AlgorithmInfo>>
                                entry
                            in grouped.entries) ...<Widget>[
                          Padding(
                            padding: const EdgeInsets.only(top: 8, bottom: 6),
                            child: Text(
                              _categoryLabel(context, entry.key),
                              style: theme.textTheme.labelLarge?.copyWith(
                                color: theme.colorScheme.primary,
                              ),
                            ),
                          ),
                          for (final AlgorithmInfo info in entry.value)
                            CheckboxListTile(
                              key: ValueKey<String>(
                                'algorithm-picker-${info.id}',
                              ),
                              dense: true,
                              value: _selected.contains(info.id),
                              onChanged: (bool? _) => _toggle(info.id),
                              title: Text(info.displayName),
                              subtitle: Text(
                                info.suffixWarning ??
                                    '${info.id} · ${info.hexLength} hex',
                              ),
                              secondary: info.suffixUsable
                                  ? const Icon(Icons.bookmark_added_outlined)
                                  : null,
                            ),
                        ],
                      ],
                    ),
            ),
          ],
        ),
      ),
      actions: <Widget>[
        TextButton(
          key: const ValueKey<String>('algorithm-picker-cancel'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.strings('common.cancel')),
        ),
        FilledButton(
          key: const ValueKey<String>('algorithm-picker-confirm'),
          onPressed: _selected.isEmpty
              ? null
              : () => Navigator.of(context).pop(<String>{..._selected}),
          child: Text(context.strings('common.confirm')),
        ),
      ],
    );
  }
}