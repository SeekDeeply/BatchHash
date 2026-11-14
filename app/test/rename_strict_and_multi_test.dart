import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/app_settings_store.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

BatchEntry okEntry(String path, Map<String, String> hashes) => BatchEntry(
  path: path,
  size: 8,
  status: EntryStatus.ok,
  hashes: hashes,
);

void main() {
  test('default suffix length is six', () {
    // The default and the first recommendation tier must agree, otherwise the
    // page shows a slider on one number and a hint recommending another.
    expect(const RenameConfig().length, 6);
    expect(kDefaultRenameLength, 6);
    expect(recommendedSuffixLength(10), 6);
    expect(recommendedSuffixLength(100), 6);
  });

  test('multiple algorithms are joined into one suffix', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchEntry entry = okEntry('/root/a.txt', <String, String>{
      'blake3': 'ABCDEF123456',
      'md5': 'FEDCBA654321',
    });
    final RenamePlanModel plan = await service.planRenames(
      entries: <BatchEntry>[entry],
      algorithms: const <String>['blake3', 'md5'],
      length: 6,
    );
    final RenamePlanRowModel row = plan.rows.single;
    expect(row.targetPath, contains('ABCDEF'));
    expect(row.targetPath, contains('FEDCBA'));
    // One bracket group, both digests: two separate groups would be ambiguous
    // to parse back and would look like a doubly-suffixed name.
    expect(RegExp(r'\[[^\]]+\]').allMatches(row.targetPath ?? '').length, 1);
  });

  test('a missing digest blocks the whole row', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchEntry entry = okEntry('/root/a.txt', <String, String>{
      'blake3': 'ABCDEF123456',
    });
    final RenamePlanModel plan = await service.planRenames(
      entries: <BatchEntry>[entry],
      algorithms: const <String>['blake3', 'md5'],
      length: 6,
    );
    // Renaming with only half the requested digests would produce a name the
    // user did not configure, so the row is refused rather than half-written.
    expect(plan.rows.single.status, RenameRowStatus.missingHash);
  });

  test('clear-suffix mode strips the existing digest', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchEntry entry = okEntry(
      '/root/movie[ABCDEF].mkv',
      <String, String>{'blake3': 'ABCDEF123456'},
    );
    final RenamePlanModel plan = await service.planRenames(
      entries: <BatchEntry>[entry],
      algorithms: const <String>['blake3'],
      length: 6,
      clearSuffix: true,
    );
    expect(plan.rows.single.targetPath, isNot(contains('[ABCDEF]')));
  });

  test('rename config survives a settings round trip', () {
    const AppSettings settings = AppSettings(
      renameLength: 10,
      renameAlgorithms: <String>['md5', 'sha1'],
      renameStrict: false,
      renameUppercase: false,
    );
    final AppSettings back = AppSettings.fromJson(settings.toJson());
    expect(back.renameLength, 10);
    expect(back.renameAlgorithms, <String>['md5', 'sha1']);
    expect(back.renameStrict, isFalse);
    expect(back.renameUppercase, isFalse);
  });

  test('a corrupt rename field does not discard the others', () {
    final AppSettings back = AppSettings.fromJson(<String, Object?>{
      'locale': 'zh',
      'rename_length': 'not a number',
      'rename_strict': true,
    });
    expect(back.locale, 'zh');
    expect(back.renameLength, isNull);
    expect(back.renameStrict, isTrue);
  });
}
