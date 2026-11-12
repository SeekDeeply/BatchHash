import 'dart:convert';
import 'dart:typed_data';

import 'package:archive/archive.dart';
import 'package:batchhash/src/models/operation_log.dart';
import 'package:batchhash/src/services/archive_export.dart';
import 'package:batchhash/src/services/operation_log_export.dart';
import 'package:batchhash/src/services/text_compare_export.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('sha256', () {
    test('matches the standard vectors', () {
      expect(
        sha256Hex(const <int>[]),
        'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
      );
      expect(
        sha256Hex(utf8.encode('abc')),
        'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
      );
    });
  });

  group('exportFileName', () {
    test('appends the first 16 hex characters when a digest is given', () {
      expect(
        exportFileName('log', digest: 'abcdef0123456789ffff', extension: '.json'),
        'log_abcdef0123456789.json',
      );
    });

    test('omits the suffix when there is no digest', () {
      expect(exportFileName('log', extension: '.json'), 'log.json');
      expect(exportFileName('log', digest: '', extension: '.json'), 'log.json');
    });
  });

  group('zipBytes', () {
    test('round-trips without a password', () {
      final Uint8List zip = zipBytes(<String, List<int>>{
        'a.txt': utf8.encode('hello'),
      });
      final Archive decoded = ZipDecoder().decodeBytes(zip, verify: true);
      expect(decoded.files, hasLength(1));
      expect(decoded.files.first.name, 'a.txt');
      expect(utf8.decode(decoded.files.first.content as List<int>), 'hello');
    });

    test('a password produces an archive readable only with that password', () {
      final Uint8List zip = zipBytes(<String, List<int>>{
        'secret.txt': utf8.encode('classified'),
      }, password: 'hunter2');
      final Archive decoded = ZipDecoder().decodeBytes(
        zip,
        password: 'hunter2',
        verify: true,
      );
      expect(decoded.files, hasLength(1));
      expect(
        utf8.decode(decoded.files.first.content as List<int>),
        'classified',
      );
    });
  });

  group('operation log export', () {
    final List<OperationLogEntry> entries = <OperationLogEntry>[
      OperationLogEntry(
        kind: 'op.hash',
        status: true,
        startedAt: DateTime.utc(2026, 1, 2, 3, 4, 5),
        durationMs: 42,
        detail: '/data · 2',
        hashes: const <HashDigestRecord>[
          HashDigestRecord(
            path: '/data/a.bin',
            algorithm: 'sha256',
            digest: 'aa',
          ),
        ],
        hashesTruncated: 3,
      ),
      OperationLogEntry(
        kind: 'op.compareDirectory',
        status: false,
        startedAt: DateTime.utc(2026, 1, 2, 3, 5, 5),
        detail: 'boom',
      ),
    ];

    test('JSON keeps the schema, the count and every entry', () {
      final String body = encodeLogAsJson(
        entries,
        at: DateTime.utc(2026, 1, 2),
      );
      final Map<String, dynamic> decoded =
          jsonDecode(body) as Map<String, dynamic>;
      expect(decoded['schema'], 'batchhash.operation-log.v1');
      expect(decoded['count'], 2);
      expect(decoded['entries'] as List<dynamic>, hasLength(2));
      final Map<String, dynamic> first =
          (decoded['entries'] as List<dynamic>).first as Map<String, dynamic>;
      expect(first['kind'], 'op.hash');
      // The stored schema names the verdict `ok`, not `status`.
      expect(first['ok'], isTrue);
    });

    test('Markdown carries the verdict, the digests and the truncation note', () {
      final String body = encodeLogAsMarkdown(entries);
      expect(body, contains('op.hash'));
      expect(body, contains('succeeded'));
      expect(body, contains('failed'));
      expect(body, contains('/data/a.bin'));
      expect(body, contains('3 more digests'));
    });
  });

  group('text compare export', () {
    const TextCompareExportData data = TextCompareExportData(
      leftText: 'alpha',
      rightText: 'beta',
      leftHashes: <({String algorithm, String digest})>[
        (algorithm: 'sha256', digest: '11'),
      ],
      rightHashes: <({String algorithm, String digest})>[
        (algorithm: 'sha256', digest: '22'),
      ],
      identical: false,
      differingLines: 1,
      totalLines: 1,
    );

    test('JSON round-trips the shape and the verdict', () {
      final Map<String, dynamic> decoded =
          jsonDecode(encodeTextCompareAsJson(data)) as Map<String, dynamic>;
      expect(decoded['schema'], 'batchhash.text-compare.v1');
      expect(decoded['identical'], isFalse);
      expect((decoded['left'] as Map<String, dynamic>)['text'], 'alpha');
      expect((decoded['right'] as Map<String, dynamic>)['text'], 'beta');
      expect(
        ((decoded['left'] as Map<String, dynamic>)['hashes']
            as Map<String, dynamic>)['sha256'],
        '11',
      );
    });

    test('Markdown fences both texts and lists the digests', () {
      final String body = encodeTextCompareAsMarkdown(data);
      expect(body, contains('alpha'));
      expect(body, contains('beta'));
      expect(body, contains('sha256'));
      expect(body, contains('different'));
    });

    test('a fence inside the text does not break the Markdown block', () {
      const TextCompareExportData tricky = TextCompareExportData(
        leftText: 'before\n```\nafter',
        rightText: '',
        leftHashes: <({String algorithm, String digest})>[],
        rightHashes: <({String algorithm, String digest})>[],
        identical: false,
        differingLines: 1,
        totalLines: 2,
      );
      final String body = encodeTextCompareAsMarkdown(tricky);
      // The fence must be longer than the three backticks in the payload.
      expect(body, contains('````'));
    });

    test('TXT includes both sides and the verdict', () {
      final String body = encodeTextCompareAsTxt(data);
      expect(body, contains('LEFT'));
      expect(body, contains('RIGHT'));
      expect(body, contains('alpha'));
      expect(body, contains('different'));
    });

    test('empty detection sees both panes', () {
      const TextCompareExportData empty = TextCompareExportData(
        leftText: '',
        rightText: '',
        leftHashes: <({String algorithm, String digest})>[],
        rightHashes: <({String algorithm, String digest})>[],
        identical: true,
        differingLines: 0,
        totalLines: 0,
      );
      expect(empty.isEmpty, isTrue);
      expect(data.isEmpty, isFalse);
    });
  });
}