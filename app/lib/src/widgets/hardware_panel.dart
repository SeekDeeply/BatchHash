import 'package:flutter/material.dart';

import '../models/models.dart';
import '../services/batch_hash_service.dart';

/// Two independently labeled acceleration sections.
///
/// The CPU section reflects the Rust hardware probe, the GPU section reflects
/// the platform rendering probe. Both render a state icon, a neutral label and
/// a state color so meaning is never carried by color alone.
class HardwarePanel extends StatelessWidget {
  const HardwarePanel({
    super.key,
    required this.hashInfo,
    required this.renderingInfo,
  });

  final HardwareInfoModel? hashInfo;
  final RenderingHardwareModel? renderingInfo;

  static AccelerationState hashState(HardwareInfoModel? info) {
    if (info == null) {
      return AccelerationState.unknown;
    }
    final List<String> backends = <String>[
      info.blake3BackendReported.toLowerCase(),
      info.sha256BackendReported.toLowerCase(),
    ];
    final bool allUnknown = backends.every(
      (String backend) => backend.isEmpty || backend == 'unknown',
    );
    if (allUnknown) {
      return AccelerationState.unknown;
    }
    final bool anySoftware = backends.any(
      (String backend) =>
          backend == 'software' ||
          backend == 'fallback' ||
          backend == 'scalar',
    );
    if (anySoftware) {
      return AccelerationState.softwareFallback;
    }
    return AccelerationState.detected;
  }

  static AccelerationState renderingState(RenderingHardwareModel? info) {
    if (info == null) {
      return AccelerationState.unknown;
    }
    final String backend = info.gpuBackend.toLowerCase();
    if (backend.isEmpty || backend == 'unknown') {
      return AccelerationState.unknown;
    }
    return info.accelerated
        ? AccelerationState.detected
        : AccelerationState.softwareFallback;
  }

  static IconData stateIcon(AccelerationState state) => switch (state) {
    AccelerationState.detected => Icons.bolt,
    AccelerationState.softwareFallback => Icons.info_outline,
    AccelerationState.unknown => Icons.help_outline,
  };

  static String stateLabel(AccelerationState state) => switch (state) {
    AccelerationState.detected => 'Acceleration detected',
    AccelerationState.softwareFallback => 'Software fallback in use',
    AccelerationState.unknown => 'Acceleration status unknown',
  };

  static Color stateAccentColor(AccelerationState state, ColorScheme scheme) =>
      switch (state) {
        AccelerationState.detected => scheme.tertiary,
        AccelerationState.softwareFallback => scheme.onSurfaceVariant,
        AccelerationState.unknown => scheme.outline,
      };

  static Color stateContainerColor(
    AccelerationState state,
    ColorScheme scheme,
  ) =>
      switch (state) {
        AccelerationState.detected => scheme.tertiaryContainer,
        AccelerationState.softwareFallback => scheme.surfaceContainerHighest,
        AccelerationState.unknown => scheme.surface,
      };

  static Color stateBorderColor(AccelerationState state, ColorScheme scheme) =>
      switch (state) {
        AccelerationState.detected => scheme.tertiary,
        AccelerationState.softwareFallback => scheme.surfaceContainerHighest,
        AccelerationState.unknown => scheme.outline,
      };

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final HardwareInfoModel? hash = hashInfo;
    final RenderingHardwareModel? rendering = renderingInfo;
    final List<Widget> cpuRows = hash == null
        ? const <Widget>[]
        : <Widget>[
            _InfoRow(label: 'Architecture', value: hash.arch),
            _InfoRow(
              label: 'Detected features',
              value: hash.detectedFeatures.isEmpty
                  ? 'none reported'
                  : hash.detectedFeatures.join(', '),
            ),
            _InfoRow(label: 'BLAKE3 backend', value: hash.blake3BackendReported),
            _InfoRow(label: 'SHA-256 backend', value: hash.sha256BackendReported),
            _InfoRow(label: 'Backend policy', value: hash.backendPolicy),
            _InfoRow(
              label: 'Multithreading',
              value: hash.multithread ? 'enabled' : 'disabled',
            ),
          ];
    final List<Widget> gpuRows = rendering == null
        ? const <Widget>[]
        : <Widget>[
            _InfoRow(label: 'GPU backend', value: rendering.gpuBackend),
          ];
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text('Hardware', style: theme.textTheme.titleLarge),
            const SizedBox(height: 4),
            Text(
              'How BatchHash reports acceleration in this environment.',
              style: theme.textTheme.bodySmall,
            ),
            const SizedBox(height: 16),
            _HardwareSection(
              key: const ValueKey<String>('hardware-cpu-section'),
              title: 'Hash acceleration (CPU)',
              state: hashState(hash),
              iconKey: const ValueKey<String>('hardware-cpu-icon'),
              labelKey: const ValueKey<String>('hardware-cpu-state-label'),
              containerKey:
                  const ValueKey<String>('hardware-cpu-state-container'),
              rows: cpuRows,
            ),
            const SizedBox(height: 12),
            _HardwareSection(
              key: const ValueKey<String>('hardware-gpu-section'),
              title: 'UI rendering acceleration (GPU)',
              state: renderingState(rendering),
              iconKey: const ValueKey<String>('hardware-gpu-icon'),
              labelKey: const ValueKey<String>('hardware-gpu-state-label'),
              containerKey:
                  const ValueKey<String>('hardware-gpu-state-container'),
              rows: gpuRows,
            ),
          ],
        ),
      ),
    );
  }
}

