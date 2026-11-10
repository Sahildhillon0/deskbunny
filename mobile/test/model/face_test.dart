import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa/model/drawing.dart';
import 'package:nixxa/model/face.dart';
import 'package:nixxa/model/face_file.dart';
import 'package:nixxa/model/presets.dart';

import '../fixtures/fixtures.dart';

void main() {
  final fx = loadFixture('faces.json');
  final cases = (fx['faces'] as List).cast<Map<String, Object?>>();

  Map<String, Object?> obj(Object? v) => (v as Map).cast<String, Object?>();

  test('presets mirror presets.rs', () {
    expect(presets.length, 34);
    for (var i = 0; i < presets.length; i++) {
      expect(presets[i].toJson(), cases[i]['json'], reason: presets[i].name);
      expect(presets[i].normalised(), presets[i], reason: '${presets[i].name} is not in range');
    }
    expect(defaultPreset.name, 'WATCHER');
    expect(presets.map((p) => p.mood).toSet().length, Mood.values.length);
  });

  test('default FaceDef == db_face_default', () {
    expect(const FaceDef().toJson(), cases[34]['json']);
  });

  test('JSON round trip and normalised() match Rust sanitized()', () {
    for (final c in cases) {
      final f = FaceDef.fromJson(obj(c['json']));
      expect(f.toJson(), c['json']);
      expect(f.normalised().toJson(), c['normalised'], reason: f.name);
      expect(f.normalised().normalised(), f.normalised());
    }
  });

  test('toJson has the 28 desktop keys and lowercase enums', () {
    final j = const FaceDef().toJson();
    expect(j.length, 28);
    expect(j['mood'], 'neutral');
    expect(j['overlay'], 'none');
    expect(j['cycle'], 'off');
  });

  test('old JSON without cycle keys gets the defaults', () {
    final j = const FaceDef(cycle: CycleMode.random, cycleSeconds: 3, cycleOverlay: false).toJson()
      ..remove('cycle')
      ..remove('cycleSeconds')
      ..remove('cycleMoods')
      ..remove('cycleOverlay');
    expect(FaceDef.fromJson(j), const FaceDef());
  });

  test('fromJson is strict like serde', () {
    final ok = const FaceDef().toJson();
    expect(() => FaceDef.fromJson({...ok}..remove('eyeW')), throwsFormatException);
    expect(() => FaceDef.fromJson({...ok, 'eyeW': 256}), throwsFormatException);
    expect(() => FaceDef.fromJson({...ok, 'eyeY': -129}), throwsFormatException);
    expect(() => FaceDef.fromJson({...ok, 'mood': 'Angry'}), throwsFormatException);
    expect(() => FaceDef.fromJson({...ok, 'overlay': 'x'}), throwsFormatException);
    expect(() => FaceDef.fromJson({...ok, 'blink': 1}), throwsFormatException);
    expect(() => FaceDef.fromJson({...ok, 'cycleMoods': ['nope']}), throwsFormatException);
    // Unknown keys are ignored.
    expect(FaceDef.fromJson({...ok, 'extra': 1}), const FaceDef());
  });

  test('normalised clamps Dart ints to byte ranges', () {
    final f = const FaceDef(eyeY: 500, lookX: -3, contrast: 999, seed: 300, cycleSeconds: 256).normalised();
    expect([f.eyeY, f.lookX, f.contrast, f.seed, f.cycleSeconds], [127, 0, 255, 255, 255]);
  });

  test('name rules', () {
    expect(normalizeFaceName('  hello, world!!  '), 'HELLO  WORLD');
    expect(normalizeFaceName('éa'), 'A');
    expect(validateFaceName(''), isNotNull);
    expect(validateFaceName('THIRTEEN CHAR'), isNotNull);
    expect(validateFaceName('HI!'), isNotNull);
    expect(validateFaceName('lower ok'), isNull);
  });

  test('copyWith keeps everything else', () {
    final f = presets[33].copyWith(name: 'X');
    expect(f.name, 'X');
    expect(f.copyWith(name: presets[33].name), presets[33]);
  });

  test('face files carry an optional drawing', () {
    final d = Drawing(mode: DrawingMode.over, frameMs: 400, frames: [unhex('ff' * 1024)]);
    final j = faceFileToJson(presets[1], d);
    expect((j['drawing'] as Map)['frames'][0].length, 1368);
    final (face, drawing) = faceFileFromJson(jsonDecode(jsonEncode(j)) as Map<String, Object?>);
    expect(face, presets[1]);
    expect(drawing, d);
    expect(faceFileToJson(presets[1], null).containsKey('drawing'), isFalse);
    final (_, none) = faceFileFromJson({...presets[1].toJson(), 'drawing': null});
    expect(none, isNull);
  });
}
