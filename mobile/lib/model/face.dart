// Face definition: the Dart form of face blob v1 (docs/ARCHITECTURE.md).
// JSON shape == the desktop FaceDef JSON, so *.deskbunny.json files import.
// Mirrors crates/deskbunny-core/src/face.rs.

enum Mood {
  neutral, happy, angry, sad, sleepy, surprised, love, suspicious, excited,
  bored, scared, dizzy, wink, laugh, crying, skeptical, smug, evil, worried,
  shy, squint, dead, asleep, starry, hypno, glitch, scan, eyeroll, confused,
  determined,
}

enum FaceOverlay { none, hearts, zzz, sweat, anger, tears, sparkles, question }

enum CycleMode { off, sequence, random }

/// Longest face name (blob bytes 10..21).
const int faceNameMax = 12;

/// Most moods in a cycle list.
const int cycleMax = 12;

/// Uppercase, map unsupported characters to spaces, trim, cap at 12
/// (`normalize_name` in face.rs).
String normalizeFaceName(String name) {
  final mapped = StringBuffer();
  for (final r in name.runes) {
    var c = r;
    if (c >= 0x61 && c <= 0x7A) c -= 0x20; // ASCII lowercase only
    final ok = (c >= 0x41 && c <= 0x5A) || (c >= 0x30 && c <= 0x39) || c == 0x20;
    mapped.writeCharCode(ok ? c : 0x20);
  }
  final trimmed = mapped.toString().trim();
  final capped = trimmed.length > faceNameMax ? trimmed.substring(0, faceNameMax) : trimmed;
  return capped.trimRight();
}

/// Why a face name can't be saved (`FaceDef::validate` in face.rs), or null.
String? validateFaceName(String name) {
  final n = name.trim();
  if (n.isEmpty) return 'Face name is empty. Give your face a name (A-Z, 0-9 and spaces).';
  if (n.runes.length > faceNameMax) return "Face name '$n' is too long: at most 12 characters.";
  final ok = n.runes.every((c) =>
      (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) || (c >= 0x30 && c <= 0x39) || c == 0x20);
  if (!ok) {
    return "Face name '$n' has characters the OLED font cannot show. Use A-Z, 0-9 and spaces.";
  }
  return null;
}

class FaceDef {
  final String name; // <= 12 chars, A-Z 0-9 space after normalised()
  final Mood mood;
  final int eyeW, eyeH, eyeR, eyeGap, eyeY;
  final bool blink, lookAround, breathe, cyclops, invert, rotate180, mouth;
  final int blinkMin, blinkMax, lookMin, lookMax, lookX, lookY;
  final int speed, contrast, seed;
  final FaceOverlay overlay;
  final CycleMode cycle;
  final int cycleSeconds;
  final List<Mood> cycleMoods; // <= 12, empty = all moods
  final bool cycleOverlay;

  const FaceDef({
    this.name = 'DESKBUNNY',
    this.mood = Mood.neutral,
    this.eyeW = 36, this.eyeH = 36, this.eyeR = 8, this.eyeGap = 16, this.eyeY = 0,
    this.blink = true, this.lookAround = true, this.breathe = true,
    this.cyclops = false, this.invert = false, this.rotate180 = false, this.mouth = false,
    this.blinkMin = 20, this.blinkMax = 60, this.lookMin = 15, this.lookMax = 50,
    this.lookX = 18, this.lookY = 8,
    this.speed = 5, this.contrast = 0xCF, this.seed = 0,
    this.overlay = FaceOverlay.none,
    this.cycle = CycleMode.off, this.cycleSeconds = 10,
    this.cycleMoods = const [], this.cycleOverlay = true,
  });

