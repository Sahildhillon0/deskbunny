// Live preview frames. The real source is the C engine (EngineSim); widget
// tests inject a fake so they don't need the native library.
import 'dart:typed_data';

import 'package:nixxa_engine/nixxa_engine.dart';

import '../model/drawing.dart';
import '../model/face.dart';
import '../protocol/blob.dart';

abstract class PreviewSource {
  void setFace(FaceDef face);
  void setDrawing(Drawing? drawing);

  /// 1024 bytes, SSD1306 page layout.
  Uint8List frame(int tMs);
  void dispose();
}

typedef PreviewFactory = PreviewSource Function(FaceDef face, Drawing? drawing);

PreviewSource enginePreview(FaceDef face, Drawing? drawing) => EnginePreview(face, drawing);

/// The C engine through FFI: same pixels as the bunny.
class EnginePreview implements PreviewSource {
  final EngineSim _sim;

  EnginePreview(FaceDef face, Drawing? drawing, {int seed = 0}) : _sim = EngineSim(encodeFace(face), seed: seed) {
    setDrawing(drawing);
  }

  @override
  void setFace(FaceDef face) => _sim.setFace(encodeFace(face));

  @override
  void setDrawing(Drawing? drawing) =>
      _sim.setDrawing(drawing == null || drawing.frames.isEmpty ? null : encodeDrawing(drawing));

  @override
  Uint8List frame(int tMs) => _sim.frame(tMs);

  @override
  void dispose() => _sim.dispose();
}

/// The clock faces (styles 1..20) drawn by the C engine. The clock picker takes
/// this so widget tests can run without the native library.
class ClockRenderer {
  /// 1024-byte page-layout frame of [style] at [hour]:[minute]:[second].
  final Uint8List Function(int style, int hour, int minute, int second) frame;

  /// Name of [style], e.g. "Big digits".
  final String Function(int style) name;

  const ClockRenderer({required this.frame, required this.name});
}

const ClockRenderer engineClocks = ClockRenderer(frame: clockFrame, name: clockName);

/// Draws nothing but the drawing (if any). Used when the engine can't load.
class BlankPreview implements PreviewSource {
  Drawing? _drawing;
  BlankPreview([this._drawing]);

  @override
  void setFace(FaceDef face) {}

  @override
  void setDrawing(Drawing? drawing) => _drawing = drawing;

  @override
  Uint8List frame(int tMs) {
    final d = _drawing;
    if (d == null || d.frames.isEmpty) return Uint8List(1024);
    final i = (tMs ~/ (d.frameMs <= 0 ? 500 : d.frameMs)) % d.frames.length;
    return Uint8List.fromList(d.frames[i]);
  }

  @override
  void dispose() {}
}
