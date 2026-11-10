/// The DeskBunny face engine (engine/*.c) over FFI: frames identical to the
/// desktop simulator (crates/deskbunny-core Engine) and to the OLED. Also the
/// firmware's clock faces (firmware/nixxa_esp32/nx_clock.c), for the picker.
library;

import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

const int engineWidth = 128;
const int engineHeight = 64;
const int engineFrameBytes = 1024;

final class _NxSim extends Opaque {}

class _Bindings {
  final Pointer<_NxSim> Function(int) simNew;
  final Pointer<NativeFunction<Void Function(Pointer<_NxSim>)>> simFreePtr;
  final void Function(Pointer<_NxSim>) simFree;
  final int Function(Pointer<_NxSim>, Pointer<Uint8>) setFace;
  final int Function(Pointer<_NxSim>, Pointer<Uint8>, int) setDrawing;
  final Pointer<Uint8> Function(Pointer<_NxSim>, int) render;
  final void Function(int, int, int, int, Pointer<Uint8>) clockPreview;
  final Pointer<Utf8> Function(int) clockStyleName;

  _Bindings(DynamicLibrary lib)
      : clockPreview = lib.lookupFunction<
            Void Function(Uint8, Uint8, Uint8, Uint8, Pointer<Uint8>),
            void Function(int, int, int, int, Pointer<Uint8>)>('nx_clock_preview'),
        clockStyleName = lib.lookupFunction<Pointer<Utf8> Function(Uint8), Pointer<Utf8> Function(int)>(
            'nx_clock_style_name'),
        simNew = lib.lookupFunction<Pointer<_NxSim> Function(Uint32), Pointer<_NxSim> Function(int)>(
            'nx_sim_new'),
        simFreePtr = lib.lookup<NativeFunction<Void Function(Pointer<_NxSim>)>>('nx_sim_free'),
        simFree = lib.lookupFunction<Void Function(Pointer<_NxSim>), void Function(Pointer<_NxSim>)>(
            'nx_sim_free'),
        setFace = lib.lookupFunction<Int32 Function(Pointer<_NxSim>, Pointer<Uint8>),
            int Function(Pointer<_NxSim>, Pointer<Uint8>)>('nx_sim_set_face'),
        setDrawing = lib.lookupFunction<Int32 Function(Pointer<_NxSim>, Pointer<Uint8>, Uint32),
            int Function(Pointer<_NxSim>, Pointer<Uint8>, int)>('nx_sim_set_drawing'),
        render = lib.lookupFunction<Pointer<Uint8> Function(Pointer<_NxSim>, Uint32),
            Pointer<Uint8> Function(Pointer<_NxSim>, int)>('nx_sim_render');
}

DynamicLibrary _open() {
  final override = EngineSim.libraryPath ?? Platform.environment['NIXXA_ENGINE_LIB'];
  if (override != null) return DynamicLibrary.open(override);
  if (Platform.isIOS || Platform.isMacOS) return DynamicLibrary.process();
  if (Platform.isWindows) return DynamicLibrary.open('nixxa_engine.dll');
  return DynamicLibrary.open('libnixxa_engine.so');
}

_Bindings? _bindingsCache;
_Bindings get _b => _bindingsCache ??= _Bindings(_open());

/// A clock face (style 1..20, see firmware nx_clock.h) at [hour]:[minute]:[second]
/// as a 1024-byte page-layout frame, the same pixels as the bunny.
Uint8List clockFrame(int style, int hour, int minute, int second) {
  final p = malloc<Uint8>(engineFrameBytes);
  try {
    _b.clockPreview(style, hour, minute, second, p);
    return Uint8List.fromList(p.asTypedList(engineFrameBytes));
  } finally {
    malloc.free(p);
  }
}

/// The name of a clock style, e.g. "Big digits" (style 0 and unknown styles
/// give the name of style 1, as the firmware does).
String clockName(int style) => _b.clockStyleName(style).toDartString();

/// One running face, like the desktop `sim_*` commands. Output is
/// deterministic for the same face blob, drawing blob, seed and sequence of
/// `frame` timestamps.
class EngineSim implements Finalizable {
  /// Path of the native library, for hosts where the plugin isn't bundled
  /// (`flutter test`). Must be set before the first EngineSim is created.
  /// The `NIXXA_ENGINE_LIB` environment variable works too.
  static String? libraryPath;