  FaceDef copyWith({
    String? name, Mood? mood, int? eyeW, int? eyeH, int? eyeR, int? eyeGap, int? eyeY,
    bool? blink, bool? lookAround, bool? breathe, bool? cyclops, bool? invert,
    bool? rotate180, bool? mouth, int? blinkMin, int? blinkMax, int? lookMin,
    int? lookMax, int? lookX, int? lookY, int? speed, int? contrast, int? seed,
    FaceOverlay? overlay, CycleMode? cycle, int? cycleSeconds,
    List<Mood>? cycleMoods, bool? cycleOverlay,
  }) =>
      FaceDef(
        name: name ?? this.name,
        mood: mood ?? this.mood,
        eyeW: eyeW ?? this.eyeW,
        eyeH: eyeH ?? this.eyeH,
        eyeR: eyeR ?? this.eyeR,
        eyeGap: eyeGap ?? this.eyeGap,
        eyeY: eyeY ?? this.eyeY,
        blink: blink ?? this.blink,
        lookAround: lookAround ?? this.lookAround,
        breathe: breathe ?? this.breathe,
        cyclops: cyclops ?? this.cyclops,
        invert: invert ?? this.invert,
        rotate180: rotate180 ?? this.rotate180,
        mouth: mouth ?? this.mouth,
        blinkMin: blinkMin ?? this.blinkMin,
        blinkMax: blinkMax ?? this.blinkMax,
        lookMin: lookMin ?? this.lookMin,
        lookMax: lookMax ?? this.lookMax,
        lookX: lookX ?? this.lookX,
        lookY: lookY ?? this.lookY,
        speed: speed ?? this.speed,
        contrast: contrast ?? this.contrast,
        seed: seed ?? this.seed,
        overlay: overlay ?? this.overlay,
        cycle: cycle ?? this.cycle,
        cycleSeconds: cycleSeconds ?? this.cycleSeconds,
        cycleMoods: cycleMoods ?? this.cycleMoods,
        cycleOverlay: cycleOverlay ?? this.cycleOverlay,
      );

  /// Clamp every field to its documented range and normalise the name,
  /// exactly like the Rust backend (`FaceDef::sanitized`). Dart ints are
  /// unbounded, so every byte field is first clamped to its blob type
  /// (`eyeY` -128..127, the rest 0..255), which Rust gets from its types.
  FaceDef normalised() {
    int u8(int v) => v.clamp(0, 255);
    final w = u8(eyeW).clamp(8, 56);
    final bMin = u8(blinkMin).clamp(1, 255);
    final lMin = u8(lookMin).clamp(1, 255);
    final secs = u8(cycleSeconds);
    return FaceDef(
      name: normalizeFaceName(name),
      mood: mood,
      eyeW: w,
      eyeH: u8(eyeH).clamp(8, 56),
      eyeR: u8(eyeR).clamp(0, w ~/ 2),
      eyeGap: u8(eyeGap).clamp(0, 40),
      eyeY: eyeY.clamp(-128, 127),
      blink: blink,
      lookAround: lookAround,
      breathe: breathe,
      cyclops: cyclops,
      invert: invert,
      rotate180: rotate180,
      mouth: mouth,
      blinkMin: bMin,
      blinkMax: u8(blinkMax).clamp(bMin, 255),
      lookMin: lMin,
      lookMax: u8(lookMax).clamp(lMin, 255),
      lookX: u8(lookX),
      lookY: u8(lookY),
      speed: u8(speed).clamp(1, 10),
      contrast: u8(contrast),
      seed: u8(seed),
      overlay: overlay,
      cycle: cycle,
      cycleSeconds: secs == 0 ? 10 : secs,
      cycleMoods: List.unmodifiable(cycleMoods.take(cycleMax)),
      cycleOverlay: cycleOverlay,
    );
  }

  /// Desktop FaceDef JSON (28 keys; `drawing` is added by faceFileToJson).
  Map<String, Object?> toJson() => {
        'name': name,
        'mood': mood.name,
        'eyeW': eyeW,
        'eyeH': eyeH,
        'eyeR': eyeR,
        'eyeGap': eyeGap,
        'eyeY': eyeY,
        'blink': blink,
        'lookAround': lookAround,
        'breathe': breathe,
        'cyclops': cyclops,
        'invert': invert,
        'rotate180': rotate180,
        'mouth': mouth,
        'blinkMin': blinkMin,
        'blinkMax': blinkMax,
        'lookMin': lookMin,
        'lookMax': lookMax,
        'lookX': lookX,
        'lookY': lookY,
        'speed': speed,
        'contrast': contrast,
        'overlay': overlay.name,
        'seed': seed,
        'cycle': cycle.name,
        'cycleSeconds': cycleSeconds,
        'cycleMoods': cycleMoods.map((m) => m.name).toList(),
        'cycleOverlay': cycleOverlay,
      };

