// Faces saved on the phone, and the .deskbunny.json file form (desktop FaceDef
// JSON with an optional `drawing` key; see ARCHITECTURE.md "Tauri IPC").
import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

import '../model/drawing.dart';
import '../model/face.dart';
import '../model/face_file.dart';

class SavedFace {
  final FaceDef face;
  final Drawing? drawing;
  const SavedFace(this.face, [this.drawing]);
}

// ---------------------------------------------------------------- JSON

Map<String, Object?> savedFaceToJson(SavedFace s) =>
    faceFileToJson(s.face, s.drawing == null || s.drawing!.frames.isEmpty ? null : s.drawing);

SavedFace savedFaceFromJson(Map<String, Object?> json) {
  final (face, drawing) = faceFileFromJson(json);
  return SavedFace(face, drawing);
}

/// Parses an imported .deskbunny.json; throws FormatException with a readable message.
SavedFace parseFaceFile(String text) {
  final Object? decoded;
  try {
    decoded = jsonDecode(text);
  } on FormatException {
    throw const FormatException('That file is not JSON. Pick a .deskbunny.json face file.');
  }
  if (decoded is! Map) throw const FormatException('That file is not a DeskBunny face.');
  final m = decoded.cast<String, Object?>();
  if (m['name'] is! String && m['mood'] is! String) {
    throw const FormatException('That file is not a DeskBunny face.');
  }
  return savedFaceFromJson(m);
}

String faceFileText(SavedFace s) => const JsonEncoder.withIndent('  ').convert(savedFaceToJson(s));

String faceFileName(FaceDef face) {
  final n = face.name.trim().isEmpty ? 'FACE' : face.name.trim().replaceAll(' ', '_');
  return '$n.deskbunny.json';
}

// ---------------------------------------------------------------- storage

abstract class FaceStore {
  Future<List<SavedFace>> list();

  /// Replaces a face with the same name.
  Future<void> save(SavedFace face);
  Future<void> delete(String name);
}

class PrefsFaceStore implements FaceStore {
  static const _key = 'nixxa.faces.v1';

  @override
  Future<List<SavedFace>> list() async {
    final prefs = await SharedPreferences.getInstance();
    final raw = prefs.getString(_key);
    if (raw == null) return [];
    final out = <SavedFace>[];
    try {
      for (final item in (jsonDecode(raw) as List).whereType<Map>()) {
        try {
          out.add(savedFaceFromJson(item.cast<String, Object?>()));
        } on Object {
          // skip a corrupt entry, keep the rest
        }
      }
    } on Object {
      return [];
    }
    return out;
  }

  Future<void> _write(List<SavedFace> faces) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_key, jsonEncode([for (final f in faces) savedFaceToJson(f)]));
  }

  @override
  Future<void> save(SavedFace face) async {
    final faces = await list();
    faces.removeWhere((f) => f.face.name == face.face.name);
    faces.insert(0, face);
    await _write(faces);
  }

  @override
  Future<void> delete(String name) async {
    final faces = await list();
    faces.removeWhere((f) => f.face.name == name);
    await _write(faces);
  }
}

class MemoryFaceStore implements FaceStore {
  final List<SavedFace> faces;
  MemoryFaceStore([List<SavedFace>? initial]) : faces = [...?initial];

  @override
  Future<List<SavedFace>> list() async => List.of(faces);

  @override
  Future<void> save(SavedFace face) async {
    faces.removeWhere((f) => f.face.name == face.face.name);
    faces.insert(0, face);
  }

  @override
  Future<void> delete(String name) async => faces.removeWhere((f) => f.face.name == name);
}
