// Loads the Rust-generated fixtures (crates/deskbunny-core/tests/mobile_fixtures.rs).
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

Map<String, Object?> loadFixture(String name) {
  // `flutter test` runs with the package root (mobile/) as the cwd.
  final f = File('test/fixtures/$name');
  return jsonDecode(f.readAsStringSync()) as Map<String, Object?>;
}

Uint8List unhex(String s) => Uint8List.fromList(
    [for (var i = 0; i < s.length; i += 2) int.parse(s.substring(i, i + 2), radix: 16)]);

String hex(List<int> b) => b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();
