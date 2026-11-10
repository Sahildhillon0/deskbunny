// Golden test: EngineSim frames == the desktop simulator's (Rust Engine),
// from mobile/test/fixtures/engine_frames.json (crates/deskbunny-core/tests/mobile_fixtures.rs).
//
// Builds the host library itself with `cc` (Linux/macOS), so plain
// `flutter test` in this package works. To use a prebuilt one instead:
//   NIXXA_ENGINE_LIB=/path/libnixxa_engine.so flutter test
@TestOn('linux || mac-os')
library;

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:nixxa_engine/nixxa_engine.dart';

Uint8List unhex(String s) => Uint8List.fromList(
    [for (var i = 0; i < s.length; i += 2) int.parse(s.substring(i, i + 2), radix: 16)]);

void main() {
  setUpAll(() {
    if (Platform.environment['NIXXA_ENGINE_LIB'] != null) return;
    final out = Directory('.dart_tool/nixxa_engine_host')..createSync(recursive: true);
    final lib = '${out.path}/libnixxa_engine.${Platform.isMacOS ? 'dylib' : 'so'}';
    final r = Process.runSync('cc', [
      '-std=c11', '-O2', '-shared', '-fPIC', '-o', lib,
      'src/deskbunny_engine.c', 'src/nixxa_engine.c', 'src/nx_clock.c',
    ]);
    if (r.exitCode != 0) fail('cc failed:\n${r.stderr}');
    EngineSim.libraryPath = File(lib).absolute.path;
  });

  final fixture = jsonDecode(File('../../test/fixtures/engine_frames.json').readAsStringSync())
      as Map<String, Object?>;
  final runs = (fixture['runs'] as List).cast<Map<String, Object?>>();

  test('plugin engine sources are current copies of engine/ and the firmware clock module', () {
    final sources = {
      'deskbunny_engine.c': '../../../engine/deskbunny_engine.c',
      'deskbunny_engine.h': '../../../engine/deskbunny_engine.h',
      'nx_clock.c': '../../../firmware/nixxa_esp32/nx_clock.c',
      'nx_clock.h': '../../../firmware/nixxa_esp32/nx_clock.h',
      'nx_font5x7.h': '../../../firmware/nixxa_esp32/nx_font5x7.h',
    };
    sources.forEach((f, origin) {
      expect(File('src/$f').readAsStringSync(), File(origin).readAsStringSync(),
          reason: 'run mobile/tool/sync_engine.sh ($f)');
    });
  });

  for (final run in runs) {
    test('frames match the desktop simulator: ${run['name']} seed ${run['seed']}'
        '${run['drawingBlob'] != null ? ' + drawing' : ''}', () {
      final sim = EngineSim(unhex(run['faceBlob'] as String), seed: run['seed'] as int);
      final d = run['drawingBlob'] as String?;
      if (d != null) expect(sim.setDrawing(unhex(d)), isTrue);
      final times = (run['times'] as List).cast<int>();
      final frames = (run['frames'] as List).cast<String>();
      for (var i = 0; i < times.length; i++) {
        expect(base64.encode(sim.frame(times[i])), frames[i], reason: 't=${times[i]}');
      }
      sim.dispose();
    });
  }

  test('setFace restarts like a fresh engine; drawing can be cleared', () {
    final a = unhex(runs[0]['faceBlob'] as String), b = unhex(runs[1]['faceBlob'] as String);
    final sim = EngineSim(a, seed: 3)..frame(0)..frame(500);
    sim.setFace(b);
    final fresh = EngineSim(b, seed: 3);
    for (final t in [0, 33, 66, 1000]) {
      expect(sim.frame(t), fresh.frame(t));
    }
    final withDrawing = runs.firstWhere((r) => r['drawingBlob'] != null);
    final s2 = EngineSim(unhex(withDrawing['faceBlob'] as String), seed: withDrawing['seed'] as int);
    final plain = s2.frame(0);
    expect(s2.setDrawing(unhex(withDrawing['drawingBlob'] as String)), isTrue);
    expect(s2.setDrawing(null), isFalse);
    expect(s2.frame(0), plain);
    expect(s2.setDrawing(Uint8List.fromList([1, 2, 3])), isFalse);
    for (final s in [sim, fresh, s2]) {
      s.dispose();
    }
    sim.dispose(); // idempotent
    expect(() => sim.frame(0), throwsStateError);
  });

  test('bad face blobs are rejected', () {
    final ok = unhex(runs[0]['faceBlob'] as String);
    expect(() => EngineSim(Uint8List(10)), throwsArgumentError);
    expect(() => EngineSim(Uint8List.fromList(ok)..[40] ^= 1), throwsArgumentError);
  });

  test('toRgba', () {
    final f = Uint8List(1024)..[0] = 0x01;
    final px = EngineSim.toRgba(f, on: 0x11223344, off: 0x000000FF);
    expect(px.sublist(0, 8), [0x11, 0x22, 0x33, 0x44, 0, 0, 0, 0xFF]);
    expect(EngineSim.pixel(f, 0, 0), isTrue);
    expect(EngineSim.pixel(f, 0, 1), isFalse);
  });

  test('clock frames: 1024 bytes, deterministic, every style named', () {
    for (var style = 1; style <= 20; style++) {
      final a = clockFrame(style, 10, 8, 30);
      expect(a.length, 1024, reason: 'style $style');
      expect(a.any((b) => b != 0), isTrue, reason: 'style $style draws something');
      expect(clockFrame(style, 10, 8, 30), a, reason: 'style $style');
      expect(clockName(style), isNotEmpty, reason: 'style $style');
    }
    expect(clockFrame(1, 10, 8, 30), isNot(clockFrame(1, 10, 8, 31)));
  });
}
