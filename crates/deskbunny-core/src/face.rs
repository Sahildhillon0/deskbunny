//! `FaceDef`: the JSON form of the 64-byte face blob (docs/ARCHITECTURE.md).
//! Blob encode/decode go through the C engine so firmware and app cannot drift.

use serde::{Deserialize, Serialize};

use crate::drawing::Drawing;

use crate::ffi::{self, db_face_t};

pub const BLOB_SIZE: usize = ffi::DB_BLOB_SIZE;
pub const NAME_MAX: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mood {
    Neutral,
    Happy,
    Angry,
    Sad,
    Sleepy,
    Surprised,
    Love,
    Suspicious,
    Excited,
    Bored,
    Scared,
    Dizzy,
}

impl Mood {
    pub const ALL: [Mood; 12] = [
        Mood::Neutral,
        Mood::Happy,
        Mood::Angry,
        Mood::Sad,
        Mood::Sleepy,
        Mood::Surprised,
        Mood::Love,
        Mood::Suspicious,
        Mood::Excited,
        Mood::Bored,
        Mood::Scared,
        Mood::Dizzy,
    ];

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Mood> {
        Mood::ALL.get(v as usize).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Overlay {
    None,
    Hearts,
    Zzz,
    Sweat,
    Anger,
    Tears,
    Sparkles,
    Question,
}

impl Overlay {
    pub const ALL: [Overlay; 8] = [
        Overlay::None,
        Overlay::Hearts,
        Overlay::Zzz,
        Overlay::Sweat,
        Overlay::Anger,
        Overlay::Tears,
        Overlay::Sparkles,
        Overlay::Question,
    ];

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Overlay> {
        Overlay::ALL.get(v as usize).copied()
    }
}

/// Mood cycle mode (blob byte 38).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cycle {
    /// One mood, forever.
    #[default]
    Off,
    /// Loop through `cycle_moods` in order.
    Sequence,
    /// Pick from `cycle_moods` at random.
    Random,
}

impl Cycle {
    pub const ALL: [Cycle; 3] = [Cycle::Off, Cycle::Sequence, Cycle::Random];

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Option<Cycle> {
        Cycle::ALL.get(v as usize).copied()
    }
}

pub const CYCLE_MAX: usize = ffi::DB_CYCLE_MAX;

fn default_cycle_seconds() -> u8 {
    10
}

fn default_true() -> bool {
    true
}

/// A face, as exchanged with the frontend (camelCase JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceDef {
    pub name: String,
    pub mood: Mood,
    pub eye_w: u8,
    pub eye_h: u8,
    pub eye_r: u8,
    pub eye_gap: u8,
    pub eye_y: i8,
    pub blink: bool,
    pub look_around: bool,
    pub breathe: bool,
    pub cyclops: bool,
    pub invert: bool,
    pub rotate180: bool,
    pub mouth: bool,
    pub blink_min: u8,
    pub blink_max: u8,
    pub look_min: u8,
    pub look_max: u8,
    pub look_x: u8,
    pub look_y: u8,
    pub speed: u8,
    pub contrast: u8,
    pub overlay: Overlay,
    pub seed: u8,
    /// The cycle fields are optional in JSON: faces saved before they existed load with the cycle off.
    #[serde(default)]
    pub cycle: Cycle,
    #[serde(default = "default_cycle_seconds")]
    pub cycle_seconds: u8,
    /// Empty = every mood.
    #[serde(default)]
    pub cycle_moods: Vec<Mood>,
    /// The overlay follows the mood while cycling.
    #[serde(default = "default_true")]
    pub cycle_overlay: bool,
    /// Frames drawn in the app, shown instead of or over the face. Not part of
    /// the 64-byte face blob: it has its own blob (crate::drawing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing: Option<Drawing>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FaceError {
    #[error("Face name is empty. Give your face a name (A-Z, 0-9 and spaces).")]
    EmptyName,
    #[error("Face name '{0}' is too long: at most 12 characters.")]
    NameTooLong(String),
    #[error("Face name '{0}' has characters the OLED font cannot show. Use A-Z, 0-9 and spaces.")]
    NameCharset(String),
    #[error("Face data is not a DeskBunny face (bad magic).")]
    BadMagic,
    #[error("Face data has an unsupported version.")]
    BadVersion,
    #[error("Face data is corrupted (CRC mismatch).")]
    BadCrc,
    #[error("Face data must be exactly 64 bytes, got {0}.")]
    BadLength(usize),
}

