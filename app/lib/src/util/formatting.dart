const List<String> _byteUnits = <String>['B', 'KiB', 'MiB', 'GiB', 'TiB'];

/// Formats a byte count with binary units and one decimal place.
String formatBytes(int bytes) {
  if (bytes <= 0) {
    return '0 B';
  }
  if (bytes < 1024) {
    return '$bytes B';
  }
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1024 && unit < _byteUnits.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return '${value.toStringAsFixed(1)} ${_byteUnits[unit]}';
}

/// Formats a timestamp as a compact local date and time.
String formatTimestamp(DateTime? value) {
  if (value == null) {
    return 'unknown';
  }
  final DateTime local = value.toLocal();
  final String month = local.month.toString().padLeft(2, '0');
  final String day = local.day.toString().padLeft(2, '0');
  final String hour = local.hour.toString().padLeft(2, '0');
  final String minute = local.minute.toString().padLeft(2, '0');
  return '${local.year}-$month-$day $hour:$minute';
}

/// Formats a probability as a percentage suitable for the rename estimate.
String formatProbabilityPercent(double fraction) {
  final double percent = fraction * 100;
  if (percent <= 0) {
    return '0%';
  }
  if (percent < 0.01) {
    return '< 0.01%';
  }
  return '${percent.toStringAsFixed(2)}%';
}
