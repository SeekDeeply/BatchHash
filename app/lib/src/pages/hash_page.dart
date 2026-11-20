import 'dart:async';
import 'dart:io' show Platform;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app_state.dart';
import '../l10n/app_strings_scope.dart';
import '../models/models.dart';
import '../util/formatting.dart';
import '../util/platform_pickers.dart';
import '../util/saf_bridge.dart';
import '../widgets/algorithm_picker_dialog.dart';
import '../widgets/empty_state.dart';
import '../widgets/entry_list.dart';
import '../widgets/path_input_dialog.dart';
import '../widgets/summary_card.dart';

/// Batch hashing workflow: choose input, configure, calculate, inspect, export.
///
/// Native file dialogs are used where the platform provides them. The scanned
/// entries, progress, report and selected algorithms survive tab changes.
class HashPage extends StatefulWidget {
  const HashPage({super.key, required this.state});

  final BatchHashAppState state;

  @override
  State<HashPage> createState() => _HashPageState();
}

class _HashPageState extends State<HashPage> {
  late Set<String> _algorithms;
  late List<BatchEntry> _entries;
  late HashProgress? _progress;
  late Duration? _elapsed;
  late String? _error;
  late bool _cancelled;
  bool _scanning = false;

  /// Algorithms shown as one-tap chips on the home step.
  ///
  /// A fixed, small set keeps the primary flow fast; everything else is one
  /// tap away through the picker. Chips reflect these ids, and picking an
  /// algorithm that is not pinned replaces the last pin.
  List<String> _pinned = <String>[...kDefaultPinnedAlgorithms];
  bool _computing = false;

  /// True from the moment an export starts until it finishes or fails.
  ///
  /// The export awaits the folder picker, so without this the buttons stay
  /// live while the dialog is open and every extra tap opens another dialog.
  bool _exporting = false;
  StreamSubscription<HashProgress>? _subscription;

  @override
  void initState() {
    super.initState();
    final BatchHashAppState app = widget.state;
    _algorithms = <String>{...app.algorithms.value};
    // The chips on screen come from _pinned, not from _algorithms. Leaving it at
    // the default list meant a restored selection that included an algorithm
    // outside those defaults rendered with nothing ticked: the value was
    // correct in state and invisible in the UI.
    _pinned = _visibleChips(_algorithms);
    _entries = app.entries.value;
    _progress = app.progress.value;
    _elapsed = app.elapsed.value;
    _error = app.error.value;
    _cancelled = app.cancelled.value;
    // The stored selection is read from disk asynchronously at start-up, which
    // usually finishes after this page is built. Copying the value once in
    // initState therefore kept the defaults forever: the shell rebuilt, but this
    // State was already created so initState never ran again.
    app.algorithms.addListener(_onAlgorithmsChangedExternally);
  }

  /// Adopt a selection that changed outside this page.
  ///
  /// Only the persisted restore path does this today; the page is the sole
  /// editor otherwise, so a plain overwrite is correct and avoids the loop a
  /// two-way binding would create.
  void _onAlgorithmsChangedExternally() {
    final Set<String> next = widget.state.algorithms.value;
    if (_sameSelection(next, _algorithms)) {
      return;
    }
    setState(() {
      _algorithms = <String>{...next};
      // Keep the restored algorithms visible as chips; otherwise a value that
      // is not in the default list has no chip and looks unselected.
      _pinned = _visibleChips(_algorithms);
    });
  }

  /// Defaults first, then any selected algorithm the defaults do not cover.
  ///
  /// Order matters: the common algorithms stay in a stable place run to run,
  /// while a restored selection still gets a chip it can be toggled from.
  static List<String> _visibleChips(Set<String> selected) {
    final List<String> chips = <String>[...kDefaultPinnedAlgorithms];
    for (final String id in selected) {
      if (!chips.contains(id)) {
        chips.add(id);
      }
    }
    return chips;
  }

  static bool _sameSelection(Set<String> a, Set<String> b) =>
      a.length == b.length && a.containsAll(b);

  @override
  void dispose() {
    widget.state.algorithms.removeListener(_onAlgorithmsChangedExternally);
    unawaited(_subscription?.cancel());
    super.dispose();
  }

