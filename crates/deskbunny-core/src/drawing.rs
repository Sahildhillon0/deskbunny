//! Drawings: 128x64 frames (drawn, or imported from a GIF) shown
//! instead of the face or on top of it. JSON form and the compressed drawing
//! blob. Contract: docs/ARCHITECTURE.md ("Drawing blob v2").

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::FB_SIZE;

pub const ART_MAGIC: &[u8; 8] = b"DBART001";
pub const ART_VERSION: u8 = 2;
pub const ART_HEADER: usize = 16;
/// Most frames a drawing can have (the real limit is the board's slot size).
pub const ART_FRAMES_MAX: usize = 10;
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
    /// Clamp into range: at most `ART_FRAMES_MAX` frames of exactly 1024 bytes each.
    /// `None` when nothing is left to show.
    pub fn sanitized(&self) -> Option<Drawing> {
        let mut d = self.clone();
        d.frame_ms = if d.frame_ms == 0 {
            FRAME_MS_DEFAULT
        } else {
            d.frame_ms.clamp(FRAME_MS_MIN, FRAME_MS_MAX)
        };
        d.frames.truncate(ART_FRAMES_MAX);
        for f in &mut d.frames {
            f.0.resize(FB_SIZE, 0);
        }
        (!d.frames.is_empty()).then_some(d)
    }

    /// Compressed frame data (no header): one PackBits stream per frame.
    /// Replace mode stores frame 0 whole and every later frame as the XOR with
    /// the frame before it; over mode stores every frame whole.
    fn frame_data(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut prev = vec![0u8; FB_SIZE];
        for f in &self.frames {
            match self.mode {
                DrawMode::Replace => {
                    let delta: Vec<u8> = f.0.iter().zip(&prev).map(|(a, b)| a ^ b).collect();
                    pack_into(&delta, &mut out);
                    prev.copy_from_slice(&f.0);
                }
                DrawMode::Over => pack_into(&f.0, &mut out),
            }
        }
        out
    }

    /// Bytes this drawing takes in a board's drawing slot (header included).
    pub fn encoded_len(&self) -> usize {
        self.sanitized()
            .map_or(ART_HEADER, |d| ART_HEADER + d.frame_data().len())
    }

    /// The drawing blob for a slot of `capacity` bytes (sanitised first; mode 0
    /// when there is nothing to show). Errors when it does not fit.
    pub fn encode(&self, capacity: usize) -> Result<Vec<u8>, String> {
        let mut blob = off_blob(capacity);
        let Some(d) = self.sanitized() else {
            return Ok(blob);
        };
        let data = d.frame_data();
        let need = ART_HEADER + data.len();
        if need > capacity {
            return Err(format!(
                "This animation needs {need} bytes but the board holds {capacity}. \
                 Use fewer frames, simpler pictures, or a board with more flash."
            ));
        }
        blob[9] = d.mode.to_u8();
        blob[10..12].copy_from_slice(&(d.frames.len() as u16).to_le_bytes());
        blob[12..14].copy_from_slice(&d.frame_ms.to_le_bytes());
        blob.extend_from_slice(&data);
        Ok(blob)
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

/// The 16-byte header of a drawing that is off, for a slot of `capacity` bytes.
pub fn off_blob(capacity: usize) -> Vec<u8> {
    let mut blob = vec![0u8; ART_HEADER];
    blob[..8].copy_from_slice(ART_MAGIC);
    blob[8] = ART_VERSION;
    blob[14..16].copy_from_slice(&(capacity.min(0xFFFF) as u16).to_le_bytes());
    blob
}

/// One frame (or XOR delta) as a PackBits stream, the payload of the live
/// link's `!F` / `!X` commands.
pub fn pack(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() / 4);
    pack_into(src, &mut out);
    out
}

