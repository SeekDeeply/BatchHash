import 'dart:convert';
import 'dart:io';

import 'package:batchhash/src/app.dart';
import 'package:batchhash/src/app_info.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:batchhash/src/services/first_run_store.dart';
import 'package:batchhash/src/widgets/first_run_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// In-memory stand-in: plan the widget tests so they never await real file I/O,
/// which the widget-test binding's fake clock does not drive.
class _FakeFirstRunStore extends FirstRunStore {
  _FakeFirstRunStore({this.accepted = false});

  bool accepted;

  @override
  Future<bool> hasAccepted() async => accepted;

  @override
  Future<void> markAccepted() async {
    accepted = true;
  }
}

void main() {
  late Directory temp;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('batchhash-first-run-test');
  });

  tearDown(() async {
    if (await temp.exists()) {
      await temp.delete(recursive: true);
    }
  });

  FirstRunStore storeOnDisk() =>
      FirstRunStore(supportDirectory: () async => temp);

  test('an unaccepted install asks; acceptance survives a restart', () async {
    expect(await storeOnDisk().hasAccepted(), isFalse);

    await storeOnDisk().markAccepted();

    // A new instance models the next launch.
    expect(await storeOnDisk().hasAccepted(), isTrue);

    final Object? decoded = jsonDecode(
      await File('${temp.path}/first_run.json').readAsString(),
    );
    expect(decoded, isA<Map<String, Object?>>());
    final Map<String, Object?> record = decoded! as Map<String, Object?>;
    expect(record['accepted'], isTrue);
    expect(record['version'], kAppVersion);
    expect(record['accepted_at'], isA<String>());
  });

  test('a malformed record is treated as not accepted', () async {
    await File('${temp.path}/first_run.json').writeAsString('{not json');
    expect(await storeOnDisk().hasAccepted(), isFalse);
  });

  test('an unavailable support directory degrades to not accepted', () async {
    final FirstRunStore store = FirstRunStore(
      supportDirectory: () async => throw const FileSystemException('no dir'),
    );
    // Reading must not throw and must not claim acceptance.
    expect(await store.hasAccepted(), isFalse);
    // Marking is best effort, never fatal.
    await store.markAccepted();
  });

  testWidgets('an injected service never auto-opens the first-run gate', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(500, 900));
    await tester.pumpWidget(
      BatchHashApp(
        service: FakeBatchHashService(),
        firstRunStore: _FakeFirstRunStore(),
        enableDynamicColor: false,
        locale: 'en',
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byType(FirstRunDialog), findsNothing);
    expect(
      find.byKey(const ValueKey<String>('first-run-dialog')),
      findsNothing,
    );
  });

  testWidgets('the real wiring gates the app until the terms are accepted', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1200));
    final _FakeFirstRunStore store = _FakeFirstRunStore();
    await tester.pumpWidget(
      BatchHashApp(
        firstRunStore: store,
        onDecline: () {},
        enableDynamicColor: false,
        locale: 'en',
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey<String>('first-run-dialog')),
      findsOneWidget,
    );

    await tester.tap(find.byKey(const ValueKey<String>('first-run-accept')));
    await tester.pumpAndSettle();

    expect(store.accepted, isTrue);
    expect(
      find.byKey(const ValueKey<String>('first-run-dialog')),
      findsNothing,
    );
  });

  testWidgets('declining runs the injected callback and closes the dialog', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1200));
    bool declined = false;
    final _FakeFirstRunStore store = _FakeFirstRunStore();
    await tester.pumpWidget(
      BatchHashApp(
        firstRunStore: store,
        onDecline: () => declined = true,
        enableDynamicColor: false,
        locale: 'en',
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey<String>('first-run-decline')));
    await tester.pumpAndSettle();

    expect(declined, isTrue);
    expect(store.accepted, isFalse);
    expect(
      find.byKey(const ValueKey<String>('first-run-dialog')),
      findsNothing,
    );
  });

  testWidgets('the dialog scrolls through the terms and offers both choices', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(800, 1200));
    bool accepted = false;
    await tester.pumpWidget(
      wrapInApp(
        Builder(
          builder: (BuildContext context) => TextButton(
            onPressed: () => showDialog<void>(
              context: context,
              builder: (BuildContext dialogContext) => FirstRunDialog(
                onAccept: () => accepted = true,
                onDecline: () {},
              ),
            ),
            child: const Text('open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();

    final Finder dialog = find.byKey(
      const ValueKey<String>('first-run-dialog'),
    );
    expect(dialog, findsOneWidget);
    expect(find.text('Welcome to BatchHash'), findsOneWidget);
    expect(
      find.descendant(
        of: dialog,
        matching: find.byType(SingleChildScrollView),
      ),
      findsOneWidget,
    );
    // The embedded legal copy runs to the license section.
    expect(find.text('Open source license'), findsOneWidget);
    expect(
      find.byKey(const ValueKey<String>('first-run-open-github')),
      findsOneWidget,
    );

    await tester.tap(find.byKey(const ValueKey<String>('first-run-accept')));
    await tester.pumpAndSettle();

    expect(accepted, isTrue);
    expect(dialog, findsNothing);
  });
}