  Future<void> _selectFiles() async {
    List<String> paths = <String>[];
    try {
      final FilePickerResult? result = await FilePicker.platform.pickFiles(
        allowMultiple: true,
        type: FileType.any,
        withData: false,
      );
      paths = <String>[
        for (final PlatformFile file in result?.files ?? const <PlatformFile>[])
          if (file.path != null && file.path!.isNotEmpty) file.path!,
      ];
    } on PlatformException {
      paths = await _fallbackFilePaths();
    } on MissingPluginException {
      paths = await _fallbackFilePaths();
    }
    if (!mounted || paths.isEmpty) {
      return;
    }
    await _scanFiles(paths);
  }

  Future<List<String>> _fallbackFilePaths() async {
    final String? input = await PathInputDialog.show(
      context,
      title: context.strings('hash.chooseFiles'),
      hintText: context.strings('path.hint.files'),
      helperText: context.strings('path.filesHelper'),
      allowMultiple: true,
      suggestions: widget.state.recentPaths.value,
    );
    if (input == null) return <String>[];
    return <String>[
      for (final String line in input.split('\n'))
        if (line.trim().isNotEmpty) line.trim(),
    ];
  }

  Future<String?> _pickFolderPath() async {
    try {
      return await PlatformPickers.directory(_fallbackFolderPath);
    } on PlatformException {
      return _fallbackFolderPath();
    } on MissingPluginException {
      return _fallbackFolderPath();
    }
  }

  Future<String?> _fallbackFolderPath() async {
    return PathInputDialog.show(
      context,
      title: context.strings('hash.chooseFolder'),
      hintText: context.strings('path.hint.folder'),
      helperText: context.strings('path.folderHelper'),
      initialValue: widget.state.recentPaths.value.isEmpty
          ? ''
          : widget.state.recentPaths.value.first,
      suggestions: widget.state.recentPaths.value,
    );
  }

  Future<void> _selectFolder() async {
    // Android cannot hand over a folder as a path: its picker grants a
    // content:// tree that no file API can open. That platform walks the tree
    // through the system instead, and hashes through descriptors.
    if (Platform.isAndroid) {
      await _selectSafFolder();
      return;
    }
    final String? input = await _pickFolderPath();
    if (!mounted || input == null || input.trim().isEmpty) {
      return;
    }
    final String folder = input.trim();
    final bool? recursive = await _askRecursive();
    if (!mounted || recursive == null) {
      return;
    }
    widget.state.scanSource.value = PathScanSource(folder);
    await _scanInputs(<String>[folder], recursive: recursive);
  }

  /// Walk a folder the user granted through the Storage Access Framework.
  Future<void> _selectSafFolder() async {
    final String treeUri;
    try {
      final SafDirectory? picked = await PlatformPickers.safDirectory();
      if (!mounted || picked == null) {
        return;
      }
      treeUri = picked.treeUri;
    } on SafException catch (error) {
      if (!mounted) {
        return;
      }
      // Reported rather than swallowed: a refused pick that looks like a
      // cancelled one leaves the user pressing the button forever.
      final String failure = context.strings(
        'hash.scanFailed',
        args: <String, String>{'error': '${error.code}: ${error.message}'},
      );
      setState(() => _error = failure);
      widget.state.error.value = failure;
      return;
    }
    final bool? recursive = await _askRecursive();
    if (!mounted || recursive == null) {
      return;
    }
    setState(() {
      _scanning = true;
      _error = null;
      _cancelled = false;
    });
    try {
      // Depth 1 is "this folder only"; the recursive dialog asks for the whole
      // tree, which the platform walker bounds with its own limit.
      final List<BatchEntry>? scanned = await widget.state.service
          .scanSafTree(
            treeUri: treeUri,
            maxDepth: recursive ? 8 : 1,
          );
      if (!mounted) {
        return;
      }
      if (scanned == null) {
        // The service has no Android storage to walk, which on this platform
        // means the native side is missing rather than that the folder is
        // empty.
        setState(() {
          _scanning = false;
          _error = context.strings('hash.scanFailed');
        });
        return;
      }
      widget.state.scanSource.value = SafScanSource(
        treeUri: treeUri,
        displayName: '',
      );
      setState(() {
        _entries = scanned;
        _progress = null;
        _elapsed = null;
        _scanning = false;
      });
      final BatchHashAppState app = widget.state;
      app.entries.value = scanned;
      app.report.value = null;
      app.progress.value = null;
      app.elapsed.value = null;
      app.error.value = null;
      app.cancelled.value = false;
      app.pickedFiles.value = const <String>[];
    } catch (error) {
      if (!mounted) {
        return;
      }
      final String failure = context.strings(
        'hash.scanFailed',
        args: <String, String>{'error': '$error'},
      );
      setState(() {
        _scanning = false;
        _error = failure;
      });
      widget.state.error.value = failure;
    }
  }

