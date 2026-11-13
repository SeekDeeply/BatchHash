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
/// Symbols are resolved lazily and cached, so a missing symbol produces a clear
/// lookup error instead of a confusing null. Every call returns a JSON envelope
/// decoded here; the rest of the app only sees maps and typed errors. No Flutter
/// import is used on purpose: this file is unit-tested by `flutter test`
/// without a device.
class BatchHashNative {
  BatchHashNative._(this._library);

  final DynamicLibrary _library;
  final Map<String, _CallDart> _calls = <String, _CallDart>{};
  final Map<String, _NoArgDart> _noArgs = <String, _NoArgDart>{};
  _FreeDart? _freeCached;

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
  /// 2. iOS: the static library linked into the process,
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
    // Desktop bundles keep the library next to the executable, in a lib/
    // subdirectory (Linux) or in Contents/Frameworks (macOS .app); Android
    // resolves the plain file name from the APK native library directory.
    final exeDir = File(Platform.resolvedExecutable).parent.path;
    final candidates = <String>[
      '$exeDir/$name',
      '$exeDir/lib/$name',
      '$exeDir/../Frameworks/$name',
      '$exeDir/../Resources/$name',
      name,
    ];
    for (final candidate in candidates) {
      try {
        return BatchHashNative.open(candidate);
      } on BridgeUnavailable {
        continue;
      }
    }
    return null;
  }

  /// Bridge contract version, C ABI version, tool name and tool version.
  Map<String, Object?> version() => _callNoArgMap('bh_api_version');

  /// Registered algorithms in canonical order.
  List<Map<String, Object?>> algorithms() => _callNoArgList('bh_algorithms');

  /// Hash in-memory text without touching the filesystem.
  ///
  /// [dataBase64] carries the payload so arbitrary bytes survive the JSON
  /// request; the core hashes them in one pass exactly like a file.
  Map<String, Object?> hashText({
    required String dataBase64,
    required List<String> algorithms,
  }) {
    return _callMap('bh_hash_text', <String, Object?>{
      'data_base64': dataBase64,
      'algorithms': algorithms,
    });
  }

  /// Hardware capability report.
  Map<String, Object?> probe({bool forcedSoft = false}) {
    return _callMap('bh_probe', <String, Object?>{'forced_soft': forcedSoft});
  }

  /// Scan a directory (or a single file) without hashing.
  Map<String, Object?> scan(String root, {bool followLinks = false}) {
    return _callMap('bh_scan', <String, Object?>{
      'root': root,
      'follow_links': followLinks,
    });
  }

  /// Run the full pipeline: scan, hash, optional suffix write, report, log.
  ///
  /// [recursive] controls whether subdirectories are scanned; a folder the user
  /// picked as "only this folder" must pass false, or the batch silently grows
  /// to the whole tree. [restrictPaths] narrows an already running batch to the
  /// root-relative paths the user previewed, so executing a rename plan cannot
  /// touch files the plan never showed. Both are omitted from the request when
  /// they keep their default, so an older native library still answers.
  Map<String, Object?> runBatch({
    required String root,
    required List<String> algorithms,
    String? rootLabel,
    bool followLinks = false,
    bool recursive = true,
    List<String>? restrictPaths,
    bool? caseSensitive,
    Map<String, Object?>? suffix,
    bool applyRenames = false,
    String? logDb,
    bool overwrite = false,
  }) {
    return _callMap('bh_run_batch', <String, Object?>{
      'root': root,
      'algorithms': algorithms,
      if (rootLabel != null) 'root_label': rootLabel,
      'follow_links': followLinks,
      'recursive': recursive,
      if (restrictPaths != null && restrictPaths.isNotEmpty)
        'restrict_paths': restrictPaths,
      if (caseSensitive != null) 'case_sensitive': caseSensitive,
      if (suffix != null) 'suffix': suffix,
      'apply_renames': applyRenames,
      if (logDb != null) 'log_db': logDb,
      'overwrite': overwrite,
    });
  }

  /// Hash an explicit list of root-relative paths.
  Map<String, Object?> hashEntries({
    required String root,
    required List<String> paths,
    required List<String> algorithms,
    bool followLinks = false,
  }) {
    return _callMap('bh_hash_entries', <String, Object?>{
      'root': root,
      'paths': paths,
      'algorithms': algorithms,
      'follow_links': followLinks,
    });
  }


  /// Start a native job and return its id immediately.
  ///
  /// [action] is one of `batch`, `hash_entries` or `diff_directory`; [payload]
  /// carries that action's request fields. The work runs on a native thread, so
  /// the calling isolate stays responsive while a large file is hashed.
  String jobStart({required String action, required Map<String, Object?> payload}) {
    final data = _callMap('bh_job_start', <String, Object?>{
      'action': action,
      'payload': payload,
    });
    final id = data['job_id'] as String?;
    if (id == null || id.isEmpty) {
      throw BridgeException('internal', 'the native library returned no job id');
    }
    return id;
  }

  /// Poll a job. Call repeatedly until `state` is terminal.
  Map<String, Object?> jobPoll(String jobId) {
    return _callMap('bh_job_poll', <String, Object?>{'job_id': jobId});
  }

  /// Ask a job to stop. The job reports its terminal state through [jobPoll].
  Map<String, Object?> jobCancel(String jobId) {
    return _callMap('bh_job_cancel', <String, Object?>{'job_id': jobId});
  }

  /// Release a job and its payload. Swallows an unknown-id error so cleanup is
  /// safe to call twice.
  void jobRelease(String jobId) {
    try {
      _callMap('bh_job_release', <String, Object?>{'job_id': jobId});
    } on BridgeException {
      // Releasing an already released job is not an error worth surfacing.
    }
  }

  /// Whether the loaded library supports asynchronous jobs.
  ///
  /// The envelope shape did not change when jobs were added, so this is
  /// advertised as a capability instead of being inferred from the version.
  bool supportsAsyncJobs() {
    final version = this.version();
    final features = version['features'];
    return features is List<Object?> && features.contains('async_jobs');
  }
  /// Plan suffix renames from entries that already carry their hashes.
  Map<String, Object?> planRenames({
    required List<Map<String, Object?>> entries,
    required String algorithm,
    required int length,
    String? root,
    bool uppercase = true,
    bool prefix = false,
    bool overwrite = false,
  }) {
    return _callMap('bh_plan_renames', <String, Object?>{
      if (root != null) 'root': root,
      'entries': entries,
      'algorithm': algorithm,
      'length': length,
      'case': uppercase ? 'upper' : 'lower',
      'prefix': prefix,
      'overwrite': overwrite,
    });
  }

  /// Plan or execute an undo for one batch.
  Map<String, Object?> undo({
    required String db,
    String? batchId,
    bool apply = false,
  }) {
    return _callMap('bh_undo', <String, Object?>{
      'db': db,
      if (batchId != null) 'batch_id': batchId,
      'apply': apply,
    });
  }

  /// Import a report file and return its canonical JSON.
  String importReport(String path) {
    final data = _callMap('bh_import_report', <String, Object?>{'path': path});
    return data['report_json'] as String? ?? '';
  }

  /// Build canonical report JSON from entries the UI already holds.
  String buildReport({
    required String root,
    required List<String> algorithms,
    required List<Map<String, Object?>> entries,
    bool? caseSensitive,
    String? generatedAt,
  }) {
    final data = _callMap('bh_build_report', <String, Object?>{
      'root': root,
      'algorithms': algorithms,
      'entries': entries,
      if (caseSensitive != null) 'case_sensitive': caseSensitive,
      if (generatedAt != null) 'generated_at': generatedAt,
    });
    return data['report_json'] as String? ?? '';
  }

  /// Compare two report JSON documents.
  Map<String, Object?> diffReports({
    required String leftJson,
    required String rightJson,
    bool? caseSensitive,
  }) {
    return _callMap('bh_diff_reports', <String, Object?>{
      'left_json': leftJson,
      'right_json': rightJson,
      if (caseSensitive != null) 'case_sensitive': caseSensitive,
    });
  }

  /// Compare a report JSON document against a live directory scan.
  Map<String, Object?> diffDirectory({
    required String reportJson,
    required String directory,
    bool? caseSensitive,
    bool followLinks = false,
    bool? recursive,
    List<String> restrictPaths = const <String>[],
    int? maxEntries,
  }) {
    return _callMap('bh_diff_directory', <String, Object?>{
      'report_json': reportJson,
      'directory': directory,
      if (caseSensitive != null) 'case_sensitive': caseSensitive,
      'follow_links': followLinks,
      // Sent explicitly so an older default can never silently widen the
      // comparison to a whole tree.
      if (recursive != null) 'recursive': recursive,
      if (restrictPaths.isNotEmpty) 'restrict_paths': restrictPaths,
      if (maxEntries != null) 'max_entries': maxEntries,
    });
  }

  /// List the batches stored in a rename log, newest first.
  List<Map<String, Object?>> listBatches(String db) =>
      _callList('bh_list_batches', <String, Object?>{'db': db});

  /// Re-export a report JSON document in another format.
  ///
  /// [locale] picks the language of the human readable formats. The field is
  /// only sent when set, so an older native library that does not know it keeps
  /// working and simply renders English.
  String exportReport({
    required String reportJson,
    required String format,
    String? locale,
  }) {
    final data = _callMap('bh_export_report', <String, Object?>{
      'report_json': reportJson,
      'format': format,
      if (locale != null && locale.isNotEmpty) 'locale': locale,
    });
    return data['text'] as String? ?? '';
  }

  Map<String, Object?> _callMap(String name, Map<String, Object?> request) {
    return _asMap(_call(name, request));
  }

  List<Map<String, Object?>> _callList(
    String name,
    Map<String, Object?> request,
  ) {
    return _asList(_call(name, request));
  }

  Map<String, Object?> _callNoArgMap(String name) {
    return _asMap(_decodePayload(_noArg(name)()));
  }

  List<Map<String, Object?>> _callNoArgList(String name) {
    return _asList(_decodePayload(_noArg(name)()));
  }

  Object? _call(String name, Map<String, Object?> request) {
    final encoded = jsonEncode(request).toNativeUtf8();
    try {
      return _decodePayload(_callFn(name)(encoded));
    } finally {
      calloc.free(encoded);
    }
  }

  _CallDart _callFn(String name) => _calls.putIfAbsent(
    name,
    () => _library.lookupFunction<_CallNative, _CallDart>(name),
  );

  _NoArgDart _noArg(String name) => _noArgs.putIfAbsent(
    name,
    () => _library.lookupFunction<_NoArgNative, _NoArgDart>(name),
  );

  _FreeDart get _free => _freeCached ??= _library
      .lookupFunction<_FreeNative, _FreeDart>('bh_free');

  Object? _decodePayload(Pointer<Utf8> pointer) {
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

  Map<String, Object?> _asMap(Object? data) {
    if (data is Map<String, Object?>) {
      return data;
    }
    throw BridgeException('internal', 'expected a JSON object from the bridge');
  }

  List<Map<String, Object?>> _asList(Object? data) {
    if (data is List<Object?>) {
      return data.cast<Map<String, Object?>>();
    }
    throw BridgeException('internal', 'expected a JSON array from the bridge');
  }
}
