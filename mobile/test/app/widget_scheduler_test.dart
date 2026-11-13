// The scheduler against a fake bunny, fake phone and a clock the test moves.
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/app/phone_data.dart';
import 'package:nixxa/app/widget_data.dart';
import 'package:nixxa/app/widget_scheduler.dart';
import 'package:nixxa/app/widget_settings.dart';
import 'package:nixxa/link/fake_device_link.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/protocol/bunny.dart';

import '../fakes.dart';

final t0 = DateTime(2026, 11, 13, 10);

class Rig {
  final link = FakeDeviceLink(delay: Duration.zero, paired: true);
  final data = FakeWidgetData();
  final phone = FakePhoneApi();
  final store = MemorySettingsStore();
  late final WidgetScheduler sched;

  Rig() {
    sched = WidgetScheduler(link: link, bunny: Bunny(link), data: data, phone: phone, store: store);
    sched.setConnected(true);
  }

  Future<void> at(Duration d) => sched.tick(t0.add(d));

  /// "put 12" / "act 12" for every slot write and activation since the last [clear].
  List<String> get trace => [
    for (final (op, p) in link.requests)
      if (op == 0x11)
        'put ${p[0]}'
      else if (op == 0x14)
        'act ${p[0]}',
  ];

  void clear() => link.requests.clear();

  WidgetSettings get s => store.settings;
  set s(WidgetSettings v) => store.settings = v;

  Future<void> putFace(int slot, String name) =>
      link.request(0x11, Bunny.encodePutSlot(slot, FaceDef(name: name), null));
}

const song = NowPlaying(title: 'Song A', artist: 'Artist', app: 'Spotify', durationMs: 200000, positionMs: 10000, playing: true);

NowPlaying at(int posMs, {String title = 'Song A', bool playing = true}) =>
    NowPlaying(title: title, artist: 'Artist', app: 'Spotify', durationMs: 200000, positionMs: posMs, playing: playing);

