import 'dart:convert';

import '../models/operation_log.dart';
import '../util/formatting.dart';

/// Serialise log entries as pretty-printed JSON.
///
/// The envelope carries a schema tag and an export timestamp so a file found
/// later can be identified without guessing what produced it.
String encodeLogAsJson(List<OperationLogEntry> entries, {DateTime? at}) {
  final DateTime exportedAt = at ?? DateTime.now();
  final Map<String, Object?> payload = <String, Object?>{
    'schema': 'batchhash.operation-log.v1',
    'exported_at': exportedAt.toIso8601String(),
    'count': entries.length,
    'entries': <Object?>[
      for (final OperationLogEntry entry in entries) entry.toJson(),
    ],
  };
  return const JsonEncoder.withIndent('  ').convert(payload);
}

/// Serialise log entries as Markdown, one block per entry.
///
/// Digests are included because they are the verifiable payload of a hash or
/// verification entry; without them the export cannot be used to check anything.
String encodeLogAsMarkdown(List<OperationLogEntry> entries, {DateTime? at}) {
  final DateTime exportedAt = at ?? DateTime.now();
  final StringBuffer out = StringBuffer();
  out.writeln('# Operation log');
  out.writeln();
  out.writeln('- Exported: ${exportedAt.toIso8601String()}');
  out.writeln('- Entries: ${entries.length}');
  out.writeln();
  for (final OperationLogEntry entry in entries) {
    out.writeln('## ${entry.kind}');
    out.writeln();
    out.writeln('- Time: ${formatTimestamp(entry.startedAt)}');
    out.writeln('- Result: ${entry.status ? 'succeeded' : 'failed'}');
    if (entry.durationMs != null) {
      out.writeln('- Duration: ${entry.durationMs} ms');
    }
    if (entry.detail != null && entry.detail!.isNotEmpty) {
      out.writeln('- Detail: ${entry.detail}');
    }
    if (entry.hashes.isNotEmpty) {
      out.writeln();
      out.writeln('| Algorithm | Path | Digest |');
      out.writeln('| --- | --- | --- |');
      for (final HashDigestRecord record in entry.hashes) {
        out.writeln('| ${record.algorithm} | ${record.path} | ${record.digest} |');
      }
      if (entry.hashesTruncated > 0) {
        out.writeln();
        out.writeln(
          '(${entry.hashesTruncated} more digests were not recorded.)',
        );
      }
    }
    out.writeln();
  }
  return out.toString();
}
