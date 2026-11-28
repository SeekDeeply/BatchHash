import 'package:flutter/material.dart';

import '../app_state.dart';
import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../theme.dart';
import '../util/formatting.dart';
import '../widgets/empty_state.dart';
import '../widgets/status_chip.dart';
import 'history_page.dart';

/// Translation key for the label of one [RenameRowStatus].
String _rowStatusKey(RenameRowStatus status) => switch (status) {
  RenameRowStatus.ready => 'rename.status.ready',
  RenameRowStatus.conflict => 'rename.status.conflict',
  RenameRowStatus.skipped => 'rename.status.skipped',
  RenameRowStatus.missingHash => 'rename.status.missingHash',
  RenameRowStatus.invalidName => 'rename.status.invalidName',
  RenameRowStatus.noPermission => 'rename.status.noPermission',
  RenameRowStatus.unsupported => 'rename.status.unsupported',
  RenameRowStatus.hashMismatch => 'rename.strictMismatch',
  RenameRowStatus.suffixMismatch => 'rename.suffixMismatch',
  RenameRowStatus.suffixUnverified => 'rename.suffixUnverified',
};

/// Suffix based rename workflow with a pure dry run before execution.
///
/// The recording log is reached from the header rather than the bottom
/// navigation: records only exist because of a rename, so the entry point sits
/// next to the page that creates them instead of competing with the four other
/// top-level destinations.
class RenamePage extends StatefulWidget {
  const RenamePage({super.key, required this.state});

  final BatchHashAppState state;

  @override
  State<RenamePage> createState() => _RenamePageState();
}

class _RenamePageState extends State<RenamePage> {
  /// Configuration is owned by the app state so it survives tab switches and
  /// restarts; the page only mirrors it for rendering.
  RenameConfig get _config => widget.state.renameConfig.value;

  bool _planning = false;
  bool _executing = false;
  String? _error;

  /// Whether the next plan strips existing suffixes instead of writing them.
  bool _clearSuffix = false;

  /// Entries this page hashed itself, kept in the app state.
  String? _message;
  RenamePlanModel? _plan;

