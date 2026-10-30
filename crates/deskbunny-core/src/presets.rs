//! Built-in drone scenes: one per flight animation plus a few extras.
//! "HOVER" is the default.

use crate::face::{Cycle, FaceDef, Mood, Overlay};

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
            // Holds station, drifting a little, lights blinking.
            mouth: true,
            ..base("HOVER", Mood::Hover)
        },
        FaceDef {
            eye_w: 30,
            look_around: false,
            overlay: Overlay::City,
            ..base("PATROL", Mood::Patrol)
        },
        FaceDef {
            look_x: 10,
            look_y: 4,
            overlay: Overlay::Hills,
            ..base("LIFTOFF", Mood::Takeoff)
        },
        FaceDef {
            speed: 6,
            mouth: true,
            ..base("STUNT", Mood::Flip)
        },
        FaceDef {
            look_around: false,
            overlay: Overlay::Clouds,
            ..base("ORBIT", Mood::Orbit)
        },
        FaceDef {
            eye_gap: 12,
            look_x: 10,
            look_y: 4,
            mouth: true,
            ..base("SQUADRON", Mood::Formation)
        },
        FaceDef {
            look_x: 8,
            look_y: 2,
            overlay: Overlay::City,
            ..base("COURIER", Mood::Delivery)
        },
        FaceDef {
            eye_w: 32,
            speed: 7,
            look_around: false,
            overlay: Overlay::Wind,
            ..base("RACER", Mood::Race)
        },
        FaceDef {
            look_x: 12,
            look_y: 2,
            overlay: Overlay::Grid,
            ..base("SEARCH", Mood::Scan)
        },
        FaceDef {
            look_around: false,
            overlay: Overlay::Stars,
            ..base("SWARM", Mood::Swarm)
        },
        FaceDef {
            eye_w: 40,
            look_x: 12,
            look_y: 6,
            speed: 4,
            ..base("TOP VIEW", Mood::Topdown)
        },
        FaceDef {
            look_x: 8,
            look_y: 4,
            ..base("RECHARGE", Mood::Charge)
        },
        FaceDef {
            // Stormy night watch from above.
            eye_w: 44,
            blink_min: 10,
            cyclops: true,
            overlay: Overlay::Rain,
            ..base("STORM", Mood::Topdown)
        },
        FaceDef {
            // Big, slow, close up.
            eye_w: 56,
            eye_y: 4,
            speed: 3,
            cyclops: true,
            mouth: true,
            ..base("HEAVY LIFT", Mood::Hover)
        },
        FaceDef {
            // A new scene every 10 s, any of the twelve, sky to match.
            mouth: true,
            cycle: Cycle::Random,
            cycle_seconds: 10,
            ..base("AIRSHOW", Mood::Hover)
        },
        FaceDef {
            // A shift: take off, patrol, deliver, scan, recharge, round again.
            cycle: Cycle::Sequence,
            cycle_seconds: 14,
            cycle_moods: vec![
                Mood::Takeoff,
                Mood::Patrol,
                Mood::Delivery,
                Mood::Scan,
                Mood::Charge,
            ],
            ..base("NIGHT SHIFT", Mood::Takeoff)
        },
    ]
}

/// The scene shown when nothing else is chosen.
pub fn default_preset() -> FaceDef {
    presets().remove(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn presets_are_valid_and_cover_every_scene() {
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
        assert_eq!(default_preset().name, "HOVER");
    }
}
