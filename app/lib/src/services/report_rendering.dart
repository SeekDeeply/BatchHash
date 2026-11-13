import '../models/models.dart';

/// Renders a [ReportModel] to text for the demo service.
///
/// The real rendering lives in the Rust core (`batchhash-report`), and the fake
/// service must not become a second implementation of the CSV or JSON schema.
/// What this file does provide is a faithful **tree** rendering, because that
/// format has no machine contract to protect and a widget test has to be able
/// to look at real structure (indentation, tree lines, digests) rather than at
/// a returned path.
///
/// Every other format returns a short deterministic placeholder that names the
/// format, so a test can tell them apart without asserting on a schema this
/// layer does not own.
///
/// Keep the tree rules in sync with the core renderer:
/// two spaces per level, directories before files, both sorted by name, and the
/// Unicode prefixes `├─ `, `└─ `, `│  `, `   `, matching the core renderer.
String renderReport(
  ReportModel report,
  ReportFormat format, {
  String? locale,
}) {
  return switch (format) {
    ReportFormat.tree => renderReportTree(report, locale: locale),
    ReportFormat.json ||
    ReportFormat.csv ||
    ReportFormat.markdown ||
    ReportFormat.txt ||
    ReportFormat.md5sum ||
    ReportFormat.bsd => _placeholder(report, format),
  };
}

/// Render [report] as an indented file tree with one digest group per file.
String renderReportTree(ReportModel report, {String? locale}) {
  final _Labels labels = _Labels.forLocale(locale);
  final _TreeNode root = _TreeNode('');
  final String rootPrefix = report.root.replaceAll('\\', '/');
  for (final BatchEntry entry in report.entries) {
    final String relative = _relativePath(entry.path, rootPrefix);
    if (relative.isEmpty) {
      continue;
    }
    _insert(root, relative.split('/'), entry);
  }

  final StringBuffer buffer = StringBuffer();
  buffer.writeln(labels.title);
  _renderChildren(buffer, root, <bool>[], report.algorithms, labels);
  buffer.writeln();
  _renderSummary(buffer, report, labels);
  return buffer.toString();
}

/// Prefix used for a child, given whether it is the last one in its group.
///
/// Box drawing characters rather than ASCII: the file exists to be read by a
/// person, and the glyphs make the nesting obvious at a glance. The ASCII
/// variant was chosen originally to survive legacy code pages, which is not a
/// real constraint here: the report body, the paths and the UI all assume UTF-8
/// already. Keep in step with the core renderer in batchhash-report.
String _branch(bool isLast) => isLast ? '└─ ' : '├─ ';

/// Prefix a child's descendants repeat: a vertical bar only while more siblings
/// follow, otherwise blank spaces of the same width.
String _continuation(bool isLast) => isLast ? '   ' : '│  ';

void _renderChildren(
  StringBuffer buffer,
  _TreeNode node,
  List<bool> ancestorsAreLast,
  List<String> algorithms,
  _Labels labels,
) {
  final List<_TreeNode> children = node.sortedChildren;
  for (var index = 0; index < children.length; index += 1) {
    final _TreeNode child = children[index];
    final bool isLast = index == children.length - 1;
    final String indent = ancestorsAreLast.map(_continuation).join();
    buffer.write(indent);
    buffer.write(_branch(isLast));
    if (child.isDirectory) {
      buffer.writeln(child.name);
      _renderChildren(buffer, child, <bool>[
        ...ancestorsAreLast,
        isLast,
      ], algorithms, labels);
    } else {
      // A leaf always carries its entry: `childNamed` sets both together.
      final BatchEntry file = child.entry!;
      buffer.write(child.name);
      final String digests = _digestColumns(file, algorithms);
      if (digests.isNotEmpty) {
        buffer.write('  ');
        buffer.write(digests);
      }
      final String? note = _entryNote(file, labels);
      if (note != null) {
        buffer.write('  ');
        buffer.write(note);
      }
      buffer.writeln();
    }
  }
}

/// `algo:hash` columns in report order, two spaces apart.
///
/// The order comes from the report, never from the map, so two runs of the same
/// report always render identically.
String _digestColumns(BatchEntry entry, List<String> algorithms) {
  final List<String> parts = <String>[];
  for (final String algorithm in algorithms) {
    final String? hex = entry.hashes[algorithm];
    if (hex != null && hex.isNotEmpty) {
      parts.add('$algorithm:$hex');
    }
  }
  return parts.join('  ');
}

String? _entryNote(BatchEntry entry, _Labels labels) {
  return switch (entry.status) {
    EntryStatus.ok => null,
    EntryStatus.failed => labels.statusFailed,
    EntryStatus.skipped => labels.statusSkipped,
  };
}

