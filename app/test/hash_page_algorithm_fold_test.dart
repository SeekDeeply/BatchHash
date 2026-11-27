import 'package:batchhash/src/app_state.dart';
import 'package:batchhash/src/pages/hash_page.dart';
import 'package:batchhash/src/services/fake_batch_hash_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

/// The algorithm row folds once it grows past the chip limit.
///
/// A dozen selected algorithms used to push the run button and the results below
/// the fold, which made the page look like it had no way forward.
void main() {
  testWidgets('a long algorithm row folds and can be expanded', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1400));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    // More than the chip limit, so folding is active.
    state.algorithms.value = <String>{
      'blake3',
      'sha256',
      'md5',
      'sha1',
      'sha512',
      'crc32',
      'xxh3-64',
      'xxh64',
      'xxh32',
      'adler32',
    };

    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    // The expand control is present and the manage chip is never hidden: hiding
    // the only route to the full catalogue would be a trap.
    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-expand')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-manage')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-collapse')),
      findsNothing,
    );

    await tester.tap(find.byKey(const ValueKey<String>('hash-algorithm-expand')));
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-collapse')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-expand')),
      findsNothing,
    );

    await tester.tap(
      find.byKey(const ValueKey<String>('hash-algorithm-collapse')),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-expand')),
      findsOneWidget,
    );
  });

  testWidgets('a short algorithm row does not fold', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1400));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    state.algorithms.value = <String>{'blake3', 'sha256'};

    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    // No fold control when there is nothing to fold: an "expand" chip on a
    // two-chip row would be noise.
    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-expand')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey<String>('hash-algorithm-collapse')),
      findsNothing,
    );
  });

  testWidgets('a selected algorithm stays visible while folded', (
    WidgetTester tester,
  ) async {
    setViewSize(tester, const Size(1000, 1400));
    final FakeBatchHashService service = FakeBatchHashService();
    final BatchHashAppState state = BatchHashAppState(service: service);
    // A single selection far down the catalogue order: if the fold simply took
    // the first N, this chip would be hidden and the tick would look lost even
    // though the selection was intact.
    state.algorithms.value = <String>{'adler32'};

    await tester.pumpWidget(wrapInApp(HashPage(state: state)));
    await tester.pumpAndSettle();

    final Finder selected = find.byWidgetPredicate(
      (Widget widget) => widget is FilterChip && widget.selected,
    );
    expect(selected, findsOneWidget);
    expect(
      tester.widget<FilterChip>(selected).label,
      isA<Text>(),
    );
  });
}
