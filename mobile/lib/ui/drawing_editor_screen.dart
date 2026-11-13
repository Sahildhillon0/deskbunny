// 5. Drawing editor: 128x64 pixel canvas with zoom and pan (pinch with two
// fingers), pen/eraser/fill/line/rect/ellipse/text, 1..10 frames, onion skin,
// replace/over, frame time, GIF import, and a size meter against maxArtBytes.
// Pops with the edited Drawing.
import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../app/art_size.dart';
import '../app/pixels.dart';
import '../app/text_raster.dart';
import '../app/theme.dart';
import '../model/drawing.dart';
import '../model/face.dart';
import '../protocol/blob.dart' show maxArtBytes;
import 'widgets/common.dart';
import 'widgets/import_sheet.dart';
import 'widgets/oled.dart';

enum DrawTool { pen, eraser, fill, line, rect, ellipse, text, pan }

class DrawingEditorScreen extends StatefulWidget {
  final Drawing? drawing;

  /// The face it goes with, for the "over" preview.
  final FaceDef? face;
  const DrawingEditorScreen({super.key, this.drawing, this.face});

  @override
  State<DrawingEditorScreen> createState() => _DrawingEditorScreenState();
}

class _DrawingEditorScreenState extends State<DrawingEditorScreen> {
  late List<Uint8List> _frames = widget.drawing == null || widget.drawing!.frames.isEmpty
      ? [blankFrame()]
      : [for (final f in widget.drawing!.frames.take(maxFrames)) _fit(f)];
  late DrawingMode _mode = widget.drawing?.mode ?? DrawingMode.replace;
  late int _frameMs = (widget.drawing?.frameMs ?? 500).clamp(frameMsMin, frameMsMax);
  int _cur = 0;
  DrawTool _tool = DrawTool.pen;
  int _brush = 1;
  int _textSize = 14;
  bool _filled = false;
  bool _onion = true;
  bool _dirty = false;

  // View.
  double _zoom = 1;
  Offset _pan = Offset.zero;

  // Gesture.
  double _startZoom = 1;
  Offset _startPan = Offset.zero;
  Offset _startFocal = Offset.zero;
  bool _multi = false;
  Uint8List? _base; // frame before the current stroke
  Pt? _from, _last;
  bool _strokeOn = true;

  // History of whole drawings (frames copies).
  final List<List<Uint8List>> _undo = [];
  final List<List<Uint8List>> _redo = [];

  static Uint8List _fit(Uint8List f) {
    final out = blankFrame();
    out.setRange(0, math.min(f.length, frameBytes), f);
    return out;
  }

  Drawing get _drawing => Drawing(mode: _mode, frameMs: _frameMs, frames: List.unmodifiable(_frames));

  List<Uint8List> _copy() => [for (final f in _frames) Uint8List.fromList(f)];

  void _checkpoint() {
    _undo.add(_copy());
    if (_undo.length > 40) _undo.removeAt(0);
    _redo.clear();
    _dirty = true;
  }

  void _restore(List<List<Uint8List>> from, List<List<Uint8List>> to) {
    if (from.isEmpty) return;
    setState(() {
      to.add(_copy());
      _frames = from.removeLast();
      _cur = _cur.clamp(0, _frames.length - 1);
      _dirty = true;
    });
  }

  // ---------------------------------------------------------------- canvas gestures

  Pt _toPixel(Offset local, double px) {
    final p = (local - _pan) / (px * _zoom);
    return (p.dx.floor(), p.dy.floor());
  }

  void _onStart(ScaleStartDetails d, double px) {
    _startZoom = _zoom;
    _startPan = _pan;
    _startFocal = d.localFocalPoint;
    _multi = d.pointerCount > 1 || _tool == DrawTool.pan;
    if (_multi) return;
    final p = _toPixel(d.localFocalPoint, px);
    if (_tool == DrawTool.text) {
      // A tap puts the text's top-left corner here; the dialog asks for the words.
      _base = null;
      _from = null;
      _last = null;
      unawaited(_placeText(p));
      return;
    }
    final frame = _frames[_cur];
    _checkpoint();
    _base = Uint8List.fromList(frame);
    _from = p;
    _last = p;
    switch (_tool) {
      case DrawTool.fill:
        setState(() => floodFill(frame, p.$1, p.$2));
        _base = null;
      case DrawTool.pen || DrawTool.eraser:
        _strokeOn = _tool == DrawTool.pen;
        setState(() => stamp(frame, p.$1, p.$2, _brush, _strokeOn));
      default:
        setState(() => _shape(p));
    }
  }