/// PackBits: token t < 128 is followed by t + 1 literal bytes; t >= 128 by one
/// byte repeated t - 126 times (2..129). Matches `db_art_frame` in the engine.
fn pack_into(src: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    let mut lit: Vec<u8> = Vec::new();
    let flush = |lit: &mut Vec<u8>, out: &mut Vec<u8>| {
        for chunk in lit.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
        lit.clear();
    };
    while i < src.len() {
        let mut run = 1;
        while i + run < src.len() && run < 129 && src[i + run] == src[i] {
            run += 1;
        }
        if run >= 2 {
            flush(&mut lit, out);
            out.push((run + 126) as u8);
            out.push(src[i]);
            i += run;
        } else {
            lit.push(src[i]);
            i += 1;
        }
    }
    flush(&mut lit, out);
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

    /// Decode a blob with the C engine's decoder, as the firmware does.
    fn play(blob: &[u8]) -> Vec<Vec<u8>> {
        use std::cell::RefCell;
        thread_local!(static SLOT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) });
        extern "C" fn rd(off: u16) -> u8 {
            SLOT.with(|s| s.borrow()[off as usize])
        }
        SLOT.with(|s| *s.borrow_mut() = blob.to_vec());
        let frames = u16::from_le_bytes([blob[10], blob[11]]) as usize;
        let replace = blob[9] == 1;
        let mut fb = [0u8; FB_SIZE];
        let mut pos = ART_HEADER as u16;
        let mut out = Vec::new();
        for _ in 0..frames {
            if !replace {
                fb = [0; FB_SIZE];
            }
            let op = if replace { 1 } else { 2 };
            let r = unsafe {
                crate::ffi::db_art_frame(rd, blob.len() as u16, &mut pos, op, fb.as_mut_ptr())
            };
            assert_eq!(r, 0);
            out.push(fb.to_vec());
        }
        assert_eq!(pos as usize, blob.len());
        out
    }

    fn noisy(seed: u32) -> Frame {
        let mut x = seed.wrapping_mul(2654435761) | 1;
        Frame(
            (0..FB_SIZE)
                .map(|i| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    if i % 7 < 3 {
                        0
                    } else {
                        x as u8
                    }
                })
                .collect(),
        )
    }

    #[test]
    fn blob_layout_and_roundtrip() {
        for mode in [DrawMode::Over, DrawMode::Replace] {
            let d = Drawing {
                mode,
                frame_ms: 250,
                frames: vec![frame(0x11), noisy(1), noisy(2), frame(0), noisy(2)],
            };
            let b = d.encode(8192).unwrap();
            assert_eq!(&b[..8], b"DBART001");
            assert_eq!(b[8], 2);
            assert_eq!(b[9], mode.to_u8());
            assert_eq!(u16::from_le_bytes([b[10], b[11]]), 5);
            assert_eq!(u16::from_le_bytes([b[12], b[13]]), 250);
            assert_eq!(u16::from_le_bytes([b[14], b[15]]), 8192);
            assert_eq!(b.len(), d.encoded_len());
            let got = play(&b);
            for (g, f) in got.iter().zip(&d.frames) {
                assert_eq!(g, &f.0);
            }
        }
    }

    #[test]
    fn simple_frames_fit_and_big_ones_are_refused() {
        // A dot moving across the screen: the full 10 frames in well under 1 KB.
        let frames: Vec<Frame> = (0..ART_FRAMES_MAX)
            .map(|i| {
                let mut f = vec![0u8; FB_SIZE];
                f[(i * 3) % FB_SIZE] = 0x18;
                Frame(f)
            })
            .collect();
        let d = Drawing {
            mode: DrawMode::Replace,
            frame_ms: 50,
            frames,
        };
        assert!(d.encoded_len() < 1024, "{}", d.encoded_len());
        assert_eq!(play(&d.encode(8192).unwrap()).len(), ART_FRAMES_MAX);
        let heavy = Drawing {
            frames: (0..10).map(noisy).collect(),
            ..d
        };
        let e = heavy.encode(8192).unwrap_err();
        assert!(e.contains("holds 8192"), "{e}");
    }

    #[test]
    fn sanitize_clamps() {
        let d = Drawing {
            mode: DrawMode::Replace,
            frame_ms: 3,
            frames: vec![Frame(vec![1; 10]); ART_FRAMES_MAX + 5],
        }
        .sanitized()
        .unwrap();
        assert_eq!(d.frame_ms, FRAME_MS_MIN);
        assert_eq!(d.frames.len(), ART_FRAMES_MAX);
        assert_eq!(d.frames[0].0.len(), FB_SIZE);
        let empty = Drawing {
            frames: vec![],
            ..d.clone()
        };
        assert!(empty.sanitized().is_none());
        assert_eq!(empty.encode(4032).unwrap(), off_blob(4032));
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
