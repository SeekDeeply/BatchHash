import 'operation_log.dart';

/// Pure filtering and sorting for the operation log.
///
/// Kept out of the page so the rules can be unit tested without a widget
/// harness, and so a future CLI can reuse them. The search matches the raw
/// [OperationLogEntry.kind] key, the detail text and the recorded digest
/// paths/digests; callers that have a locale can pass [kindLabel] so a user
/// can also search the label actually shown on screen.
List<OperationLogEntry> filterLogEntries(
  List<OperationLogEntry> entries, {
  String query = '',
  String? kind,
  bool? ok,
  bool newestFirst = true,
  String Function(String kind)? kindLabel,
}) {
  final String needle = query.trim().toLowerCase();

  bool matches(OperationLogEntry entry) {
    if (kind != null && entry.kind != kind) {
      return false;
    }
    if (ok != null && entry.status != ok) {
      return false;
    }
    if (needle.isEmpty) {
      return true;
    }
    if (entry.kind.toLowerCase().contains(needle)) {
      return true;
    }
    if (kindLabel != null &&
        kindLabel(entry.kind).toLowerCase().contains(needle)) {
      return true;
    }
    final String? detail = entry.detail;
    if (detail != null && detail.toLowerCase().contains(needle)) {
      return true;
    }
    for (final HashDigestRecord record in entry.hashes) {
      if (record.path.toLowerCase().contains(needle) ||
          record.digest.toLowerCase().contains(needle)) {
        return true;
      }
    }
    return false;
  }

  // The index is carried along so equal timestamps can fall back to the
  // original order: the sort would otherwise be free to shuffle rows that the
  // user perceives as "the same moment".
  final List<MapEntry<int, OperationLogEntry>> selected =
      <MapEntry<int, OperationLogEntry>>[
        for (int i = 0; i < entries.length; i++)
          if (matches(entries[i])) MapEntry<int, OperationLogEntry>(i, entries[i]),
      ];
  selected.sort((
    MapEntry<int, OperationLogEntry> a,
    MapEntry<int, OperationLogEntry> b,
  ) {
    final int byTime = a.value.startedAt.compareTo(b.value.startedAt);
    if (byTime != 0) {
      return newestFirst ? -byTime : byTime;
    }
    return a.key.compareTo(b.key);
  });
  return <OperationLogEntry>[
    for (final MapEntry<int, OperationLogEntry> item in selected) item.value,
  ];
}
