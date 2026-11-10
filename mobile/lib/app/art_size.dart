// Bytes a drawing takes on the bunny (drawing blob v2, header included).
import '../model/drawing.dart';
import '../protocol/blob.dart';

/// null when the encoder is unavailable or the drawing can't be encoded.
int? artBytes(Drawing? d) {
  if (d == null || d.frames.isEmpty) return 0;
  try {
    return drawingSize(d);
  } on Object {
    return null;
  }
}

bool artFits(Drawing? d) {
  final n = artBytes(d);
  return n == null || n <= maxArtBytes;
}
