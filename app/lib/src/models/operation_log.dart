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

  Map<String, Object?> toJson() => <String, Object?>{
    'kind': kind,
    'ok': status,
    'at': startedAt.toUtc().toIso8601String(),
    if (detail != null) 'detail': detail,
    if (durationMs != null) 'duration_ms': durationMs,
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
    return OperationLogEntry(
      kind: kind,
      status: raw['ok'] == true,
      startedAt: startedAt.toLocal(),
      detail: detail is String && detail.isNotEmpty ? detail : null,
      durationMs: duration is int ? duration : null,
    );
  }
}