  /// Parse desktop FaceDef JSON with the same strictness as serde: every
  /// non-cycle key is required, numbers must fit their byte type, enums must
  /// be known lowercase names. The four cycle keys default to off/10/[]/true.
  /// Unknown keys (including `drawing`) are ignored. Throws FormatException.
  static FaceDef fromJson(Map<String, Object?> json) {
    T req<T>(String k) {
      final v = json[k];
      if (v is! T) {
        throw FormatException(v == null ? "Face is missing '$k'." : "Face field '$k' has the wrong type.");
      }
      return v;
    }

    int num(String k, {int lo = 0, int hi = 255, int? dflt}) {
      final Object? raw = json[k];
      if (raw == null && dflt != null) return dflt;
      final v = raw is double && raw == raw.truncateToDouble() ? raw.toInt() : raw;
      if (v is! int) {
        throw FormatException(raw == null ? "Face is missing '$k'." : "Face field '$k' must be a whole number.");
      }
      if (v < lo || v > hi) throw FormatException("Face field '$k' = $v is out of range $lo..$hi.");
      return v;
    }

    E enumOf<E extends Enum>(List<E> values, Object? v, String k) {
      if (v is String) {
        for (final e in values) {
          if (e.name == v) return e;
        }
      }
      throw FormatException("Face field '$k' has an unknown value '$v'.");
    }

    bool flag(String k, {bool? dflt}) {
      final v = json[k];
      if (v == null && dflt != null) return dflt;
      if (v is! bool) throw FormatException("Face field '$k' must be true or false.");
      return v;
    }

    final rawMoods = json['cycleMoods'];
    final List<Mood> moods;
    if (rawMoods == null) {
      moods = const [];
    } else if (rawMoods is List) {
      moods = List.unmodifiable(rawMoods.map((m) => enumOf(Mood.values, m, 'cycleMoods')));
    } else {
      throw const FormatException("Face field 'cycleMoods' must be a list.");
    }

    return FaceDef(
      name: req<String>('name'),
      mood: enumOf(Mood.values, json['mood'], 'mood'),
      eyeW: num('eyeW'),
      eyeH: num('eyeH'),
      eyeR: num('eyeR'),
      eyeGap: num('eyeGap'),
      eyeY: num('eyeY', lo: -128, hi: 127),
      blink: flag('blink'),
      lookAround: flag('lookAround'),
      breathe: flag('breathe'),
      cyclops: flag('cyclops'),
      invert: flag('invert'),
      rotate180: flag('rotate180'),
      mouth: flag('mouth'),
      blinkMin: num('blinkMin'),
      blinkMax: num('blinkMax'),
      lookMin: num('lookMin'),
      lookMax: num('lookMax'),
      lookX: num('lookX'),
      lookY: num('lookY'),
      speed: num('speed'),
      contrast: num('contrast'),
      overlay: enumOf(FaceOverlay.values, json['overlay'], 'overlay'),
      seed: num('seed'),
      cycle: json['cycle'] == null ? CycleMode.off : enumOf(CycleMode.values, json['cycle'], 'cycle'),
      cycleSeconds: num('cycleSeconds', dflt: 10),
      cycleMoods: moods,
      cycleOverlay: flag('cycleOverlay', dflt: true),
    );
  }

  @override
  bool operator ==(Object other) {
    if (other is! FaceDef) return false;
    if (other.cycleMoods.length != cycleMoods.length) return false;
    for (var i = 0; i < cycleMoods.length; i++) {
      if (other.cycleMoods[i] != cycleMoods[i]) return false;
    }
    return other.name == name && other.mood == mood && other.eyeW == eyeW &&
        other.eyeH == eyeH && other.eyeR == eyeR && other.eyeGap == eyeGap &&
        other.eyeY == eyeY && other.blink == blink && other.lookAround == lookAround &&
        other.breathe == breathe && other.cyclops == cyclops && other.invert == invert &&
        other.rotate180 == rotate180 && other.mouth == mouth &&
        other.blinkMin == blinkMin && other.blinkMax == blinkMax &&
        other.lookMin == lookMin && other.lookMax == lookMax &&
        other.lookX == lookX && other.lookY == lookY && other.speed == speed &&
        other.contrast == contrast && other.seed == seed && other.overlay == overlay &&
        other.cycle == cycle && other.cycleSeconds == cycleSeconds &&
        other.cycleOverlay == cycleOverlay;
  }

  @override
  int get hashCode => Object.hashAll([
        name, mood, eyeW, eyeH, eyeR, eyeGap, eyeY, blink, lookAround, breathe,
        cyclops, invert, rotate180, mouth, blinkMin, blinkMax, lookMin, lookMax,
        lookX, lookY, speed, contrast, seed, overlay, cycle, cycleSeconds,
        Object.hashAll(cycleMoods), cycleOverlay,
      ]);

  @override
  String toString() => 'FaceDef(${toJson()})';
}
