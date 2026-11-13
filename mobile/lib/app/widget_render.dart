// Draws the weather and air-quality widgets as 128x64 frames (SSD1306 page
// layout) with the pixel font and a few shapes. Pure functions: the same
// reading always gives the same picture.
import 'dart:math' as math;
import 'dart:typed_data';

import 'phone_data.dart';
import 'pixel_font.dart';
import 'pixels.dart';
import 'widget_data.dart';

enum Sky { clear, partly, cloudy, fog, drizzle, rain, snow, storm }

/// WMO weather code to a [Sky].
Sky skyFor(int code) {
  if (code == 0) return Sky.clear;
  if (code == 1 || code == 2) return Sky.partly;
  if (code == 3) return Sky.cloudy;
  if (code == 45 || code == 48) return Sky.fog;
  if (code >= 51 && code <= 57) return Sky.drizzle;
  if ((code >= 61 && code <= 67) || (code >= 80 && code <= 82)) return Sky.rain;
  if ((code >= 71 && code <= 77) || code == 85 || code == 86) return Sky.snow;
  if (code >= 95) return Sky.storm;
  return Sky.cloudy;
}

String skyLabel(Sky s, {bool isDay = true}) => switch (s) {
  Sky.clear => isDay ? 'CLEAR' : 'CLEAR NIGHT',
  Sky.partly => 'PART CLOUDY',
  Sky.cloudy => 'CLOUDY',
  Sky.fog => 'FOG',
  Sky.drizzle => 'DRIZZLE',
  Sky.rain => 'RAIN',
  Sky.snow => 'SNOW',
  Sky.storm => 'STORM',
};

// ---------------------------------------------------------------- shapes

void _disc(Uint8List f, int cx, int cy, int r, {bool on = true}) =>
    plot(f, ellipsePoints((cx - r, cy - r), (cx + r, cy + r), filled: true), 1, on);

void _line(Uint8List f, int x0, int y0, int x1, int y1, {int w = 1, bool on = true}) =>
    plot(f, linePoints((x0, y0), (x1, y1)), w, on);

void _box(Uint8List f, int x0, int y0, int x1, int y1, {bool on = true}) =>
    plot(f, rectPoints((x0, y0), (x1, y1), filled: true), 1, on);

void _sun(Uint8List f, int cx, int cy, int r) {
  _disc(f, cx, cy, r);
  const rays = 8;
  for (var i = 0; i < rays; i++) {
    final a = i * math.pi * 2 / rays;
    final c = math.cos(a), s = math.sin(a);
    _line(f, (cx + c * (r + 4)).round(), (cy + s * (r + 4)).round(), (cx + c * (r + 8)).round(), (cy + s * (r + 8)).round());
  }
}

void _moon(Uint8List f, int cx, int cy, int r) {
  _disc(f, cx, cy, r);
  _disc(f, cx + r ~/ 2 + 2, cy - r ~/ 3, r - 1, on: false);
}

/// A cloud about 30 wide and 20 tall, its bottom edge at [cy] + 9. [grow]
/// widens the shape, to cut a clear margin around it with `on: false`.
void _cloud(Uint8List f, int cx, int cy, {bool on = true, int grow = 0}) {
  _disc(f, cx - 9, cy + 2, 7 + grow, on: on);
  _disc(f, cx + 1, cy - 3, 10 + grow, on: on);
  _disc(f, cx + 11, cy + 3, 6 + grow, on: on);
  _box(f, cx - 9, cy + 3 - grow, cx + 11, cy + 9 + grow, on: on);
}

void _drops(Uint8List f, int cx, int top, {required int count, required bool heavy}) {
  for (var i = 0; i < count; i++) {
    final x = cx - 10 + i * (20 ~/ math.max(1, count - 1));
    _line(f, x + 2, top, x - 1, top + (heavy ? 7 : 4), w: 1);
  }
}

void _flakes(Uint8List f, int cx, int top) {
  for (final (dx, dy) in const [(-10, 0), (0, 4), (10, 0), (-5, 9), (5, 9)]) {
    final x = cx + dx, y = top + dy;
    _line(f, x - 2, y, x + 2, y);
    _line(f, x, y - 2, x, y + 2);
  }
}

