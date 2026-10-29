//! Safe wrapper over the C engine, used by the simulator.

use crate::drawing::{invert_rotate, Drawing};
use crate::face::FaceDef;
use crate::ffi::{self, db_engine_t, DB_FB_SIZE};

pub const FB_SIZE: usize = DB_FB_SIZE;
pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;

/// One running face. Output is deterministic for the same face, seed and
/// sequence of timestamps.
pub struct Engine {
    state: Box<db_engine_t>,
    seed: u32,
    /// With a drawing the engine draws upright and invert/rotate180 are applied
    /// after compositing, as the panel does on the board.
    drawing: Option<Drawing>,
    invert: bool,
    rotate180: bool,
}

impl Engine {
    pub fn new(face: &FaceDef, seed: u32) -> Engine {
        let mut e = Engine {
            state: Box::default(),
            seed,
            drawing: None,
            invert: false,
            rotate180: false,
        };
        e.set_face(face);
        e
    }

    /// Switch to another face (restarts the animation timers with the same seed).
    pub fn set_face(&mut self, face: &FaceDef) {
        let mut raw = face.to_raw();
        self.drawing = face.drawing.as_ref().and_then(Drawing::sanitized);
        self.invert = face.invert;
        self.rotate180 = face.rotate180;
        if self.drawing.is_some() {
            raw.flags &= !(ffi::DB_FLAG_INVERT | ffi::DB_FLAG_ROTATE180);
        }
        // SAFETY: state is a valid, exclusively owned db_engine_t.
        unsafe { ffi::db_engine_init(&mut *self.state, &raw, self.seed) };
    }

    /// Render the frame at `t_ms` in SSD1306 page layout:
    /// `fb[(y >> 3) * 128 + x]`, bit `y & 7`, 1 = lit.
    pub fn render(&mut self, t_ms: u32) -> [u8; FB_SIZE] {
        let mut fb = [0u8; FB_SIZE];
        // SAFETY: fb is DB_FB_SIZE bytes; state is valid.
        unsafe { ffi::db_engine_render(&mut *self.state, t_ms, fb.as_mut_ptr()) };
        if let Some(d) = &self.drawing {
            d.compose(&mut fb, t_ms);
            invert_rotate(&mut fb, self.invert, self.rotate180);
        }
        fb
    }
}

/// Whether pixel (x, y) is lit in a page-layout framebuffer.
pub fn pixel(fb: &[u8; FB_SIZE], x: usize, y: usize) -> bool {
    fb[(y >> 3) * WIDTH + x] & (1 << (y & 7)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::presets;

    fn run(face: &FaceDef, seed: u32) -> Vec<[u8; FB_SIZE]> {
        let mut e = Engine::new(face, seed);
        (0..300).map(|i| e.render(i * 33)).collect()
    }

    #[test]
    fn render_is_deterministic() {
        for face in presets() {
            let a = run(&face, 7);
            let b = run(&face, 7);
            assert!(a == b, "{} not deterministic", face.name);
            let lit: u32 = a[10].iter().map(|b| b.count_ones()).sum();
            assert!(lit > 50, "{} draws almost nothing ({lit} px)", face.name);
        }
    }

    #[test]
    fn set_face_matches_fresh_engine() {
        let p = presets();
        let mut e = Engine::new(&p[0], 3);
        e.render(0);
        e.render(500);
        e.set_face(&p[1]);
        let mut fresh = Engine::new(&p[1], 3);
        for t in [0, 33, 66, 1000] {
            assert!(e.render(t) == fresh.render(t));
        }
    }

    #[test]
    fn invert_rotate_matches_the_engine() {
        // The simulator's own invert/rotate180 (used with drawings) must equal the engine's.
        for (invert, rotate180) in [(true, false), (false, true), (true, true)] {
            let face = FaceDef {
                invert,
                rotate180,
                cyclops: true,
                eye_gap: 30,
                eye_y: -9,
                ..presets()[0].clone()
            };
            let upright = FaceDef {
                invert: false,
                rotate180: false,
                ..face.clone()
            };
            let (mut a, mut b) = (Engine::new(&face, 5), Engine::new(&upright, 5));
            for t in [0, 400, 1700] {
                let mut fb = b.render(t);
                invert_rotate(&mut fb, invert, rotate180);
                assert!(
                    a.render(t) == fb,
                    "invert {invert} rotate {rotate180} t {t}"
                );
            }
        }
    }

    #[test]
    fn drawing_replaces_or_overlays() {
        use crate::drawing::{DrawMode, Frame};
        let mut dot = vec![0u8; FB_SIZE];
        dot[0] = 1; // pixel (0, 0)
        let base = presets()[0].clone();
        let mut face = FaceDef {
            drawing: Some(Drawing {
                mode: DrawMode::Replace,
                frame_ms: 500,
                frames: vec![Frame(dot.clone())],
            }),
            ..base.clone()
        };
        let fb = Engine::new(&face, 1).render(100);
        assert_eq!(fb[0], 1);
        assert_eq!(fb.iter().map(|b| b.count_ones()).sum::<u32>(), 1);

        face.drawing.as_mut().unwrap().mode = DrawMode::Over;
        let plain = Engine::new(&base, 1).render(100);
        let over = Engine::new(&face, 1).render(100);
        assert_eq!(over[0], plain[0] | 1);
        assert_eq!(over[1..], plain[1..]);

        // Panel flags still apply: rotate180 moves the dot to (127, 63), invert flips it.
        face.drawing.as_mut().unwrap().mode = DrawMode::Replace;
        face.rotate180 = true;
        face.invert = true;
        let fb = Engine::new(&face, 1).render(0);
        assert!(!pixel(&fb, 127, 63));
        assert!(pixel(&fb, 0, 0));
    }

    #[test]
    fn animation_changes_over_time() {
        let frames = run(&presets()[0], 1);
        assert!(frames.windows(2).any(|w| w[0] != w[1]));
    }
}
