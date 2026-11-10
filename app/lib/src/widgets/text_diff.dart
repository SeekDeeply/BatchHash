import 'package:flutter/material.dart';


/// Result of comparing two line sequences.
enum TextDiffKind { same, added, removed, changed }

/// One aligned row in a side-by-side text comparison.
class TextDiffRow {
  const TextDiffRow({
    required this.kind,
    this.left,
    this.right,
    this.leftLine,
    this.rightLine,
  });

  final TextDiffKind kind;
  final String? left;
  final String? right;
  final int? leftLine;
  final int? rightLine;
}

/// Compare two texts line by line.
///
/// This is a longest-common-subsequence walk, so a line inserted near the top
/// does not cascade into every later row being reported as changed: unchanged
/// lines stay aligned, and only genuinely inserted/deleted lines are marked.
/// Pairs of differing lines that sit at the same point in the sequence are
/// reported as [TextDiffKind.changed] so the UI can show them side by side.
List<TextDiffRow> diffLines(String left, String right) {
  final List<String> a = _splitLines(left);
  final List<String> b = _splitLines(right);
  final List<List<int>> lcs = List<List<int>>.generate(
    a.length + 1,
    (int _) => List<int>.filled(b.length + 1, 0),
  );
  for (int i = a.length - 1; i >= 0; i--) {
    for (int j = b.length - 1; j >= 0; j--) {
      lcs[i][j] = a[i] == b[j]
          ? lcs[i + 1][j + 1] + 1
          : (lcs[i + 1][j] >= lcs[i][j + 1] ? lcs[i + 1][j] : lcs[i][j + 1]);
    }
  }

  final List<TextDiffRow> rows = <TextDiffRow>[];
  int i = 0;
  int j = 0;
  // Collected runs of deletions/additions between two anchors. A run that has
  // rows on both sides is a block of edits, which reads better as changed pairs
  // than as every line deleted and then every line added.
  final List<int> pendingRemoved = <int>[];
  final List<int> pendingAdded = <int>[];

  void flushPending() {
    final int paired = pendingRemoved.length < pendingAdded.length
        ? pendingRemoved.length
        : pendingAdded.length;
    for (int k = 0; k < paired; k++) {
      rows.add(
        TextDiffRow(
          kind: TextDiffKind.changed,
          left: a[pendingRemoved[k]],
          right: b[pendingAdded[k]],
          leftLine: pendingRemoved[k] + 1,
          rightLine: pendingAdded[k] + 1,
        ),
      );
    }
    for (int k = paired; k < pendingRemoved.length; k++) {
      rows.add(
        TextDiffRow(
          kind: TextDiffKind.removed,
          left: a[pendingRemoved[k]],
          leftLine: pendingRemoved[k] + 1,
        ),
      );
    }
    for (int k = paired; k < pendingAdded.length; k++) {
      rows.add(
        TextDiffRow(
          kind: TextDiffKind.added,
          right: b[pendingAdded[k]],
          rightLine: pendingAdded[k] + 1,
        ),
      );
    }
    pendingRemoved.clear();
    pendingAdded.clear();
  }

  while (i < a.length && j < b.length) {
    if (a[i] == b[j]) {
      flushPending();
      rows.add(
        TextDiffRow(
          kind: TextDiffKind.same,
          left: a[i],
          right: b[j],
          leftLine: i + 1,
          rightLine: j + 1,
        ),
      );
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      pendingRemoved.add(i);
      i++;
    } else {
      pendingAdded.add(j);
      j++;
    }
  }
  while (i < a.length) {
    pendingRemoved.add(i);
    i++;
  }
  while (j < b.length) {
    pendingAdded.add(j);
    j++;
  }
  flushPending();
  return rows;
}

List<String> _splitLines(String text) {
  if (text.isEmpty) {
    return const <String>[];
  }
  return text.replaceAll('\r\n', '\n').replaceAll('\r', '\n').split('\n');
}

/// Counts per [TextDiffKind], for the summary line above the table.
class TextDiffSummary {
  const TextDiffSummary({
    required this.added,
    required this.removed,
    required this.changed,
    required this.same,
  });

  factory TextDiffSummary.of(List<TextDiffRow> rows) {
    int added = 0;
    int removed = 0;
    int changed = 0;
    int same = 0;
    for (final TextDiffRow row in rows) {
      switch (row.kind) {
        case TextDiffKind.added:
          added += 1;
        case TextDiffKind.removed:
          removed += 1;
        case TextDiffKind.changed:
          changed += 1;
        case TextDiffKind.same:
          same += 1;
      }
    }
    return TextDiffSummary(
      added: added,
      removed: removed,
      changed: changed,
      same: same,
    );
  }

  final int added;
  final int removed;
  final int changed;
  final int same;

  bool get isIdentical => added == 0 && removed == 0 && changed == 0;
}