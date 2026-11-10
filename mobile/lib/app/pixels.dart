// 128x64 frames in SSD1306 page layout (fb[(y >> 3) * 128 + x], bit y & 7) and
// the pixel tools that edit them. Mirrors app/src/lib/drawing.ts.
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:image/image.dart' as img;

const int oledW = 128;
const int oledH = 64;
const int frameBytes = 1024;
const int maxFrames = 10;
const int frameMsMin = 50;
const int frameMsMax = 10000;

typedef Pt = (int, int);

Uint8List blankFrame() => Uint8List(frameBytes);

bool getPx(Uint8List f, int x, int y) {
  if (x < 0 || x >= oledW || y < 0 || y >= oledH) return false;
  return f[(y >> 3) * oledW + x] & (1 << (y & 7)) != 0;
}

void setPx(Uint8List f, int x, int y, bool on) {
  if (x < 0 || x >= oledW || y < 0 || y >= oledH) return;
  final i = (y >> 3) * oledW + x;
  if (on) {
    f[i] |= 1 << (y & 7);
  } else {
    f[i] &= ~(1 << (y & 7)) & 0xFF;
  }
}

bool isBlank(Uint8List f) => f.every((b) => b == 0);

/// Square brush of side [size] centred on (x, y).
void stamp(Uint8List f, int x, int y, int size, bool on) {
  final o = (size - 1) ~/ 2;
  for (var dy = 0; dy < size; dy++) {
    for (var dx = 0; dx < size; dx++) {
      setPx(f, x - o + dx, y - o + dy, on);
    }
  }
}

List<Pt> linePoints(Pt a, Pt b) {
  var (x0, y0) = a;
  final (x1, y1) = b;
  final pts = <Pt>[];
  final dx = (x1 - x0).abs();
  final dy = -(y1 - y0).abs();
  final sx = x0 < x1 ? 1 : -1;
  final sy = y0 < y1 ? 1 : -1;
  var err = dx + dy;
  while (true) {
    pts.add((x0, y0));
    if (x0 == x1 && y0 == y1) return pts;
    final e2 = 2 * err;
    if (e2 >= dy) {
      err += dy;
      x0 += sx;
    }
    if (e2 <= dx) {
      err += dx;
      y0 += sy;
    }
  }
}

List<Pt> rectPoints(Pt a, Pt b, {bool filled = false}) {
  final l = math.min(a.$1, b.$1), r = math.max(a.$1, b.$1);
  final t = math.min(a.$2, b.$2), bt = math.max(a.$2, b.$2);
  final pts = <Pt>[];
  for (var y = t; y <= bt; y++) {
    for (var x = l; x <= r; x++) {
      if (filled || y == t || y == bt || x == l || x == r) pts.add((x, y));
    }
  }
  return pts;
}

/// Ellipse inscribed in the box from a to b.
List<Pt> ellipsePoints(Pt a, Pt b, {bool filled = false}) {
  final l = math.min(a.$1, b.$1), r = math.max(a.$1, b.$1);
  final t = math.min(a.$2, b.$2), bt = math.max(a.$2, b.$2);
  final cx = (l + r) / 2, cy = (t + bt) / 2;
  final rx = (r - l) / 2 + 0.5, ry = (bt - t) / 2 + 0.5;
  bool inside(int x, int y) => math.pow((x - cx) / rx, 2) + math.pow((y - cy) / ry, 2) <= 1;
  final pts = <Pt>[];
  for (var y = t; y <= bt; y++) {
    for (var x = l; x <= r; x++) {
      if (!inside(x, y)) continue;
      if (filled || !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1)) {
        pts.add((x, y));
      }
    }
  }
  return pts;
}

void plot(Uint8List f, Iterable<Pt> pts, int size, bool on) {
  for (final (x, y) in pts) {
    stamp(f, x, y, size, on);
  }
}

/// Flood fill the 4-connected region at (x, y) with its opposite colour.
void floodFill(Uint8List f, int x, int y) {
  if (x < 0 || x >= oledW || y < 0 || y >= oledH) return;
  final target = getPx(f, x, y);
  final stack = <Pt>[(x, y)];
  while (stack.isNotEmpty) {
    final (px, py) = stack.removeLast();
    if (px < 0 || px >= oledW || py < 0 || py >= oledH || getPx(f, px, py) != target) continue;
    setPx(f, px, py, !target);
    stack
      ..add((px + 1, py))
      ..add((px - 1, py))
      ..add((px, py + 1))
      ..add((px, py - 1));
  }
}

