import 'dart:math' as math;

/// Grouping an algorithm belongs to, as reported by the core.
///
/// The wire values are stable identifiers; the core owns the classification and
/// this enum only mirrors it. [AlgorithmCategory.legacy] is also the fallback
/// for a value this build does not know, so a newer native library can add a
/// category without making the picker throw.
enum AlgorithmCategory {
  crypto('crypto'),
  checksum('checksum'),
  fast('fast'),
  legacy('legacy');

  const AlgorithmCategory(this.wireName);

  /// Identifier used by the native layer.
  final String wireName;

  /// Map a native category string; unknown values fall back to [legacy].
  static AlgorithmCategory fromWire(String? value) {
    for (final AlgorithmCategory category in AlgorithmCategory.values) {
      if (category.wireName == value) {
        return category;
      }
    }
    return AlgorithmCategory.legacy;
  }

  /// Every category, in the order the picker presents them.
  static const List<AlgorithmCategory> ordered = <AlgorithmCategory>[
    AlgorithmCategory.crypto,
    AlgorithmCategory.checksum,
    AlgorithmCategory.fast,
    AlgorithmCategory.legacy,
  ];
}

/// One algorithm as advertised by the core.
///
/// The UI must build its picker from `BatchHashService.listAlgorithms()` rather
/// than a hardcoded list: the core is the single source of truth for which
/// algorithms exist and how they are classified.
class AlgorithmInfo {
  const AlgorithmInfo({
    required this.id,
    required this.displayName,
    required this.hexLength,
    required this.digestLength,
    this.cryptographic = false,
    this.suffixUsable = false,
    this.category = AlgorithmCategory.legacy,
    this.suffixWarning,
    this.defaultSuffixAlgorithm = false,
  });

  /// Stable identifier used in requests, reports and filenames.
  final String id;

  /// Human readable name shown in the picker.
  final String displayName;

  /// Digest length in hex characters.
  final int hexLength;

  /// Digest length in bytes.
  final int digestLength;

  /// Whether the algorithm is considered cryptographically secure.
  final bool cryptographic;

  /// Whether the digest may be written into a filename suffix.
  final bool suffixUsable;

  /// Group this algorithm belongs to.
  final AlgorithmCategory category;

  /// Warning to show when the algorithm is picked for a suffix.
  final String? suffixWarning;

  /// Whether the core picks this algorithm for suffixes by default.
  final bool defaultSuffixAlgorithm;

  /// Decode one entry of the `bh_algorithms` payload.
  factory AlgorithmInfo.fromJson(Map<String, Object?> json) {
    final Object? warning = json['suffix_warning'];
    return AlgorithmInfo(
      id: json['id'] as String? ?? '',
      displayName: json['display_name'] as String? ?? '',
      hexLength: (json['hex_len'] as num?)?.toInt() ?? 0,
      digestLength: (json['digest_len'] as num?)?.toInt() ?? 0,
      cryptographic: json['cryptographic'] as bool? ?? false,
      suffixUsable: json['suffix_usable'] as bool? ?? false,
      category: AlgorithmCategory.fromWire(json['category'] as String?),
      suffixWarning: warning is String && warning.isNotEmpty ? warning : null,
      defaultSuffixAlgorithm:
          json['default_suffix_algorithm'] as bool? ?? false,
    );
  }

  @override
  String toString() => 'AlgorithmInfo($id, $category)';
}

