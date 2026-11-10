// Drawing: the Dart form of drawing blob v2 (docs/ARCHITECTURE.md).
// Mirrors crates/deskbunny-core/src/drawing.rs.
import 'dart:convert';
import 'dart:typed_data';

enum DrawingMode { replace, over }

class Drawing {
  /// Bytes per frame (128x64, SSD1306 page layout: fb[(y >> 3) * 128 + x], bit y & 7).
  static const int frameBytes = 1024;
  static const int width = 128;
  static const int height = 64;
  static const int framesMax = 10;
  static const int frameMsMin = 50;
  static const int frameMsMax = 10000;
  static const int frameMsDefault = 500;

  final DrawingMode mode;
  final int frameMs; // 50..10000
  final List<Uint8List> frames; // 1..10 frames, 1024 bytes each, SSD1306 page layout

  const Drawing({required this.mode, this.frameMs = 500, required this.frames});

  Drawing copyWith({DrawingMode? mode, int? frameMs, List<Uint8List>? frames}) =>
      Drawing(mode: mode ?? this.mode, frameMs: frameMs ?? this.frameMs, frames: frames ?? this.frames);

  /// Clamp into range like `Drawing::sanitized`: frameMs 0 -> 500, else
  /// 50..10000; at most 10 frames; every frame padded or cut to 1024 bytes.
  /// null when there are no frames (nothing to show).
  Drawing? sanitized() {
    if (frames.isEmpty) return null;
    final ms = frameMs == 0 ? frameMsDefault : frameMs.clamp(frameMsMin, frameMsMax);
    final fs = frames.take(framesMax).map((f) {
      if (f.length == frameBytes) return Uint8List.fromList(f);
      final out = Uint8List(frameBytes);
      out.setRange(0, f.length < frameBytes ? f.length : frameBytes, f);
      return out;
    }).toList(growable: false);
    return Drawing(mode: mode, frameMs: ms, frames: fs);
  }

  /// Problems that sanitized() would silently fix, for the editor to show.
  /// Empty when the drawing is already valid.
  List<String> validate() {
    final errs = <String>[];
    if (frames.isEmpty) errs.add('A drawing needs at least one frame.');
    if (frames.length > framesMax) errs.add('At most $framesMax frames.');
    if (frameMs < frameMsMin || frameMs > frameMsMax) {
      errs.add('Frame time must be $frameMsMin..$frameMsMax ms.');
    }
    for (var i = 0; i < frames.length; i++) {
      if (frames[i].length != frameBytes) errs.add('Frame ${i + 1} is not $frameBytes bytes.');
    }
    return errs;
  }

  bool get isValid => validate().isEmpty;

  /// The frame index shown at `tMs` (as the firmware picks it).
  int frameIndexAt(int tMs) => (tMs ~/ (frameMs < 1 ? 1 : frameMs)) % frames.length;

  /// Desktop JSON: `{mode, frameMs, frames: base64[]}`.
  Map<String, Object?> toJson() => {
        'mode': mode.name,
        'frameMs': frameMs,
        'frames': frames.map(base64.encode).toList(),
      };

  /// Parse the desktop JSON like serde: `mode` defaults to replace, `frameMs`
  /// to 500, `frames` is required. Not sanitised. Throws FormatException.
  static Drawing fromJson(Map<String, Object?> json) {
    final m = json['mode'];
    final DrawingMode mode;
    if (m == null) {
      mode = DrawingMode.replace;
    } else if (m == 'replace') {
      mode = DrawingMode.replace;
    } else if (m == 'over') {
      mode = DrawingMode.over;
    } else {
      throw FormatException("Drawing mode '$m' is not 'replace' or 'over'.");
    }
    final ms = json['frameMs'];
    if (ms != null && (ms is! int || ms < 0 || ms > 0xFFFF)) {
      throw const FormatException('Drawing frameMs must be a whole number 0..65535.');
    }
    final fs = json['frames'];
    if (fs is! List) throw const FormatException("Drawing is missing 'frames'.");
    final frames = <Uint8List>[];
    for (final f in fs) {
      if (f is! String) throw const FormatException('Drawing frames must be base64 strings.');
      try {
        frames.add(base64.decode(f.trim()));
      } on FormatException catch (e) {
        throw FormatException('drawing frame is not base64: ${e.message}');
      }
    }
    return Drawing(mode: mode, frameMs: (ms as int?) ?? frameMsDefault, frames: frames);
  }

  @override
  bool operator ==(Object other) {
    if (other is! Drawing || other.mode != mode || other.frameMs != frameMs) return false;
    if (other.frames.length != frames.length) return false;
    for (var i = 0; i < frames.length; i++) {
      final a = frames[i], b = other.frames[i];
      if (a.length != b.length) return false;
      for (var j = 0; j < a.length; j++) {
        if (a[j] != b[j]) return false;
      }
    }
    return true;
  }

  @override
  int get hashCode => Object.hash(mode, frameMs, frames.length);

  /// Whether pixel (x, y) is lit in a page-layout frame.
  static bool pixel(Uint8List frame, int x, int y) => frame[(y >> 3) * width + x] & (1 << (y & 7)) != 0;

  static void setPixel(Uint8List frame, int x, int y, bool on) {
    final i = (y >> 3) * width + x;
    final bit = 1 << (y & 7);
    frame[i] = on ? frame[i] | bit : frame[i] & ~bit & 0xFF;
  }
}
