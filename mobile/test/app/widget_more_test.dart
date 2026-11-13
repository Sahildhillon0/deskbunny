// The newer widgets' pictures, the text wrapping they rely on, and the
// settings document the app and the background scheduler share.
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/app/phone_data.dart';
import 'package:nixxa/app/pixel_font.dart';
import 'package:nixxa/app/pixels.dart';
import 'package:nixxa/app/widget_data.dart';
import 'package:nixxa/app/widget_render.dart';
import 'package:nixxa/app/widget_settings.dart';

int lit(Uint8List f) => f.fold<int>(0, (n, b) {
  var c = 0;
  for (var v = b; v != 0; v >>= 1) {
    c += v & 1;
  }
  return n + c;
});

/// Lit pixels inside a box.
int litIn(Uint8List f, int x0, int y0, int x1, int y1) {
  var n = 0;
  for (var y = y0; y < y1; y++) {
    for (var x = x0; x < x1; x++) {
      if (getPx(f, x, y)) n++;
    }
  }
  return n;
}

void main() {
  group('wrapText', () {
    test('breaks at spaces and keeps every line inside the width', () {
      final lines = wrapText('DESIGN REVIEW WITH THE WHOLE TEAM', 126, maxLines: 3);
      // 22 characters would need 131 px; 126 px holds 21, so "THE" goes down.
      expect(lines, ['DESIGN REVIEW WITH', 'THE WHOLE TEAM']);
      expect(lines.every((l) => textWidth(l) <= 126), isTrue);
    });

    test('breaks inside a word that is too long, and marks text that did not fit', () {
      expect(wrapText('SUPERCALIFRAGILISTIC', 60, maxLines: 3), ['SUPERCALIF', 'RAGILISTIC']);
      final cut = wrapText('ONE TWO THREE FOUR FIVE SIX SEVEN', 60, maxLines: 2);
      expect(cut, hasLength(2));
      expect(cut.last, endsWith('..'));
      expect(cut.every((l) => textWidth(l) <= 60), isTrue);
    });

    test('scale narrows the lines, and there is nothing to wrap in nothing', () {
      expect(wrapText('HELLO WORLD', 126, scale: 2, maxLines: 3), ['HELLO', 'WORLD']);
      expect(wrapText('', 126), isEmpty);
      expect(wrapText('ABC', 3), isEmpty, reason: 'not even one letter fits');
    });
  });

  group('small helpers', () {
    test('clockText', () {
      expect(clockText(0), '0:00');
      expect(clockText(83000), '1:23');
      expect(clockText(3599000), '59:59');
      expect(clockText(3661000), '1:01:01');
      expect(clockText(-5), '0:00');
    });

    test('event day labels', () {
      final now = DateTime(2026, 11, 13, 10);
      expect(eventDayLabel(DateTime(2026, 11, 13, 15), now), 'TODAY');
      expect(eventDayLabel(DateTime(2026, 11, 13, 8), now), 'TODAY');
      expect(eventDayLabel(DateTime(2026, 11, 14), now), 'TOMORROW');
      expect(eventDayLabel(DateTime(2026, 11, 18, 9), now), 'WED 18 NOV');
    });
  });

  group('pictures', () {
    test('battery: the fill follows the charge and charging says so', () {
      final low = renderBattery(const PhoneBattery(10, false));
      final high = renderBattery(const PhoneBattery(90, false));
      expect(litIn(high, 18, 7, 98, 22), greaterThan(litIn(low, 18, 7, 98, 22) * 3));
      expect(renderBattery(const PhoneBattery(50, true)), isNot(renderBattery(const PhoneBattery(50, false))));
      for (final p in [0, 1, 50, 100]) {
        final f = renderBattery(PhoneBattery(p, false));
        expect(f.length, frameBytes);
        expect(lit(f), greaterThan(100), reason: '$p %');
      }
    });

    test('event: all-day, timed and empty all differ and stay inside the panel', () {
      final now = DateTime(2026, 11, 13, 10);
      final timed = renderEvent(CalendarEvent('Standup', DateTime(2026, 11, 13, 15, 30)), now);
      final allDay = renderEvent(CalendarEvent('Holiday', DateTime(2026, 11, 14), allDay: true), now);
      final none = renderEvent(null, now);
      expect({timed.hashCode, allDay.hashCode, none.hashCode}.length, 3);
      for (final f in [timed, allDay, none]) {
        expect(lit(f), greaterThan(100));
      }
      // A title in a script the font lacks still gives a picture ("EVENT").
      expect(lit(renderEvent(CalendarEvent('会議', DateTime(2026, 11, 13, 15)), now)), greaterThan(100));
    });

    test('counter: any value and label fits', () {
      for (final v in [0, 7, -7, 42, 12345, -9999, 99999]) {
        final f = renderCounter(v, 'Glasses of water today please');
        expect(lit(f), greaterThan(100), reason: '$v');
      }
      expect(renderCounter(1, 'A'), isNot(renderCounter(2, 'A')));
      expect(lit(renderCounter(5, '')), greaterThan(100), reason: 'an empty label still draws');
    });

    test('status: presets and long text fit inside the frame', () {
      for (final t in [...statusPresets, '', 'GONE FISHING UNTIL FURTHER NOTICE OK', '会議']) {
        final f = renderStatus(t);
        expect(lit(f), greaterThan(150), reason: t);
        // The border is intact: the whole outer ring is lit.
        for (var x = 0; x < oledW; x++) {
          expect(getPx(f, x, 0), isTrue);
          expect(getPx(f, x, oledH - 1), isTrue);
        }
        // Text stays clear of the border.
        expect(litIn(f, 2, 2, oledW - 2, 8), 0, reason: 'nothing in the top margin: $t');
      }
      expect(renderStatus('FOCUS'), isNot(renderStatus('BRB')));
    });

    test('now playing: the bar tracks the position and unknown durations draw no bar', () {
      NowPlaying np(int pos, {int dur = 200000, bool playing = true, String title = 'Song'}) =>
          NowPlaying(title: title, artist: 'Artist', app: 'Spotify', durationMs: dur, positionMs: pos, playing: playing);
      final start = renderNowPlaying(np(0));
      final half = renderNowPlaying(np(100000));
      final end = renderNowPlaying(np(200000));
      final fill = [start, half, end].map((f) => litIn(f, 2, 51, 126, 56)).toList();
      expect(fill[0], lessThan(fill[1]));
      expect(fill[1], lessThan(fill[2]));
      expect(renderNowPlaying(np(999999)), end, reason: 'a position past the end is clamped');

      final live = renderNowPlaying(np(5000, dur: 0));
      expect(litIn(live, 2, 51, 126, 56), 0, reason: 'a stream has no bar');

      expect(renderNowPlaying(np(5000, playing: false)), isNot(renderNowPlaying(np(5000))), reason: 'PAUSED vs NOW PLAYING');
    });

    test('now playing: awkward titles still make a picture on the panel', () {
      for (final t in [
        'Blinding Lights',
        'Bohemian Rhapsody - Remastered 2011 Extended Edition Live At Wembley Stadium',
        'A',
        '',
        '너를 만나', // Hangul: the font cannot draw it
        'Tum Hi Ho (From "Aashiqui 2")',
      ]) {
        final f = renderNowPlaying(NowPlaying(title: t, artist: 'Someone Famous', app: 'YT Music', durationMs: 180000, positionMs: 61000, playing: true));
        expect(f.length, frameBytes);
        expect(lit(f), greaterThan(150), reason: 'title "$t"');
      }
    });
  });

  group('settings document', () {
    test('survives a round trip', () {
      const s = WidgetSettings(
        autoOn: true,
        refreshMins: 60,
        rotateSecs: 300,
        rotateHome: false,
        musicOn: true,
        slots: {'weather': 14, 'nowplaying': 9},
        rotation: {'weather', 'counter'},
        place: GeoPlace('Pune', 18.5, 73.8, region: 'Maharashtra', country: 'India'),
        fahrenheit: true,
        counter: 12,
        counterLabel: 'STREAK',
        statusText: 'FOCUS',
        statusDnd: true,
      );
      final back = WidgetSettings.fromJson(s.toJson());
      expect(back.toJson(), s.toJson());
      expect(back.place, s.place);
    });

    test('bad stored values fall back instead of breaking the app', () {
      expect(WidgetSettings.fromJson(null).toJson(), const WidgetSettings().toJson());
      expect(WidgetSettings.fromJson('junk').autoOn, isFalse);
      final s = WidgetSettings.fromJson({
        'refreshMins': 7, // not one of the choices
        'rotateSecs': 2,
        'slots': {'weather': 99, 'air': -1, 'battery': 5, 3: 4, 'event': 'x'},
        'rotation': ['weather', 5, null, 'air'],
        'counter': 10000000,
      });
      expect(s.refreshMins, 30);
      expect(s.rotateSecs, 60);
      expect(s.slots, {'battery': 5});
      expect(s.rotation, {'weather', 'air'});
      expect(s.counter, 99999);
    });

    test('only widgets that are switched on and have a slot rotate, in the fixed order', () {
      const s = WidgetSettings(
        slots: {'counter': 13, 'weather': 14, 'battery': 12, 'air': 11},
        rotation: {'counter', 'weather', 'battery', 'event'}, // event has no slot, air is not switched on
      );
      expect(s.rotating, ['weather', 'battery', 'counter']);
      expect(const WidgetSettings().rotating, isEmpty);
    });

    test('the status the scheduler reports survives storage', () {
      final st = BgStatus(at: DateTime(2026, 11, 13, 10), lastRefresh: DateTime(2026, 11, 13, 9, 30), showingSlot: 4, homeSlot: 0, error: 'x');
      final back = BgStatus.fromJson(st.toJson());
      expect((back.at, back.lastRefresh, back.showingSlot, back.homeSlot, back.error), (st.at, st.lastRefresh, 4, 0, 'x'));
      expect(BgStatus.fromJson(null).showingSlot, isNull);
    });

    test('face names fit the 12 characters the bunny keeps', () {
      for (final id in [...WidgetIds.rotating, WidgetIds.status, WidgetIds.nowPlaying]) {
        expect(WidgetIds.faceName(id).length, lessThanOrEqualTo(12), reason: id);
      }
    });
  });
}