/// Algorithms the core is known to ship, in canonical order.
///
/// This is the demo/fallback catalogue: it is what the fake service returns and
/// what the UI keeps as a last resort if a native build ever answers with an
/// empty list. A real build reads the list from the core through
/// `BatchHashService.listAlgorithms`; nothing in the app may treat this
/// constant as the authority on what is supported.
const List<AlgorithmInfo> kAlgorithmCatalogue = <AlgorithmInfo>[
  AlgorithmInfo(
    id: 'md5',
    displayName: 'MD5',
    hexLength: 32,
    digestLength: 16,
    suffixUsable: true,
    category: AlgorithmCategory.legacy,
    suffixWarning:
        'MD5 is cryptographically broken; a truncated suffix is not a '
        'security digest.',
  ),
  AlgorithmInfo(
    id: 'sha1',
    displayName: 'SHA-1',
    hexLength: 40,
    digestLength: 20,
    suffixUsable: true,
    category: AlgorithmCategory.legacy,
    suffixWarning:
        'SHA-1 is cryptographically broken; a truncated suffix is not a '
        'security digest.',
  ),
  AlgorithmInfo(
    id: 'sha224',
    displayName: 'SHA-224',
    hexLength: 56,
    digestLength: 28,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha256',
    displayName: 'SHA-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha384',
    displayName: 'SHA-384',
    hexLength: 96,
    digestLength: 48,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha512',
    displayName: 'SHA-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha3-256',
    displayName: 'SHA3-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha3-512',
    displayName: 'SHA3-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'blake2b',
    displayName: 'BLAKE2b-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'blake2s',
    displayName: 'BLAKE2s-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'blake3',
    displayName: 'BLAKE3',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    suffixUsable: true,
    category: AlgorithmCategory.crypto,
    defaultSuffixAlgorithm: true,
  ),
  AlgorithmInfo(
    id: 'xxh3-64',
    displayName: 'XXH3-64',
    hexLength: 16,
    digestLength: 8,
    suffixUsable: true,
    category: AlgorithmCategory.fast,
    suffixWarning:
        'XXH3 is non-cryptographic; a suffix is a convenience token, not a '
        'security digest.',
  ),
  AlgorithmInfo(
    id: 'xxh3-128',
    displayName: 'XXH3-128',
    hexLength: 32,
    digestLength: 16,
    suffixUsable: true,
    category: AlgorithmCategory.fast,
    suffixWarning:
        'XXH3 is non-cryptographic; a suffix is a convenience token, not a '
        'security digest.',
  ),
  AlgorithmInfo(
    id: 'xxh64',
    displayName: 'XXH64',
    hexLength: 16,
    digestLength: 8,
    suffixUsable: true,
    category: AlgorithmCategory.fast,
    suffixWarning:
        'XXH64 is non-cryptographic; a suffix is a convenience token, not a '
        'security digest.',
  ),
  AlgorithmInfo(
    id: 'crc32',
    displayName: 'CRC32',
    hexLength: 8,
    digestLength: 4,
    suffixUsable: true,
    category: AlgorithmCategory.checksum,
    suffixWarning:
        'CRC32 is an error-detection checksum, not a content digest; a suffix '
        'is convenience only.',
  ),
  AlgorithmInfo(
    id: 'crc32c',
    displayName: 'CRC32C',
    hexLength: 8,
    digestLength: 4,
    suffixUsable: true,
    category: AlgorithmCategory.checksum,
    suffixWarning:
        'CRC32C is an error-detection checksum, not a content digest; a '
        'suffix is convenience only.',
  ),
  AlgorithmInfo(
    id: 'md2',
    displayName: 'MD2',
    hexLength: 32,
    digestLength: 16,
    suffixUsable: true,
    category: AlgorithmCategory.legacy,
    suffixWarning: 'MD2 is obsolete and cryptographically broken.',
  ),
  AlgorithmInfo(
    id: 'md4',
    displayName: 'MD4',
    hexLength: 32,
    digestLength: 16,
    suffixUsable: true,
    category: AlgorithmCategory.legacy,
    suffixWarning: 'MD4 is cryptographically broken.',
  ),
  AlgorithmInfo(
    id: 'sha512-224',
    displayName: 'SHA-512/224',
    hexLength: 56,
    digestLength: 28,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha512-256',
    displayName: 'SHA-512/256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha3-224',
    displayName: 'SHA3-224',
    hexLength: 56,
    digestLength: 28,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sha3-384',
    displayName: 'SHA3-384',
    hexLength: 96,
    digestLength: 48,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'keccak224',
    displayName: 'Keccak-224',
    hexLength: 56,
    digestLength: 28,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'keccak256',
    displayName: 'Keccak-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'keccak384',
    displayName: 'Keccak-384',
    hexLength: 96,
    digestLength: 48,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'keccak512',
    displayName: 'Keccak-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'shake128',
    displayName: 'SHAKE128',
    hexLength: 32,
    digestLength: 16,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'shake256',
    displayName: 'SHAKE256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'ripemd128',
    displayName: 'RIPEMD-128',
    hexLength: 32,
    digestLength: 16,
    suffixUsable: true,
    category: AlgorithmCategory.legacy,
    suffixWarning: 'RIPEMD-128 is not collision resistant.',
  ),
  AlgorithmInfo(
    id: 'ripemd160',
    displayName: 'RIPEMD-160',
    hexLength: 40,
    digestLength: 20,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'ripemd256',
    displayName: 'RIPEMD-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'ripemd320',
    displayName: 'RIPEMD-320',
    hexLength: 80,
    digestLength: 40,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'sm3',
    displayName: 'SM3',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'gost94',
    displayName: 'GOST R 34.11-94',
    hexLength: 64,
    digestLength: 32,
    suffixUsable: true,
    category: AlgorithmCategory.legacy,
    suffixWarning: 'GOST R 34.11-94 is superseded by Streebog.',
  ),
  AlgorithmInfo(
    id: 'streebog256',
    displayName: 'Streebog-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'streebog512',
    displayName: 'Streebog-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'whirlpool',
    displayName: 'Whirlpool',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'tiger',
    displayName: 'Tiger',
    hexLength: 48,
    digestLength: 24,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'skein256',
    displayName: 'Skein-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'skein512',
    displayName: 'Skein-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'groestl256',
    displayName: 'Grøstl-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'groestl512',
    displayName: 'Grøstl-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'jh256',
    displayName: 'JH-256',
    hexLength: 64,
    digestLength: 32,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
  AlgorithmInfo(
    id: 'jh512',
    displayName: 'JH-512',
    hexLength: 128,
    digestLength: 64,
    cryptographic: true,
    category: AlgorithmCategory.crypto,
  ),
];

/// Algorithm IDs of [kAlgorithmCatalogue], in canonical order.
///
/// A convenience for code that only needs the identifiers. Anything that
/// groups or labels algorithms must read [AlgorithmInfo] instead.
final List<String> kAvailableAlgorithms = List<String>.unmodifiable(
  <String>[for (final AlgorithmInfo info in kAlgorithmCatalogue) info.id],
);

/// Group [algorithms] by category, keeping the registry order inside a group.
///
/// Groups with no members are omitted, so the UI can render the result directly
/// instead of filtering empty headings.
Map<AlgorithmCategory, List<AlgorithmInfo>> groupAlgorithmsByCategory(
  List<AlgorithmInfo> algorithms,
) {
  final Map<AlgorithmCategory, List<AlgorithmInfo>> grouped =
      <AlgorithmCategory, List<AlgorithmInfo>>{};
  for (final AlgorithmCategory category in AlgorithmCategory.ordered) {
    final List<AlgorithmInfo> members = <AlgorithmInfo>[
      for (final AlgorithmInfo info in algorithms)
        if (info.category == category) info,
    ];
    if (members.isNotEmpty) {
      grouped[category] = List<AlgorithmInfo>.unmodifiable(members);
    }
  }
  return grouped;
}

/// Algorithms pinned as one-tap chips on the hash step.
///
/// Deliberately a short list of common, distinct-purpose algorithms: one legacy
/// checksum, the two SHA-2 staples, the modern default and a fast checksum. The
/// full catalogue stays reachable through the picker, so this list is a
/// convenience and not a limit on what can be computed.
const List<String> kDefaultPinnedAlgorithms = <String>[
  'md5',
  'sha256',
  'sha512',
  'blake3',
  'crc32',
];
/// Algorithms offered for suffix based renaming.
const List<String> kSuffixAlgorithms = <String>[
  'blake3',
  'sha256',
  'sha1',
  'md5',
  'xxh3-64',
];

/// Outcome of hashing a single file.
enum EntryStatus {
  ok,
  failed,
  skipped;

  String get label => switch (this) {
    EntryStatus.ok => 'ok',
    EntryStatus.failed => 'failed',
    EntryStatus.skipped => 'skipped',
  };
}

/// Comparison status of one row produced by the diff engine.
enum DiffStatus {
  unchanged,
  changed,
  added,
  removed,
  unknown,
  ambiguous,
  duplicateKey,
  outOfRoot,
  corrupt,
  unstable;

  String get label => switch (this) {
    DiffStatus.unchanged => 'unchanged',
    DiffStatus.changed => 'changed',
    DiffStatus.added => 'added',
    DiffStatus.removed => 'removed',
    DiffStatus.unknown => 'unknown',
    DiffStatus.ambiguous => 'ambiguous',
    DiffStatus.duplicateKey => 'duplicate key',
    DiffStatus.outOfRoot => 'outside root',
    DiffStatus.corrupt => 'corrupt',
    DiffStatus.unstable => 'unstable',
  };

  bool get isDifference => this != DiffStatus.unchanged;
}

/// Reported acceleration state of a hardware subsystem.
enum AccelerationState { detected, softwareFallback, unknown }

/// Report export and import formats supported by the core.
///
/// The declaration order is the order the UI lists the formats in, so new
/// values are appended after the existing ones and never inserted in between.
/// [ReportFormat.name] is the wire value the native layer parses.
enum ReportFormat {
  json,
  csv,
  markdown,
  txt,
  md5sum,
  bsd,
  tree;

  String get label => switch (this) {
    ReportFormat.json => 'JSON',
    ReportFormat.csv => 'CSV',
    ReportFormat.markdown => 'Markdown',
    ReportFormat.txt => 'TXT',
    ReportFormat.md5sum => 'md5sum',
    ReportFormat.bsd => 'BSD',
    ReportFormat.tree => 'Tree',
  };

  /// English fallback copy.
  ///
  /// The pages render localized descriptions from the string table
  /// (`report.format.<name>`); this getter only exists for callers that have no
  /// `BuildContext`, such as logs and error messages.
  String get description => switch (this) {
    ReportFormat.json =>
      'Canonical machine readable report with a schema version.',
    ReportFormat.csv => 'Fixed columns for spreadsheets.',
    ReportFormat.markdown => 'Human readable table, truncated at 500 rows.',
    ReportFormat.txt => 'Aligned plain text, truncated at 500 rows.',
    ReportFormat.md5sum => 'GNU coreutils compatible digest list.',
    ReportFormat.bsd => 'BSD style digest list.',
    ReportFormat.tree => 'Indented directory tree with each file digest.',
  };

  /// Suffix used for the exported file name.
  ///
  /// [ReportFormat.tree] shares `txt` with [ReportFormat.txt]: both are plain
  /// text a person reads, and the core reports the same extension for them.
  String get fileExtension => switch (this) {
    ReportFormat.json => 'json',
    ReportFormat.csv => 'csv',
    ReportFormat.markdown => 'md',
    ReportFormat.txt => 'txt',
    ReportFormat.md5sum => 'md5',
    ReportFormat.bsd => 'bsd',
    ReportFormat.tree => 'txt',
  };

  /// Whether the core can read this format back into a report.
  ///
  /// Mirrors `ReportFormat::can_import` in the core: only the structured and
  /// checksum formats round-trip. Markdown and TXT are renderings (they
  /// truncate and they lose fields), and Tree is a drawn view, so none of the
  /// three may be offered as an import source.
  bool get canImport => switch (this) {
    ReportFormat.json => true,
    ReportFormat.csv => true,
    ReportFormat.markdown => false,
    ReportFormat.txt => false,
    ReportFormat.md5sum => true,
    ReportFormat.bsd => true,
    ReportFormat.tree => false,
  };

  /// Whether the text of this format depends on the export `locale`.
  ///
  /// The machine formats keep fixed English keys and columns whatever the UI
  /// language is: translating them would break every consumer that parses them.
  /// Only the human readable renderings follow the locale.
  bool get isLocalized => switch (this) {
    ReportFormat.json => false,
    ReportFormat.csv => false,
    ReportFormat.markdown => true,
    ReportFormat.txt => true,
    ReportFormat.md5sum => false,
    ReportFormat.bsd => false,
    ReportFormat.tree => true,
  };

  /// Parse a wire name or a file extension.
  ///
  /// Accepts the enum name (`tree`), the core's format string (the same value)
  /// and the file extension (`md`, `txt`). The Markdown alias `md` is
  /// included because the native parser accepts it too.
  static ReportFormat? fromName(String? value) {
    final String name = (value ?? '').trim().toLowerCase();
    if (name.isEmpty) {
      return null;
    }
    for (final ReportFormat format in ReportFormat.values) {
      if (format.name == name) {
        return format;
      }
    }
    // Extensions are not unique across formats (txt and tree share one), so
    // the exact wire name is always tried first.
    for (final ReportFormat format in ReportFormat.values) {
      if (format.fileExtension == name) {
        return format;
      }
    }
    return name == 'md' ? ReportFormat.markdown : null;
  }
}

/// One scanned or hashed file.
class BatchEntry {
  const BatchEntry({
    required this.path,
    required this.size,
    this.mtime,
    this.hashes = const <String, String>{},
    this.status = EntryStatus.ok,
    this.errorKind,
    this.suffixApplied,
    this.renamedTo,
  });

  final String path;
  final int size;
  final DateTime? mtime;
  final Map<String, String> hashes;
  final EntryStatus status;
  final String? errorKind;
  final String? suffixApplied;
  final String? renamedTo;

  BatchEntry copyWith({
    String? path,
    int? size,
    DateTime? mtime,
    Map<String, String>? hashes,
    EntryStatus? status,
    String? errorKind,
    String? suffixApplied,
    String? renamedTo,
  }) {
    return BatchEntry(
      path: path ?? this.path,
      size: size ?? this.size,
      mtime: mtime ?? this.mtime,
      hashes: hashes ?? this.hashes,
      status: status ?? this.status,
      errorKind: errorKind ?? this.errorKind,
      suffixApplied: suffixApplied ?? this.suffixApplied,
      renamedTo: renamedTo ?? this.renamedTo,
    );
  }
}

/// A complete hashing report for one root.
class ReportModel {
  const ReportModel({
    this.schemaVersion = 1,
    required this.root,
    required this.generatedAt,
    this.algorithms = const <String>[],
    this.entries = const <BatchEntry>[],
  });

  final int schemaVersion;
  final String root;
  final DateTime generatedAt;
  final List<String> algorithms;
  final List<BatchEntry> entries;
}

/// One row of a report-to-report or report-to-directory comparison.
class DiffRowModel {
  const DiffRowModel({
    required this.path,
    required this.status,
    this.left,
    this.right,
    this.note,
  });

  final String path;
  final DiffStatus status;
  final BatchEntry? left;
  final BatchEntry? right;
  final String? note;
}

/// Stage a long-running native job is currently in.
///
/// The values match the phase strings the Rust core reports, so a sample can be
/// mapped without a translation table that could silently drift.
enum HashPhase {
  scanning('scanning'),
  hashing('hashing'),
  planningSuffix('planning_suffix'),
  applyingSuffix('applying_suffix'),
  comparing('comparing'),
  completed('completed');

  const HashPhase(this.wireName);

  /// Identifier used by the native layer.
  final String wireName;

  /// Map a native phase string; unknown values fall back to [hashing], which
  /// keeps the progress bar moving instead of freezing on an unexpected stage.
  static HashPhase fromWire(Object? value) {
    final name = value is String ? value : '';
    for (final phase in HashPhase.values) {
      if (phase.wireName == name) {
        return phase;
      }
    }
    return HashPhase.hashing;
  }
}

/// Progress event emitted while a batch is hashed.
class HashProgress {
  const HashProgress({
    required this.completed,
    required this.total,
    this.entries = const <BatchEntry>[],
    this.done = false,
    this.phase = HashPhase.hashing,
    this.bytesDone = 0,
    this.bytesTotal = 0,
    this.etaSeconds,
    this.currentPath = '',
  });

  final int completed;
  final int total;
  final List<BatchEntry> entries;
  final bool done;

  /// Stage that produced this sample.
  final HashPhase phase;

  /// Bytes read so far. A large file reports byte progress even while its
  /// entry count is still zero completed, which is what makes the bar move.
  final int bytesDone;

  /// Total bytes to read; zero when the size is not known yet.
  final int bytesTotal;

  /// Estimated seconds remaining, when the rate is known.
  final double? etaSeconds;

  /// Display path of the file currently being read.
  final String currentPath;

  /// Completion in the range 0.0..1.0.
  ///
  /// Byte progress wins when it is available: hashing one huge file would
  /// otherwise sit at zero completed entries for its whole duration.
  double get fraction {
    if (bytesTotal > 0 && bytesDone >= 0) {
      final byteFraction = bytesDone / bytesTotal;
      if (byteFraction > 0) {
        return byteFraction.clamp(0.0, 1.0);
      }
    }
    return total <= 0 ? 0.0 : (completed / total).clamp(0.0, 1.0);
  }

  /// Whether a determinate bar can be shown, i.e. any total is known.
  ///
  /// A determinate bar at 0% is honest and still moves; an indeterminate one
  /// looks identical to a stalled job, which is what users reported.
  bool get isDeterminate => bytesTotal > 0 || total > 0;
}

/// Progress event emitted while a report is compared against a directory.
class DiffProgress {
  const DiffProgress({
    this.rows = const <DiffRowModel>[],
    this.done = false,
    this.cancelled = false,
    this.phase = HashPhase.comparing,
    this.completed = 0,
    this.total = 0,
    this.bytesDone = 0,
    this.bytesTotal = 0,
    this.etaSeconds,
    this.currentPath = '',
  });

  /// Rows produced so far; the full set once [done] is true.
  final List<DiffRowModel> rows;

  /// Whether the comparison reached a terminal state.
  final bool done;

  /// Whether the terminal state is a cancellation rather than a result.
  final bool cancelled;

  /// Stage that produced this sample.
  final HashPhase phase;

  /// Entries finished in the current stage.
  final int completed;

  /// Entries in the current stage; zero while the total is unknown.
  final int total;

  /// Bytes read so far.
  final int bytesDone;

  /// Total bytes to read; zero while unknown.
  final int bytesTotal;

  /// Estimated seconds remaining, when the rate is known.
  final double? etaSeconds;

  /// Display path of the file currently being read.
  final String currentPath;

  /// Completion in the range 0.0..1.0, preferring byte progress.
  double get fraction {
    if (bytesTotal > 0 && bytesDone > 0) {
      return (bytesDone / bytesTotal).clamp(0.0, 1.0);
    }
    if (total > 0) {
      return (completed / total).clamp(0.0, 1.0);
    }
    return 0.0;
  }

  /// Whether a determinate bar can be shown.
  bool get isDeterminate => bytesTotal > 0 || total > 0;
}

/// Status of one row in a rename dry run.
enum RenameRowStatus {
  ready,
  conflict,
  skipped,
  missingHash,
  invalidName,
  noPermission,
  unsupported,

  /// The file on disk no longer matches the digest the plan expected.
  ///
  /// Its own status rather than a skip: a skip means "nothing to do", while
  /// this means "something changed behind our back", which the user has to see.
  hashMismatch;

  String get label => switch (this) {
    RenameRowStatus.ready => 'ready',
    RenameRowStatus.conflict => 'conflict',
    RenameRowStatus.skipped => 'skipped',
    RenameRowStatus.missingHash => 'missing hash',
    RenameRowStatus.invalidName => 'invalid name',
    RenameRowStatus.noPermission => 'no permission',
    RenameRowStatus.unsupported => 'unsupported',
    RenameRowStatus.hashMismatch => 'hash mismatch',
  };
}

/// One row of a rename dry run.
class RenamePlanRowModel {
  const RenamePlanRowModel({
    required this.path,
    required this.status,
    this.targetPath,
    this.message,
  });

  final String path;
  final RenameRowStatus status;
  final String? targetPath;
  final String? message;
}

/// A complete rename dry run.
class RenamePlanModel {
  const RenamePlanModel({
    required this.algorithm,
    required this.length,
    required this.uppercase,
    required this.rows,
    this.algorithms = const <String>[],
  });

  /// Primary algorithm, kept for callers that only care about one.
  final String algorithm;

  /// Every algorithm written into this plan. Empty means "just [algorithm]",
  /// so a plan built before multi-algorithm support still reads correctly.
  final List<String> algorithms;

  /// The full algorithm list, falling back to the primary one.
  List<String> get effectiveAlgorithms =>
      algorithms.isEmpty ? <String>[algorithm] : algorithms;
  final int length;
  final bool uppercase;
  final List<RenamePlanRowModel> rows;

  int get readyCount => rows
      .where((RenamePlanRowModel row) => row.status == RenameRowStatus.ready)
      .length;

  int get conflictCount => rows
      .where((RenamePlanRowModel row) => row.status == RenameRowStatus.conflict)
      .length;

  int get blockedCount => rows.length - readyCount;
}

/// Result of applying a rename plan.
class RenameExecuteResult {
  const RenameExecuteResult({
    required this.applied,
    required this.skipped,
    required this.failed,
    this.batchId,
  });

  final int applied;
  final int skipped;
  final int failed;
  final String? batchId;
}

/// One file written by the report exporter.
class ExportedReport {
  const ExportedReport({required this.format, required this.path});

  final ReportFormat format;
  final String path;
}

/// One batch stored in the rename log.
class HistoryBatchModel {
  const HistoryBatchModel({
    required this.id,
    required this.createdAt,
    required this.fileCount,
    required this.appliedCount,
    this.undone = false,
  });

  final String id;
  final DateTime createdAt;
  final int fileCount;
  final int appliedCount;
  final bool undone;

  HistoryBatchModel copyWith({bool? undone}) {
    return HistoryBatchModel(
      id: id,
      createdAt: createdAt,
      fileCount: fileCount,
      appliedCount: appliedCount,
      undone: undone ?? this.undone,
    );
  }
}

/// One algorithm's digest of a pasted text.
class TextHashResult {
  const TextHashResult({
    required this.algorithmId,
    required this.displayName,
    required this.digest,
  });

  /// Stable algorithm id, for example `sha256`.
  final String algorithmId;

  /// Human readable name, so the UI does not repeat the catalogue lookup.
  final String displayName;

  /// Lowercase hex digest.
  final String digest;
}
/// CPU side hardware information reported by the core probe.
class HardwareInfoModel {
  const HardwareInfoModel({
    required this.arch,
    this.detectedFeatures = const <String>[],
    this.blake3BackendReported = 'unknown',
    this.sha256BackendReported = 'unknown',
    this.backendPolicy = 'unknown',
    this.multithread = false,
  });

  final String arch;
  final List<String> detectedFeatures;
  final String blake3BackendReported;
  final String sha256BackendReported;
  final String backendPolicy;
  final bool multithread;
}

/// GPU rendering acceleration information reported by the platform layer.
class RenderingHardwareModel {
  const RenderingHardwareModel({
    this.accelerated = false,
    this.gpuBackend = 'unknown',
  });

  final bool accelerated;
  final String gpuBackend;
}

/// Recommended suffix length for a given number of files.
///
/// The first tier returns 6, matching the default. A default that differs from
/// the recommendation makes the page contradict itself: the slider would sit on
/// one number while the hint recommended another.
int recommendedSuffixLength(int fileCount) {
  if (fileCount <= 100) {
    return 6;
  }
  if (fileCount <= 1000) {
    return 8;
  }
  if (fileCount <= 100000) {
    return 10;
  }
  return 12;
}

/// Birthday bound estimate of a suffix collision over the hex alphabet.
double collisionProbability(int fileCount, int length) {
  if (fileCount <= 1 || length <= 0) {
    return 0.0;
  }
  final double space = math.pow(16, length).toDouble();
  final double pairs = fileCount * (fileCount - 1) / 2;
  return 1 - math.exp(-pairs / space);
}

/// Settings the rename page keeps between visits and across restarts.
///
/// Separate from the hash page selection on purpose: writing a suffix and
/// computing a report answer different questions, and a user who hashes with
/// twenty algorithms should still be able to write a five-character suffix.
class RenameConfig {
  const RenameConfig({
    this.algorithms = const <String>[kDefaultRenameAlgorithm],
    this.length = kDefaultRenameLength,
    this.uppercase = true,
    this.strict = true,
  });

  /// Algorithms written into the filename, in the order they appear.
  final List<String> algorithms;

  /// Hex characters taken from each digest.
  final int length;

  /// Whether the hex token is uppercase.
  final bool uppercase;

  /// When true, a file whose current digest does not match the recorded one is
  /// refused instead of renamed.
  final bool strict;

  RenameConfig copyWith({
    List<String>? algorithms,
    int? length,
    bool? uppercase,
    bool? strict,
  }) {
    return RenameConfig(
      algorithms: algorithms ?? this.algorithms,
      length: length ?? this.length,
      uppercase: uppercase ?? this.uppercase,
      strict: strict ?? this.strict,
    );
  }
}

/// Suffix length used before the user picks one.
///
/// Six rather than the historical five: a shorter token collides as soon as a
/// folder holds a few thousand files, and six is what the default suffix
/// algorithm already emits for its own token.
const int kDefaultRenameLength = 6;

/// Algorithm written when the user has never chosen one.
const String kDefaultRenameAlgorithm = 'blake3';

/// Bounds accepted for the suffix length, matching the Rust contract.
const int kMinRenameLength = 4;
const int kMaxRenameLength = 16;

/// Algorithms that may be written into a filename suffix.
///
/// Derived from the catalogue rather than hand-listed so the rename page cannot
/// drift behind the hash page: the old fixed five-entry list is exactly how the
/// two pages ended up offering different algorithm sets.
final List<AlgorithmInfo> kSuffixUsableAlgorithms = <AlgorithmInfo>[
  for (final AlgorithmInfo info in kAlgorithmCatalogue)
    if (info.suffixUsable) info,
];

/// Outcome of importing a rename log archive.
class RenameLogImportResult {
  const RenameLogImportResult({
    required this.batches,
    required this.operations,
    this.skipped = 0,
  });

  /// Batches actually inserted.
  final int batches;

  /// Operations actually inserted.
  final int operations;

  /// Batches skipped because their id was already present.
  ///
  /// Reported separately so re-importing the same file reads as "nothing to
  /// do" instead of looking like a failed restore.
  final int skipped;
}
