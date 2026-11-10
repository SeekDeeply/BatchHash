import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// Guards the algorithm catalogue the UI groups by.
///
/// The core owns the real list; this constant is what the demo build and the
/// fallback path use, so it has to stay a faithful copy of the registry shape:
/// same order, same lengths, same classification. These tests need no native
/// library, so they run in the plain `flutter test` job.
void main() {
  test('the catalogue is complete and uses unique ids', () {
    expect(kAlgorithmCatalogue, hasLength(44));
    final ids = kAlgorithmCatalogue.map((info) => info.id).toList();
    expect(ids.toSet(), hasLength(44), reason: 'ids must be unique');
    expect(kAvailableAlgorithms, ids);
  });

  test('every entry carries a consistent digest geometry', () {
    for (final AlgorithmInfo info in kAlgorithmCatalogue) {
      expect(info.displayName, isNotEmpty, reason: info.id);
      expect(info.digestLength, greaterThan(0), reason: info.id);
      expect(
        info.hexLength,
        info.digestLength * 2,
        reason: '${info.id} hex length must be twice the byte length',
      );
    }
  });

  test('categories really partition the catalogue', () {
    final Map<AlgorithmCategory, List<AlgorithmInfo>> grouped =
        groupAlgorithmsByCategory(kAlgorithmCatalogue);
    expect(grouped, isNotEmpty);
    // A single bucket would make the grouping pointless.
    expect(grouped.keys.length, greaterThanOrEqualTo(3));
    expect(
      grouped.values.expand((group) => group).length,
      kAlgorithmCatalogue.length,
      reason: 'every algorithm belongs to exactly one group',
    );
    // Groups come out in the documented display order.
    final List<AlgorithmCategory> order = grouped.keys.toList();
    final List<AlgorithmCategory> expected =
        AlgorithmCategory.ordered.where(grouped.containsKey).toList();
    expect(order, expected);
  });

  test('the classification matches what the core reports', () {
    final Map<String, AlgorithmInfo> byId = <String, AlgorithmInfo>{
      for (final AlgorithmInfo info in kAlgorithmCatalogue) info.id: info,
    };
    expect(byId['sha256']!.category, AlgorithmCategory.crypto);
    expect(byId['blake3']!.category, AlgorithmCategory.crypto);
    expect(byId['crc32']!.category, AlgorithmCategory.checksum);
    expect(byId['crc32c']!.category, AlgorithmCategory.checksum);
    expect(byId['xxh3-64']!.category, AlgorithmCategory.fast);
    expect(byId['xxh64']!.category, AlgorithmCategory.fast);
    // Broken digests are legacy and carry a warning; without the warning the
    // rename page would offer them as if they were safe suffixes.
    for (final String id in <String>['md5', 'sha1', 'md2', 'md4']) {
      expect(byId[id]!.category, AlgorithmCategory.legacy, reason: id);
      expect(byId[id]!.cryptographic, isFalse, reason: id);
      expect(byId[id]!.suffixWarning, isNotNull, reason: id);
    }
    // Exactly one algorithm is the suffix default.
    final List<String> defaults = <String>[
      for (final AlgorithmInfo info in kAlgorithmCatalogue)
        if (info.defaultSuffixAlgorithm) info.id,
    ];
    expect(defaults, <String>['blake3']);
  });

  test('the suffix picker only offers suffix-capable algorithms', () {
    final Map<String, AlgorithmInfo> byId = <String, AlgorithmInfo>{
      for (final AlgorithmInfo info in kAlgorithmCatalogue) info.id: info,
    };
    expect(kSuffixAlgorithms, isNotEmpty);
    for (final String id in kSuffixAlgorithms) {
      expect(byId, contains(id), reason: 'unknown suffix algorithm $id');
      expect(
        byId[id]!.suffixUsable,
        isTrue,
        reason: '$id may not be written into a filename suffix',
      );
    }
  });

  test('decodes a core payload and survives an unknown category', () {
    final AlgorithmInfo info = AlgorithmInfo.fromJson(<String, Object?>{
      'id': 'sha256',
      'display_name': 'SHA-256',
      'hex_len': 64,
      'digest_len': 32,
      'cryptographic': true,
      'suffix_usable': true,
      'category': 'crypto',
      'suffix_warning': null,
      'default_suffix_algorithm': false,
    });
    expect(info.id, 'sha256');
    expect(info.category, AlgorithmCategory.crypto);
    expect(info.hexLength, 64);
    expect(info.digestLength, 32);
    expect(info.suffixWarning, isNull);
    expect(info.cryptographic, isTrue);

    // A newer core may add a category. Decoding must not throw; the entry lands
    // in legacy so the picker still shows it instead of dropping it.
    final AlgorithmInfo future = AlgorithmInfo.fromJson(<String, Object?>{
      'id': 'future',
      'category': 'post-quantum',
    });
    expect(future.category, AlgorithmCategory.legacy);
    expect(AlgorithmCategory.fromWire('checksum'), AlgorithmCategory.checksum);
    expect(AlgorithmCategory.fromWire(null), AlgorithmCategory.legacy);
  });

  test('the fake service serves the catalogue', () async {
    final FakeBatchHashService fake = FakeBatchHashService();
    final List<AlgorithmInfo> served = await fake.listAlgorithms();
    expect(served, kAlgorithmCatalogue);
    // Demo digests have to look like real ones, otherwise a length mismatch
    // looks like a decoder bug rather than demo data. The demo tree contains
    // deliberately failed entries, so pick one that actually hashes.
    final List<BatchEntry> scanned = await fake.scanDirectory(
      FakeBatchHashService.rootPath,
    );
    final BatchEntry entry = scanned.firstWhere(
      (BatchEntry candidate) => candidate.status == EntryStatus.ok,
    );
    // computeHashes streams progress samples; the hashed entries ride along on
    // each sample, so take the final one rather than a returned list.
    final List<HashProgress> samples = await fake
        .computeHashes(
          entries: <BatchEntry>[entry],
          algorithms: <String>['sha256', 'xxh3-64', 'crc32', 'md5'],
        )
        .toList();
    final Map<String, String> hashes = samples.last.entries.first.hashes;
    expect(hashes['sha256'], hasLength(64));
    expect(hashes['xxh3-64'], hasLength(16));
    expect(hashes['crc32'], hasLength(8));
    expect(hashes['md5'], hasLength(32));
  });
}
