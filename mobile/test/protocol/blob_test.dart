import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/model/drawing.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/protocol/blob.dart';

import '../fixtures/fixtures.dart';

void main() {
  final faces = loadFixture('faces.json');
  final drawings = (loadFixture('drawings.json')['drawings'] as List).cast<Map<String, Object?>>();
  Map<String, Object?> obj(Object? v) => (v as Map).cast<String, Object?>();

  test('CRC-16/CCITT-FALSE check value', () {
    expect(crc16('123456789'.codeUnits), 0x29B1);
  });

  test('encodeFace is byte-identical to Rust', () {
    for (final c in (faces['faces'] as List).cast<Map<String, Object?>>()) {
      final f = FaceDef.fromJson(obj(c['json']));
      expect(hex(encodeFace(f)), c['blob'], reason: f.name);
    }
  });

  test('decodeFace matches Rust (including clamping of damaged fields)', () {
    final all = [
      ...(faces['faces'] as List).cast<Map<String, Object?>>(),
      ...(faces['decode'] as List).cast<Map<String, Object?>>(),
    ];
    for (final c in all) {
      final d = decodeFace(unhex(c['blob'] as String));
      expect(d.toJson(), c['decoded']);
    }
  });

  test('decodeFace rejects damage', () {
    Uint8List blob() => encodeFace(const FaceDef());
    expect(() => decodeFace(blob()..[40] ^= 1), throwsFormatException);
    expect(() => decodeFace(blob()..[0] = 0x58), throwsFormatException);
    expect(() => decodeFace(blob()..[8] = 2), throwsFormatException);
    expect(() => decodeFace(Uint8List(3)), throwsFormatException);
  });

  test('encodeDrawing / drawingSize are byte-identical to Rust', () {
    for (final c in drawings) {
      final d = Drawing.fromJson(obj(c['json']));
      final blob = encodeDrawing(d);
      expect(hex(blob), c['blob']);
      expect(drawingSize(d), c['size']);
      expect(blob.length, c['size']);
    }
  });

  test('decodeDrawing inverts encodeDrawing', () {
    for (final c in drawings) {
      final back = decodeDrawing(unhex(c['blob'] as String));
      expect(back, isNotNull);
      expect(back!.toJson(), c['normalised']);
    }
  });

  test('empty drawing encodes as the off blob; bad blobs decode to null', () {
    final off = encodeDrawing(const Drawing(mode: DrawingMode.over, frames: []));
    expect(off.length, 16);
    expect(String.fromCharCodes(off.sublist(0, 8)), 'DBART001');
    expect(off[8], 2);
    expect(off[9], 0);
    expect(decodeDrawing(off), isNull);
    final good = unhex(drawings[0]['blob'] as String);
    expect(decodeDrawing(Uint8List.fromList(good)..[0] = 0), isNull);
    expect(decodeDrawing(Uint8List.fromList(good)..[8] = 1), isNull);
    expect(decodeDrawing(Uint8List.fromList(good)..[9] = 3), isNull);
    expect(decodeDrawing(Uint8List.fromList(good)..[10] = 11), isNull);
    expect(decodeDrawing(Uint8List.sublistView(good, 0, good.length - 1)), isNull);
  });
}