  Future<bool?> _askRecursive() {
    return showDialog<bool>(
      context: context,
      builder: (BuildContext dialogContext) => AlertDialog(
        title: Text(context.strings('hash.recursiveTitle')),
        content: Text(context.strings('hash.recursiveQuestion')),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text(context.strings('hash.recursiveNo')),
          ),
          FilledButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: Text(context.strings('hash.recursiveYes')),
          ),
        ],
      ),
    );
  }

  /// Scan exactly the files a native picker returned.
  Future<void> _scanFiles(List<String> paths) async {
    setState(() {
      _scanning = true;
      _error = null;
      _cancelled = false;
    });
    try {
      final BatchHashAppState app = widget.state;
      final List<BatchEntry> scanned = await app.service.scanFiles(paths);
      if (!mounted) {
        return;
      }
      for (final String path in paths.reversed) {
        app.rememberPath(path);
      }
      setState(() {
        _entries = scanned;
        _progress = null;
        _elapsed = null;
        _scanning = false;
      });
      app.entries.value = scanned;
      app.report.value = null;
      app.progress.value = null;
      app.elapsed.value = null;
      app.error.value = null;
      app.cancelled.value = false;
      app.scanRoot.value = app.service.lastScannedRoot;
      app.pickedFiles.value = List<String>.unmodifiable(paths);
    } catch (error) {
      if (!mounted) {
        return;
      }
      final String failure = context.strings(
        'hash.scanFailed',
        args: <String, String>{'error': '$error'},
      );
      setState(() {
        _scanning = false;
        _error = failure;
      });
      widget.state.error.value = failure;
    }
  }

  /// Scan every chosen folder path.
  Future<void> _scanInputs(
    List<String> paths, {
    required bool recursive,
  }) async {
    setState(() {
      _scanning = true;
      _error = null;
      _cancelled = false;
    });
    try {
      final List<BatchEntry> scanned = <BatchEntry>[];
      for (final String path in paths) {
        final List<BatchEntry> entries = await widget.state.service
            .scanDirectory(path, recursive: recursive);
        scanned.addAll(entries);
      }
      if (!mounted) {
        return;
      }
      final BatchHashAppState app = widget.state;
      for (final String path in paths.reversed) {
        app.rememberPath(path);
      }
      setState(() {
        _entries = scanned;
        _progress = null;
        _elapsed = null;
        _scanning = false;
      });
      app.entries.value = scanned;
      app.report.value = null;
      app.progress.value = null;
      app.elapsed.value = null;
      app.error.value = null;
      app.cancelled.value = false;
      app.scanRoot.value = app.service.lastScannedRoot;
      app.pickedFiles.value = const <String>[];
    } catch (error) {
      if (!mounted) {
        return;
      }
      final String failure = context.strings(
        'hash.scanFailed',
        args: <String, String>{'error': '$error'},
      );
      setState(() {
        _scanning = false;
        _error = failure;
      });
      widget.state.error.value = failure;
    }
  }

  void _clear() {
    final BatchHashAppState app = widget.state;
    setState(() {
      _entries = const <BatchEntry>[];
      _progress = null;
      _elapsed = null;
      _cancelled = false;
      _error = null;
    });
    app.entries.value = const <BatchEntry>[];
    app.report.value = null;
    app.progress.value = null;
    app.elapsed.value = null;
    app.error.value = null;
    app.cancelled.value = false;
    app.scanRoot.value = '';
    app.pickedFiles.value = const <String>[];
  }

  /// Display name for an algorithm id, falling back to the bare id.
  ///
  /// The catalogue is the single source of truth for names; a id the catalogue
  /// does not know still renders so a stale saved selection is not invisible.
  String _labelFor(String id) {
    for (final AlgorithmInfo info in kAlgorithmCatalogue) {
      if (info.id == id) {
        return info.displayName;
      }
    }
    return id;
  }

  /// Open the full picker; selections outside the pinned set replace a pin.
  Future<void> _openAlgorithmPicker() async {
    final Set<String>? chosen = await AlgorithmPickerDialog.show(
      context,
      catalogue: kAlgorithmCatalogue,
      selected: _algorithms,
    );
    if (chosen == null || chosen.isEmpty) {
      return;
    }
    setState(() {
      _algorithms = <String>{...chosen};
      // Keep the home chips useful: pin up to five of the chosen algorithms so
      // the common case needs no second trip through the picker.
      final List<String> next = <String>[
        for (final String id in chosen)
          if (_pinned.contains(id)) id,
      ];
      for (final String id in chosen) {
        if (next.length >= kDefaultPinnedAlgorithms.length) {
          break;
        }
        if (!next.contains(id)) {
          next.add(id);
        }
      }
      if (next.isNotEmpty) {
        _pinned = next;
      }
    });
    widget.state.algorithms.value = <String>{..._algorithms};
  }

  void _toggleAlgorithm(String algorithm, bool selected) {
    setState(() {
      if (selected) {
        _algorithms.add(algorithm);
      } else if (_algorithms.length > 1) {
        _algorithms.remove(algorithm);
      }
    });
    widget.state.algorithms.value = <String>{..._algorithms};
  }

  /// Icon for [format], matching the report page so one format looks the same
  /// wherever it is offered.
  static IconData _iconForFormat(ReportFormat format) => switch (format) {
    ReportFormat.json => Icons.data_object,
    ReportFormat.csv => Icons.table_chart,
    ReportFormat.markdown => Icons.article_outlined,
    ReportFormat.txt => Icons.notes,
    ReportFormat.md5sum => Icons.tag,
    ReportFormat.bsd => Icons.format_list_bulleted,
    ReportFormat.tree => Icons.account_tree_outlined,
  };

  /// Ask for an export destination, preferring the native folder picker.
  ///
  /// Returns null when the user cancels, which must not fall through to the
  /// service default: silently writing into an app-private folder is exactly
  /// the behaviour this replaced.
  Future<String?> _chooseExportDirectory() async {
    final String? picked = await PlatformPickers.directory(() async {
      return PathInputDialog.show(
        context,
        title: context.strings('hash.exportChooseTitle'),
        // Not path.hint.destination: that copy offers an empty value as "use
        // the app default", which this path deliberately refuses.
        hintText: context.strings('path.hint.folder'),
        helperText: context.strings('hash.exportChooseHint'),
        initialValue: widget.state.exportDirectory.value,
        suggestions: widget.state.recentPaths.value,
      );
    });
    if (!mounted) {
      return null;
    }
    if (picked == null || picked.trim().isEmpty) {
      return null;
    }
    final String directory = picked.trim();
    widget.state.exportDirectory.value = directory;
    widget.state.rememberPath(directory);
    return directory;
  }

  Future<void> _exportQuick(ReportFormat format) async {
    final ReportModel? report = widget.state.report.value;
    if (report == null || _exporting) return;
    setState(() {
      _exporting = true;
    });
    try {
      await _runExport(format, report);
    } finally {
      if (mounted) {
        setState(() {
          _exporting = false;
        });
      }
    }
  }

  /// Choose a destination and write one format, reporting either outcome.
  Future<void> _runExport(ReportFormat format, ReportModel report) async {
    final String? directory = await _chooseExportDirectory();
    if (directory == null) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.strings('hash.exportNoDestination'))),
      );
      return;
    }
    try {
      final List<ExportedReport> files = await widget.state.service
          .exportReport(
            report: report,
            formats: <ReportFormat>{format},
            directory: directory,
            // The switch defaults on: the log is what makes a later undo or
            // import possible, and a user who loses it cannot recover it.
            includeLog: widget.state.exportSql.value,
          );
      if (!mounted) {
        return;
      }
      if (files.isEmpty) {
        setState(() {
          _error = context.strings(
            'hash.exportFailed',
            args: <String, String>{'error': 'no file was written'},
          );
        });
        return;
      }
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            context.strings(
              'hash.exported',
              args: <String, String>{'path': files.first.path},
            ),
          ),
        ),
      );
      // The export produced a report, so show it: leaving the user on the hash
      // page would hide the very thing they just asked for.
      widget.state.reportFocus.value = widget.state.reportFocus.value + 1;
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _error = context.strings(
          'hash.exportFailed',
          args: <String, String>{'error': '$error'},
        );
      });
    }
  }

  /// Hash entries that live in a granted Android tree.
  ///
  /// Descriptors are opened and released in bounded groups, and the whole run
  /// is one future rather than a stream: progress is reported per group, which
  /// is the only granularity the platform can offer.
  Future<void> _startSafHashing(
    SafScanSource source,
    List<String> algorithms,
    Stopwatch clock,
  ) async {
    try {
      final List<BatchEntry> hashed = await widget.state.service.hashSafEntries(
        treeUri: source.treeUri,
        entries: _entries,
        algorithms: algorithms,
      );
      if (!mounted) {
        return;
      }
      final int done = hashed
          .where((BatchEntry entry) => entry.hashes.isNotEmpty)
          .length;
      final HashProgress progress = HashProgress(
        completed: done,
        total: hashed.length,
        done: true,
        entries: hashed,
      );
      setState(() {
        _entries = hashed;
        _progress = progress;
        // elapsed is a getter, not a method: the parentheses would call the
        // returned Duration.
        _elapsed = clock.elapsed;
        _computing = false;
      });
      final BatchHashAppState app = widget.state;
      app.entries.value = hashed;
      app.progress.value = progress;
      app.elapsed.value = _elapsed;
    } catch (error) {
      if (!mounted) {
        return;
      }
      final String failure = context.strings(
        'hash.hashFailed',
        args: <String, String>{'error': '$error'},
      );
      setState(() {
        _computing = false;
        _error = failure;
      });
      widget.state.error.value = failure;
    }
  }
  void _startHashing() {
    if (_entries.isEmpty || _algorithms.isEmpty) {
      return;
    }
    setState(() {
      _computing = true;
      _cancelled = false;
      _error = null;
      _progress = null;
      _elapsed = null;
    });
    widget.state.error.value = null;
    widget.state.cancelled.value = false;
    final Stopwatch stopwatch = Stopwatch()..start();
    final List<String> algorithms = _algorithms.toList()..sort();
    // A granted Android tree is hashed through descriptors rather than paths,
    // so the source decides which call runs. Falling through to the path
    // variant would ask the core to open a content URI, which fails on every
    // entry.
    final ScanSource? source = widget.state.scanSource.value;
    if (source is SafScanSource) {
      _startSafHashing(source, algorithms, stopwatch);
      return;
    }
    final Stream<HashProgress> stream = widget.state.service.computeHashes(
      entries: _entries,
      algorithms: algorithms,
    );
    _subscription = stream.listen(
      (HashProgress progress) {
        if (!mounted) {
          return;
        }
        setState(() {
          _progress = progress;
          if (progress.entries.isNotEmpty) {
            _entries = progress.entries;
          }
        });
        widget.state.progress.value = progress;
        if (progress.entries.isNotEmpty) {
          widget.state.entries.value = progress.entries;
        }
        if (progress.done) {
          stopwatch.stop();
          _finishHashing(progress, stopwatch.elapsed, algorithms);
        }
      },
      onError: (Object error) {
        if (!mounted) {
          return;
        }
        final String failure = context.strings(
          'hash.hashFailed',
          args: <String, String>{'error': '$error'},
        );
        setState(() {
          _computing = false;
          _error = failure;
        });
        widget.state.error.value = failure;
      },
      onDone: () {
        if (!mounted) {
          return;
        }
        if (_computing && !_cancelled) {
          setState(() {
            _computing = false;
          });
        }
      },
    );
  }

  void _finishHashing(
    HashProgress progress,
    Duration elapsed,
    List<String> algorithms,
  ) {
    final BatchHashAppState app = widget.state;
    final String root = app.scanRoot.value.trim();
    // The service roots explicit file selections at their shared directory, so
    // the cached value is always a directory; a fallback keeps a report valid
    // when nothing was scanned yet.
    final String reportRoot = root.isNotEmpty
        ? root
        : app.service.lastScannedRoot;
    final ReportModel report = ReportModel(
      root: reportRoot,
      generatedAt: DateTime.now(),
      algorithms: algorithms,
      entries: progress.entries,
    );
    _subscription = null;
    setState(() {
      _computing = false;
      _progress = progress;
      _entries = progress.entries;
      _elapsed = elapsed;
    });
    app.entries.value = progress.entries;
    app.report.value = report;
    app.progress.value = progress;
    app.elapsed.value = elapsed;
  }

  /// Human readable remaining time for the progress line.
  String _formatEta(double seconds) {
    if (seconds.isNaN || seconds.isInfinite || seconds < 0) {
      return context.strings('common.unknown');
    }
    final int whole = seconds.round();
    if (whole < 60) {
      return '${whole}s';
    }
    final int minutes = whole ~/ 60;
    final int rest = whole % 60;
    return rest == 0 ? '${minutes}m' : '${minutes}m ${rest}s';
  }

  Future<void> _cancelHashing() async {
    // Ask the native worker to stop first: dropping the Dart subscription alone
    // would leave the Rust job hashing a large file in the background.
    await widget.state.service.cancelCurrentOperation();
    final StreamSubscription<HashProgress>? subscription = _subscription;
    _subscription = null;
    await subscription?.cancel();
    if (!mounted) {
      return;
    }
    setState(() {
      _computing = false;
      _cancelled = true;
      _progress = null;
    });
    widget.state.cancelled.value = true;
    widget.state.progress.value = null;
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final String? error = _error;
    final HashProgress? progress = _progress;
    final Duration? elapsed = _elapsed;
    final String progressText = progress == null
        ? ''
        : context.strings(
            'hash.progressText',
            args: <String, String>{
              'done': '${progress.completed}',
              'total': '${progress.total}',
            },
          );
    final int okCount = _entries
        .where((BatchEntry entry) => entry.status == EntryStatus.ok)
        .length;
    final int failedCount = _entries
        .where((BatchEntry entry) => entry.status == EntryStatus.failed)
        .length;
    final int skippedCount = _entries
        .where((BatchEntry entry) => entry.status == EntryStatus.skipped)
        .length;
    final int totalBytes = _entries.fold<int>(
      0,
      (int sum, BatchEntry entry) => sum + entry.size,
    );
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            context.strings('hash.title'),
            style: theme.textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(
            context.strings('hash.subtitle'),
            style: theme.textTheme.bodyMedium?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 16),
          _StepHeader(
            index: 1,
            label: context.strings('hash.stepInput'),
            complete: _entries.isNotEmpty,
          ),
          const SizedBox(height: 8),
          _buildToolbar(),
          if (error != null) ...<Widget>[
            const SizedBox(height: 12),
            _MessageCard(message: error, isError: true),
          ],
          if (_cancelled) ...<Widget>[
            const SizedBox(height: 12),
            _MessageCard(
              message: context.strings('hash.cancelled'),
              isError: false,
            ),
          ],
          if (_entries.isNotEmpty) ...<Widget>[
            const SizedBox(height: 12),
            _InputSummary(entries: _entries, onClear: _clear),
          ],
          const SizedBox(height: 20),
          _StepHeader(
            index: 2,
            label: context.strings('hash.stepAlgorithms'),
            complete: _algorithms.isNotEmpty,
            detail: context.strings(
              'hash.algorithmCount',
              args: <String, String>{'count': '${_algorithms.length}'},
            ),
          ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: <Widget>[
              for (final String algorithm in _pinned)
                FilterChip(
                  key: ValueKey<String>('hash-algorithm-$algorithm'),
                  label: Text(_labelFor(algorithm)),
                  selected: _algorithms.contains(algorithm),
                  onSelected: _computing
                      ? null
                      : (bool selected) => _toggleAlgorithm(algorithm, selected),
                ),
              ActionChip(
                key: const ValueKey<String>('hash-algorithm-manage'),
                avatar: const Icon(Icons.tune, size: 18),
                label: Text(context.strings('algorithm.manage')),
                onPressed: _computing ? null : _openAlgorithmPicker,
              ),
            ],
          ),
          const SizedBox(height: 20),
          _StepHeader(
            index: 3,
            label: context.strings('hash.stepRun'),
            complete: _entries.isNotEmpty && !_computing && elapsed != null,
          ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: <Widget>[
              FilledButton.icon(
                key: const ValueKey<String>('hash-start'),
                onPressed: _entries.isEmpty || _computing || _scanning
                    ? null
                    : _startHashing,
                icon: const Icon(Icons.play_arrow),
                label: Text(
                  context.strings(_computing ? 'hash.hashing' : 'hash.start'),
                ),
              ),
              // A library without the async job API hashes synchronously, so
              // there is nothing to cancel; offering the button would lie.
              if (_computing && widget.state.service.supportsCancellation)
                OutlinedButton.icon(
                  key: const ValueKey<String>('hash-cancel'),
                  onPressed: _cancelHashing,
                  icon: const Icon(Icons.stop),
                  label: Text(context.strings('hash.cancel')),
                ),
            ],
          ),
          if (_scanning) ...<Widget>[
            const SizedBox(height: 12),
            const LinearProgressIndicator(),
          ],
          if (progress != null && _computing) ...<Widget>[
            const SizedBox(height: 12),
            LinearProgressIndicator(
              // Determinate whenever the native side reported a byte or entry
              // total; indeterminate only before the first sample arrives.
              value: progress.isDeterminate ? progress.fraction : null,
              semanticsLabel: context.strings('hash.progressLabel'),
            ),
            const SizedBox(height: 6),
            Text(
              context.strings(
                progress.phase == HashPhase.scanning
                    ? 'hash.phaseScanning'
                    : 'hash.phaseHashing',
              ),
              style: theme.textTheme.bodyMedium,
            ),
            const SizedBox(height: 2),
            Text(progressText, style: theme.textTheme.bodySmall),
            if (progress.bytesTotal > 0) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                context.strings(
                  'hash.progressBytes',
                  args: <String, String>{
                    'done': formatBytes(progress.bytesDone),
                    'total': formatBytes(progress.bytesTotal),
                  },
                ),
                style: theme.textTheme.bodySmall,
              ),
            ],
            if (progress.etaSeconds != null) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                context.strings(
                  'hash.progressEta',
                  args: <String, String>{
                    'eta': _formatEta(progress.etaSeconds!),
                  },
                ),
                style: theme.textTheme.bodySmall,
              ),
            ],
            if (progress.currentPath.isNotEmpty) ...<Widget>[
              const SizedBox(height: 2),
              Text(
                context.strings(
                  'hash.progressCurrent',
                  args: <String, String>{'path': progress.currentPath},
                ),
                style: theme.textTheme.bodySmall,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ],
          ],
          if (_entries.isNotEmpty) ...<Widget>[
            const SizedBox(height: 16),
            SummaryCard(
              title: context.strings('hash.summary'),
              metrics: <SummaryMetric>[
                SummaryMetric(
                  label: context.strings('metric.files'),
                  value: '${_entries.length}',
                ),
                SummaryMetric(
                  label: context.strings('metric.ok'),
                  value: '$okCount',
                  color: scheme.tertiary,
                ),
                SummaryMetric(
                  label: context.strings('metric.failed'),
                  value: '$failedCount',
                  color: scheme.error,
                ),
                SummaryMetric(
                  label: context.strings('metric.skipped'),
                  value: '$skippedCount',
                  color: scheme.outline,
                ),
                SummaryMetric(
                  label: context.strings('metric.totalSize'),
                  value: formatBytes(totalBytes),
                ),
                if (elapsed != null)
                  SummaryMetric(
                    label: context.strings('metric.elapsed'),
                    value: '${elapsed.inMilliseconds} ms',
                  ),
              ],
            ),
            const SizedBox(height: 20),
            _StepHeader(
              index: 4,
              label: context.strings('hash.stepResults'),
              complete: _entries.any(
                (BatchEntry entry) => entry.hashes.isNotEmpty,
              ),
            ),
            const SizedBox(height: 8),
            Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    context.strings('hash.results'),
                    style: theme.textTheme.titleMedium,
                  ),
                ),
                Text(
                  context.strings(
                    'common.fileCount',
                    args: <String, String>{'count': '${_entries.length}'},
                  ),
                  style: theme.textTheme.bodySmall,
                ),
              ],
            ),
            const SizedBox(height: 8),
            Align(
              alignment: Alignment.centerRight,
              child: Wrap(
                spacing: 8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: <Widget>[
                  // One menu instead of a button per format: the report page
                  // already offers seven, and two buttons could never show
                  // them all without turning the toolbar into a wall.
                  PopupMenuButton<ReportFormat>(
                    key: const ValueKey<String>('hash-export-menu'),
                    enabled: !_computing && !_exporting,
                    tooltip: context.strings('hash.export'),
                    onSelected: _exportQuick,
                    itemBuilder: (BuildContext context) => <
                        PopupMenuEntry<ReportFormat>>[
                      for (final ReportFormat format in ReportFormat.values)
                        PopupMenuItem<ReportFormat>(
                          value: format,
                          child: Row(
                            children: <Widget>[
                              Icon(_iconForFormat(format), size: 18),
                              const SizedBox(width: 10),
                              Text(format.label),
                            ],
                          ),
                        ),
                    ],
                    child: OutlinedButton.icon(
                      onPressed: null,
                      icon: const Icon(Icons.ios_share),
                      label: Text(context.strings('hash.export')),
                    ),
                  ),
                  // On by default: the log is what a later undo or import
                  // reads, and a user who loses it cannot get it back. It is
                  // switchable because it is noise for a report-only export.
                  ValueListenableBuilder<bool>(
                    valueListenable: widget.state.exportSql,
                    builder: (BuildContext context, bool value, Widget? _) {
                      return Row(
                        mainAxisSize: MainAxisSize.min,
                        children: <Widget>[
                          Checkbox(
                            key: const ValueKey<String>('hash-export-sql'),
                            value: value,
                            onChanged: (bool? next) {
                              widget.state.exportSql.value = next ?? true;
                            },
                          ),
                          Text(context.strings('hash.exportSql')),
                        ],
                      );
                    },
                  ),
                ],
              ),
            ),
            const SizedBox(height: 8),
            EntryList(entries: _entries),
          ] else if (!_scanning) ...<Widget>[
            const SizedBox(height: 48),
            EmptyState(
              icon: Icons.fingerprint,
              title: context.strings('hash.noFiles'),
              message: context.strings('hash.noFilesHint'),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildToolbar() {
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: <Widget>[
        FilledButton.icon(
          key: const ValueKey<String>('hash-choose-files'),
          onPressed: _computing || _scanning ? null : _selectFiles,
          icon: const Icon(Icons.insert_drive_file_outlined),
          label: Text(context.strings('hash.chooseFiles')),
        ),
        OutlinedButton.icon(
          key: const ValueKey<String>('hash-choose-folder'),
          onPressed: _computing || _scanning ? null : _selectFolder,
          icon: const Icon(Icons.folder_open_outlined),
          label: Text(context.strings('hash.chooseFolder')),
        ),
        if (_entries.isNotEmpty)
          TextButton.icon(
            onPressed: _computing || _scanning ? null : _clear,
            icon: const Icon(Icons.clear_all),
            label: Text(context.strings('hash.clear')),
          ),
      ],
    );
  }
}

