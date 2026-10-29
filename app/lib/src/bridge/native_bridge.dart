import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

/// Error raised by the native bridge.
class BridgeException implements Exception {
  BridgeException(this.kind, this.message);

  final String kind;
  final String message;

  @override
  String toString() => 'BridgeException($kind): $message';
}

/// Thrown when the native library cannot be located.
class BridgeUnavailable implements Exception {
  BridgeUnavailable(this.message);

  final String message;

  @override
  String toString() => 'BridgeUnavailable: $message';
}

typedef _CallNative = Pointer<Utf8> Function(Pointer<Utf8>);
typedef _CallDart = Pointer<Utf8> Function(Pointer<Utf8>);
typedef _NoArgNative = Pointer<Utf8> Function();
typedef _NoArgDart = Pointer<Utf8> Function();
typedef _FreeNative = Void Function(Pointer<Utf8>);
typedef _FreeDart = void Function(Pointer<Utf8>);

/// Binding over the Rust C ABI (see crates/batchhash-ffi/src/c_api.rs).
///
/// Every call returns a JSON envelope decoded here, so the rest of the app only
/// deals with maps and typed errors. No Flutter dependency is imported on
/// purpose: this file is unit-tested by `flutter test` without a device.
class BatchHashNative {
  BatchHashNative._(DynamicLibrary library)
    : _algorithms = library.lookupFunction<_NoArgNative, _NoArgDart>(
        'bh_algorithms',
      ),
      _apiVersion = library.lookupFunction<_NoArgNative, _NoArgDart>(
        'bh_api_version',
      ),
      _probe = library.lookupFunction<_CallNative, _CallDart>('bh_probe'),
      _scan = library.lookupFunction<_CallNative, _CallDart>('bh_scan'),
      _runBatch = library.lookupFunction<_CallNative, _CallDart>(
        'bh_run_batch',
      ),
      _undo = library.lookupFunction<_CallNative, _CallDart>('bh_undo'),
      _export = library.lookupFunction<_CallNative, _CallDart>(
        'bh_export_report',
      ),
      _free = library.lookupFunction<_FreeNative, _FreeDart>('bh_free');

  final _NoArgDart _algorithms;
  final _NoArgDart _apiVersion;
  final _CallDart _probe;
  final _CallDart _scan;
  final _CallDart _runBatch;
  final _CallDart _undo;
  final _CallDart _export;
  final _FreeDart _free;

  /// Open the library from an explicit filesystem path.
  factory BatchHashNative.open(String path) {
    try {
      return BatchHashNative._(DynamicLibrary.open(path));
    } on ArgumentError catch (error) {
      throw BridgeUnavailable('cannot load $path: $error');
    }
  }

  /// Open the library the way the packaged app does.
  ///
  /// The lookup order is:
  /// 1. `BATCHHASH_FFI_LIB` (used by CI and tests),
  /// 2. iOS/macOS static library linked into the process,
  /// 3. the platform dynamic library next to the executable.
  static BatchHashNative? tryOpen({
    Map<String, String>? environment,
    String? overridePath,
  }) {
    final env = environment ?? Platform.environment;
    final explicit = overridePath ?? env['BATCHHASH_FFI_LIB'];
    if (explicit != null && explicit.isNotEmpty) {
      try {
        return BatchHashNative.open(explicit);
      } on BridgeUnavailable {
        return null;
      }
    }
    if (Platform.isIOS) {
      try {
        return BatchHashNative._(DynamicLibrary.process());
      } on ArgumentError {
        return null;
      }
    }
    final name = switch (Platform.operatingSystem) {
      'windows' => 'batchhash_ffi.dll',
      'macos' => 'libbatchhash_ffi.dylib',
      _ => 'libbatchhash_ffi.so',
    };
    try {
      return BatchHashNative.open(name);
    } on BridgeUnavailable {
      return null;
    }
  }

  /// Bridge contract version, C ABI version, tool name and tool version.
  Map<String, Object?> version() => _decodeMap(_apiVersion());

  /// Registered algorithms in canonical order.
  List<Map<String, Object?>> algorithms() => _decodeList(_algorithms());

  /// Hardware capability report.
  Map<String, Object?> probe({bool forcedSoft = false}) {
    return _decodeMap(
      _callJson(_probe, <String, Object?>{'forced_soft': forcedSoft}),
    );
  }

  /// Scan a directory (or a single file) without hashing.
  Map<String, Object?> scan(String root, {bool followLinks = false}) {
    return _decodeMap(
      _callJson(_scan, <String, Object?>{
        'root': root,
        'follow_links': followLinks,
      }),
    );
  }

  /// Run the full pipeline: scan, hash, optional suffix write, report, log.
  Map<String, Object?> runBatch({
    required String root,
    required List<String> algorithms,
    String? rootLabel,
    bool followLinks = false,
    bool? caseSensitive,
    Map<String, Object?>? suffix,
    bool applyRenames = false,
    String? logDb,
    bool overwrite = false,
  }) {
    return _decodeMap(
      _callJson(_runBatch, <String, Object?>{
        'root': root,
        'algorithms': algorithms,
        if (rootLabel != null) 'root_label': rootLabel,
        'follow_links': followLinks,
        if (caseSensitive != null) 'case_sensitive': caseSensitive,
        if (suffix != null) 'suffix': suffix,
        'apply_renames': applyRenames,
        if (logDb != null) 'log_db': logDb,
        'overwrite': overwrite,
      }),
    );
  }

  /// Plan or execute an undo for one batch.
  Map<String, Object?> undo({
    required String db,
    String? batchId,
    bool apply = false,
  }) {
    return _decodeMap(
      _callJson(_undo, <String, Object?>{
        'db': db,
        if (batchId != null) 'batch_id': batchId,
        'apply': apply,
      }),
    );
  }

  /// Re-export a report JSON document in another format.
  String exportReport({
    required String reportJson,
    required String format,
  }) {
    final data = _decodeMap(
      _callJson(_export, <String, Object?>{
        'report_json': reportJson,
        'format': format,
      }),
    );
    return data['text'] as String? ?? '';
  }

  Pointer<Utf8> _callJson(_CallDart function, Map<String, Object?> request) {
    final encoded = jsonEncode(request).toNativeUtf8();
    try {
      return function(encoded);
    } finally {
      calloc.free(encoded);
    }
  }

  Object? _decode(Pointer<Utf8> pointer) {
    if (pointer == nullptr) {
      throw BridgeException('internal', 'the native library returned null');
    }
    final text = pointer.toDartString();
    _free(pointer);
    final decoded = jsonDecode(text);
    if (decoded is! Map<String, Object?>) {
      throw BridgeException('internal', 'malformed bridge envelope');
    }
    if (decoded['ok'] != true) {
      throw BridgeException(
        decoded['kind'] as String? ?? 'internal',
        decoded['error'] as String? ?? 'unknown native error',
      );
    }
    return decoded['data'];
  }

  Map<String, Object?> _decodeMap(Pointer<Utf8> pointer) {
    final data = _decode(pointer);
    if (data is Map<String, Object?>) {
      return data;
    }
    throw BridgeException('internal', 'expected a JSON object from the bridge');
  }

  List<Map<String, Object?>> _decodeList(Pointer<Utf8> pointer) {
    final data = _decode(pointer);
    if (data is List<Object?>) {
      return data.cast<Map<String, Object?>>();
    }
    throw BridgeException('internal', 'expected a JSON array from the bridge');
  }
}