  /// Pushes the recording log as its own route.
  ///
  /// It is deliberately not a destination: the log is only meaningful after a
  /// rename, so it opens from here and dismisses back to this page.
  void _openHistory() {
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (BuildContext context) =>
            HistoryPage(service: widget.state.service),
      ),
    );
  }

  /// Algorithms the user may write, taken from the hash page selection.
  ///
  /// The rename page used to offer the whole catalogue, which let a user write
  /// a suffix from an algorithm the main page never computed. The two pages
  /// then disagreed about what "the hash" means for the same file. The list is
  /// now the hash page selection intersected with the algorithms that can
  /// actually produce a filename suffix; when that intersection is empty the
  /// fallback keeps the page usable instead of showing an empty dropdown.
  List<AlgorithmInfo> _availableAlgorithms(List<BatchEntry> entries) {
    // What the files in hand actually have, not what the hash page happens to
    // have ticked right now. The two drift apart constantly: the user switches
    // to this page after a run, then changes the selection on the hash page, and
    // the stored config still names an algorithm no entry was hashed with. The
    // core then reports every row as `missingHash`, which reads as a data problem
    // rather than a stale setting.
    final Set<String> computed = <String>{
      for (final BatchEntry entry in entries) ...entry.hashes.keys,
    };
    final Set<String> selected = widget.state.algorithms.value;
    final List<AlgorithmInfo> usable = <AlgorithmInfo>[
      for (final AlgorithmInfo info in kSuffixUsableAlgorithms)
        if (computed.contains(info.id)) info,
    ];
    if (usable.isNotEmpty) {
      return usable;
    }
    // Nothing has been hashed yet, or nothing usable. Fall back to the hash page
    // selection so the dropdown is not empty, and let [_preview] refuse with an
    // explanation rather than sending a request that cannot succeed.
    final List<AlgorithmInfo> fromSelection = <AlgorithmInfo>[
      for (final AlgorithmInfo info in kSuffixUsableAlgorithms)
        if (selected.contains(info.id)) info,
    ];
    if (fromSelection.isNotEmpty) {
      return fromSelection;
    }
    return kSuffixUsableAlgorithms;
  }

  /// Whether the entries carry a digest for any writable algorithm.
  bool _hasUsableHashes(List<BatchEntry> entries) {
    final Set<String> computed = <String>{
      for (final BatchEntry entry in entries) ...entry.hashes.keys,
    };
    for (final AlgorithmInfo info in kSuffixUsableAlgorithms) {
      if (computed.contains(info.id)) {
        return true;
      }
    }
    return false;
  }

  /// The effective algorithm list for both the UI and the request.
  ///
  /// One source for both: the dropdown, the extra-algorithm checkboxes and the
  /// preview call all read this, so the request cannot name an algorithm the UI
  /// never offered.
  List<String> _effectiveAlgorithms(List<BatchEntry> entries) {
    final List<AlgorithmInfo> available = _availableAlgorithms(entries);
    final Set<String> availableIds = <String>{
      for (final AlgorithmInfo info in available) info.id,
    };
    final List<String> wanted = <String>[
      for (final String id in _config.algorithms)
        if (availableIds.contains(id)) id,
    ];
    if (wanted.isNotEmpty) {
      return wanted;
    }
    return <String>[available.first.id];
  }

  /// The algorithm the dropdown shows.
  ///
  /// Taken straight from [_effectiveAlgorithms] so the dropdown, the checkbox
  /// list and the request can never disagree. Resolving it separately was how the
  /// dropdown ended up showing a stale stored algorithm while the preview sent a
  /// filtered one.
  String _primaryAlgorithm(List<BatchEntry> entries) {
    return _effectiveAlgorithms(entries).first;
  }

  /// The longest suffix the current primary algorithm can produce.
  ///
  /// A digest has a fixed number of hex characters; asking for more would
  /// require padding or silent truncation, and asking for exactly that many is
  /// the most a user can legitimately write.
  int _maxSuffixLength(List<BatchEntry> entries) {
    final String primary = _primaryAlgorithm(entries);
    for (final AlgorithmInfo info in kSuffixUsableAlgorithms) {
      if (info.id == primary && info.hexLength > 0) {
        return info.hexLength;
      }
    }
    return kMaxRenameLength;
  }

  /// Write [next] into the shared state, which is what persists it.
  ///
  /// Going through the notifier instead of a local field is what makes the
  /// length and algorithm choices survive a restart: the app-level listener
  /// saves on change, and the shell rebuilds this page from the same value.
  void _updateConfig(RenameConfig next) {
    widget.state.renameConfig.value = next;
  }

  Future<void> _preview(List<BatchEntry> entries) async {
    setState(() {
      _planning = true;
      _error = null;
      _message = null;
    });
    try {
      final RenamePlanModel plan = await widget.state.service.planRenames(
        entries: entries,
        algorithms: _effectiveAlgorithms(entries),
        // Clamped like the slider: a stored length longer than the current
        // algorithm can produce would be rejected by the core, and the slider
        // already refuses to show it.
        length: _config.length.clamp(0, _maxSuffixLength(entries)),
        uppercase: _config.uppercase,
        strict: _config.strict,
        clearSuffix: _clearSuffix,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _plan = plan;
        _planning = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _planning = false;
        _error = context.strings(
          'rename.previewFailed',
          args: <String, String>{'error': '$error'},
        );
      });
    }
  }

  /// Folder the batch renames into, taken from the first planned row.
  ///
  /// Every row of one plan shares a root, so the first ready row names it. A
  /// plan with nothing ready has no target and returns null.
  static String? _renameTargetDirectory(RenamePlanModel plan) {
    for (final RenamePlanRowModel row in plan.rows) {
      if (row.status != RenameRowStatus.ready) {
        continue;
      }
      final int slash = row.path.lastIndexOf('/');
      final int backslash = row.path.lastIndexOf('\\');
      final int cut = slash > backslash ? slash : backslash;
      if (cut <= 0) {
        return null;
      }
      return row.path.substring(0, cut);
    }
    return null;
  }

  Future<void> _execute() async {
    final RenamePlanModel? plan = _plan;
    if (plan == null) {
      return;
    }
    setState(() {
      _executing = true;
      _error = null;
      _message = null;
    });
    try {
      final RenameExecuteResult result = await widget.state.service
          .executeRenames(plan);
      if (!mounted) {
        return;
      }
      // The record is what makes a later undo possible, so when the user asked
      // for it the copy goes beside the renamed files rather than only next to
      // the scanned root. A failure here must not read as a failed rename: the
      // files are already renamed and saying otherwise would be a lie.
      String? logPath;
      String? logError;
      if (widget.state.exportSql.value) {
        final String? directory = _renameTargetDirectory(plan);
        if (directory != null) {
          try {
            logPath = await widget.state.service.exportLogTo(directory);
          } catch (error) {
            logError = '$error';
          }
        }
      }
      if (!mounted) {
        return;
      }
      setState(() {
        _executing = false;
        _message = context.strings(
          'rename.resultSummary',
          args: <String, String>{
            'applied': '${result.applied}',
            'skipped': '${result.skipped}',
            'failed': '${result.failed}',
          },
        );
        if (logPath != null) {
          _message = '$_message\n';
          _message = '$_message${context.strings('rename.logCopied', args: <String, String>{'path': logPath})}';
        } else if (logError != null) {
          _message = '${_message!}\n';
          _message = '$_message${context.strings('rename.logCopyFailed', args: <String, String>{'error': logError})}';
        }
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _executing = false;
        _error = context.strings(
          'rename.executeFailed',
          args: <String, String>{'error': '$error'},
        );
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    // One listenable: the hash page owns the file list now, so this page only
    // has to repaint when that list changes.
    return ValueListenableBuilder<List<BatchEntry>>(
      valueListenable: widget.state.entries,
      builder: (BuildContext context, List<BatchEntry> entries, Widget? _) {

          final int hashedCount = entries
              .where((BatchEntry entry) => entry.hashes.isNotEmpty)
              .length;
          final int fileCount = hashedCount > 0 ? hashedCount : entries.length;
          final int recommended = recommendedSuffixLength(fileCount);
          final double probability = collisionProbability(fileCount, _config.length);
          final RenamePlanModel? plan = _plan;
          final String? error = _error;
          final String? message = _message;
          return SingleChildScrollView(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Expanded(
                      child: Text(
                        context.strings('rename.title'),
                        style: theme.textTheme.headlineSmall,
                      ),
                    ),
                    // Recording entry point: kept in the header so it is reachable
                    // even when there is nothing to rename yet.
                    IconButton(
                      key: const ValueKey<String>('rename-open-history'),
                      onPressed: _openHistory,
                      icon: const Icon(Icons.history),
                      tooltip: context.strings('history.open'),
                    ),
                  ],
                ),
                const SizedBox(height: 4),
                Text(
                  context.strings('rename.subtitle'),
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
                const SizedBox(height: 12),
                // No source picker here on purpose: the hash page already owns
                // that job, and a second entry point meant two lists that could
                // disagree about what "the current files" are. This page reads
                // whatever the hash page produced, so switching tabs is the
                // whole workflow.
                Text(
                  context.strings('rename.followsHashPage'),
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
                if (entries.isEmpty) ...<Widget>[
                  const SizedBox(height: 48),
                  EmptyState(
                    icon: Icons.drive_file_rename_outline,
                    title: context.strings('rename.noHashed'),
                    message: context.strings('rename.noHashedHint'),
                  ),
                ] else ...<Widget>[
                  const SizedBox(height: 20),
                  Row(
                    children: <Widget>[
                      SizedBox(
                        width: 120,
                        child: Text(
                          context.strings('rename.algorithm'),
                          style: theme.textTheme.bodyMedium,
                        ),
                      ),
                      Expanded(
                        child: InputDecorator(
                          decoration: const InputDecoration(
                            border: OutlineInputBorder(),
                            isDense: true,
                            contentPadding: EdgeInsets.symmetric(
                              horizontal: 12,
                              vertical: 8,
                            ),
                          ),
                          child: DropdownButtonHideUnderline(
                            child: DropdownButton<String>(
                              key: const ValueKey<String>(
                                'rename-algorithm-dropdown',
                              ),
                              // Must be one of the offered items, or Dropdown
                              // asserts: the hash page selection can exclude the
                              // stored primary, so fall back to the first offered
                              // algorithm rather than crash.
                              value: _primaryAlgorithm(entries),
                              isDense: true,
                              isExpanded: true,
                              onChanged: (String? value) {
                                if (value == null) {
                                  return;
                                }
                                setState(() {
                                  // Replacing the first entry keeps the primary
                                  // algorithm as the one shown in the dropdown;
                                  // extra algorithms are managed below it.
                                  _updateConfig(
                                    _config.copyWith(
                                      algorithms: <String>[
                                        value,
                                        ..._config.algorithms.skip(1),
                                      ],
                                    ),
                                  );
                                  _plan = null;
                                });
                              },
                              items: <DropdownMenuItem<String>>[
                                for (final AlgorithmInfo info
                                    in _availableAlgorithms(entries))
                                  DropdownMenuItem<String>(
                                    value: info.id,
                                    child: Text(info.displayName),
                                  ),
                              ],
                            ),
                          ),
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: <Widget>[
                      SizedBox(
                        width: 120,
                        child: Text(
                          context.strings('rename.length'),
                          style: theme.textTheme.bodyMedium,
                        ),
                      ),
                      Expanded(
                        // The upper bound is the selected algorithm's own hex
                        // length: taking more characters than the digest holds
                        // would either pad or silently truncate. Zero is allowed
                        // because a zero-length suffix is a meaningful choice —
                        // it writes the separator only.
                        child: Slider(
                          key: const ValueKey<String>('rename-length-slider'),
                          value: _config.length
                              .clamp(0, _maxSuffixLength(entries))
                              .toDouble(),
                          min: 0,
                          max: _maxSuffixLength(entries).toDouble(),
                          divisions: _maxSuffixLength(entries),
                          label: '${_config.length.clamp(0, _maxSuffixLength(entries))}',
                          onChanged: (double value) {
                            setState(() {
                              _updateConfig(
                                _config.copyWith(length: value.round()),
                              );
                              _plan = null;
                              _message = null;
                            });
                          },
                        ),
                      ),
                      SizedBox(
                        width: 32,
                        child: Text('${_config.length}', textAlign: TextAlign.end),
                      ),
                    ],
                  ),
                  // Extra algorithms: the primary one is the dropdown above, and
                  // any ticked here is appended to the same bracket group. Restricted
                  // to suffix-usable algorithms because writing an algorithm whose
                  // digest length is ambiguous would make the name unparseable.
                  ExpansionTile(
                    key: const ValueKey<String>('rename-extra-algorithms'),
                    tilePadding: EdgeInsets.zero,
                    title: Text(context.strings('rename.moreAlgorithms')),
                    subtitle: Text(
                      context.strings('rename.moreAlgorithmsHint'),
                    ),
                    children: <Widget>[
                      for (final AlgorithmInfo info in _availableAlgorithms(entries))
                        CheckboxListTile(
                          key: ValueKey<String>(
                            'rename-extra-${info.id}',
                          ),
                          dense: true,
                          contentPadding: EdgeInsets.zero,
                          title: Text(info.displayName),
                          // Read from the effective list, not the stored config:
                          // a stored algorithm the files were never hashed with
                          // would otherwise render as ticked and be sent to the
                          // core, which answers `missingHash` for every row.
                          value: _effectiveAlgorithms(entries).contains(info.id),
                          // The primary algorithm is never removable here: the
                          // dropdown above owns it, and unchecking it from a second
                          // control would leave the page with no algorithm at all.
                          onChanged: info.id == _primaryAlgorithm(entries)
                              ? null
                              : (bool? selected) {
                                  // Start from the effective list, so an obsolete
                                  // stored algorithm is dropped rather than carried
                                  // forward by the next edit.
                                  final List<String> next = <String>[
                                    ..._effectiveAlgorithms(entries),
                                  ];
                                  if (selected == true) {
                                    if (!next.contains(info.id)) {
                                      next.add(info.id);
                                    }
                                  } else {
                                    next.remove(info.id);
                                  }
                                  setState(() {
                                    _updateConfig(
                                      _config.copyWith(algorithms: next),
                                    );
                                    _plan = null;
                                  });
                                },
                        ),
                    ],
                  ),
                  // Default on: the record is what makes a later undo possible,
                  // and a user who loses it cannot get it back. It is switchable
                  // because the file is noise for someone who only wants the
                  // renamed files.
                  ValueListenableBuilder<bool>(
                    valueListenable: widget.state.exportSql,
                    builder: (BuildContext context, bool value, Widget? _) {
                      return SwitchListTile(
                        key: const ValueKey<String>('rename-copy-log-switch'),
                        contentPadding: EdgeInsets.zero,
                        title: Text(context.strings('rename.copyLog')),
                        subtitle: Text(context.strings('rename.copyLogHint')),
                        value: value,
                        onChanged: (bool next) {
                          widget.state.exportSql.value = next;
                        },
                      );
                    },
                  ),
                  SwitchListTile(
                    key: const ValueKey<String>('rename-strict-switch'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(context.strings('rename.strictMode')),
                    subtitle: Text(context.strings('rename.strictModeHint')),
                    value: _config.strict,
                    onChanged: (bool value) {
                      setState(() {
                        _updateConfig(_config.copyWith(strict: value));
                        _plan = null;
                      });
                    },
                  ),
                  SwitchListTile(
                    key: const ValueKey<String>('rename-clear-suffix-switch'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(context.strings('rename.clearSuffix')),
                    subtitle: Text(context.strings('rename.clearSuffixHint')),
                    value: _clearSuffix,
                    onChanged: (bool value) {
                      setState(() {
                        _clearSuffix = value;
                        _plan = null;
                      });
                    },
                  ),
                  SwitchListTile(
                    key: const ValueKey<String>('rename-uppercase-switch'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(context.strings('rename.uppercase')),
                    value: _config.uppercase,
                    onChanged: (bool value) {
                      setState(() {
                        _updateConfig(_config.copyWith(uppercase: value));
                        _plan = null;
                      });
                    },
                  ),
                  Text(
                    context.strings(
                      'rename.collisionEstimate',
                      args: <String, String>{
                        'length': '${_config.length}',
                        'files': '$fileCount',
                        'probability': formatProbabilityPercent(probability),
                        'recommended': '$recommended',
                      },
                    ),
                    key: const ValueKey<String>('rename-collision-estimate'),
                    style: theme.textTheme.bodyMedium,
                  ),
                  if (!_hasUsableHashes(entries)) ...<Widget>[
                    const SizedBox(height: 12),
                    Container(
                      padding: const EdgeInsets.all(12),
                      decoration: BoxDecoration(
                        color: Theme.of(context).colorScheme.surfaceContainerHighest,
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: Row(
                        children: <Widget>[
                          Icon(
                            Icons.info_outline,
                            size: 18,
                            color: Theme.of(context).colorScheme.onSurfaceVariant,
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              context.strings('rename.needHashes'),
                              style: Theme.of(context).textTheme.bodySmall,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                  const SizedBox(height: 16),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: <Widget>[
                      FilledButton.icon(
                        key: const ValueKey<String>('rename-dry-run-button'),
                        // Refused up front when nothing usable has been hashed:
                        // the request would come back with every row marked
                        // `missingHash`, which looks like corrupt data instead of
                        // a step the user has not done yet.
                        onPressed:
                            _planning || _executing || !_hasUsableHashes(entries)
                            ? null
                            : () => _preview(entries),
                        icon: const Icon(Icons.preview),
                        label: Text(context.strings('rename.preview')),
                      ),
                      FilledButton.tonalIcon(
                        key: const ValueKey<String>('rename-execute-button'),
                        onPressed:
                            plan == null || plan.readyCount == 0 || _executing
                            ? null
                            : _execute,
                        icon: const Icon(Icons.drive_file_rename_outline),
                        label: Text(context.strings('rename.apply')),
                      ),
                    ],
                  ),
                  if (_planning) ...<Widget>[
                    const SizedBox(height: 12),
                    const LinearProgressIndicator(),
                  ],
                  if (error != null) ...<Widget>[
                    const SizedBox(height: 12),
                    _InlineMessage(message: error, isError: true),
                  ],
                  if (message != null) ...<Widget>[
                    const SizedBox(height: 12),
                    _InlineMessage(message: message, isError: false),
                  ],
                  if (plan != null) ...<Widget>[
                    const SizedBox(height: 20),
                    Row(
                      children: <Widget>[
                        Expanded(
                          child: Text(
                            context.strings('rename.dryRun'),
                            style: theme.textTheme.titleMedium,
                          ),
                        ),
                        Text(
                          context.strings(
                            'rename.planCounts',
                            args: <String, String>{
                              'ready': '${plan.readyCount}',
                              'conflict': '${plan.conflictCount}',
                            },
                          ),
                          style: theme.textTheme.bodySmall,
                        ),
                        // Permission failures are called out on their own: the
                        // files are present and unchanged, so the user can fix
                        // this by granting access rather than by editing names.
                        if (plan.noPermissionCount > 0) ...<Widget>[
                          const SizedBox(height: 4),
                          Text(
                            context.strings(
                              'rename.noPermissionNotice',
                              args: <String, String>{
                                'count': '${plan.noPermissionCount}',
                              },
                            ),
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: scheme.error,
                            ),
                          ),
                        ],
                      ],
                    ),
                    const SizedBox(height: 8),
                    Card(
                      child: Column(
                        children: <Widget>[
                          for (final RenamePlanRowModel row in plan.rows)
                            _RenamePlanTile(row: row),
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
}

class _RenamePlanTile extends StatelessWidget {
  const _RenamePlanTile({required this.row});

  final RenamePlanRowModel row;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? target = row.targetPath;
    final String? message = row.message;
    return ListTile(
      dense: true,
      title: Text(
        row.path,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: theme.textTheme.bodyMedium,
      ),
      subtitle: Text(
        target ?? message ?? context.strings('rename.noTarget'),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: theme.textTheme.bodySmall,
      ),
      trailing: StatusChip(
        label: context.strings(_rowStatusKey(row.status)),
        color: renameRowStatusColor(row.status, scheme),
        icon: renameRowStatusIcon(row.status),
      ),
    );
  }
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
