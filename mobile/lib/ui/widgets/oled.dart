// The OLED: a 1024-byte page-layout framebuffer drawn with crisp square
// pixels (nearest neighbour, no smoothing), and a live preview driven by a
// PreviewSource (the C engine).
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:provider/provider.dart';

import '../../app/app_state.dart';
import '../../app/pixels.dart';
import '../../app/preview.dart';
import '../../app/theme.dart';
import '../../model/drawing.dart';
import '../../model/face.dart';

/// Paints a framebuffer. [onion] (optional) is drawn dimmed underneath.
class OledPainter extends CustomPainter {
  final Uint8List fb;
  final Uint8List? onion;
  final double brightness;
  final bool grid;
  final int version;

  OledPainter(this.fb, {this.onion, this.brightness = 1, this.grid = false, this.version = 0});

  static Float32List _points(Uint8List fb, double px) {
    var n = 0;
    final tmp = Float32List(oledW * oledH * 2);
    for (var page = 0; page < 8; page++) {
      for (var x = 0; x < oledW; x++) {
        final b = fb[page * oledW + x];
        if (b == 0) continue;
        for (var bit = 0; bit < 8; bit++) {
          if (b & (1 << bit) == 0) continue;
          tmp[n++] = (x + 0.5) * px;
          tmp[n++] = (page * 8 + bit + 0.5) * px;
        }
      }
    }
    return Float32List.sublistView(tmp, 0, n);
  }

  @override
  void paint(Canvas canvas, Size size) {
    final px = size.width / oledW;
    canvas.drawRect(Offset.zero & size, Paint()..color = NixxaColors.panel);
    final paint = Paint()
      ..isAntiAlias = false
      ..strokeCap = StrokeCap.square
      ..strokeWidth = px;
    final o = onion;
    if (o != null && o.length >= frameBytes) {
      paint.color = NixxaColors.lit.withValues(alpha: 0.22);
      canvas.drawRawPoints(ui.PointMode.points, _points(o, px), paint);
    }
    if (fb.length >= frameBytes) {
      paint.color = NixxaColors.lit.withValues(alpha: brightness);
      canvas.drawRawPoints(ui.PointMode.points, _points(fb, px), paint);
    }
    if (grid && px >= 6) {
      final g = Paint()
        ..color = const Color(0x80000000)
        ..strokeWidth = 1
        ..isAntiAlias = false;
      for (var x = 1; x < oledW; x++) {
        canvas.drawLine(Offset(x * px, 0), Offset(x * px, size.height), g);
      }
      for (var y = 1; y < oledH; y++) {
        canvas.drawLine(Offset(0, y * px), Offset(size.width, y * px), g);
      }
    }
  }

  @override
  bool shouldRepaint(OledPainter old) =>
      old.version != version ||
      !identical(old.fb, fb) ||
      !identical(old.onion, onion) ||
      old.brightness != brightness ||
      old.grid != grid;
}

/// A static frame at a 2:1 aspect ratio, filling the available width.
class OledFrame extends StatelessWidget {
  final Uint8List fb;
  final double brightness;
  const OledFrame(this.fb, {super.key, this.brightness = 1});

  @override
  Widget build(BuildContext context) => AspectRatio(
    aspectRatio: 2,
    child: CustomPaint(painter: OledPainter(fb, brightness: brightness)),
  );
}

/// Bezel around a screen, like the module.
class OledBezel extends StatelessWidget {
  final Widget child;
  const OledBezel({super.key, required this.child});

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.all(6),
    decoration: BoxDecoration(
      color: const Color(0xFF050607),
      borderRadius: BorderRadius.circular(10),
      border: Border.all(color: NixxaColors.line2),
    ),
    child: ClipRRect(borderRadius: BorderRadius.circular(3), child: child),
  );
}

PreviewSource _makeSource(BuildContext context, FaceDef face, Drawing? drawing) {
  try {
    return context.read<AppState>().previewFactory(face, drawing);
  } on Object {
    return BlankPreview(drawing);
  }
}

double _brightness(FaceDef face) {
  try {
    return 0.55 + 0.45 * (face.contrast.clamp(0, 255) / 255);
  } on Object {
    return 1;
  }
}

/// Live, animated preview of a face (and drawing), pixel-identical to the bunny.
class OledPreview extends StatefulWidget {
  /// Widget tests turn this off so pumpAndSettle can settle.
  static bool animationsEnabled = true;

  final FaceDef face;
  final Drawing? drawing;
  const OledPreview({super.key, required this.face, this.drawing});

  @override
  State<OledPreview> createState() => _OledPreviewState();
}

class _OledPreviewState extends State<OledPreview> with SingleTickerProviderStateMixin {
  late PreviewSource _src;
  Ticker? _ticker;
  Uint8List _fb = blankFrame();
  int _lastMs = -1000;
  int _version = 0;
  bool _built = false;

  @override
  void initState() {
    super.initState();
    _src = _makeSource(context, widget.face, widget.drawing);
    _render(0);
    if (OledPreview.animationsEnabled) {
      _ticker = createTicker(_tick)..start();
    }
  }

  void _tick(Duration elapsed) {
    final ms = elapsed.inMilliseconds;
    if (ms - _lastMs < 33) return; // ~30 fps, like the device
    _render(ms);
  }

  void _render(int ms) {
    _lastMs = ms;
    try {
      final f = _src.frame(ms);
      if (!mounted) return;
      _fb = f;
      _version++;
      if (_ticker != null || _built) setState(() {});
    } on Object {
      // keep the last frame
    }
  }

  @override
  void didUpdateWidget(OledPreview old) {
    super.didUpdateWidget(old);
    try {
      if (!identical(old.face, widget.face)) _src.setFace(widget.face);
      if (!identical(old.drawing, widget.drawing)) _src.setDrawing(widget.drawing);
    } on Object {
      // the face is mid-edit; keep showing the last one
    }
    if (_ticker == null) _render(_lastMs < 0 ? 0 : _lastMs);
  }

  @override
  void dispose() {
    _ticker?.dispose();
    _src.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    _built = true;
    return AspectRatio(
      aspectRatio: 2,
      child: CustomPaint(
        painter: OledPainter(_fb, brightness: _brightness(widget.face), version: _version),
      ),
    );
  }
}

/// One still frame of a face, for lists.
class OledThumb extends StatefulWidget {
  final FaceDef face;
  final Drawing? drawing;
  const OledThumb({super.key, required this.face, this.drawing});

  @override
  State<OledThumb> createState() => _OledThumbState();
}

class _OledThumbState extends State<OledThumb> {
  Uint8List? _fb;

  void _build() {
    final src = _makeSource(context, widget.face, widget.drawing);
    try {
      _fb = src.frame(1200);
    } on Object {
      _fb = blankFrame();
    } finally {
      src.dispose();
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_fb == null) _build();
  }

  @override
  void didUpdateWidget(OledThumb old) {
    super.didUpdateWidget(old);
    if (!identical(old.face, widget.face) || !identical(old.drawing, widget.drawing)) _build();
  }

  @override
  Widget build(BuildContext context) =>
      ClipRRect(borderRadius: BorderRadius.circular(4), child: OledFrame(_fb ?? blankFrame()));
}
