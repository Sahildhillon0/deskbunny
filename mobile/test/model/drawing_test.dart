import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/model/drawing.dart';

import '../fixtures/fixtures.dart';

void main() {
  final cases = (loadFixture('drawings.json')['drawings'] as List).cast<Map<String, Object?>>();
  Map<String, Object?> obj(Object? v) => (v as Map).cast<String, Object?>();

  test('JSON round trip and sanitized() match Rust', () {
    for (final c in cases) {
      final d = Drawing.fromJson(obj(c['json']));
      expect(d.toJson(), c['json']);
      expect(d.sanitized()!.toJson(), c['normalised']);
    }
  });

  test('fromJson defaults and errors', () {
    final d = Drawing.fromJson({'frames': <Object?>[]});
    expect(d.mode, DrawingMode.replace);
    expect(d.frameMs, 500);
    expect(d.sanitized(), isNull);
    expect(() => Drawing.fromJson({'frames': ['@@@']}), throwsFormatException);
    expect(() => Drawing.fromJson({'mode': 'off', 'frames': <Object?>[]}), throwsFormatException);
    expect(() => Drawing.fromJson({}), throwsFormatException);
  });

  test('validate reports what sanitized would fix', () {
    final ok = Drawing(mode: DrawingMode.replace, frames: [Uint8List(1024)]);
    expect(ok.isValid, isTrue);
    expect(ok.copyWith(frameMs: 10).validate(), isNotEmpty);
    expect(ok.copyWith(frames: [Uint8List(3)]).validate(), isNotEmpty);
    expect(ok.copyWith(frames: List.generate(11, (_) => Uint8List(1024))).validate(), isNotEmpty);
    expect(ok.copyWith(frames: []).validate(), isNotEmpty);
  });

  test('pixels and frame timing', () {
    final f = Uint8List(1024);
    Drawing.setPixel(f, 127, 63, true);
    expect(f[1023], 0x80);
    expect(Drawing.pixel(f, 127, 63), isTrue);
    Drawing.setPixel(f, 127, 63, false);
    expect(f[1023], 0);
    final d = Drawing(mode: DrawingMode.replace, frameMs: 100, frames: [f, f, f]);
    expect([d.frameIndexAt(0), d.frameIndexAt(199), d.frameIndexAt(300)], [0, 1, 0]);
  });
}
