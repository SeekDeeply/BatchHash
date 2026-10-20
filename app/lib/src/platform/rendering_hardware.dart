import '../models/models.dart';

/// Source of rendering (GPU) acceleration information.
///
/// The real platform channel implementation lands with P5; until then the
/// default provider reports an unknown backend so the UI stays honest.
abstract class RenderingHardwareProvider {
  Future<RenderingHardwareModel> probe();
}

/// Default provider used until a real platform channel exists.
class UnknownRenderingHardwareProvider implements RenderingHardwareProvider {
  const UnknownRenderingHardwareProvider();

  @override
  Future<RenderingHardwareModel> probe() async {
    return const RenderingHardwareModel(accelerated: false, gpuBackend: 'unknown');
  }
}