Uint8List mapPixels(bool Function(int x, int y) src) {
  final out = blankFrame();
  for (var y = 0; y < oledH; y++) {
    for (var x = 0; x < oledW; x++) {
      if (src(x, y)) setPx(out, x, y, true);
    }
  }
  return out;
}

Uint8List invertFrame(Uint8List f) => Uint8List.fromList([for (final b in f) ~b & 0xFF]);
Uint8List flipH(Uint8List f) => mapPixels((x, y) => getPx(f, oledW - 1 - x, y));
Uint8List flipV(Uint8List f) => mapPixels((x, y) => getPx(f, x, oledH - 1 - y));

// ---------------------------------------------------------------- import

/// Which of [n] frames to keep when at most [max] fit: evenly spread, first included.
List<int> sampleFrames(int n, int max) {
  if (n <= max) return List.generate(n, (i) => i);
  return List.generate(max, (i) => (i * n) ~/ max);
}

class ImportedFrames {
  final List<Uint8List> frames;
  final int frameMs;
  final int sourceFrames;
  const ImportedFrames(this.frames, this.frameMs, this.sourceFrames);
}

/// Decodes a GIF (or any still image package:image understands), fits every
/// frame inside 128x64 centred on black and dithers it to 1 bpp.
ImportedFrames importImage(Uint8List bytes, {int threshold = 128, bool dither = true, bool invert = false}) {
  final decoded = img.decodeImage(bytes);
  if (decoded == null) throw const FormatException('Could not read that image. Use a GIF, PNG or JPEG.');
  final all = decoded.frames;
  final keep = sampleFrames(all.length, maxFrames);
  final frames = [
    for (final i in keep) lumaToFrame(imageLuma(all[i]), threshold: threshold, dither: dither, invert: invert),
  ];
  // Spread the original duration over the kept frames.
  final totalMs = all.fold<int>(0, (s, f) => s + (f.frameDuration < 20 ? 100 : f.frameDuration));
  final ms = all.length <= 1 ? 500 : (totalMs / keep.length).round().clamp(frameMsMin, frameMsMax);
  return ImportedFrames(frames, ms, all.length);
}

/// Grey levels (0..255) of an image fitted inside 128x64, centred on black.
Float32List imageLuma(img.Image src) {
  final s = math.min(oledW / src.width, oledH / src.height);
  final w = math.max(1, (src.width * s).round());
  final h = math.max(1, (src.height * s).round());
  final scaled = img.copyResize(src, width: w, height: h, interpolation: img.Interpolation.average);
  final ox = (oledW - w) ~/ 2, oy = (oledH - h) ~/ 2;
  final luma = Float32List(oledW * oledH);
  for (var y = 0; y < h; y++) {
    for (var x = 0; x < w; x++) {
      final p = scaled.getPixel(x, y);
      final a = p.aNormalized.toDouble();
      final v = (0.299 * p.rNormalized + 0.587 * p.gNormalized + 0.114 * p.bNormalized) * 255 * a;
      final tx = ox + x, ty = oy + y;
      if (tx >= 0 && tx < oledW && ty >= 0 && ty < oledH) luma[ty * oledW + tx] = v.toDouble();
    }
  }
  return luma;
}

/// One-bit frame from grey levels: a threshold, or Floyd-Steinberg around it.
Uint8List lumaToFrame(Float32List luma, {int threshold = 128, bool dither = true, bool invert = false}) {
  final g = Float32List.fromList([for (final v in luma) invert ? 255 - v : v]);
  final out = blankFrame();
  for (var y = 0; y < oledH; y++) {
    for (var x = 0; x < oledW; x++) {
      final i = y * oledW + x;
      final on = g[i] >= threshold;
      if (on) setPx(out, x, y, true);
      if (!dither) continue;
      final err = g[i] - (on ? 255 : 0);
      if (x + 1 < oledW) g[i + 1] += err * 7 / 16;
      if (y + 1 < oledH) {
        if (x > 0) g[i + oledW - 1] += err * 3 / 16;
        g[i + oledW] += err * 5 / 16;
        if (x + 1 < oledW) g[i + oledW + 1] += err / 16;
      }
    }
  }
  return out;
}