/// Loads both hardware probes and renders a [HardwarePanel].
class HardwarePanelLoader extends StatefulWidget {
  const HardwarePanelLoader({super.key, required this.service});

  final BatchHashService service;

  @override
  State<HardwarePanelLoader> createState() => _HardwarePanelLoaderState();
}

class _HardwarePanelLoaderState extends State<HardwarePanelLoader> {
  HardwareInfoModel? _hashInfo;
  RenderingHardwareModel? _renderingInfo;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final HardwareInfoModel hashInfo = await widget.service.probeHardware();
      final RenderingHardwareModel renderingInfo =
          await widget.service.probeRenderingHardware();
      if (!mounted) {
        return;
      }
      setState(() {
        _hashInfo = hashInfo;
        _renderingInfo = renderingInfo;
        _loading = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _loading = false;
        _error = 'Hardware probe failed: $error';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Padding(
        padding: EdgeInsets.all(16),
        child: Row(
          children: <Widget>[
            SizedBox(
              width: 20,
              height: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
            SizedBox(width: 12),
            Text('Probing hardware...'),
          ],
        ),
      );
    }
    final String? error = _error;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        HardwarePanel(hashInfo: _hashInfo, renderingInfo: _renderingInfo),
        if (error != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(error, style: Theme.of(context).textTheme.bodySmall),
          ),
      ],
    );
  }
}

class _HardwareSection extends StatelessWidget {
  const _HardwareSection({
    super.key,
    required this.title,
    required this.state,
    required this.iconKey,
    required this.labelKey,
    required this.containerKey,
    required this.rows,
  });

  final String title;
  final AccelerationState state;
  final Key iconKey;
  final Key labelKey;
  final Key containerKey;
  final List<Widget> rows;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme scheme = theme.colorScheme;
    return Container(
      key: containerKey,
      decoration: BoxDecoration(
        color: HardwarePanel.stateContainerColor(state, scheme),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: HardwarePanel.stateBorderColor(state, scheme),
        ),
      ),
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Row(
            children: <Widget>[
              Icon(
                HardwarePanel.stateIcon(state),
                key: iconKey,
                color: HardwarePanel.stateAccentColor(state, scheme),
                semanticLabel: HardwarePanel.stateLabel(state),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(title, style: theme.textTheme.titleSmall),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text(
            HardwarePanel.stateLabel(state),
            key: labelKey,
            style: theme.textTheme.bodyMedium?.copyWith(
              color: HardwarePanel.stateAccentColor(state, scheme),
            ),
          ),
          if (rows.isNotEmpty) ...<Widget>[
            const SizedBox(height: 8),
            ...rows,
          ],
        ],
      ),
    );
  }
}

class _InfoRow extends StatelessWidget {
  const _InfoRow({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          SizedBox(
            width: 132,
            child: Text(
              label,
              style: theme.textTheme.bodySmall
                  ?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
          ),
          Expanded(
            child: Text(value, style: theme.textTheme.bodySmall),
          ),
        ],
      ),
    );
  }
}
