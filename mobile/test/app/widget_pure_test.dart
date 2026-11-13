// The pure parts of the weather and air-quality widgets: the pixel font, the
// renderer and the Open-Meteo client (against a fake fetcher, no network).
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/app/pixel_font.dart';
import 'package:nixxa/app/pixels.dart';
import 'package:nixxa/app/widget_data.dart';
import 'package:nixxa/app/widget_render.dart';

int lit(Uint8List f) {
  var n = 0;
  for (final b in f) {
    for (var v = b; v != 0; v >>= 1) {
      n += v & 1;
    }
  }
  return n;
}

/// Bounding box of the lit pixels inside a region.
(int, int, int, int) bounds(Uint8List f, {int x0 = 0, int y0 = 0, int x1 = oledW, int y1 = oledH}) {
  var l = oledW, t = oledH, r = -1, b = -1;
  for (var y = y0; y < y1; y++) {
    for (var x = x0; x < x1; x++) {
      if (getPx(f, x, y)) {
        if (x < l) l = x;
        if (x > r) r = x;
        if (y < t) t = y;
        if (y > b) b = y;
      }
    }
  }
  return (l, t, r, b);
}

void main() {
  group('pixel font', () {
    test('fontSafe upper-cases, drops accents and unknown characters', () {
      expect(fontSafe('São Paulo'), 'SAO PAULO');
      expect(fontSafe('Zürich'), 'ZURICH');
      expect(fontSafe('東京 Tokyo'), 'TOKYO');
      expect(fontSafe('  a   b  '), 'A B');
      expect(fontSafe('21°C'), '21^C');
    });

    test('width, fit and drawing agree', () {
      expect(textWidth(''), 0);
      expect(textWidth('A'), 5);
      expect(textWidth('AB'), 11);
      expect(textWidth('AB', scale: 2), 22);
      expect(fitText('ABCDEFG', 20), 'ABC'); // 3 glyphs = 17 px, 4 = 23 px
      final f = blankFrame();
      // 'H' fills its whole 5x7 cell, so the lit box is exactly the text box.
      final w = drawText(f, 'HH', 10, 10);
      expect(w, textWidth('HH'));
      final (l, t, r, b) = bounds(f);
      expect((l, t, r, b), (10, 10, 10 + w - 1, 16));
    });

    test('scale multiplies the size, and on:false erases', () {
      final one = blankFrame(), two = blankFrame();
      drawText(one, '8', 0, 0);
      drawText(two, '8', 0, 0, scale: 2);
      expect(lit(two), lit(one) * 4);
      drawText(two, '8', 0, 0, scale: 2, on: false);
      expect(lit(two), 0);
    });

    test('every character the widgets use can be drawn', () {
      for (final ch in 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,-+/%:()!?^'.split('')) {
        final f = blankFrame();
        drawText(f, ch, 0, 0);
        expect(lit(f) > 0 || ch == ' ', isTrue, reason: 'glyph "$ch" is blank');
      }
    });
  });

  group('renderer', () {
    WeatherNow w(int code, {double t = 20, bool day = true}) =>
        WeatherNow(tempC: t, code: code, isDay: day, humidity: 50, windKmh: 10);

    test('WMO codes map to skies', () {
      expect(skyFor(0), Sky.clear);
      expect(skyFor(2), Sky.partly);
      expect(skyFor(3), Sky.cloudy);
      expect(skyFor(45), Sky.fog);
      expect(skyFor(55), Sky.drizzle);
      expect(skyFor(63), Sky.rain);
      expect(skyFor(81), Sky.rain);
      expect(skyFor(73), Sky.snow);
      expect(skyFor(86), Sky.snow);
      expect(skyFor(96), Sky.storm);
      expect(skyFor(999), Sky.storm);
      expect(skyFor(-1), Sky.cloudy);
    });

    test('every code renders a full-size, non-blank frame inside the panel', () {
      for (final code in [0, 1, 2, 3, 45, 48, 51, 55, 61, 65, 71, 75, 80, 85, 95, 99]) {
        for (final day in [true, false]) {
          final f = renderWeather(w(code, day: day), 'Pune');
          expect(f.length, frameBytes);
          expect(lit(f), greaterThan(150), reason: 'code $code day=$day');
        }
      }
    });

    test('the sky decides the picture; the unit decides the number', () {
      final icon = <int>{};
      for (final code in [0, 2, 3, 45, 53, 63, 73, 95]) {
        final f = renderWeather(w(code), 'X');
        // Icon region only (left of the text column).
        final cut = blankFrame();
        for (var y = 0; y < oledH; y++) {
          for (var x = 0; x < 52; x++) {
            setPx(cut, x, y, getPx(f, x, y));
          }
        }
        icon.add(Object.hashAll(cut));
      }
      expect(icon, hasLength(8), reason: 'each sky has its own icon');
      expect(renderWeather(w(0, day: true), 'X'), isNot(renderWeather(w(0, day: false), 'X')));
      expect(renderWeather(w(0, t: 20), 'X'), isNot(renderWeather(w(0, t: 20), 'X', fahrenheit: true)));
      expect(renderWeather(w(0, t: 20), 'X'), renderWeather(w(0, t: 20), 'X'), reason: 'deterministic');
    });

    test('text stays on the panel for long cities and extreme readings', () {
      final f = renderWeather(w(3, t: -45.7), 'Llanfairpwllgwyngyll-Gogerychwyrndrobwll');
      final (_, _, r, b) = bounds(f, x0: 54);
      expect(r, lessThan(oledW));
      expect(b, lessThan(oledH));
      // Nothing of the icon column spills into the text column and vice versa.
      expect(bounds(f, x1: 53).$3, lessThan(53));
    });

    test('AQI bands follow the US scale', () {
      expect(aqiBand(0), 0);
      expect(aqiBand(50), 0);
      expect(aqiBand(51), 1);
      expect(aqiBand(100), 1);
      expect(aqiBand(101), 2);
      expect(aqiBand(150), 2);
      expect(aqiBand(151), 3);
      expect(aqiBand(200), 3);
      expect(aqiBand(201), 4);
      expect(aqiBand(300), 4);
      expect(aqiBand(301), 5);
      expect(aqiBand(900), 5);
    });

    test('air quality renders for every band and keeps its numbers on the panel', () {
      for (final aqi in [0, 32, 78, 132, 178, 262, 412, 999]) {
        final f = renderAir(AirNow(usAqi: aqi, pm25: aqi * 0.9, pm10: aqi * 1.3), 'New Delhi Central');
        expect(f.length, frameBytes);
        expect(lit(f), greaterThan(200), reason: 'aqi $aqi');
        final (_, _, r, b) = bounds(f);
        expect(r, lessThan(oledW));
        expect(b, lessThan(oledH));
      }
      expect(renderAir(const AirNow(usAqi: 30), 'X'), isNot(renderAir(const AirNow(usAqi: 130), 'X')));
      // Without PM figures there is nothing in the right column.
      expect(bounds(renderAir(const AirNow(usAqi: 30), 'X'), x0: 80, y0: 12, y1: 40).$3, -1);
    });
  });

  group('Open-Meteo client', () {
    late List<Uri> asked;
    OpenMeteo client(Object? Function(Uri) answer) {
      asked = [];
      return OpenMeteo(
        get: (uri) async {
          asked.add(uri);
          return answer(uri);
        },
      );
    }

    test('search parses places, asks the geocoder politely, and handles no matches', () async {
      final c = client(
        (_) => {
          'results': [
            {'name': 'Pune', 'latitude': 18.5196, 'longitude': 73.8553, 'admin1': 'Maharashtra', 'country': 'India'},
            {'name': 'Pune', 'latitude': 'bad', 'longitude': 1},
            'junk',
          ],
        },
      );
      final r = await c.search('  pune ');
      expect(r, hasLength(1));
      expect(r.single.name, 'Pune');
      expect(r.single.label, 'Pune, Maharashtra, India');
      expect(asked.single.host, 'geocoding-api.open-meteo.com');
      expect(asked.single.scheme, 'https');
      expect(asked.single.queryParameters['name'], 'pune');

      expect(await client((_) => <String, Object?>{}).search('nowhere'), isEmpty);
      await expectLater(client((_) => null).search('pune'), throwsA(isA<WidgetDataException>()));
      await expectLater(client((_) => {}).search('p'), throwsA(isA<WidgetDataException>()));
      expect(asked, isEmpty, reason: 'a one-letter search never leaves the phone');
    });

    test('weather parses the current reading and sends only coordinates', () async {
      final c = client(
        (_) => {
          'current': {
            'temperature_2m': 27.4,
            'relative_humidity_2m': 61,
            'weather_code': 2,
            'is_day': 0,
            'wind_speed_10m': 11.6,
          },
        },
      );
      final w = await c.weather(const GeoPlace('Pune', 18.5196, 73.8553));
      expect((w.tempC, w.code, w.isDay, w.humidity, w.windKmh), (27.4, 2, false, 61, 11.6));
      expect(asked.single.host, 'api.open-meteo.com');
      expect(asked.single.queryParameters.keys, containsAll(['latitude', 'longitude', 'current']));
      expect(asked.single.toString(), isNot(contains('Pune')));
      await expectLater(client((_) => {'current': {}}).weather(const GeoPlace('X', 0, 0)), throwsA(isA<WidgetDataException>()));
    });

    test('air quality parses, and a missing AQI is a readable error', () async {
      final c = client((_) => {'current': {'us_aqi': 87.4, 'pm2_5': 31.2, 'pm10': 55}});
      final a = await c.air(const GeoPlace('Pune', 18.5, 73.8));
      expect((a.usAqi, a.pm25, a.pm10), (87, 31.2, 55.0));
      expect(asked.single.host, 'air-quality-api.open-meteo.com');
      await expectLater(
        client((_) => {'current': {'us_aqi': null}}).air(const GeoPlace('X', 0, 0)),
        throwsA(isA<WidgetDataException>().having((e) => e.message, 'message', contains('air-quality'))),
      );
    });

    test('a place survives a round trip through JSON', () {
      const p = GeoPlace('Pune', 18.5196, 73.8553, region: 'Maharashtra', country: 'India');
      expect(GeoPlace.fromJson(p.toJson()), p);
      expect(GeoPlace.fromJson('nope'), isNull);
      expect(GeoPlace.fromJson({'name': 'X'}), isNull);
    });
  });
}
