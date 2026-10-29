import 'package:batchhash/src/models/models.dart';
import 'package:batchhash/src/theme.dart';
import 'package:batchhash/src/widgets/hardware_panel.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/test_helpers.dart';

Future<void> pumpPanel(
  WidgetTester tester, {
  required HardwareInfoModel? hashInfo,
  required RenderingHardwareModel? renderingInfo,
}) async {
  setViewSize(tester, const Size(500, 900));
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.light(null),
      home: Scaffold(
        body: SingleChildScrollView(
          child: HardwarePanel(
            hashInfo: hashInfo,
            renderingInfo: renderingInfo,
          ),
        ),
      ),
    ),
  );
}

ColorScheme schemeOf(WidgetTester tester) {
  return Theme.of(tester.element(find.byType(HardwarePanel))).colorScheme;
}

String stateLabelOf(WidgetTester tester, String side) {
  final Text text = tester.widget<Text>(
    find.byKey(ValueKey<String>('hardware-$side-state-label')),
  );
  return text.data ?? '';
}

void expectSectionsDistinct(WidgetTester tester) {
  expect(
    find.byKey(const ValueKey<String>('hardware-cpu-section')),
    findsOneWidget,
  );
  expect(
    find.byKey(const ValueKey<String>('hardware-gpu-section')),
    findsOneWidget,
  );
  expect(find.text('Hash acceleration (CPU)'), findsOneWidget);
  expect(find.text('UI rendering acceleration (GPU)'), findsOneWidget);
}

void main() {
  testWidgets('detected state uses the lightning icon and tertiary accent', (
    WidgetTester tester,
  ) async {
    await pumpPanel(
      tester,
      hashInfo: const HardwareInfoModel(
        arch: 'x86_64',
        detectedFeatures: <String>['sse2', 'avx2', 'sha_ni'],
        blake3BackendReported: 'avx2',
        sha256BackendReported: 'sha_ni',
        backendPolicy: 'auto',
        multithread: true,
      ),
      renderingInfo: const RenderingHardwareModel(
        accelerated: true,
        gpuBackend: 'vulkan',
      ),
    );
    final ColorScheme scheme = schemeOf(tester);

    expectSectionsDistinct(tester);
    final Icon cpuIcon = tester.widget<Icon>(
      find.byKey(const ValueKey<String>('hardware-cpu-icon')),
    );
    final Icon gpuIcon = tester.widget<Icon>(
      find.byKey(const ValueKey<String>('hardware-gpu-icon')),
    );
    expect(cpuIcon.icon, Icons.bolt);
    expect(gpuIcon.icon, Icons.bolt);
    expect(cpuIcon.color, scheme.tertiary);
    expect(gpuIcon.color, scheme.tertiary);
    expect(stateLabelOf(tester, 'cpu'), 'Acceleration detected');
    expect(stateLabelOf(tester, 'gpu'), 'Acceleration detected');
  });

  testWidgets('software fallback uses info icon and surfaceVariant color', (
    WidgetTester tester,
  ) async {
    await pumpPanel(
      tester,
      hashInfo: const HardwareInfoModel(
        arch: 'x86_64',
        blake3BackendReported: 'software',
        sha256BackendReported: 'software',
        backendPolicy: 'forced-software',
      ),
      renderingInfo: const RenderingHardwareModel(
        accelerated: false,
        gpuBackend: 'software',
      ),
    );
    final ColorScheme scheme = schemeOf(tester);

    expectSectionsDistinct(tester);
    final Icon cpuIcon = tester.widget<Icon>(
      find.byKey(const ValueKey<String>('hardware-cpu-icon')),
    );
    final Icon gpuIcon = tester.widget<Icon>(
      find.byKey(const ValueKey<String>('hardware-gpu-icon')),
    );
    expect(cpuIcon.icon, Icons.info_outline);
    expect(gpuIcon.icon, Icons.info_outline);
    final Container cpuContainer = tester.widget<Container>(
      find.byKey(const ValueKey<String>('hardware-cpu-state-container')),
    );
    final BoxDecoration decoration = cpuContainer.decoration! as BoxDecoration;
    expect(decoration.color, scheme.surfaceContainerHighest);
    expect(stateLabelOf(tester, 'cpu'), 'Software fallback in use');
    expect(stateLabelOf(tester, 'gpu'), 'Software fallback in use');
  });

  testWidgets('unknown state uses question icon and outline color', (
    WidgetTester tester,
  ) async {
    await pumpPanel(tester, hashInfo: null, renderingInfo: null);
    final ColorScheme scheme = schemeOf(tester);

    expectSectionsDistinct(tester);
    final Icon cpuIcon = tester.widget<Icon>(
      find.byKey(const ValueKey<String>('hardware-cpu-icon')),
    );
    final Icon gpuIcon = tester.widget<Icon>(
      find.byKey(const ValueKey<String>('hardware-gpu-icon')),
    );
    expect(cpuIcon.icon, Icons.help_outline);
    expect(gpuIcon.icon, Icons.help_outline);
    expect(cpuIcon.color, scheme.outline);
    expect(gpuIcon.color, scheme.outline);
    expect(stateLabelOf(tester, 'cpu'), 'Acceleration status unknown');
    expect(stateLabelOf(tester, 'gpu'), 'Acceleration status unknown');
  });
}
