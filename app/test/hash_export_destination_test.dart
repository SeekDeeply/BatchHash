import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/pages/hash_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// A recorded export call, so a test can prove which directory was used.
class _RecordingService extends FakeBatchHashService {
  String? lastDirectory;
  bool exportCalled = false;

  @override
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
  }) async {
    exportCalled = true;
    lastDirectory = directory;
    return super.exportReport(
      report: report,
      formats: formats,
      directory: directory,
    );
  }
}


/// Counts export calls so a test can prove a second tap did nothing.
class _BlockingExportService extends FakeBatchHashService {
  int exportCalls = 0;

  @override
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
  }) async {
    exportCalls += 1;
    return super.exportReport(
      report: report,
      formats: formats,
      directory: directory,
    );
  }
}

void main() {
  testWidgets('a second tap while exporting is ignored', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1600));
    final _BlockingExportService service = _BlockingExportService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    state.entries.value = service.sampleReport.entries;
    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    final Finder json = find.byKey(const ValueKey<String>('hash-export-json'));
    await tester.ensureVisible(json);
    await tester.tap(json);
    await tester.pumpAndSettle();

    // The destination chooser is still open and the export has not finished:
    // both buttons must be disabled so a second tap cannot open another
    // chooser or start a concurrent write.
    expect(
      tester.widget<OutlinedButton>(json).onPressed,
      isNull,
      reason: 'the JSON button must be disabled while exporting',
    );
    final Finder csv = find.byKey(const ValueKey<String>('hash-export-csv'));
    expect(
      tester.widget<OutlinedButton>(csv).onPressed,
      isNull,
      reason: 'the CSV button must be disabled while exporting',
    );
    expect(
      find.byKey(const ValueKey<String>('path-input-field')),
      findsOneWidget,
      reason: 'only one chooser may be open at a time',
    );

    await tester.enterText(
      find.byKey(const ValueKey<String>('path-input-field')),
      '/tmp/batchhash-single',
    );
    await tester.tap(find.byKey(const ValueKey<String>('path-confirm')));
    await tester.pumpAndSettle();

    expect(service.exportCalls, 1);
  });
  testWidgets('export asks for a directory and passes it to the service', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1600));
    final _RecordingService service = _RecordingService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    // The page snapshots the shared state in initState, so the report and its
    // entries must be in place before the first frame.
    state.report.value = service.sampleReport;
    state.entries.value = service.sampleReport.entries;
    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    final Finder exportButton = find.byKey(
      const ValueKey<String>('hash-export-json'),
    );
    expect(exportButton, findsOneWidget);
    await tester.ensureVisible(exportButton);
    await tester.pumpAndSettle();
    await tester.tap(exportButton);
    await tester.pumpAndSettle();

    // Without a platform picker the app falls back to its own path dialog;
    // the export must not have run before a directory was confirmed.
    expect(
      service.exportCalled,
      isFalse,
      reason: 'export must wait for a chosen directory',
    );
    final Finder field = find.byKey(const ValueKey<String>('path-input-field'));
    expect(field, findsOneWidget, reason: 'the fallback chooser must be shown');
    await tester.enterText(field, '/tmp/batchhash-out');
    await tester.tap(find.byKey(const ValueKey<String>('path-confirm')));
    await tester.pumpAndSettle();

    expect(service.exportCalled, isTrue);
    expect(service.lastDirectory, '/tmp/batchhash-out');
  });

  testWidgets('cancelling the chooser exports nothing', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1600));
    final _RecordingService service = _RecordingService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    state.entries.value = service.sampleReport.entries;
    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    final Finder exportButton = find.byKey(
      const ValueKey<String>('hash-export-csv'),
    );
    await tester.ensureVisible(exportButton);
    await tester.pumpAndSettle();
    await tester.tap(exportButton);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('path-cancel')));
    await tester.pumpAndSettle();

    expect(
      service.exportCalled,
      isFalse,
      reason: 'a cancelled chooser must not fall back to the app default folder',
    );
  });
}