import 'package:flutter_hbb/desktop/pages/monitor_wall_page.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('tile limit is enforced and duplicates are refused', () {
    final t = WallTiles(2);
    expect(t.add('a'), isTrue);
    expect(t.add('a'), isFalse, reason: 'already watching');
    expect(t.add('b'), isTrue);
    expect(t.full, isTrue);
    expect(t.add('c'), isFalse, reason: 'wall is full');
    t.remove('a');
    expect(t.full, isFalse);
    expect(t.add('c'), isTrue);
    expect(t.deviceIds, ['b', 'c']);
  });

  test('grid columns grow with tile count', () {
    final t = WallTiles(6);
    expect(t.columns(), 1);
    t.add('1');
    expect(t.columns(), 1);
    t.add('2');
    expect(t.columns(), 2);
    t.add('3');
    t.add('4');
    expect(t.columns(), 2);
    t.add('5');
    expect(t.columns(), 3);
  });

  test('active session list parses the spec shape and surfaces errors', () {
    final list = ActiveSession.parse('''{"sessions":[
      {"session_id":"s1","device_id":"999","device_name":"Lab","peer_id":"111",
       "display_name":"Ada","role":"manager","status":"active",
       "elapsed_seconds":1830,"remaining_seconds":12570},
      {"session_id":"s2","device_id":"998","peer_id":"222","role":"user","status":"stale",
       "elapsed_seconds":5}
    ]}''');
    expect(list.length, 2);
    expect(list[0].deviceName, 'Lab');
    expect(list[0].remainingSeconds, 12570);
    expect(list[1].deviceName, '');
    expect(list[1].remainingSeconds, isNull);
    expect(list[1].status, 'stale');
    expect(() => ActiveSession.parse('{"error":"nope"}'), throwsException);
    expect(ActiveSession.parse('{"sessions":[]}'), isEmpty);
  });
}
