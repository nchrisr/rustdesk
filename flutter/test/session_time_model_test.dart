import 'package:fake_async/fake_async.dart';
import 'package:flutter_hbb/models/session_time_model.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('inactive until the first report; unlimited hides the countdown', () {
    final m = SessionTimeModel();
    expect(m.active, isFalse);
    expect(m.showCountdown, isFalse);
    m.update(elapsedSecs: 10, remainingSecs: null);
    expect(m.active, isTrue);
    expect(m.showCountdown, isFalse);
  });

  test('countdown shows only at or below the threshold and goes red', () {
    final m = SessionTimeModel(showBelowSecs: 4 * 3600, redBelowSecs: 1800);
    m.update(elapsedSecs: 0, remainingSecs: 5 * 3600);
    expect(m.showCountdown, isFalse);
    m.update(elapsedSecs: 30, remainingSecs: 4 * 3600);
    expect(m.showCountdown, isTrue);
    expect(m.isRed, isFalse);
    m.update(elapsedSecs: 60, remainingSecs: 1800);
    expect(m.isRed, isTrue);
  });

  test('ticks locally between reports and never goes below zero', () {
    fakeAsync((async) {
      final m = SessionTimeModel();
      m.update(elapsedSecs: 100, remainingSecs: 3);
      async.elapse(const Duration(seconds: 5));
      expect(m.elapsed.value, 105);
      expect(m.remaining.value, 0);
      m.dispose();
    });
  });

  test('a report resyncs the local count', () {
    fakeAsync((async) {
      final m = SessionTimeModel();
      m.update(elapsedSecs: 0, remainingSecs: 600);
      async.elapse(const Duration(seconds: 30));
      m.update(elapsedSecs: 28, remainingSecs: 572); // device is authoritative
      expect(m.elapsed.value, 28);
      expect(m.remaining.value, 572);
      m.dispose();
    });
  });

  test('each milestone fires once, including when several are skipped', () {
    final fired = <int>[];
    final m = SessionTimeModel(
      milestones: [600, 300, 60],
      onMilestone: fired.add,
    );
    m.update(elapsedSecs: 0, remainingSecs: 700);
    expect(fired, isEmpty);
    m.update(elapsedSecs: 1, remainingSecs: 600);
    expect(fired, [600]);
    m.update(elapsedSecs: 2, remainingSecs: 599);
    expect(fired, [600], reason: 'no repeat while still under 600');
    m.update(elapsedSecs: 3, remainingSecs: 50); // skips 300, lands under 60
    expect(fired, [600, 50]);
    m.update(elapsedSecs: 4, remainingSecs: 40);
    expect(fired, [600, 50], reason: '300 and 60 already consumed');
  });

  test('format is hh:mm:ss', () {
    expect(SessionTimeModel.format(0), '00:00:00');
    expect(SessionTimeModel.format(59), '00:00:59');
    expect(SessionTimeModel.format(3661), '01:01:01');
    expect(SessionTimeModel.format(4 * 3600), '04:00:00');
    expect(SessionTimeModel.format(-5), '00:00:00');
  });
}
