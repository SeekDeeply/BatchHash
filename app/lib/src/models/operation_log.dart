import 'models.dart';

/// One digest captured by an operation.
///
/// History records digests because they are the actual result of the work: a
/// row that only says "hashed 12 files" cannot be used to verify anything later.
/// The path is root-relative when the operation had a root, otherwise a label.
class HashDigestRecord {
  const HashDigestRecord({
    required this.path,
    required this.algorithm,
    required this.digest,
  });

  /// File path or label the digest belongs to.
  final String path;

  /// Stable algorithm id, for example `sha256`.
  final String algorithm;

  /// Lowercase hex digest.
  final String digest;

  Map<String, Object?> toJson() => <String, Object?>{
    'path': path,
    'algorithm': algorithm,
    'digest': digest,
  };

  /// Decode one stored record, skipping anything malformed.
  static HashDigestRecord? fromJson(Object? raw) {
    if (raw is! Map<String, Object?>) {
      return null;
    }
    final Object? path = raw['path'];
    final Object? algorithm = raw['algorithm'];
    final Object? digest = raw['digest'];
    if (path is! String || algorithm is! String || digest is! String) {
      return null;
    }
    if (digest.isEmpty) {
      return null;
    }
    return HashDigestRecord(path: path, algorithm: algorithm, digest: digest);
  }
}
/// One recorded operation, shown in the history view.
///
/// Recording happens in a service decorator so every page gets history for free
/// and no call site can forget to log.
class OperationLogEntry {
  const OperationLogEntry({
    required this.kind,
    required this.status,
    required this.startedAt,
    this.detail,
    this.durationMs,
    this.hashes = const <HashDigestRecord>[],
    this.hashesTruncated = 0,
  });

  /// Translation key describing the operation, for example `op.scan`.
  final String kind;

  /// Whether the operation finished without throwing.
  final bool status;

  /// When the operation started, in local time.
  final DateTime startedAt;

  /// Short human readable summary: a path, a count, an error message.
  final String? detail;

  /// Wall clock duration, when the operation finished.
  final int? durationMs;

  /// Digests the operation produced, in the order they were computed.
  final List<HashDigestRecord> hashes;

  /// How many digests were left out of [hashes] because of the per-entry cap.
  ///
  /// A batch can hash thousands of files; keeping only a bounded sample keeps
  /// the history file small, and reporting the remainder keeps the record
  /// honest instead of silently implying the list is complete.
  final int hashesTruncated;

  Map<String, Object?> toJson() => <String, Object?>{
    'kind': kind,
    'ok': status,
    'at': startedAt.toUtc().toIso8601String(),
    if (detail != null) 'detail': detail,
    if (durationMs != null) 'duration_ms': durationMs,
    if (hashes.isNotEmpty)
      'hashes': <Object?>[
        for (final HashDigestRecord record in hashes) record.toJson(),
      ],
    if (hashesTruncated > 0) 'hashes_truncated': hashesTruncated,
  };

  /// Decode one stored entry, skipping anything malformed.
  static OperationLogEntry? fromJson(Object? raw) {
    if (raw is! Map<String, Object?>) {
      return null;
    }
    final Object? kind = raw['kind'];
    final Object? at = raw['at'];
    if (kind is! String || at is! String) {
      return null;
    }
    final DateTime? startedAt = DateTime.tryParse(at);
    if (startedAt == null) {
      return null;
    }
    final Object? detail = raw['detail'];
    final Object? duration = raw['duration_ms'];
    final Object? hashes = raw['hashes'];
    final Object? truncated = raw['hashes_truncated'];
    return OperationLogEntry(
      kind: kind,
      status: raw['ok'] == true,
      startedAt: startedAt.toLocal(),
      detail: detail is String && detail.isNotEmpty ? detail : null,
      durationMs: duration is int ? duration : null,
      hashes: hashes is List<Object?>
          ? <HashDigestRecord>[
              for (final Object? row in hashes)
                if (HashDigestRecord.fromJson(row)
                    case final HashDigestRecord record)
                  record,
            ]
          : const <HashDigestRecord>[],
      hashesTruncated: truncated is int ? truncated : 0,
    );
  }
}


/// Maximum digests kept per history entry.
///
/// A batch can cover thousands of files, and history is read by eye; keeping a
/// bounded sample plus the withheld count keeps the file small without
/// pretending the list is complete.
const int kMaxRecordedDigests = 200;

/// Flatten computed entries into digest records, capped at [kMaxRecordedDigests].
///
/// Returns the records plus how many were withheld, so a caller can say so.
({List<HashDigestRecord> records, int truncated}) collectDigests(
  List<BatchEntry> entries, {
  int limit = kMaxRecordedDigests,
}) {
  final List<HashDigestRecord> records = <HashDigestRecord>[];
  int withheld = 0;
  for (final BatchEntry entry in entries) {
    for (final MapEntry<String, String> hash in entry.hashes.entries) {
      if (hash.value.isEmpty) {
        continue;
      }
      if (records.length >= limit) {
        withheld += 1;
        continue;
      }
      records.add(
        HashDigestRecord(
          path: entry.path,
          algorithm: hash.key,
          digest: hash.value,
        ),
      );
    }
  }
  return (records: records, truncated: withheld);
}
