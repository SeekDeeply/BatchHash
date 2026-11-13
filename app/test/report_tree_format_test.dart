import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter_test/flutter_test.dart';

/// Covers the tree export format at the service boundary.
///
/// The real rendering is Rust; these tests guard the Dart side of the contract:
/// the format exists, it is offered with the right extension, the demo service
/// renders a structure a test can read, and the locale reaches the renderer.
void main() {
  /// A digest of [length] hex characters, repeating [unit].
  String digest(String unit, int length) =>
      List<String>.filled(length, unit).join();

  ReportModel report() {
    return ReportModel(
      root: '/data/site',
      generatedAt: DateTime.utc(2024, 6, 1, 12),
      algorithms: <String>['sha256', 'xxh3-64'],
      entries: <BatchEntry>[
        BatchEntry(
          path: '/data/site/docs/guide.md',
          size: 120,
          hashes: <String, String>{
            'sha256': digest('a', 64),
            'xxh3-64': '0123456789abcdef',
          },
        ),
        BatchEntry(
          path: '/data/site/index.html',
          size: 64,
          hashes: <String, String>{
            'sha256': digest('b', 64),
            'xxh3-64': 'fedcba9876543210',
          },
        ),
        BatchEntry(
          path: '/data/site/docs/api.md',
          size: 32,
          status: EntryStatus.failed,
          errorKind: 'permission_denied',
        ),
      ],
    );
  }

  test('tree is a first class export format', () {
    // Appended after bsd so the existing order, and the default selection,
    // never shift when the format list grows.
    expect(ReportFormat.values.last, ReportFormat.tree);
    expect(ReportFormat.values.first, ReportFormat.json);
    expect(ReportFormat.tree.name, 'tree');
    expect(ReportFormat.tree.label, 'Tree');
    // Plain text: the format carries digests but no machine schema of its own.
    expect(ReportFormat.tree.fileExtension, 'txt');
    // Nothing reads a drawn tree back, so the UI must not offer it as a source.
    expect(ReportFormat.tree.canImport, isFalse);
    // Mirrors ReportFormat::can_import in the core: Markdown and TXT are
    // renderings that truncate and lose fields, so they are export only.
    for (final ReportFormat format in <ReportFormat>[
      ReportFormat.json,
      ReportFormat.csv,
      ReportFormat.md5sum,
      ReportFormat.bsd,
    ]) {
      expect(format.canImport, isTrue, reason: format.name);
    }
    for (final ReportFormat format in <ReportFormat>[
      ReportFormat.markdown,
      ReportFormat.txt,
      ReportFormat.tree,
    ]) {
      expect(format.canImport, isFalse, reason: format.name);
    }
  });

  test('only the human readable formats follow the locale', () {
    // Translating a machine format's keys would break every parser of it.
    for (final ReportFormat format in <ReportFormat>[
      ReportFormat.json,
      ReportFormat.csv,
      ReportFormat.md5sum,
      ReportFormat.bsd,
    ]) {
      expect(format.isLocalized, isFalse, reason: format.name);
    }
    for (final ReportFormat format in <ReportFormat>[
      ReportFormat.markdown,
      ReportFormat.txt,
      ReportFormat.tree,
    ]) {
      expect(format.isLocalized, isTrue, reason: format.name);
    }
  });

  test('every format has a label, description and extension', () {
    for (final ReportFormat format in ReportFormat.values) {
      expect(format.label, isNotEmpty, reason: format.name);
      expect(format.description, isNotEmpty, reason: format.name);
      expect(format.fileExtension, isNotEmpty, reason: format.name);
      expect(format.fileExtension, isNot(contains('.')), reason: format.name);
    }
  });

  test('fromName parses the wire name and the extension', () {
    expect(ReportFormat.fromName('tree'), ReportFormat.tree);
    expect(ReportFormat.fromName('TREE'), ReportFormat.tree);
    expect(ReportFormat.fromName('json'), ReportFormat.json);
    expect(ReportFormat.fromName('md'), ReportFormat.markdown);
    expect(ReportFormat.fromName('md5'), ReportFormat.md5sum);
    expect(ReportFormat.fromName(''), isNull);
    expect(ReportFormat.fromName(null), isNull);
    expect(ReportFormat.fromName('nonsense'), isNull);
    // txt is ambiguous (txt and tree share it); the exact wire name wins over
    // the extension lookup, which falls back to the first format claiming it.
    expect(ReportFormat.fromName('txt'), ReportFormat.txt);
    for (final ReportFormat format in ReportFormat.values) {
      expect(ReportFormat.fromName(format.name), format, reason: format.name);
    }
  });

  test('the demo service renders a real tree', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    await service.exportReport(
      report: report(),
      formats: <ReportFormat>{ReportFormat.tree},
      directory: '/tmp/batchhash-tree',
    );
    final String text = service.lastExportContents['tree']!;

    // The whole drawing, so a change in indentation or ordering shows up in
    // the diff instead of hiding behind a pair of `contains` checks.
    final String expected =
        'BatchHash Report\n'
        '|- docs\n'
        '|  |- api.md  (failed)\n'
        '|  \\- guide.md  sha256:${digest('a', 64)}  xxh3-64:0123456789abcdef\n'
        '\\- index.html  sha256:${digest('b', 64)}  xxh3-64:fedcba9876543210\n';
    expect(text.startsWith(expected), isTrue, reason: text);

    // The summary counts every entry, not only the hashed ones.
    expect(text, contains('Summary:'));
    expect(text, contains('Total: 3'));
    expect(text, contains('failed: 1'));
    expect(text, contains('Total bytes: 216'));
  });

  test('digests follow the report order, not the map order', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    await service.exportReport(
      report: report(),
      formats: <ReportFormat>{ReportFormat.tree},
      directory: '/tmp/batchhash-tree-order',
    );
    final String text = service.lastExportContents['tree']!;
    // Two digests on one line, two spaces apart, sha256 first because the
    // report lists it first.
    expect(
      text,
      contains('sha256:${digest('a', 64)}  xxh3-64:0123456789abcdef'),
    );
  });

  test('the locale reaches the renderer', () async {
    final FakeBatchHashService service = FakeBatchHashService();
    await service.exportReport(
      report: report(),
      formats: <ReportFormat>{ReportFormat.tree},
      directory: '/tmp/batchhash-tree-zh',
      locale: 'zh-CN',
    );
    expect(service.lastExportLocale, 'zh-CN');
    final String chinese = service.lastExportContents['tree']!;
    expect(chinese, contains('汇总'));
    expect(chinese, contains('总字节数'));

    await service.exportReport(
      report: report(),
      formats: <ReportFormat>{ReportFormat.tree},
      directory: '/tmp/batchhash-tree-en',
      locale: 'en',
    );
    expect(service.lastExportContents['tree']!, contains('Summary'));

    // An omitted locale keeps the historical English output.
    await service.exportReport(
      report: report(),
      formats: <ReportFormat>{ReportFormat.tree},
      directory: '/tmp/batchhash-tree-default',
    );
    expect(service.lastExportLocale, isNull);
    expect(service.lastExportContents['tree']!, contains('Summary'));
  });

  test('tree and txt exports do not collide in one directory', () async {
    // Both formats use the .txt extension, so the second file has to be
    // numbered rather than overwriting the first.
    final FakeBatchHashService service = FakeBatchHashService();
    final List<ExportedReport> written = await service.exportReport(
      report: report(),
      formats: <ReportFormat>{ReportFormat.txt, ReportFormat.tree},
      directory: '/tmp/batchhash-collide',
    );
    expect(written, hasLength(2));
    final Set<String> paths = written.map((item) => item.path).toSet();
    expect(paths, hasLength(2), reason: 'two exports must not share a path');
    for (final ExportedReport item in written) {
      expect(item.path, endsWith('.txt'));
    }
  });
}