  void _shape(Pt to) {
    final base = _base, from = _from;
    if (base == null || from == null) return;
    final f = _frames[_cur]..setAll(0, base);
    final pts = switch (_tool) {
      DrawTool.line => linePoints(from, to),
      DrawTool.rect => rectPoints(from, to, filled: _filled),
      DrawTool.ellipse => ellipsePoints(from, to, filled: _filled),
      _ => const <Pt>[],
    };
    plot(f, pts, _tool == DrawTool.line ? _brush : 1, true);
  }

  void _onUpdate(ScaleUpdateDetails d, double px, Size box) {
    if (!_multi && d.pointerCount > 1) {
      // A second finger landed: undo the stroke and switch to zoom/pan.
      final base = _base;
      if (base != null) _frames[_cur].setAll(0, base);
      if (_undo.isNotEmpty) _undo.removeLast();
      _base = null;
      _multi = true;
      _startZoom = _zoom;
      _startPan = _pan;
      _startFocal = d.localFocalPoint;
    }
    if (_multi) {
      setState(() {
        final z = (_startZoom * d.scale).clamp(1.0, 12.0);
        // Keep the content under the focal point fixed while zooming.
        final content = (_startFocal - _startPan) / _startZoom;
        var pan = d.localFocalPoint - content * z;
        _zoom = z;
        _pan = _clampPan(pan, box);
      });
      return;
    }
    final p = _toPixel(d.localFocalPoint, px);
    if (p == _last) return;
    final last = _last ?? p;
    _last = p;
    setState(() {
      if (_tool == DrawTool.pen || _tool == DrawTool.eraser) {
        plot(_frames[_cur], linePoints(last, p), _brush, _strokeOn);
      } else if (_tool != DrawTool.fill) {
        _shape(p);
      }
    });
  }

  void _onEnd(ScaleEndDetails d) {
    _base = null;
    _from = null;
    _last = null;
    _multi = false;
    setState(() {});
  }

  Offset _clampPan(Offset pan, Size box) {
    final w = box.width * _zoom, h = box.height * _zoom;
    return Offset(pan.dx.clamp(box.width - w, 0), pan.dy.clamp(box.height - h, 0));
  }

  void _setZoom(double z, Size box) => setState(() {
    final c = Offset(box.width / 2, box.height / 2);
    final content = (c - _pan) / _zoom;
    _zoom = z.clamp(1.0, 12.0);
    _pan = _clampPan(c - content * _zoom, box);
  });

  // ---------------------------------------------------------------- frames

  void _frameOp(void Function() op) {
    _checkpoint();
    setState(op);
  }

  void _addFrame({bool duplicate = false}) {
    if (_frames.length >= maxFrames) return;
    _frameOp(() {
      _frames.insert(_cur + 1, duplicate ? Uint8List.fromList(_frames[_cur]) : blankFrame());
      _cur++;
    });
  }

  void _deleteFrame() {
    if (_frames.length <= 1) {
      _frameOp(() => _frames[0] = blankFrame());
      return;
    }
    _frameOp(() {
      _frames.removeAt(_cur);
      _cur = _cur.clamp(0, _frames.length - 1);
    });
  }

  void _moveFrame(int dir) {
    final to = _cur + dir;
    if (to < 0 || to >= _frames.length) return;
    _frameOp(() {
      final f = _frames.removeAt(_cur);
      _frames.insert(to, f);
      _cur = to;
    });
  }

  void _whole(Uint8List Function(Uint8List) op) => _frameOp(() => _frames[_cur] = op(_frames[_cur]));

  Future<void> _placeText(Pt at) async {
    final choice = await showDialog<_TextChoice>(
      context: context,
      builder: (_) => _TextDialog(size: _textSize),
    );
    if (choice == null || !mounted) return;
    _textSize = choice.size;
    final pts = await textPixels(choice.text, size: choice.size);
    if (pts.isEmpty || !mounted) return;
    _checkpoint();
    setState(() {
      final frame = _frames[_cur];
      for (final (dx, dy) in pts) {
        setPx(frame, at.$1 + dx, at.$2 + dy, true);
      }
    });
  }

