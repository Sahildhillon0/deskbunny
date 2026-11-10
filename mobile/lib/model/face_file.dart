// `.deskbunny.json` files: the desktop FaceDef JSON plus its optional
// `drawing` key (absent or null = no drawing).
import 'drawing.dart';
import 'face.dart';

/// JSON for export. `drawing` is omitted when null, as the desktop does.
Map<String, Object?> faceFileToJson(FaceDef face, Drawing? drawing) => {
      ...face.toJson(),
      if (drawing != null) 'drawing': drawing.toJson(),
    };

/// Parse an imported file: strict FaceDef, then normalised like the desktop's
/// import (clamped; drawing sanitised, an empty one becomes null).
/// Throws FormatException.
(FaceDef, Drawing?) faceFileFromJson(Map<String, Object?> json) {
  final face = FaceDef.fromJson(json).normalised();
  final d = json['drawing'];
  if (d == null) return (face, null);
  if (d is! Map) throw const FormatException("Face field 'drawing' must be an object.");
  return (face, Drawing.fromJson(d.cast<String, Object?>()).sanitized());
}
