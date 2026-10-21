import 'dart:async';

import 'package:get/get.dart';

/// Elapsed and remaining time of the current session, as reported by the
/// controlled device after every access-control heartbeat (RustDesk-Velour).
///
/// Between reports the model ticks locally once a second so the display looks
/// live. Milestone warnings fire once each as the remaining time crosses them.
/// The model is plain Dart (no FFI) so it is unit-testable.
class SessionTimeModel {
  /// Seconds since the session was approved. Negative = no report yet.
  final elapsed = (-1).obs;

  /// Seconds left before the backend ends the session; null = unlimited.
  final remaining = Rxn<int>();

  /// Countdown turns red at or below this many seconds.
  int redBelowSecs;

  /// Remaining-time thresholds (seconds) that trigger a one-off warning.
  final List<int> milestones;

  /// Called with the remaining seconds when a milestone is crossed.
  void Function(int remainingSecs)? onMilestone;

  final Set<int> _fired = {};
  Timer? _ticker;

  SessionTimeModel({
    this.redBelowSecs = 30 * 60,
    List<int>? milestones,
    this.onMilestone,
  }) : milestones = milestones ?? defaultMilestones;

  /// 4:00, 3:30, … 0:30, then 10, 5 and 1 minute.
  static final List<int> defaultMilestones = [
    for (var m = 240; m >= 30; m -= 30) m * 60,
    10 * 60,
    5 * 60,
    60,
  ];

  bool get active => elapsed.value >= 0;

  /// Whether the countdown is on screen: whenever the time left is known,
  /// from the moment the session starts. It used to appear only under 4 h,
  /// with a stopwatch counting up before that, which read as a timer going the
  /// wrong way (Velour owner, 2026-10-21).
  bool get showCountdown => active && remaining.value != null;

  /// The session has no limit: say so rather than show a stopwatch.
  bool get unlimited => active && remaining.value == null;

  bool get isRed =>
      remaining.value != null && remaining.value! <= redBelowSecs;

  /// A report from the device. `remainingSecs` null means unlimited.
  void update({required int elapsedSecs, required int? remainingSecs}) {
    elapsed.value = elapsedSecs;
    _setRemaining(remainingSecs);
    _ticker ??= Timer.periodic(const Duration(seconds: 1), (_) => _tick());
  }

  void _tick() {
    if (!active) return;
    elapsed.value = elapsed.value + 1;
    final r = remaining.value;
    if (r != null && r > 0) _setRemaining(r - 1);
  }

  void _setRemaining(int? value) {
    final previous = remaining.value;
    remaining.value = value;
    if (value == null) return;
    // One warning per report even if several milestones were skipped over.
    var crossedAny = false;
    for (final m in milestones) {
      final crossed = value <= m && (previous == null || previous > m);
      if (crossed && _fired.add(m)) crossedAny = true;
    }
    if (crossedAny) onMilestone?.call(value);
  }

  void reset() {
    _ticker?.cancel();
    _ticker = null;
    elapsed.value = -1;
    remaining.value = null;
    _fired.clear();
  }

  void dispose() => reset();

  static String format(int secs) {
    if (secs < 0) secs = 0;
    final h = secs ~/ 3600;
    final m = (secs % 3600) ~/ 60;
    final s = secs % 60;
    String two(int v) => v.toString().padLeft(2, '0');
    return '${two(h)}:${two(m)}:${two(s)}';
  }
}