impl Default for FaceDef {
    /// The engine's built-in default face (`db_face_default`).
    fn default() -> Self {
        let mut raw = db_face_t::default();
        // SAFETY: db_face_default only writes the struct.
        unsafe { ffi::db_face_default(&mut raw) };
        FaceDef::from_raw(&raw)
    }
}

/// Uppercase, map unsupported characters to spaces, trim, cap at 12.
pub fn normalize_name(name: &str) -> String {
    let up: String = name
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .map(|c| {
            if c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let trimmed: String = up.trim().chars().take(NAME_MAX).collect();
    trimmed.trim_end().to_string()
}

impl FaceDef {
    /// Strict check used before saving: the name must already be valid.
    pub fn validate(&self) -> Result<(), FaceError> {
        let n = self.name.trim();
        if n.is_empty() {
            return Err(FaceError::EmptyName);
        }
        if n.chars().count() > NAME_MAX {
            return Err(FaceError::NameTooLong(n.to_string()));
        }
        if !n
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == ' ')
        {
            return Err(FaceError::NameCharset(n.to_string()));
        }
        Ok(())
    }

    /// Clamp every field into its documented range and normalise the name.
    /// Identity on valid faces (mirrors the C `face_sanitize`, plus timing sanity).
    pub fn sanitized(&self) -> FaceDef {
        let mut f = self.clone();
        f.name = normalize_name(&f.name);
        f.eye_w = f.eye_w.clamp(8, 56);
        f.eye_h = f.eye_h.clamp(8, 56);
        f.eye_r = f.eye_r.min(f.eye_w / 2);
        f.eye_gap = f.eye_gap.min(40);
        f.speed = f.speed.clamp(1, 10);
        f.blink_min = f.blink_min.max(1);
        f.blink_max = f.blink_max.max(f.blink_min);
        f.look_min = f.look_min.max(1);
        f.look_max = f.look_max.max(f.look_min);
        if f.cycle_seconds == 0 {
            f.cycle_seconds = default_cycle_seconds();
        }
        f.cycle_moods.truncate(CYCLE_MAX);
        f.drawing = f.drawing.as_ref().and_then(Drawing::sanitized);
        f
    }

    pub fn flags(&self) -> u8 {
        let mut fl = 0;
        let mut set = |on: bool, bit: u8| {
            if on {
                fl |= bit
            }
        };
        set(self.blink, ffi::DB_FLAG_BLINK);
        set(self.look_around, ffi::DB_FLAG_LOOK);
        set(self.breathe, ffi::DB_FLAG_BREATHE);
        set(self.cyclops, ffi::DB_FLAG_CYCLOPS);
        set(self.invert, ffi::DB_FLAG_INVERT);
        set(self.rotate180, ffi::DB_FLAG_ROTATE180);
        set(self.mouth, ffi::DB_FLAG_MOUTH);
        fl
    }

    /// Convert to the C struct (sanitised first).
    pub fn to_raw(&self) -> db_face_t {
        let f = self.sanitized();
        let mut name = [0 as std::os::raw::c_char; 13];
        for (dst, b) in name.iter_mut().zip(f.name.bytes().take(NAME_MAX)) {
            *dst = b as std::os::raw::c_char;
        }
        let mut cycle_moods = [0u8; CYCLE_MAX];
        for (dst, m) in cycle_moods.iter_mut().zip(&f.cycle_moods) {
            *dst = m.to_u8();
        }
        db_face_t {
            mood: f.mood.to_u8(),
            name,
            eye_w: f.eye_w,
            eye_h: f.eye_h,
            eye_r: f.eye_r,
            eye_gap: f.eye_gap,
            eye_y: f.eye_y,
            flags: f.flags(),
            blink_min: f.blink_min,
            blink_max: f.blink_max,
            look_min: f.look_min,
            look_max: f.look_max,
            look_x: f.look_x,
            look_y: f.look_y,
            speed: f.speed,
            contrast: f.contrast,
            overlay: f.overlay.to_u8(),
            seed: f.seed,
            cycle: f.cycle.to_u8(),
            cycle_secs: f.cycle_seconds,
            cycle_count: f.cycle_moods.len() as u8,
            cycle_flags: if f.cycle_overlay {
                ffi::DB_CYCLE_FLAG_OVERLAY
            } else {
                0
            },
            cycle_moods,
        }
    }

    pub fn from_raw(r: &db_face_t) -> FaceDef {
        let name: String = r
            .name
            .iter()
            .take(NAME_MAX)
            .take_while(|c| **c != 0)
            .map(|c| *c as u8 as char)
            .collect();
        let has = |bit: u8| r.flags & bit != 0;
        FaceDef {
            name,
            mood: Mood::from_u8(r.mood).unwrap_or(Mood::Neutral),
            eye_w: r.eye_w,
            eye_h: r.eye_h,
            eye_r: r.eye_r,
            eye_gap: r.eye_gap,
            eye_y: r.eye_y,
            blink: has(ffi::DB_FLAG_BLINK),
            look_around: has(ffi::DB_FLAG_LOOK),
            breathe: has(ffi::DB_FLAG_BREATHE),
            cyclops: has(ffi::DB_FLAG_CYCLOPS),
            invert: has(ffi::DB_FLAG_INVERT),
            rotate180: has(ffi::DB_FLAG_ROTATE180),
            mouth: has(ffi::DB_FLAG_MOUTH),
            blink_min: r.blink_min,
            blink_max: r.blink_max,
            look_min: r.look_min,
            look_max: r.look_max,
            look_x: r.look_x,
            look_y: r.look_y,
            speed: r.speed,
            contrast: r.contrast,
            overlay: Overlay::from_u8(r.overlay).unwrap_or(Overlay::None),
            seed: r.seed,
            cycle: Cycle::from_u8(r.cycle).unwrap_or(Cycle::Off),
            cycle_seconds: r.cycle_secs,
            cycle_moods: r.cycle_moods[..(r.cycle_count as usize).min(CYCLE_MAX)]
                .iter()
                .map(|m| Mood::from_u8(*m).unwrap_or(Mood::Neutral))
                .collect(),
            cycle_overlay: r.cycle_flags & ffi::DB_CYCLE_FLAG_OVERLAY != 0,
            drawing: None,
        }
    }

    /// Encode to the 64-byte blob (magic, version, CRC) via `db_face_encode`.
    pub fn encode(&self) -> [u8; BLOB_SIZE] {
        let raw = self.to_raw();
        let mut blob = [0u8; BLOB_SIZE];
        // SAFETY: blob is DB_BLOB_SIZE bytes; raw is a valid db_face_t.
        unsafe { ffi::db_face_encode(&raw, blob.as_mut_ptr()) };
        blob
    }

    /// Decode a blob via `db_face_decode`.
    pub fn decode(blob: &[u8]) -> Result<FaceDef, FaceError> {
        let blob: &[u8; BLOB_SIZE] = blob
            .try_into()
            .map_err(|_| FaceError::BadLength(blob.len()))?;
        let mut raw = db_face_t::default();
        // SAFETY: blob is DB_BLOB_SIZE bytes; raw is writable.
        let rc = unsafe { ffi::db_face_decode(blob.as_ptr(), &mut raw) };
        match rc {
            0 => Ok(FaceDef::from_raw(&raw)),
            -1 => Err(FaceError::BadMagic),
            -2 => Err(FaceError::BadVersion),
            _ => Err(FaceError::BadCrc),
        }
    }
}

/// CRC-16/CCITT-FALSE, pure Rust (cross-checked against the C `db_crc16`).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::presets;

    #[test]
    fn crc_matches_c_and_check_value() {
        assert_eq!(crc16(b"123456789"), 0x29B1);
        let data: Vec<u8> = (0..200u32).map(|i| (i * 73 + 11) as u8).collect();
        for len in [0, 1, 7, 62, 200] {
            let c = unsafe { ffi::db_crc16(data.as_ptr(), len) };
            assert_eq!(crc16(&data[..len]), c, "len {len}");
        }
    }

    #[test]
    fn blob_layout_and_round_trip() {
        for face in presets() {
            let blob = face.encode();
            assert_eq!(&blob[..8], b"DBFACE01");
            assert_eq!(blob[8], 1);
            assert_eq!(blob[9], face.mood.to_u8());
            assert_eq!(blob[27], face.flags());
            assert_eq!(blob[36], face.overlay.to_u8());
            assert_eq!(blob[38], face.cycle.to_u8());
            assert!(
                blob[54..62].iter().all(|b| *b == 0),
                "reserved must be zero"
            );
            let crc = crc16(&blob[..62]);
            assert_eq!(u16::from_le_bytes([blob[62], blob[63]]), crc);
            assert_eq!(FaceDef::decode(&blob).unwrap(), face);
        }
    }

    #[test]
    fn decode_rejects_damage() {
        let mut blob = FaceDef::default().encode();
        blob[40] ^= 1;
        assert_eq!(FaceDef::decode(&blob), Err(FaceError::BadCrc));
        let mut blob = FaceDef::default().encode();
        blob[0] = b'X';
        assert_eq!(FaceDef::decode(&blob), Err(FaceError::BadMagic));
        let mut blob = FaceDef::default().encode();
        blob[8] = 2;
        assert_eq!(FaceDef::decode(&blob), Err(FaceError::BadVersion));
        assert_eq!(FaceDef::decode(&[0; 3]), Err(FaceError::BadLength(3)));
    }

    #[test]
    fn sanitize_clamps_and_normalises_name() {
        let mut f = FaceDef {
            name: "  hello, world!!  ".into(),
            eye_w: 200,
            eye_h: 1,
            eye_r: 90,
            eye_gap: 99,
            speed: 0,
            blink_min: 50,
            blink_max: 10,
            ..FaceDef::default()
        };
        f = f.sanitized();
        assert_eq!(f.name, "HELLO  WORLD");
        assert_eq!(
            (f.eye_w, f.eye_h, f.eye_r, f.eye_gap, f.speed),
            (56, 8, 28, 40, 1)
        );
        assert!(f.blink_max >= f.blink_min);
        // The C decoder's sanitiser agrees with ours on a sanitised face.
        assert_eq!(FaceDef::decode(&f.encode()).unwrap(), f);
    }

    #[test]
    fn cycle_round_trips_and_defaults_for_old_json() {
        let f = FaceDef {
            cycle: Cycle::Random,
            cycle_seconds: 42,
            cycle_moods: vec![Mood::Love, Mood::Dizzy, Mood::Love],
            cycle_overlay: false,
            ..FaceDef::default()
        };
        let blob = f.encode();
        assert_eq!(
            &blob[38..45],
            &[
                2,
                42,
                3,
                0,
                Mood::Love as u8,
                Mood::Dizzy as u8,
                Mood::Love as u8
            ]
        );
        assert_eq!(FaceDef::decode(&blob).unwrap(), f);

        // Too many moods are cut to 12; zero seconds becomes the default.
        let long = FaceDef {
            cycle: Cycle::Sequence,
            cycle_seconds: 0,
            cycle_moods: vec![Mood::Sad; 20],
            ..FaceDef::default()
        };
        let back = FaceDef::decode(&long.encode()).unwrap();
        assert_eq!(back.cycle_moods.len(), CYCLE_MAX);
        assert_eq!(back.cycle_seconds, 10);
        assert_eq!(back, long.sanitized());

        // A face saved before the cycle existed.
        let mut v = serde_json::to_value(FaceDef::default()).unwrap();
        for k in ["cycle", "cycleSeconds", "cycleMoods", "cycleOverlay"] {
            v.as_object_mut().unwrap().remove(k);
        }
        let old: FaceDef = serde_json::from_value(v).unwrap();
        assert_eq!(old, FaceDef::default());
    }

    #[test]
    fn validate_name_rules() {
        let ok = FaceDef::default();
        assert!(ok.validate().is_ok());
        let bad = |n: &str| {
            FaceDef {
                name: n.into(),
                ..FaceDef::default()
            }
            .validate()
        };
        assert_eq!(bad(""), Err(FaceError::EmptyName));
        assert!(matches!(
            bad("THIRTEEN CHAR"),
            Err(FaceError::NameTooLong(_))
        ));
        assert!(matches!(bad("HI!"), Err(FaceError::NameCharset(_))));
        assert!(bad("lower ok").is_ok());
    }

    #[test]
    fn json_shape_is_camel_case() {
        let v = serde_json::to_value(FaceDef::default()).unwrap();
        for k in [
            "name",
            "mood",
            "eyeW",
            "eyeH",
            "eyeR",
            "eyeGap",
            "eyeY",
            "blink",
            "lookAround",
            "breathe",
            "cyclops",
            "invert",
            "rotate180",
            "mouth",
            "blinkMin",
            "blinkMax",
            "lookMin",
            "lookMax",
            "lookX",
            "lookY",
            "speed",
            "contrast",
            "overlay",
            "seed",
            "cycle",
            "cycleSeconds",
            "cycleMoods",
            "cycleOverlay",
        ] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        // `drawing` is omitted when there is none.
        assert_eq!(v.as_object().unwrap().len(), 28);
        assert_eq!(v["cycle"], "off");
        assert_eq!(v["mood"], "neutral");
        assert_eq!(v["overlay"], "none");
    }
}