void _bolt(Uint8List f, int cx, int top, {bool on = true}) {
  final pts = [(cx + 3, top), (cx - 3, top + 7), (cx + 1, top + 7), (cx - 4, top + 15)];
  for (var i = 0; i < pts.length - 1; i++) {
    _line(f, pts[i].$1, pts[i].$2, pts[i + 1].$1, pts[i + 1].$2, w: 2, on: on);
  }
}

/// The weather picture, about 48x48, centred on ([cx], [cy]).
void drawSky(Uint8List f, Sky sky, bool isDay, int cx, int cy) {
  switch (sky) {
    case Sky.clear:
      isDay ? _sun(f, cx, cy, 10) : _moon(f, cx, cy, 15);
    case Sky.partly:
      if (isDay) {
        _sun(f, cx - 8, cy - 8, 7);
      } else {
        _moon(f, cx - 9, cy - 9, 10);
      }
      _cloud(f, cx + 3, cy + 4, on: false, grow: 2);
      _cloud(f, cx + 3, cy + 4);
    case Sky.cloudy:
      _cloud(f, cx, cy);
    case Sky.fog:
      _cloud(f, cx, cy - 10);
      for (var i = 0; i < 3; i++) {
        _line(f, cx - 16 + i * 3, cy + 6 + i * 6, cx + 16 - i * 3, cy + 6 + i * 6, w: 2);
      }
    case Sky.drizzle:
      _cloud(f, cx, cy - 8);
      _drops(f, cx, cy + 6, count: 3, heavy: false);
    case Sky.rain:
      _cloud(f, cx, cy - 8);
      _drops(f, cx, cy + 6, count: 4, heavy: true);
    case Sky.snow:
      _cloud(f, cx, cy - 8);
      _flakes(f, cx, cy + 9);
    case Sky.storm:
      _cloud(f, cx, cy - 8);
      _bolt(f, cx, cy + 4);
  }
}

// ---------------------------------------------------------------- weather

/// Largest scale (3, 2, 1) at which [text] fits [maxWidth].
int _bestScale(String text, int maxWidth) {
  for (var s = 3; s > 1; s--) {
    if (textWidth(text, scale: s) <= maxWidth) return s;
  }
  return 1;
}

Uint8List renderWeather(WeatherNow w, String city, {bool fahrenheit = false}) {
  final f = blankFrame();
  final sky = skyFor(w.code);
  drawSky(f, sky, w.isDay, 25, 26);

  const x = 54, room = oledW - x - 1;
  final t = fahrenheit ? w.tempC * 9 / 5 + 32 : w.tempC;
  final temp = '${t.round()}^${fahrenheit ? 'F' : 'C'}';
  final scale = _bestScale(temp, room);
  drawText(f, temp, x, 4, scale: scale);

  drawText(f, fitText(skyLabel(sky, isDay: w.isDay), room), x, 4 + glyphH * scale + 5);
  drawText(f, fitText(fontSafe(city), room), x, 44);
  drawText(f, fitText('${w.humidity}% ${w.windKmh.round()}KM/H', room), x, 55);
  return f;
}

// ---------------------------------------------------------------- air quality

/// US AQI bands: upper bound, label.
const List<(int, String)> aqiBands = [
  (50, 'GOOD'),
  (100, 'MODERATE'),
  (150, 'SENSITIVE'),
  (200, 'UNHEALTHY'),
  (300, 'VERY BAD'),
  (500, 'HAZARDOUS'),
];

int aqiBand(int aqi) {
  for (var i = 0; i < aqiBands.length; i++) {
    if (aqi <= aqiBands[i].$1) return i;
  }
  return aqiBands.length - 1;
}

