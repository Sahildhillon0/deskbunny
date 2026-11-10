// Built-in faces, mirroring crates/deskbunny-core/src/presets.rs (checked
// against the Rust fixture in test/model/face_test.dart). "WATCHER" is the default.
import 'face.dart';

FaceDef _base(String name, Mood mood) => FaceDef(name: name, mood: mood);

final List<FaceDef> presets = List.unmodifiable(<FaceDef>[
  _base('WATCHER', Mood.neutral).copyWith(lookX: 24, lookY: 10, lookMin: 10, lookMax: 35),
  _base('HAPPY', Mood.happy).copyWith(eyeH: 32, eyeR: 12, mouth: true, speed: 6),
  _base('GRUMPY', Mood.angry).copyWith(
      eyeW: 38, eyeH: 28, eyeR: 6, eyeGap: 12, lookX: 10, lookY: 4,
      overlay: FaceOverlay.anger, mouth: true),
  _base('SAD', Mood.sad).copyWith(
      eyeH: 30, eyeY: 4, blinkMin: 30, blinkMax: 80, lookX: 10, lookY: 4, speed: 3,
      overlay: FaceOverlay.tears),
  _base('SLEEPY', Mood.sleepy).copyWith(
      eyeH: 24, eyeY: 6, blinkMin: 15, blinkMax: 40, lookX: 8, lookY: 3, speed: 2,
      overlay: FaceOverlay.zzz),
  _base('WOW', Mood.surprised).copyWith(
      eyeW: 40, eyeH: 44, eyeR: 20, eyeGap: 14, eyeY: -2, blinkMin: 30, blinkMax: 70,
      mouth: true),
  _base('LOVE', Mood.love).copyWith(
      eyeW: 40, eyeH: 38, eyeR: 14, overlay: FaceOverlay.hearts, speed: 6),
  _base('SUS', Mood.suspicious).copyWith(
      eyeH: 28, lookX: 22, lookY: 2, lookMin: 20, lookMax: 45,
      overlay: FaceOverlay.question),
  _base('HYPER', Mood.excited).copyWith(
      eyeW: 38, eyeH: 40, eyeR: 14, lookX: 22, lookY: 10, lookMin: 5, lookMax: 18,
      blinkMin: 10, blinkMax: 30, speed: 9, overlay: FaceOverlay.sparkles, mouth: true),
  _base('BORED', Mood.bored).copyWith(
      eyeH: 26, eyeY: 4, lookX: 12, lookY: 2, lookMin: 30, lookMax: 80, speed: 3,
      mouth: true),
  _base('SCARED', Mood.scared).copyWith(
      eyeW: 30, eyeH: 40, eyeR: 14, eyeGap: 20, lookX: 20, lookY: 8, lookMin: 5,
      lookMax: 15, speed: 8, overlay: FaceOverlay.sweat, mouth: true),
  _base('DIZZY', Mood.dizzy).copyWith(
      eyeW: 36, eyeH: 36, eyeR: 18, speed: 6, lookAround: false, mouth: true),
  _base('WINKY', Mood.wink).copyWith(mouth: true),
  _base('LOL', Mood.laugh).copyWith(mouth: true, overlay: FaceOverlay.sparkles, speed: 7),
  _base('WEEPY', Mood.crying).copyWith(overlay: FaceOverlay.tears, lookAround: false),
  _base('REALLY', Mood.skeptical).copyWith(overlay: FaceOverlay.question),
  _base('SMUG', Mood.smug).copyWith(mouth: true, speed: 4),
  _base('VILLAIN', Mood.evil).copyWith(overlay: FaceOverlay.anger),
  _base('NERVOUS', Mood.worried).copyWith(overlay: FaceOverlay.sweat, speed: 7),
  _base('BASHFUL', Mood.shy).copyWith(overlay: FaceOverlay.hearts, lookAround: false),
  _base('SQUINT', Mood.squint).copyWith(speed: 4),
  _base('KO', Mood.dead).copyWith(mouth: true, lookAround: false),
  _base('ASLEEP', Mood.asleep).copyWith(overlay: FaceOverlay.zzz, speed: 3),
  _base('STARSTRUCK', Mood.starry).copyWith(overlay: FaceOverlay.sparkles),
  _base('HYPNO', Mood.hypno).copyWith(lookAround: false),
  _base('GLITCH', Mood.glitch).copyWith(speed: 6),
  _base('SCANNER', Mood.scan).copyWith(lookAround: false, speed: 4),
  _base('EYE ROLL', Mood.eyeroll).copyWith(lookAround: false, speed: 4),
  _base('HUH', Mood.confused).copyWith(overlay: FaceOverlay.question),
  _base('FOCUSED', Mood.determined).copyWith(speed: 6),
  _base('CYCLOPS', Mood.neutral).copyWith(
      eyeW: 48, eyeH: 44, eyeR: 16, cyclops: true, lookX: 20, lookY: 8),
  _base('BUBBLES', Mood.happy).copyWith(
      eyeW: 34, eyeH: 34, eyeR: 17, eyeGap: 18, lookX: 14, lookY: 6, speed: 4),
  // A new mood every 8 s, any of the thirty.
  _base('MOODY', Mood.neutral).copyWith(mouth: true, cycle: CycleMode.random, cycleSeconds: 8),
  // A little day: awake, happy, bored, sleepy, then round again.
  _base('MY DAY', Mood.neutral).copyWith(
      mouth: true, cycle: CycleMode.sequence, cycleSeconds: 12,
      cycleMoods: const [Mood.neutral, Mood.happy, Mood.excited, Mood.bored, Mood.sleepy]),
]);

/// The face shown when nothing else is chosen.
FaceDef get defaultPreset => presets.first;
