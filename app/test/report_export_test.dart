import 'dart:io';

import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/pages/report_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

const String _chosen = '/tmp/batchhash-exports';

const List<ExportedReport> _written = <ExportedReport>[
  ExportedReport(
    format: ReportFormat.json,
    path: '/tmp/batchhash-exports/batchhash-report.json',
  ),
  ExportedReport(
    format: ReportFormat.csv,
    path: '/tmp/batchhash-exports/batchhash-report.csv',
  ),
];

/// True on the desktop platforms where the page offers "open containing
/// folder"; every CI runner is a desktop.
final bool _desktop =
    Platform.isLinux || Platform.isMacOS || Platform.isWindows;

/// Returns fixed export paths so assertions never depend on the fake's
/// timestamped default directory, and records the directory it received.
class _FixedExportService extends FakeBatchHashService {
  _FixedExportService(this.written);

  final List<ExportedReport> written;
  String? lastDirectory;

  @override
  Future<List<ExportedReport>> exportReport({
    required ReportModel report,
    required Set<ReportFormat> formats,
    String? directory,
    String? locale,
  }) async {
    lastDirectory = directory;
    // The page also forwards the active language; keep the parameter so this
    // double stays a faithful implementation, but assert on the directory only:
    // the language path is covered by the tree format service test.
    return written;
  }
}

/// Replaces the clipboard channel so `await Clipboard.setData` completes.
///
/// Without a handler the platform call never returns under `flutter test`, so
/// the code after the await (including the toast) would never run.
List<String> _mockClipboard(WidgetTester tester) {
  // A list, not a captured value: the helper returns before the tap and the
  // caller reads whatever the handler recorded afterwards.
  final List<String> recorded = <String>[];
  tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
    SystemChannels.platform,
    (MethodCall call) async {
      if (call.method == 'Clipboard.setData') {
        final Object? arguments = call.arguments;
        if (arguments is Map<Object?, Object?>) {
          recorded.add('${arguments['text']}');
        }
      }
      return null;
    },
  );
  addTearDown(
    () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      null,
    ),
  );
  return recorded;
}

void main() {
  testWidgets('destination card shows the app default until one is chosen', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1600));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    await tester.pumpWidget(wrapInApp(ReportPage(state: state)));
    await tester.pumpAndSettle();

    expect(find.text('App default folder'), findsOneWidget);

    state.exportDirectory.value = _chosen;
    await tester.pump();

    expect(find.text(_chosen), findsOneWidget);
    expect(find.text('App default folder'), findsNothing);
  });

  testWidgets('export passes the chosen directory and lists every path', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1600));
    final _FixedExportService service = _FixedExportService(_written);
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    state.exportDirectory.value = _chosen;
    await tester.pumpWidget(wrapInApp(ReportPage(state: state)));
    await tester.pumpAndSettle();

    final Finder button = find.byKey(
      const ValueKey<String>('report-export-button'),
    );
    await tester.ensureVisible(button);
    await tester.tap(button);
    await tester.pumpAndSettle();

    expect(service.lastDirectory, _chosen);
    expect(find.text(_written[0].path), findsOneWidget);
    expect(find.text(_written[1].path), findsOneWidget);
    if (_desktop) {
      // Open-folder is keyed to the exported file path, never to report.root.
      expect(
        find.byKey(ValueKey<String>('report-open-folder-${_written[0].path}')),
        findsOneWidget,
      );
    }
  });

  testWidgets('export without a chosen directory still defers to the service', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1600));
    final _FixedExportService service = _FixedExportService(_written);
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    await tester.pumpWidget(wrapInApp(ReportPage(state: state)));
    await tester.pumpAndSettle();

    final Finder button = find.byKey(
      const ValueKey<String>('report-export-button'),
    );
    await tester.ensureVisible(button);
    await tester.tap(button);
    await tester.pumpAndSettle();

    // No explicit folder: the service default is used, and the card keeps the
    // localized default label instead of pretending a folder was chosen.
    expect(service.lastDirectory, isNull);
    expect(find.text('App default folder'), findsOneWidget);
    // The result rows show where the files really landed, not the report root.
    expect(find.text(_written[0].path), findsOneWidget);
    expect(find.text(service.sampleReport.root), findsOneWidget);
  });

  testWidgets('tapping an exported file copies its path', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1600));
    final List<String> copied = _mockClipboard(tester);
    final _FixedExportService service = _FixedExportService(_written);
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    state.exportDirectory.value = _chosen;
    await tester.pumpWidget(wrapInApp(ReportPage(state: state)));
    await tester.pumpAndSettle();

    final Finder button = find.byKey(
      const ValueKey<String>('report-export-button'),
    );
    await tester.ensureVisible(button);
    await tester.tap(button);
    await tester.pumpAndSettle();

    final Finder row = find.byKey(
      ValueKey<String>('report-exported-${_written[0].path}'),
    );
    await tester.ensureVisible(row);
    await tester.tap(row);
    await tester.pumpAndSettle();

    // Asserting the clipboard text is stronger than asserting the toast: it
    // proves the right path was copied, not merely that a SnackBar ran.
    expect(copied, <String>[_written[0].path]);
    expect(find.text('Path copied'), findsOneWidget);
  });

  testWidgets('choosing a folder uses the fallback dialog and persists it', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1600));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    await tester.pumpWidget(wrapInApp(ReportPage(state: state)));
    await tester.pumpAndSettle();

    final Finder choose = find.byKey(
      const ValueKey<String>('report-choose-destination'),
    );
    await tester.ensureVisible(choose);
    await tester.tap(choose);
    await tester.pumpAndSettle();

    // No platform plugin is registered under flutter test, so the page must
    // fall back to its own dialog instead of failing.
    final Finder field = find.byKey(const ValueKey<String>('path-input-field'));
    expect(field, findsOneWidget);
    await tester.enterText(field, '/tmp/report-picked');
    await tester.tap(find.byKey(const ValueKey<String>('path-confirm')));
    await tester.pumpAndSettle();

    expect(state.exportDirectory.value, '/tmp/report-picked');
    expect(find.text('/tmp/report-picked'), findsOneWidget);
  });

  testWidgets('cancelling the chooser keeps the previous destination', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1600));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.report.value = service.sampleReport;
    state.exportDirectory.value = _chosen;
    await tester.pumpWidget(wrapInApp(ReportPage(state: state)));
    await tester.pumpAndSettle();

    final Finder choose = find.byKey(
      const ValueKey<String>('report-choose-destination'),
    );
    await tester.ensureVisible(choose);
    await tester.tap(choose);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('path-cancel')));
    await tester.pumpAndSettle();

    expect(state.exportDirectory.value, _chosen);
    expect(find.text(_chosen), findsOneWidget);
  });
}