  static final NativeFinalizer _finalizer = NativeFinalizer(_b.simFreePtr.cast());

  Pointer<_NxSim> _sim;

  /// Throws ArgumentError when [faceBlob] isn't a valid 64-byte face blob.
  EngineSim(Uint8List faceBlob, {int seed = 0})
      : _sim = _b.simNew(seed & 0xFFFFFFFF) {
    if (_sim == nullptr) throw StateError('nixxa_engine: out of memory');
    _finalizer.attach(this, _sim.cast(), detach: this, externalSize: 12 * 1024);
    try {
      setFace(faceBlob);
    } catch (_) {
      dispose();
      rethrow;
    }
  }

  void _alive() {
    if (_sim == nullptr) throw StateError('EngineSim used after dispose()');
  }

  /// Switch face (restarts the animation timers with the same seed, like the
  /// desktop `sim_set_face`). The drawing, if any, is kept.
  /// Throws ArgumentError on a bad length, magic, version or CRC.
  void setFace(Uint8List faceBlob) {
    _alive();
    if (faceBlob.length != 64) {
      throw ArgumentError.value(faceBlob.length, 'faceBlob', 'must be 64 bytes');
    }
    final p = malloc<Uint8>(64);
    final int rc;
    try {
      p.asTypedList(64).setAll(0, faceBlob);
      rc = _b.setFace(_sim, p);
    } finally {
      malloc.free(p);
    }
    if (rc != 0) {
      const why = {-1: 'bad magic', -2: 'bad version', -3: 'bad CRC'};
      throw ArgumentError('Not a valid face blob (${why[rc] ?? rc}).');
    }
  }

  /// Show a drawing blob v2 instead of / over the face (null = none). Restarts
  /// the animation timers. An invalid or "off" blob shows the face alone, as
  /// the device does; returns whether a drawing is shown.
  bool setDrawing(Uint8List? drawingBlob) {
    _alive();
    if (drawingBlob == null || drawingBlob.isEmpty) {
      _b.setDrawing(_sim, nullptr, 0);
      return false;
    }
    final p = malloc<Uint8>(drawingBlob.length);
    try {
      p.asTypedList(drawingBlob.length).setAll(0, drawingBlob);
      return _b.setDrawing(_sim, p, drawingBlob.length) == 1;
    } finally {
      malloc.free(p);
    }
  }

  /// The 1024-byte frame at [tMs] (SSD1306 page layout:
  /// `fb[(y >> 3) * 128 + x]`, bit `y & 7`, 1 = lit). Call with
  /// non-decreasing times from a clock that starts near 0.
  Uint8List frame(int tMs) {
    _alive();
    final p = _b.render(_sim, tMs & 0xFFFFFFFF);
    return Uint8List.fromList(p.asTypedList(engineFrameBytes));
  }

  void dispose() {
    if (_sim == nullptr) return;
    _finalizer.detach(this);
    _b.simFree(_sim);
    _sim = nullptr;
  }

  /// Whether pixel (x, y) is lit in a page-layout frame.
  static bool pixel(Uint8List frame, int x, int y) =>
      frame[(y >> 3) * engineWidth + x] & (1 << (y & 7)) != 0;

  /// Expand a frame to 128x64 RGBA8888 pixels (for `ui.decodeImageFromPixels`
  /// with `PixelFormat.rgba8888`). Colours are 0xRRGGBBAA.
  static Uint8List toRgba(Uint8List frame, {int on = 0xFFFFFFFF, int off = 0x000000FF, Uint8List? into}) {
    final out = into ?? Uint8List(engineWidth * engineHeight * 4);
    final px = out.buffer.asUint32List(out.offsetInBytes, engineWidth * engineHeight);
    // asUint32List is host-endian (little on every Flutter target): store ABGR.
    int le(int c) => ((c & 0xFF) << 24) | ((c >> 8 & 0xFF) << 16) | ((c >> 16 & 0xFF) << 8) | (c >> 24 & 0xFF);
    final onLe = le(on), offLe = le(off);
    for (var y = 0; y < engineHeight; y++) {
      final row = (y >> 3) * engineWidth, bit = 1 << (y & 7), o = y * engineWidth;
      for (var x = 0; x < engineWidth; x++) {
        px[o + x] = frame[row + x] & bit != 0 ? onLe : offLe;
      }
    }
    return out;
  }
}