void _renderSummary(StringBuffer buffer, ReportModel report, _Labels labels) {
  var ok = 0;
  var failed = 0;
  var skipped = 0;
  var bytes = 0;
  for (final BatchEntry entry in report.entries) {
    bytes += entry.size;
    switch (entry.status) {
      case EntryStatus.ok:
        ok += 1;
      case EntryStatus.failed:
        failed += 1;
      case EntryStatus.skipped:
        skipped += 1;
    }
  }
  buffer.writeln('${labels.summary}:');
  buffer.writeln('  ${labels.total}: ${report.entries.length}');
  buffer.writeln('  ${labels.ok}: $ok');
  buffer.writeln('  ${labels.failed}: $failed');
  buffer.writeln('  ${labels.skipped}: $skipped');
  buffer.writeln('  ${labels.totalBytes}: $bytes');
}

void _insert(_TreeNode root, List<String> segments, BatchEntry entry) {
  _TreeNode current = root;
  for (var index = 0; index < segments.length; index += 1) {
    final String segment = segments[index];
    if (segment.isEmpty) {
      continue;
    }
    final bool isLeaf = index == segments.length - 1;
    current = current.childNamed(segment, isDirectory: !isLeaf, entry: entry);
  }
}

/// Path of [path] below [root], or the unchanged path when it is not under it.
///
/// Mirrors the core helper: the report root may be empty (a checksum import) or
/// carry a trailing separator, so the root is normalized and trimmed instead of
/// being assumed to have one fixed shape.
String _relativePath(String path, String root) {
  final String normalized = path.replaceAll('\\', '/');
  if (root.isEmpty) {
    return normalized;
  }
  String base = root.replaceAll('\\', '/');
  while (base.endsWith('/') && base.length > 1) {
    base = base.substring(0, base.length - 1);
  }
  if (normalized == base) {
    return '';
  }
  if (!normalized.startsWith(base)) {
    return normalized;
  }
  final String rest = normalized.substring(base.length);
  return rest.replaceFirst(RegExp(r'^/+'), '');
}

String _placeholder(ReportModel report, ReportFormat format) {
  final String extension = format.fileExtension;
  return '# ${format.label} report for ${report.root} ($extension)\n';
}

/// One directory or file node in the tree being built.
class _TreeNode {
  _TreeNode(this.name);

  final String name;
  final Map<String, _TreeNode> _children = <String, _TreeNode>{};

  /// Set for leaves only; null for directories.
  BatchEntry? entry;
  bool isDirectory = true;

  /// Children sorted with directories first, then by name.
  List<_TreeNode> get sortedChildren {
    final List<_TreeNode> children = _children.values.toList();
    children.sort((_TreeNode left, _TreeNode right) {
      if (left.isDirectory != right.isDirectory) {
        return left.isDirectory ? -1 : 1;
      }
      return left.name.compareTo(right.name);
    });
    return children;
  }

  _TreeNode childNamed(
    String name, {
    required bool isDirectory,
    required BatchEntry entry,
  }) {
    final _TreeNode child = _children.putIfAbsent(
      name,
      () => _TreeNode(name),
    );
    if (isDirectory) {
      child.isDirectory = true;
    } else {
      // A leaf only demotes a node that has no children yet. When a report
      // lists both `a` and `a/b`, the subtree must survive: turning the node
      // into a file would drop every entry underneath it.
      if (child._children.isEmpty) {
        child.isDirectory = false;
      }
      child.entry = entry;
    }
    return child;
  }
}

/// Localized labels for the rendered text.
///
/// Only the tree format is rendered here, so only the labels it needs are
/// covered. The Rust core owns the full label set for the real exports.
class _Labels {
  const _Labels({
    required this.title,
    required this.summary,
    required this.total,
    required this.ok,
    required this.failed,
    required this.skipped,
    required this.totalBytes,
    required this.statusFailed,
    required this.statusSkipped,
  });

  final String title;
  final String summary;
  final String total;
  final String ok;
  final String failed;
  final String skipped;
  final String totalBytes;
  final String statusFailed;
  final String statusSkipped;

  static const _Labels _en = _Labels(
    title: 'BatchHash Report',
    summary: 'Summary',
    total: 'Total',
    ok: 'Ok',
    failed: 'Failed',
    skipped: 'Skipped',
    totalBytes: 'Total bytes',
    statusFailed: '(failed)',
    statusSkipped: '(skipped)',
  );

  static const _Labels _zh = _Labels(
    title: 'BatchHash 报告',
    summary: '汇总',
    total: '总数',
    ok: '正常',
    failed: '失败',
    skipped: '跳过',
    totalBytes: '总字节数',
    statusFailed: '（失败）',
    statusSkipped: '（跳过）',
  );

  /// Pick labels for [locale]; anything unrecognized stays English, which is
  /// what the core does when it cannot resolve a code.
  static _Labels forLocale(String? locale) {
    final String code = (locale ?? '').trim().toLowerCase();
    if (code == 'zh' || code.startsWith('zh-') || code.startsWith('zh_')) {
      return _zh;
    }
    return _en;
  }
}
