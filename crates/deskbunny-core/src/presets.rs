//! Built-in faces: one per mood plus a couple of extras. "WATCHER" is the default.

use crate::face::{FaceDef, Mood, Overlay};

/// Start from the engine default and tweak.
fn base(name: &str, mood: Mood) -> FaceDef {
    FaceDef {
        name: name.into(),
        mood,
        ..FaceDef::default()
    }
}

pub fn presets() -> Vec<FaceDef> {
    vec![
        FaceDef {
            // Keeps an eye on you: wide, frequent glances.
            look_x: 24,
            look_y: 10,
            look_min: 10,
            look_max: 35,
            ..base("WATCHER", Mood::Neutral)
        },
        FaceDef {
            eye_h: 32,
            eye_r: 12,
            mouth: true,
            speed: 6,
            ..base("HAPPY", Mood::Happy)
        },
        FaceDef {
            eye_w: 38,
            eye_h: 28,
            eye_r: 6,
            eye_gap: 12,
            look_x: 10,
            look_y: 4,
            overlay: Overlay::Anger,
            mouth: true,
            ..base("GRUMPY", Mood::Angry)
        },
        FaceDef {
            eye_h: 30,
            eye_y: 4,
            blink_min: 30,
            blink_max: 80,
            look_x: 10,
            look_y: 4,
            speed: 3,
            overlay: Overlay::Tears,
            ..base("SAD", Mood::Sad)
        },
        FaceDef {
            eye_h: 24,
            eye_y: 6,
            blink_min: 15,
            blink_max: 40,
            look_x: 8,
            look_y: 3,
            speed: 2,
            overlay: Overlay::Zzz,
            ..base("SLEEPY", Mood::Sleepy)
        },
        FaceDef {
            eye_w: 40,
            eye_h: 44,
            eye_r: 20,
            eye_gap: 14,
            eye_y: -2,
            blink_min: 30,
            blink_max: 70,
            mouth: true,
            ..base("WOW", Mood::Surprised)
        },
        FaceDef {
            eye_w: 40,
            eye_h: 38,
            eye_r: 14,
            overlay: Overlay::Hearts,
            speed: 6,
            ..base("LOVE", Mood::Love)
        },
        FaceDef {
            eye_h: 28,
            look_x: 22,
            look_y: 2,
            look_min: 20,
            look_max: 45,
            overlay: Overlay::Question,
            ..base("SUS", Mood::Suspicious)
        },
        FaceDef {
            eye_w: 38,
            eye_h: 40,
            eye_r: 14,
            look_x: 22,
            look_y: 10,
            look_min: 5,
            look_max: 18,
            blink_min: 10,
            blink_max: 30,
            speed: 9,
            overlay: Overlay::Sparkles,
            mouth: true,
            ..base("HYPER", Mood::Excited)
        },
        FaceDef {
            eye_h: 26,
            eye_y: 4,
            look_x: 12,
            look_y: 2,
            look_min: 30,
            look_max: 80,
            speed: 3,
            mouth: true,
            ..base("BORED", Mood::Bored)
        },
        FaceDef {
            eye_w: 30,
            eye_h: 40,
            eye_r: 14,
            eye_gap: 20,
            look_x: 20,
            look_y: 8,
            look_min: 5,
            look_max: 15,
            speed: 8,
            overlay: Overlay::Sweat,
            mouth: true,
            ..base("SCARED", Mood::Scared)
        },
        FaceDef {
            eye_w: 36,
            eye_h: 36,
            eye_r: 18,
            speed: 6,
            look_around: false,
            mouth: true,
            ..base("DIZZY", Mood::Dizzy)
        },
        FaceDef {
            eye_w: 48,
            eye_h: 44,
            eye_r: 16,
            cyclops: true,
            look_x: 20,
            look_y: 8,
            ..base("CYCLOPS", Mood::Neutral)
        },
        FaceDef {
            eye_w: 34,
            eye_h: 34,
            eye_r: 17,
            eye_gap: 18,
            look_x: 14,
            look_y: 6,
            speed: 4,
            ..base("BUBBLES", Mood::Happy)
        },
    ]
}

/// The face shown when nothing else is chosen.
pub fn default_preset() -> FaceDef {
    presets().remove(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn presets_are_valid_and_cover_every_mood() {
        let all = presets();
        assert!(all.len() >= 12);
        let mut names = HashSet::new();
        let mut moods = HashSet::new();
        for f in &all {
            f.validate().unwrap();
            assert_eq!(&f.sanitized(), f, "{} is not in range", f.name);
            assert_eq!(f.name, f.name.to_ascii_uppercase());
            assert!(names.insert(f.name.clone()), "duplicate {}", f.name);
            moods.insert(f.mood);
        }
        assert_eq!(moods.len(), Mood::ALL.len());
        assert_eq!(default_preset().name, "WATCHER");
    }
}