/// One numbered step of the hashing workflow.
///
/// The check mark is the only state signal: a step is complete when its
/// output exists, so the user can see at a glance what is still missing.
class _StepHeader extends StatelessWidget {
  const _StepHeader({
    required this.index,
    required this.label,
    required this.complete,
    this.detail,
  });

  final int index;
  final String label;
  final bool complete;
  final String? detail;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    final Color color = complete ? scheme.primary : scheme.onSurfaceVariant;
    final String? detailText = detail;
    return Row(
      children: <Widget>[
        CircleAvatar(
          radius: 12,
          backgroundColor: complete
              ? scheme.primaryContainer
              : scheme.surfaceContainerHighest,
          child: complete
              ? Icon(Icons.check, size: 14, color: scheme.onPrimaryContainer)
              : Text(
                  '$index',
                  style: theme.textTheme.labelSmall?.copyWith(color: color),
                ),
        ),
        const SizedBox(width: 10),
        Expanded(
          child: Text(
            label,
            style: theme.textTheme.titleSmall?.copyWith(color: color),
          ),
        ),
        if (detailText != null)
          Text(
            detailText,
            style: theme.textTheme.bodySmall?.copyWith(
              color: scheme.onSurfaceVariant,
            ),
          ),
      ],
    );
  }
}
class _InputSummary extends StatelessWidget {
  const _InputSummary({required this.entries, required this.onClear});

  final List<BatchEntry> entries;
  final VoidCallback onClear;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Card(
      color: theme.colorScheme.surfaceContainerHighest,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 12),
        child: Row(
          children: <Widget>[
            Icon(Icons.folder_copy_outlined, color: theme.colorScheme.primary),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                context.strings(
                  'hash.inputSummary',
                  args: <String, String>{'count': '${entries.length}'},
                ),
                style: theme.textTheme.titleSmall,
              ),
            ),
            IconButton(
              tooltip: context.strings('hash.clear'),
              onPressed: onClear,
              icon: const Icon(Icons.close),
            ),
          ],
        ),
      ),
    );
  }
}

class _MessageCard extends StatelessWidget {
  const _MessageCard({required this.message, required this.isError});

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
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Icon(
              isError ? Icons.error_outline : Icons.info_outline,
              color: foreground,
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                message,
                style: Theme.of(context).textTheme.bodySmall
                    ?.copyWith(color: foreground),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
