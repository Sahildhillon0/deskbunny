// Text for the drawing editor: rasterises a string with the platform font and
// thresholds it to lit pixels, so it can be stamped into a 128x64 frame.
import 'dart:ui' as ui;

import 'package:flutter/painting.dart';

import 'pixels.dart';

/// Font sizes (px) offered by the drawing editor's text tool.
const List<int> textSizes = [10, 14, 20, 28];

const int maxTextChars = 40;

/// The lit pixels of [text], relative to the top-left of its box. Anti-aliased
/// edges are cut at half coverage. Empty for blank text.
Future<List<Pt>> textPixels(String text, {required int size, bool bold = true}) async {
  final t = text.replaceAll(RegExp(r'\s+'), ' ').trim();
  if (t.isEmpty) return const [];
  final painter = TextPainter(
    text: TextSpan(
      text: t.length > maxTextChars ? t.substring(0, maxTextChars) : t,
      style: TextStyle(fontSize: size.toDouble(), fontWeight: bold ? FontWeight.w700 : FontWeight.w400, color: const Color(0xFFFFFFFF), height: 1),
    ),
    textDirection: TextDirection.ltr,
    maxLines: 1,
  )..layout();
  // Nothing wider than a few panels is useful; keep the raster small.
  final w = painter.width.ceil().clamp(1, oledW * 4);
  final h = painter.height.ceil().clamp(1, oledH * 2);
  final recorder = ui.PictureRecorder();
  painter.paint(ui.Canvas(recorder), ui.Offset.zero);
  final image = await recorder.endRecording().toImage(w, h);
  try {
    final data = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
    if (data == null) return const [];
    final pts = <Pt>[];
    for (var y = 0; y < h; y++) {
      for (var x = 0; x < w; x++) {
        if (data.getUint8((y * w + x) * 4 + 3) >= 128) pts.add((x, y));
      }
    }
    return pts;
  } finally {
    image.dispose();
    painter.dispose();
  }
}