Uint8List renderAir(AirNow a, String city) {
  final f = blankFrame();
  const unit = 'US AQI';
  drawText(f, unit, oledW - textWidth(unit) - 2, 2);
  drawText(f, fitText(fontSafe(city), oledW - textWidth(unit) - 16), 2, 2);

  final aqi = a.usAqi.clamp(0, 999);
  final band = aqiBand(aqi);
  final n = '$aqi';
  final nScale = textWidth(n, scale: 4) <= 74 ? 4 : 3;
  drawText(f, n, 4, 12, scale: nScale);

  // Right-aligned so any number of digits stays on the panel; a long reading
  // drops its "PM" so it never runs into the big number.
  void pm(String label, double? v, int y) {
    if (v == null) return;
    var s = '$label ${v.round()}';
    if (oledW - 2 - textWidth(s) < 4 + textWidth(n, scale: nScale) + 4) s = s.substring(2);
    drawText(f, s, oledW - 2 - textWidth(s), y);
  }

  pm('PM2.5', a.pm25, 16);
  pm('PM10', a.pm10, 28);

  // Six segments, filled up to the current band.
  const segW = 20, gap = 1, x0 = 3, y0 = 42;
  for (var i = 0; i < aqiBands.length; i++) {
    final x = x0 + i * (segW + gap);
    if (i <= band) {
      _box(f, x, y0, x + segW - 1, y0 + 3);
    } else {
      plot(f, rectPoints((x, y0), (x + segW - 1, y0 + 3)), 1, true);
    }
  }
  drawText(f, fitText(aqiBands[band].$2, oledW - 4, scale: 2), 4, 49, scale: 2);
  return f;
}

// ---------------------------------------------------------------- shared helpers

/// Draws [text] centred on the panel at row [y].
void _centred(Uint8List f, String text, int y, {int scale = 1}) =>
    drawText(f, text, (oledW - textWidth(text, scale: scale)) ~/ 2, y, scale: scale);

String _two(int n) => n.toString().padLeft(2, '0');

const _weekdays = ['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'];
const _months = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];

/// m:ss, or h:mm:ss from an hour up.
String clockText(int ms) {
  final total = math.max(0, ms) ~/ 1000;
  final h = total ~/ 3600, m = (total % 3600) ~/ 60, s = total % 60;
  return h > 0 ? '$h:${_two(m)}:${_two(s)}' : '$m:${_two(s)}';
}

// ---------------------------------------------------------------- phone battery

Uint8List renderBattery(PhoneBattery b) {
  final f = blankFrame();
  final pct = b.percent.clamp(0, 100);
  // Body with a 2 px wall, a tip on the right, a fill that follows the charge.
  plot(f, rectPoints((14, 2), (101, 26)), 1, true);
  plot(f, rectPoints((15, 3), (100, 25)), 1, true);
  _box(f, 102, 9, 106, 19);
  const inner = 80; // fill width at 100 %
  final w = pct == 0 ? 0 : math.max(2, (inner * pct / 100).round());
  if (w > 0) _box(f, 18, 7, 18 + w - 1, 21);
  if (b.charging) _bolt(f, 58, 5, on: 18 + w <= 58);
  _centred(f, '$pct%', 32, scale: 3);
  _centred(f, b.charging ? 'CHARGING' : 'PHONE BATTERY', 57);
  return f;
}

// ---------------------------------------------------------------- next event

String eventDayLabel(DateTime start, DateTime now) {
  final d = DateTime(start.year, start.month, start.day).difference(DateTime(now.year, now.month, now.day)).inDays;
  if (d <= 0) return 'TODAY';
  if (d == 1) return 'TOMORROW';
  return '${_weekdays[start.weekday - 1]} ${start.day} ${_months[start.month - 1]}';
}

Uint8List renderEvent(CalendarEvent? e, DateTime now) {
  final f = blankFrame();
  if (e == null) {
    _centred(f, 'NO EVENTS', 18, scale: 2);
    _centred(f, 'NEXT 7 DAYS', 40);
    return f;
  }
  drawText(f, fitText(eventDayLabel(e.start, now), 120, scale: 2), 4, 4, scale: 2);
  final time = e.allDay ? 'ALL DAY' : '${_two(e.start.hour)}:${_two(e.start.minute)}';
  final ts = textWidth(time, scale: 3) <= oledW - 12 ? 3 : 2;
  drawText(f, time, 4, ts == 3 ? 21 : 24, scale: ts);
  var title = fontSafe(e.title);
  if (title.isEmpty) title = 'EVENT';
  final lines = wrapText(title, oledW - 8, maxLines: 2);
  for (var i = 0; i < lines.length; i++) {
    drawText(f, lines[i], 4, 47 + i * 9);
  }
  return f;
}