  Future<void> _importGif() async {
    await guard(context, () async {
      final r = await pickImportedImage(
        context,
        note: 'Replaces every frame. GIFs keep up to 10 frames, evenly spread.',
      );
      if (r == null || !mounted) return;
      _checkpoint();
      setState(() {
        _frames = r.frames;
        _frameMs = r.frameMs;
        _cur = 0;
      });
      if (mounted && r.sourceFrames > r.frames.length) {
        showMessage(context, 'Kept ${r.frames.length} of ${r.sourceFrames} frames (the bunny shows up to $maxFrames).');
      }
    });
  }

  Future<void> _play() => showDialog<void>(
    context: context,
    builder: (ctx) => Dialog(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            OledBezel(
              child: OledPreview(face: widget.face ?? const FaceDef(), drawing: _drawing),
            ),
            const SizedBox(height: 12),
            TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Close')),
          ],
        ),
      ),
    ),
  );

  // ---------------------------------------------------------------- build

  @override
  Widget build(BuildContext context) {
    final size = artBytes(_drawing);
    final over = size != null && size > maxArtBytes;
    return PopScope(
      canPop: !_dirty,
      onPopInvokedWithResult: (didPop, _) async {
        if (didPop) return;
        final nav = Navigator.of(context);
        if (await confirm(
          context,
          title: 'Discard the drawing changes?',
          body: 'Tap Done to keep them.',
          action: 'Discard',
          destructive: true,
        )) {
          _dirty = false;
          nav.pop();
        }
      },
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Drawing'),
          actions: [
            IconButton(
              tooltip: 'Undo',
              onPressed: _undo.isEmpty ? null : () => _restore(_undo, _redo),
              icon: const Icon(Icons.undo),
            ),
            IconButton(
              tooltip: 'Redo',
              onPressed: _redo.isEmpty ? null : () => _restore(_redo, _undo),
              icon: const Icon(Icons.redo),
            ),
            IconButton(tooltip: 'Play', onPressed: _play, icon: const Icon(Icons.play_arrow_outlined)),
            TextButton(
              key: const Key('drawing-done'),
              onPressed: over
                  ? null
                  : () {
                      _dirty = false;
                      Navigator.of(context).pop(_drawing);
                    },
              child: Text(
                'Done',
                style: TextStyle(color: over ? NixxaColors.text3 : NixxaColors.accent, fontWeight: FontWeight.w600),
              ),
            ),
          ],
        ),
        body: Column(
          children: [
            Padding(padding: const EdgeInsets.fromLTRB(12, 0, 12, 8), child: _canvas()),
            _toolbar(),
            const Divider(height: 1),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 24),
                children: [
                  _framesStrip(),
                  const SectionLabel('Playback'),
                  SegmentedButton<DrawingMode>(
                    segments: const [
                      ButtonSegment(value: DrawingMode.replace, label: Text('Replace the face')),
                      ButtonSegment(value: DrawingMode.over, label: Text('Over the face')),
                    ],
                    selected: {_mode},
                    showSelectedIcon: false,
                    onSelectionChanged: (s) => setState(() {
                      _mode = s.first;
                      _dirty = true;
                    }),
                  ),
                  const SizedBox(height: 8),
                  SliderRow(
                    label: 'Frame time',
                    value: _frameMs ~/ 50,
                    min: 1,
                    max: 200,
                    format: (v) =>
                        v * 50 >= 1000 ? '${(v * 50 / 1000).toStringAsFixed(v % 20 == 0 ? 0 : 2)} s' : '${v * 50} ms',
                    onChanged: (v) => setState(() {
                      _frameMs = v * 50;
                      _dirty = true;
                    }),
                  ),
                  SwitchRow(
                    label: 'Onion skin',
                    hint: 'Show the previous frame faintly',
                    value: _onion,
                    onChanged: (v) => setState(() => _onion = v),
                  ),
                  const SectionLabel('Size on the bunny'),
                  _SizeMeter(bytes: size),
                  const SectionLabel('Frame'),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      OutlinedButton.icon(
                        onPressed: _importGif,
                        icon: const Icon(Icons.gif_box_outlined),
                        label: const Text('Import GIF / image'),
                      ),
                      OutlinedButton(onPressed: () => _whole(invertFrame), child: const Text('Invert')),
                      OutlinedButton(onPressed: () => _whole(flipH), child: const Text('Flip ↔')),
                      OutlinedButton(onPressed: () => _whole(flipV), child: const Text('Flip ↕')),
                      OutlinedButton(onPressed: () => _whole((_) => blankFrame()), child: const Text('Clear')),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _canvas() => LayoutBuilder(
    builder: (context, c) {
      final w = c.maxWidth;
      final box = Size(w, w / 2);
      final px = w / oledW;
      final prev = _onion && _frames.length > 1 ? _frames[(_cur + _frames.length - 1) % _frames.length] : null;
      return Column(
        children: [
          Container(
            decoration: BoxDecoration(
              border: Border.all(color: NixxaColors.line2),
              borderRadius: BorderRadius.circular(4),
            ),
            child: ClipRect(
              child: SizedBox(
                width: box.width,
                height: box.height,
                child: GestureDetector(
                  key: const Key('drawing-canvas'),
                  behavior: HitTestBehavior.opaque,
                  onScaleStart: (d) => _onStart(d, px),
                  onScaleUpdate: (d) => _onUpdate(d, px, box),
                  onScaleEnd: _onEnd,
                  child: Stack(
                    clipBehavior: Clip.hardEdge,
                    children: [
                      Positioned(
                        left: _pan.dx,
                        top: _pan.dy,
                        width: box.width * _zoom,
                        height: box.height * _zoom,
                        child: CustomPaint(
                          painter: OledPainter(_frames[_cur], onion: prev, grid: true, version: _version()),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
          Row(
            children: [
              Text('${(_zoom * 100).round()}%', style: const TextStyle(fontSize: 12, color: NixxaColors.text3)),
              Expanded(
                child: Slider(value: _zoom, min: 1, max: 12, onChanged: (z) => _setZoom(z, box)),
              ),
              IconButton(
                tooltip: 'Fit',
                onPressed: () => setState(() {
                  _zoom = 1;
                  _pan = Offset.zero;
                }),
                icon: const Icon(Icons.fit_screen_outlined, size: 20),
              ),
            ],
          ),
        ],
      );
    },
  );

  int _v = 0;
  int _version() => ++_v; // every build repaints; the canvas is cheap

  Widget _toolbar() {
    Widget tool(DrawTool t, IconData icon, String tip) => IconButton(
      key: Key('tool-${t.name}'),
      tooltip: tip,
      isSelected: _tool == t,
      style: IconButton.styleFrom(
        backgroundColor: _tool == t ? NixxaColors.accentSoft : null,
        foregroundColor: _tool == t ? NixxaColors.accent : NixxaColors.text2,
      ),
      onPressed: () => setState(() => _tool = t),
      icon: Icon(icon, size: 20),
    );
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          tool(DrawTool.pen, Icons.edit, 'Pen'),
          tool(DrawTool.eraser, Icons.auto_fix_normal_outlined, 'Eraser'),
          tool(DrawTool.fill, Icons.format_color_fill, 'Fill'),
          tool(DrawTool.line, Icons.horizontal_rule, 'Line'),
          tool(DrawTool.rect, Icons.crop_square, 'Rectangle'),
          tool(DrawTool.ellipse, Icons.circle_outlined, 'Ellipse'),
          tool(DrawTool.text, Icons.text_fields, 'Text'),
          tool(DrawTool.pan, Icons.pan_tool_outlined, 'Move the view'),
          const SizedBox(width: 8),
          if (_tool == DrawTool.rect || _tool == DrawTool.ellipse)
            TextButton(onPressed: () => setState(() => _filled = !_filled), child: Text(_filled ? 'Filled' : 'Outline'))
          else if (_tool == DrawTool.text)
            const Padding(
              padding: EdgeInsets.symmetric(horizontal: 8),
              child: Text('Tap the canvas to place text', style: TextStyle(fontSize: 12, color: NixxaColors.text3)),
            )
          else
            TextButton(onPressed: () => setState(() => _brush = _brush % 4 + 1), child: Text('Brush $_brush px')),
        ],
      ),
    );
  }

  Widget _framesStrip() => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      SectionLabel('Frames ${_cur + 1} / ${_frames.length}'),
      SizedBox(
        height: 44,
        child: ListView.separated(
          scrollDirection: Axis.horizontal,
          itemCount: _frames.length,
          separatorBuilder: (_, _) => const SizedBox(width: 6),
          itemBuilder: (_, i) => GestureDetector(
            key: Key('frame-$i'),
            onTap: () => setState(() => _cur = i),
            child: Container(
              width: 84,
              decoration: BoxDecoration(
                border: Border.all(color: i == _cur ? NixxaColors.accent : NixxaColors.line2, width: i == _cur ? 2 : 1),
                borderRadius: BorderRadius.circular(4),
              ),
              child: OledFrame(_frames[i]),
            ),
          ),
        ),
      ),
      const SizedBox(height: 8),
      Wrap(
        spacing: 4,
        children: [
          IconButton(
            key: const Key('frame-add'),
            tooltip: 'New frame',
            onPressed: _frames.length < maxFrames ? _addFrame : null,
            icon: const Icon(Icons.add_box_outlined),
          ),
          IconButton(
            tooltip: 'Duplicate frame',
            onPressed: _frames.length < maxFrames ? () => _addFrame(duplicate: true) : null,
            icon: const Icon(Icons.copy_outlined),
          ),
          IconButton(tooltip: 'Delete frame', onPressed: _deleteFrame, icon: const Icon(Icons.delete_outline)),
          IconButton(
            tooltip: 'Move left',
            onPressed: _cur > 0 ? () => _moveFrame(-1) : null,
            icon: const Icon(Icons.chevron_left),
          ),
          IconButton(
            tooltip: 'Move right',
            onPressed: _cur < _frames.length - 1 ? () => _moveFrame(1) : null,
            icon: const Icon(Icons.chevron_right),
          ),
        ],
      ),
    ],
  );
}

class _SizeMeter extends StatelessWidget {
  final int? bytes;
  const _SizeMeter({required this.bytes});

  @override
  Widget build(BuildContext context) {
    final b = bytes;
    if (b == null) {
      return const Text(
        'Size unknown (encoder not available).',
        style: TextStyle(fontSize: 12, color: NixxaColors.text3),
      );
    }
    final ratio = (b / maxArtBytes).clamp(0.0, 1.0);
    final over = b > maxArtBytes;
    final color = over ? NixxaColors.danger : (ratio > 0.85 ? NixxaColors.warn : NixxaColors.accent);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ClipRRect(
          borderRadius: BorderRadius.circular(3),
          child: LinearProgressIndicator(value: ratio, minHeight: 6, color: color),
        ),
        const SizedBox(height: 6),
        Text(
          over
              ? '${_kb(b)} of ${_kb(maxArtBytes)}: too big. Use fewer frames or simpler pictures.'
              : '${_kb(b)} of ${_kb(maxArtBytes)}',
          key: const Key('drawing-size'),
          style: TextStyle(fontSize: 12, color: over ? NixxaColors.danger : NixxaColors.text2),
        ),
      ],
    );
  }

  static String _kb(int n) => n < 1024 ? '$n B' : '${(n / 1024).toStringAsFixed(1)} KB';
}

class _TextChoice {
  final String text;
  final int size;
  const _TextChoice(this.text, this.size);
}

/// Asks for the words and a size; pops a [_TextChoice] or null.
class _TextDialog extends StatefulWidget {
  final int size;
  const _TextDialog({required this.size});

  @override
  State<_TextDialog> createState() => _TextDialogState();
}

class _TextDialogState extends State<_TextDialog> {
  final _text = TextEditingController();
  late int _size = widget.size;

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  void _done() {
    final t = _text.text.trim();
    Navigator.pop(context, t.isEmpty ? null : _TextChoice(t, _size));
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('Add text'),
    content: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          key: const Key('text-input'),
          controller: _text,
          autofocus: true,
          maxLength: maxTextChars,
          textInputAction: TextInputAction.done,
          decoration: const InputDecoration(hintText: 'What should it say?'),
          onSubmitted: (_) => _done(),
        ),
        const SizedBox(height: 8),
        const Text('Size', style: TextStyle(fontSize: 12, color: NixxaColors.text3)),
        const SizedBox(height: 6),
        SegmentedButton<int>(
          segments: [
            for (var i = 0; i < textSizes.length; i++)
              ButtonSegment(value: textSizes[i], label: Text(const ['S', 'M', 'L', 'XL'][i], key: Key('text-size-$i'))),
          ],
          selected: {_size},
          showSelectedIcon: false,
          onSelectionChanged: (s) => setState(() => _size = s.first),
        ),
      ],
    ),
    actions: [
      TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
      TextButton(key: const Key('text-add'), onPressed: _done, child: const Text('Add')),
    ],
  );
}
