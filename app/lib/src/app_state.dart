import 'package:flutter/foundation.dart';

import 'models/models.dart';
import 'services/batch_hash_service.dart';

/// Shared state owned by the app shell and passed to the pages.
class BatchHashAppState {
  BatchHashAppState({required this.service});

  final BatchHashService service;
  final ValueNotifier<List<BatchEntry>> entries =
      ValueNotifier<List<BatchEntry>>(const <BatchEntry>[]);
  final ValueNotifier<ReportModel?> report =
      ValueNotifier<ReportModel?>(null);

  void dispose() {
    entries.dispose();
    report.dispose();
  }
}