// ---------------------------------------------------------------- counter

Uint8List renderCounter(int value, String label) {
  final f = blankFrame();
  final l = fontSafe(label).isEmpty ? 'COUNT' : fontSafe(label);
  final big = textWidth(l, scale: 2) <= oledW - 8;
  final scale = big ? 2 : 1;
  _centred(f, fitText(l, oledW - 8, scale: scale), 4, scale: scale);
  final n = '$value';
  final ns = textWidth(n, scale: 4) <= oledW - 4 ? 4 : 3;
  _centred(f, n, 26, scale: ns);
  plot(f, linePoints((44, 58), (83, 58)), 2, true);
  return f;
}

// ---------------------------------------------------------------- status sign

/// Signs offered as one-tap choices.
const List<String> statusPresets = ['IN A MEETING', 'FOCUS', 'BRB', 'LUNCH', 'DO NOT DISTURB'];

Uint8List renderStatus(String text) {
  final f = blankFrame();
  plot(f, rectPoints((0, 0), (oledW - 1, oledH - 1)), 1, true);
  plot(f, rectPoints((1, 1), (oledW - 2, oledH - 2)), 1, true);
  var t = fontSafe(text);
  if (t.isEmpty) t = 'STATUS';
  const boxW = 112, boxH = 48;
  var scale = 1;
  var lines = wrapText(t, boxW, maxLines: boxH ~/ 10);
  for (final s in [3, 2, 1]) {
    final maxLines = math.max(1, boxH ~/ (9 * s));
    final tryLines = wrapText(t, boxW, scale: s, maxLines: maxLines + 1);
    if (tryLines.length <= maxLines) {
      scale = s;
      lines = tryLines;
      break;
    }
  }
  final lineH = 9 * scale;
  final top = 8 + (boxH - (lines.length * lineH - 2 * scale)) ~/ 2;
  for (var i = 0; i < lines.length; i++) {
    _centred(f, lines[i], top + i * lineH, scale: scale);
  }
  return f;
}

// ---------------------------------------------------------------- now playing

Uint8List renderNowPlaying(NowPlaying n) {
  final f = blankFrame();
  drawText(f, n.playing ? 'NOW PLAYING' : 'PAUSED', 2, 2);
  final app = fitText(fontSafe(n.app), 52);
  if (app.isNotEmpty) drawText(f, app, oledW - 2 - textWidth(app), 2);

  var title = fontSafe(n.title);
  final artist = fontSafe(n.artist);
  if (title.isEmpty) title = artist.isEmpty ? 'UNKNOWN TRACK' : artist;
  var scale = 1;
  var lines = wrapText(title, oledW - 4, maxLines: 3);
  final big = wrapText(title, oledW - 4, scale: 2, maxLines: 3);
  if (big.length <= 2) {
    scale = 2;
    lines = big;
  }
  final lineH = scale == 2 ? 15 : 9;
  for (var i = 0; i < lines.length; i++) {
    drawText(f, lines[i], 2, 12 + i * lineH, scale: scale);
  }
  if (artist.isNotEmpty && artist != title) {
    drawText(f, fitText(artist, oledW - 4), 2, 12 + lines.length * lineH + (scale == 2 ? 1 : 3));
  }

  final dur = n.durationMs;
  final pos = dur > 0 ? n.positionMs.clamp(0, dur) : math.max(0, n.positionMs);
  if (dur > 0) {
    const x0 = 2, x1 = oledW - 3;
    plot(f, linePoints((x0, 53), (x1, 53)), 1, true);
    final x = x0 + ((x1 - x0) * pos / dur).round();
    _box(f, x0, 51, math.max(x0, x), 55);
  }
  drawText(f, clockText(pos), 2, 57);
  if (dur > 0) {
    final total = clockText(dur);
    drawText(f, total, oledW - 2 - textWidth(total), 57);
  }
  return f;
}
