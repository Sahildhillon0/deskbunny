import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:nixxa_engine/nixxa_engine.dart';

// A valid face blob: the engine default (neutral, "DESKBUNNY").
final Uint8List _defaultFace = Uint8List.fromList([
  0x44, 0x42, 0x46, 0x41, 0x43, 0x45, 0x30, 0x31, 0x01, 0x00, 0x44, 0x45, //
  0x53, 0x4b, 0x42, 0x55, 0x4e, 0x4e, 0x59, 0x00, 0x00, 0x00, 0x24, 0x24, //
  0x08, 0x10, 0x00, 0x07, 0x14, 0x3c, 0x0f, 0x32, 0x12, 0x08, 0x05, 0xcf, //
  0x00, 0x00, 0x00, 0x0a, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
  0x00, 0x00, 0x00, 0x00,
]);

void main() => runApp(const MaterialApp(home: _Preview()));

class _Preview extends StatefulWidget {
  const _Preview();
  @override
  State<_Preview> createState() => _PreviewState();
}

class _PreviewState extends State<_Preview> {
  EngineSim? _sim;
  final _clock = Stopwatch()..start();
  Timer? _timer;
  ui.Image? _image;
  String? _error;

  @override
  void initState() {
    super.initState();
    try {
      final b = Uint8List.fromList(_defaultFace);
      _fixCrc(b);
      _sim = EngineSim(b);
    } catch (e) {
      _error = '$e';
      return;
    }
    _timer = Timer.periodic(const Duration(milliseconds: 33), (_) => _tick());
  }

  static void _fixCrc(Uint8List b) {
    var crc = 0xFFFF;
    for (var i = 0; i < 62; i++) {
      crc ^= b[i] << 8;
      for (var k = 0; k < 8; k++) {
        crc = (crc & 0x8000) != 0 ? ((crc << 1) ^ 0x1021) & 0xFFFF : (crc << 1) & 0xFFFF;
      }
    }
    b[62] = crc & 0xFF;
    b[63] = crc >> 8;
  }

  void _tick() {
    final rgba = EngineSim.toRgba(_sim!.frame(_clock.elapsedMilliseconds));
    ui.decodeImageFromPixels(rgba, engineWidth, engineHeight, ui.PixelFormat.rgba8888, (img) {
      if (!mounted) return img.dispose();
      setState(() {
        _image?.dispose();
        _image = img;
      });
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    _sim?.dispose();
    _image?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
        backgroundColor: Colors.black,
        body: Center(
          child: _error != null
              ? Text(_error!, style: const TextStyle(color: Colors.white))
              : AspectRatio(
                  aspectRatio: 2,
                  child: RawImage(image: _image, filterQuality: FilterQuality.none, fit: BoxFit.contain),
                ),
        ),
      );
}
