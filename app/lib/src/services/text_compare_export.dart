import 'dart:convert';

/// One finished text comparison, in the shape the exporters need.
///
/// A plain value object rather than the panel's `State`: exporting must not
/// depend on widgets, which also makes the three encoders testable without
/// pumping a UI.
class TextCompareExportData {
  const TextCompareExportData({
    required this.leftText,
    required this.rightText,
    required this.leftHashes,
    required this.rightHashes,
    required this.identical,
    required this.differingLines,
    required this.totalLines,
    this.exportedAt,
  });

  /// Pasted text of the left pane.
  final String leftText;

  /// Pasted text of the right pane.
  final String rightText;

  /// Digests of the left pane, in the order they were computed.
  final List<({String algorithm, String digest})> leftHashes;

  /// Digests of the right pane.
  final List<({String algorithm, String digest})> rightHashes;

  /// Whether the two panes are byte-identical.
  final bool identical;

  /// How many rows the comparison reported as changed.
  final int differingLines;

  /// Number of rows compared.
  final int totalLines;

  /// When the export was produced; defaults to now.
  final DateTime? exportedAt;

  DateTime get timestamp => exportedAt ?? DateTime.now();

  /// Whether both panes are empty, in which case there is nothing to export.
  bool get isEmpty => leftText.isEmpty && rightText.isEmpty;

  Map<String, Object?> toJson() {
    return <String, Object?>{
      'schema': 'batchhash.text-compare.v1',
      'exported_at': timestamp.toIso8601String(),
      'identical': identical,
      'differing_lines': differingLines,
      'total_lines': totalLines,
      'left': <String, Object?>{
        'length': leftText.length,
        'hashes': <String, String>{
          for (final ({String algorithm, String digest}) hash in leftHashes)
            hash.algorithm: hash.digest,
        },
        'text': leftText,
      },
      'right': <String, Object?>{
        'length': rightText.length,
        'hashes': <String, String>{
          for (final ({String algorithm, String digest}) hash in rightHashes)
            hash.algorithm: hash.digest,
        },
        'text': rightText,
      },
    };
  }
}

/// Serialise a comparison as pretty-printed JSON.
String encodeTextCompareAsJson(TextCompareExportData data) {
  return const JsonEncoder.withIndent('  ').convert(data.toJson());
}

/// Serialise a comparison as Markdown.
///
/// The texts are fenced, but a fence inside the text would end the block early,
/// so the fence length is chosen to be longer than any run of backticks in the
/// payload. Otherwise pasting Markdown into the panel silently corrupts the
/// export.
String encodeTextCompareAsMarkdown(TextCompareExportData data) {
  final StringBuffer out = StringBuffer();
  out.writeln('# Text comparison');
  out.writeln();
  out.writeln('- Exported: ${data.timestamp.toIso8601String()}');
  out.writeln('- Result: ${data.identical ? 'identical' : 'different'}');
  out.writeln(
    '- Lines: ${data.totalLines} compared, ${data.differingLines} differing',
  );
  out.writeln();
  for (final String side in <String>['left', 'right']) {
    final String text = side == 'left' ? data.leftText : data.rightText;
    final List<({String algorithm, String digest})> hashes =
        side == 'left' ? data.leftHashes : data.rightHashes;
    out.writeln('## ${side == 'left' ? 'Left' : 'Right'}');
    out.writeln();
    if (hashes.isEmpty) {
      out.writeln('_No digests computed._');
    } else {
      for (final ({String algorithm, String digest}) hash in hashes) {
        out.writeln('- `${hash.algorithm}`: `${hash.digest}`');
      }
    }
    out.writeln();
    final String fence = _fenceFor(text);
    out.writeln(fence);
    out.write(text);
    if (!text.endsWith('\n')) {
      out.writeln();
    }
    out.writeln(fence);
    out.writeln();
  }
  return out.toString();
}

/// Pick a backtick fence longer than any run inside [text].
String _fenceFor(String text) {
  int longest = 0;
  for (final RegExpMatch match in RegExp(r'`+').allMatches(text)) {
    if (match.group(0)!.length > longest) {
      longest = match.group(0)!.length;
    }
  }
  return '`' * (longest < 3 ? 3 : longest + 1);
}

/// Serialise a comparison as plain text.
String encodeTextCompareAsTxt(TextCompareExportData data) {
  final StringBuffer out = StringBuffer();
  out.writeln('Text comparison');
  out.writeln('Exported: ${data.timestamp.toIso8601String()}');
  out.writeln('Result: ${data.identical ? 'identical' : 'different'}');
  out.writeln(
    'Lines: ${data.totalLines} compared, ${data.differingLines} differing',
  );
  out.writeln();
  for (final ({String label, String text, List<({String algorithm, String digest})> hashes})
      side in <({
        String label,
        String text,
        List<({String algorithm, String digest})> hashes
      })>[
    (label: 'LEFT', text: data.leftText, hashes: data.leftHashes),
    (label: 'RIGHT', text: data.rightText, hashes: data.rightHashes),
  ]) {
    out.writeln('--- $side.label ---');
    if (side.hashes.isEmpty) {
      out.writeln('(no digests)');
    } else {
      for (final ({String algorithm, String digest}) hash in side.hashes) {
        out.writeln('${hash.algorithm}: ${hash.digest}');
      }
    }
    out.writeln();
    out.write(side.text);
    if (!side.text.endsWith('\n')) {
      out.writeln();
    }
    out.writeln();
  }
  return out.toString();
}