void main() {
  test('switched off, it does not touch the bunny at all', () async {
    final r = Rig();
    await r.at(Duration.zero);
    await r.at(const Duration(minutes: 10));
    expect(r.link.requests, isEmpty);
  });

  test('each widget is pushed once, then only when due and different', () async {
    final r = Rig()
      ..s = const WidgetSettings(
        autoOn: true,
        rotateHome: false,
        slots: {'battery': 12, 'counter': 13},
        rotation: {'battery', 'counter'},
        counter: 5,
      );
    await r.at(Duration.zero);
    expect(r.trace.where((e) => e.startsWith('put')), ['put 12', 'put 13']);

    r.clear();
    await r.at(const Duration(seconds: 10));
    expect(r.trace.where((e) => e.startsWith('put')), isEmpty, reason: 'not due');

    r.phone.batteryNow = const PhoneBattery(63, false);
    r.clear();
    await r.at(const Duration(minutes: 6));
    expect(r.trace.where((e) => e.startsWith('put')), ['put 12'], reason: 'the counter picture did not change');

    r.s = r.s.copyWith(counter: 6);
    r.clear();
    await r.at(const Duration(minutes: 12));
    expect(r.trace.where((e) => e.startsWith('put')), ['put 13'], reason: 'the new count is picked up');
  });

  test('internet widgets refresh on the long timer only', () async {
    final r = Rig()
      ..s = const WidgetSettings(
        autoOn: true,
        refreshMins: 30,
        rotateHome: false,
        slots: {'weather': 14},
        rotation: {'weather'},
        place: GeoPlace('Pune', 18.5, 73.8),
      );
    await r.at(Duration.zero);
    expect(r.data.weatherCalls, 1);
    await r.at(const Duration(minutes: 29));
    expect(r.data.weatherCalls, 1);
    await r.at(const Duration(minutes: 31));
    expect(r.data.weatherCalls, 2);
    expect(r.store.status.lastRefresh, t0.add(const Duration(minutes: 31)));
  });

  test('it rotates through the face and the widgets, then hands the screen back when switched off', () async {
    final r = Rig()
      ..s = const WidgetSettings(
        autoOn: true,
        rotateSecs: 30,
        slots: {'battery': 12, 'counter': 13},
        rotation: {'battery', 'counter'},
      );
    expect(r.link.activeSlot, 0);
    await r.at(Duration.zero);
    expect(r.link.activeSlot, 12, reason: 'the first switch is immediate');
    await r.at(const Duration(seconds: 10));
    expect(r.link.activeSlot, 12);
    await r.at(const Duration(seconds: 30));
    expect(r.link.activeSlot, 13);
    await r.at(const Duration(seconds: 60));
    expect(r.link.activeSlot, 0, reason: 'my face is in the rotation');
    await r.at(const Duration(seconds: 90));
    expect(r.link.activeSlot, 12);
    expect(r.sched.homeSlot, 0);
    expect(r.store.status.homeSlot, 0);

    r.s = r.s.copyWith(autoOn: false);
    await r.at(const Duration(seconds: 95));
    expect(r.link.activeSlot, 0, reason: 'switching off puts my face back');
    r.clear();
    await r.at(const Duration(minutes: 5));
    expect(r.link.requests, isEmpty, reason: 'and then it leaves the bunny alone');
  });

  test('without "show my face too" it cycles the widgets only', () async {
    final r = Rig()
      ..s = const WidgetSettings(
        autoOn: true,
        rotateSecs: 30,
        rotateHome: false,
        slots: {'battery': 12, 'counter': 13},
        rotation: {'battery', 'counter'},
      );
    await r.at(Duration.zero);
    expect(r.link.activeSlot, 12);
    await r.at(const Duration(seconds: 30));
    expect(r.link.activeSlot, 13);
    await r.at(const Duration(seconds: 60));
    expect(r.link.activeSlot, 12);
  });

  test('nothing is sent while the bunny is away, and it carries on when it is back', () async {
    final r = Rig()
      ..s = const WidgetSettings(autoOn: true, rotateHome: false, slots: {'battery': 12}, rotation: {'battery'});
    r.sched.setConnected(false);
    await r.at(Duration.zero);
    expect(r.link.requests, isEmpty);
    r.sched.setConnected(true);
    await r.at(const Duration(seconds: 5));
    expect(r.trace, contains('put 12'));
  });

  test('a face you pick yourself is left alone for a while', () async {
    final r = Rig()
      ..s = const WidgetSettings(
        autoOn: true,
        rotateSecs: 30,
        slots: {'battery': 12, 'counter': 13},
        rotation: {'battery', 'counter'},
      );
    await r.at(Duration.zero);
    expect(r.link.activeSlot, 12);

    await r.putFace(3, 'MINE');
    await r.link.request(0x14, Uint8List.fromList([3]));
    r.clear();
    await r.at(const Duration(seconds: 40));
    expect(r.trace.where((e) => e.startsWith('act')), isEmpty, reason: 'the rotation backs off');
    expect(r.sched.showingSlot, 3);
    await r.at(const Duration(minutes: 5));
    expect(r.link.activeSlot, 3);

    await r.at(const Duration(minutes: 11));
    expect(r.link.activeSlot, 0, reason: 'after ten minutes the rotation resumes');
  });

  group('now playing', () {
    Rig musicRig({bool auto = false}) => Rig()
      ..s = WidgetSettings(
        musicOn: true,
        autoOn: auto,
        rotateSecs: 30,
        slots: {'nowplaying': 9, if (auto) 'battery': 12},
        rotation: {if (auto) 'battery'},
      );

    test('a song that starts goes on the bunny at once, and its progress moves every ten seconds', () async {
      final r = musicRig()..phone.song = song;
      await r.at(Duration.zero);
      expect(r.trace, ['put 9', 'act 9']);
      expect(r.link.activeSlot, 9);
      expect(r.sched.homeSlot, 0);

      r.phone.song = at(15000);
      r.clear();
      await r.at(const Duration(seconds: 5));
      expect(r.link.requests, isEmpty, reason: 'too soon for another redraw');

      r.phone.song = at(20000);
      await r.at(const Duration(seconds: 10));
      expect(r.trace, ['put 9'], reason: 'redrawn in place, no need to switch');
    });

    test('a new song, a pause and a resume are redrawn at once', () async {
      final r = musicRig()..phone.song = song;
      await r.at(Duration.zero);

      r.phone.song = at(1000, title: 'Song B');
      r.clear();
      await r.at(const Duration(seconds: 2));
      expect(r.trace, ['put 9'], reason: 'new song');

      r.phone.song = at(3000, title: 'Song B', playing: false);
      r.clear();
      await r.at(const Duration(seconds: 4));
      expect(r.trace, ['put 9'], reason: 'paused');

      r.phone.song = at(3000, title: 'Song B');
      r.clear();
      await r.at(const Duration(seconds: 6));
      expect(r.trace, ['put 9'], reason: 'resumed');
      expect(r.link.activeSlot, 9);
    });

    test('a paused song lets go after fifteen seconds and the face comes back', () async {
      final r = musicRig()..phone.song = song;
      await r.at(Duration.zero);
      r.phone.song = at(5000, playing: false);
      await r.at(const Duration(seconds: 5));
      expect(r.link.activeSlot, 9, reason: 'shows PAUSED for a moment');
      await r.at(const Duration(seconds: 15));
      expect(r.link.activeSlot, 9);
      await r.at(const Duration(seconds: 21));
      expect(r.link.activeSlot, 0);
    });

    test('when the player closes, the face comes back at once', () async {
      final r = musicRig()..phone.song = song;
      await r.at(Duration.zero);
      r.phone.song = null;
      await r.at(const Duration(seconds: 3));
      expect(r.link.activeSlot, 0);
    });

    test('while a song plays the rotation stays away; afterwards it takes over', () async {
      final r = musicRig(auto: true)..phone.song = song;
      await r.at(Duration.zero);
      expect(r.link.activeSlot, 9);
      r.clear();
      for (final s in [30, 60, 90, 120]) {
        r.phone.song = at(s * 1000);
        await r.at(Duration(seconds: s));
      }
      expect(r.trace.where((e) => e.startsWith('act')), isEmpty, reason: 'rotation held while it plays');
      expect(r.link.activeSlot, 9);

      r.phone.song = null;
      await r.at(const Duration(seconds: 125));
      expect(r.link.activeSlot, isNot(9), reason: 'rotation is back');
    });

    test('without notification access it says so and sends nothing', () async {
      final r = musicRig()
        ..phone.songFails = 'NO_ACCESS'
        ..phone.song = song;
      await r.at(Duration.zero);
      expect(r.trace, isEmpty);
      expect(r.store.status.error, contains('Now playing'));
    });

    test('switching it off while it shows gives the face back', () async {
      final r = musicRig()..phone.song = song;
      await r.at(Duration.zero);
      r.s = r.s.copyWith(musicOn: false);
      await r.at(const Duration(seconds: 5));
      expect(r.link.activeSlot, 0);
    });
  });

  test('a failed update is reported, retried after two minutes, and the report clears', () async {
    final r = Rig()
      ..s = const WidgetSettings(
        autoOn: true,
        rotateHome: false,
        slots: {'weather': 14},
        rotation: {'weather'},
        place: GeoPlace('Pune', 18.5, 73.8),
      )
      ..data.failWith = 'No internet connection.';
    await r.at(Duration.zero);
    expect(r.trace, isEmpty);
    expect(r.store.status.error, 'WEATHER: No internet connection.');
    expect(r.data.weatherCalls, 1);

    r.data.failWith = null;
    await r.at(const Duration(minutes: 1));
    expect(r.data.weatherCalls, 1, reason: 'not yet');
    await r.at(const Duration(minutes: 2));
    expect(r.data.weatherCalls, 2);
    expect(r.trace, ['put 14']);
    expect(r.store.status.error, isNull);
  });

  test('a tick that lands during a pass is not lost, and never runs two passes at once', () async {
    final r = Rig()
      ..s = const WidgetSettings(autoOn: true, rotateHome: false, slots: {'battery': 12}, rotation: {'battery'});
    await Future.wait([r.at(Duration.zero), r.at(Duration.zero), r.at(Duration.zero)]);
    expect(r.trace.where((e) => e == 'put 12'), hasLength(1));
    expect(r.phone.batteryCalls, lessThanOrEqualTo(2));
  });
}
