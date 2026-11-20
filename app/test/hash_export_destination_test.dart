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
    String? locale,
    bool includeLog = false,
  }) async {
    exportCalled = true;
    lastDirectory = directory;
    return super.exportReport(
      report: report,
      formats: formats,
      directory: directory,
      locale: locale,
      includeLog: includeLog,
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
    String? locale,
    bool includeLog = false,
  }) async {
    exportCalls += 1;
    return super.exportReport(
      report: report,
      formats: formats,
      directory: directory,
      locale: locale,
      includeLog: includeLog,
    );
  }
}

void main() {
  /// Open the export menu and choose [format].
  ///
  /// The toolbar used to carry one button per format; it now carries a single
  /// menu, so every export test has to make the same two taps.
  Future<void> pickFormat(WidgetTester tester, String label) async {
    final Finder menu = find.byKey(const ValueKey<String>('hash-export-menu'));
    await tester.ensureVisible(menu);
    await tester.pumpAndSettle();
    await tester.tap(menu);
    await tester.pumpAndSettle();
    await tester.tap(find.text(label).last);
    await tester.pumpAndSettle();
  }

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

    await pickFormat(tester, 'JSON');

    // The destination chooser is still open and the export has not finished:
    // both buttons must be disabled so a second tap cannot open another
    // chooser or start a concurrent write.
    expect(
      tester
          .widget<PopupMenuButton<ReportFormat>>(
            find.byKey(const ValueKey<String>('hash-export-menu')),
          )
          .enabled,
      isFalse,
      reason: 'the export menu must be disabled while exporting',
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

    await pickFormat(tester, 'JSON');

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

    await pickFormat(tester, 'CSV');
    await tester.tap(find.byKey(const ValueKey<String>('path-cancel')));
    await tester.pumpAndSettle();

    expect(
      service.exportCalled,
      isFalse,
      reason: 'a cancelled chooser must not fall back to the app default folder',
    );
  });
}