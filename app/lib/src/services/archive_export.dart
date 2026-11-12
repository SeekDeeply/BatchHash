import 'dart:typed_data';

import 'package:archive/archive.dart';
import 'package:crypto/crypto.dart';

/// Hex SHA-256 of [bytes].
String sha256Hex(List<int> bytes) => sha256.convert(bytes).toString();

/// Pack [files] into a ZIP archive.
///
/// A non-empty [password] produces a WinZip AES archive (AE-1/AE-2), which the
/// Windows, macOS and common Linux archive tools all recognise and prompt for,
/// rather than the legacy ZipCrypto scheme that newer tools refuse.
Uint8List zipBytes(Map<String, List<int>> files, {String? password}) {
  final Archive archive = Archive();
  for (final MapEntry<String, List<int>> entry in files.entries) {
    final List<int> data = entry.value;
    archive.addFile(ArchiveFile(entry.key, data.length, data));
  }
  final String? trimmed = password == null || password.isEmpty
      ? null
      : password;
  final List<int> encoded = ZipEncoder(password: trimmed).encode(archive);
  return Uint8List.fromList(encoded);
}

/// Build an export file name with an optional digest suffix.
///
/// [digest] is the SHA-256 of the file actually being written, and only its
/// first 16 hex characters go into the name: enough to spot a mismatch by eye,
/// short enough to keep the name usable on every platform.
String exportFileName(
  String base, {
  String? digest,
  required String extension,
}) {
  final String suffix = digest == null || digest.isEmpty
      ? ''
      : '_${digest.substring(0, digest.length < 16 ? digest.length : 16)}';
  return '$base$suffix$extension';
}

/// Local-time stamp for export names, sortable and free of separators a file
/// system might dislike.
String exportTimestamp([DateTime? at]) {
  final DateTime now = at ?? DateTime.now();
  String two(int value) => value.toString().padLeft(2, '0');
  return '${now.year}${two(now.month)}${two(now.day)}'
      '-${two(now.hour)}${two(now.minute)}${two(now.second)}';
}
