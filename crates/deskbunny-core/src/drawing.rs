//! Drawings: up to four 128x64 frames shown instead of the face or on top of
//! it. JSON form and the 4112-byte drawing blob. Contract: docs/ARCHITECTURE.md
//! ("Drawing blob v1").

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::FB_SIZE;

pub const ART_MAGIC: &[u8; 8] = b"DBART001";
pub const ART_VERSION: u8 = 1;
pub const ART_HEADER: usize = 16;
/// Most frames a drawing can have.
pub const ART_FRAMES: usize = 4;
pub const ART_SIZE: usize = ART_HEADER + ART_FRAMES * FB_SIZE;
pub const FRAME_MS_MIN: u16 = 50;
pub const FRAME_MS_MAX: u16 = 10_000;
pub const FRAME_MS_DEFAULT: u16 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DrawMode {
    /// Only the drawing; the face engine does not run.
    #[default]
    Replace,
    /// The drawing OR-ed on top of the animated face.
    Over,
}

impl DrawMode {
    pub fn to_u8(self) -> u8 {
        match self {
            DrawMode::Replace => 1,
            DrawMode::Over => 2,
        }
    }
}

/// One frame in SSD1306 page layout, like the framebuffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame(pub Vec<u8>);

impl Serialize for Frame {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&B64.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Frame {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        B64.decode(s.trim())
            .map(Frame)
            .map_err(|e| serde::de::Error::custom(format!("drawing frame is not base64: {e}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Drawing {
    #[serde(default)]
    pub mode: DrawMode,
    #[serde(default = "default_frame_ms")]
    pub frame_ms: u16,
    pub frames: Vec<Frame>,
}

fn default_frame_ms() -> u16 {
    FRAME_MS_DEFAULT
}

impl Drawing {
    /// Clamp into range: at most 4 frames of exactly 1024 bytes each.
    /// `None` when nothing is left to show.
    pub fn sanitized(&self) -> Option<Drawing> {
        let mut d = self.clone();
        d.frame_ms = if d.frame_ms == 0 {
            FRAME_MS_DEFAULT
        } else {
            d.frame_ms.clamp(FRAME_MS_MIN, FRAME_MS_MAX)
        };
        d.frames.truncate(ART_FRAMES);
        for f in &mut d.frames {
            f.0.resize(FB_SIZE, 0);
        }
        (!d.frames.is_empty()).then_some(d)
    }

    /// The 4112-byte drawing blob (sanitised first). Mode 0 when there is nothing to show.
    pub fn encode(&self) -> [u8; ART_SIZE] {
        match self.sanitized() {
            Some(d) => {
                let mut blob = off_blob();
                blob[9] = d.mode.to_u8();
                blob[10] = d.frames.len() as u8;
                blob[12..14].copy_from_slice(&d.frame_ms.to_le_bytes());
                for (i, f) in d.frames.iter().enumerate() {
                    let at = ART_HEADER + i * FB_SIZE;
                    blob[at..at + FB_SIZE].copy_from_slice(&f.0);
                }
                blob
            }
            None => off_blob(),
        }
    }

    /// The frame shown at `t_ms`, as the firmware picks it.
    pub fn frame_at(&self, t_ms: u32) -> &[u8] {
        let ms = u32::from(self.frame_ms.max(1));
        &self.frames[((t_ms / ms) % self.frames.len() as u32) as usize].0
    }

    /// Composite onto an upright engine frame (before invert/rotate180).
    pub fn compose(&self, fb: &mut [u8; FB_SIZE], t_ms: u32) {
        let frame = self.frame_at(t_ms);
        match self.mode {
            DrawMode::Replace => fb.copy_from_slice(frame),
            DrawMode::Over => fb.iter_mut().zip(frame).for_each(|(d, s)| *d |= s),
        }
    }
}

/// A drawing blob that turns the drawing off (what the firmware ships with).
pub fn off_blob() -> [u8; ART_SIZE] {
    let mut blob = [0u8; ART_SIZE];
    blob[..8].copy_from_slice(ART_MAGIC);
    blob[8] = ART_VERSION;
    blob
}

/// What the panel does for the invert and rotate180 flags, applied to a framebuffer.
pub fn invert_rotate(fb: &mut [u8; FB_SIZE], invert: bool, rotate180: bool) {
    if rotate180 {
        // (x, y) -> (127 - x, 63 - y): reverse the byte order and each byte's bits.
        fb.reverse();
        fb.iter_mut().for_each(|b| *b = b.reverse_bits());
    }
    if invert {
        fb.iter_mut().for_each(|b| *b = !*b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(fill: u8) -> Frame {
        Frame(vec![fill; FB_SIZE])
    }

    #[test]
    fn blob_layout() {
        let d = Drawing {
            mode: DrawMode::Over,
            frame_ms: 250,
            frames: vec![frame(0x11), frame(0x22)],
        };
        let b = d.encode();
        assert_eq!(&b[..8], b"DBART001");
        assert_eq!(b[8], 1);
        assert_eq!(b[9], 2);
        assert_eq!(b[10], 2);
        assert_eq!(u16::from_le_bytes([b[12], b[13]]), 250);
        assert!(b[16..16 + 1024].iter().all(|&x| x == 0x11));
        assert!(b[16 + 1024..16 + 2048].iter().all(|&x| x == 0x22));
        assert!(b[16 + 2048..].iter().all(|&x| x == 0));
        assert_eq!(b.len(), 4112);
    }

    #[test]
    fn sanitize_clamps() {
        let d = Drawing {
            mode: DrawMode::Replace,
            frame_ms: 3,
            frames: vec![Frame(vec![1; 10]), frame(0), frame(0), frame(0), frame(0)],
        }
        .sanitized()
        .unwrap();
        assert_eq!(d.frame_ms, FRAME_MS_MIN);
        assert_eq!(d.frames.len(), 4);
        assert_eq!(d.frames[0].0.len(), FB_SIZE);
        let empty = Drawing {
            frames: vec![],
            ..d.clone()
        };
        assert!(empty.sanitized().is_none());
        assert_eq!(empty.encode(), off_blob());
        assert_eq!(
            Drawing { frame_ms: 0, ..d }.sanitized().unwrap().frame_ms,
            FRAME_MS_DEFAULT
        );
    }

    #[test]
    fn frames_advance_and_compose() {
        let d = Drawing {
            mode: DrawMode::Replace,
            frame_ms: 100,
            frames: vec![frame(1), frame(2), frame(3)],
        };
        assert_eq!(d.frame_at(0)[0], 1);
        assert_eq!(d.frame_at(199)[0], 2);
        assert_eq!(d.frame_at(300)[0], 1);
        let mut fb = [0x80u8; FB_SIZE];
        d.compose(&mut fb, 250);
        assert_eq!(fb[5], 3);
        let over = Drawing {
            mode: DrawMode::Over,
            ..d
        };
        let mut fb = [0x80u8; FB_SIZE];
        over.compose(&mut fb, 0);
        assert_eq!(fb[5], 0x81);
    }

    #[test]
    fn json_is_base64() {
        let d = Drawing {
            mode: DrawMode::Over,
            frame_ms: 400,
            frames: vec![frame(0xFF)],
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["mode"], "over");
        assert_eq!(v["frameMs"], 400);
        assert_eq!(v["frames"][0].as_str().unwrap().len(), 1368);
        let back: Drawing = serde_json::from_value(v).unwrap();
        assert_eq!(back, d);
        assert!(serde_json::from_str::<Drawing>(r#"{"frames":["@@@"]}"#).is_err());
    }
}
