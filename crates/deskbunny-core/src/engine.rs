//! Safe wrapper over the C engine, used by the simulator.

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
}

impl Engine {
    pub fn new(face: &FaceDef, seed: u32) -> Engine {
        let mut e = Engine {
            state: Box::default(),
            seed,
        };
        e.set_face(face);
        e
    }

    /// Switch to another face (restarts the animation timers with the same seed).
    pub fn set_face(&mut self, face: &FaceDef) {
        let raw = face.to_raw();
        // SAFETY: state is a valid, exclusively owned db_engine_t.
        unsafe { ffi::db_engine_init(&mut *self.state, &raw, self.seed) };
    }

    /// Render the frame at `t_ms` in SSD1306 page layout:
    /// `fb[(y >> 3) * 128 + x]`, bit `y & 7`, 1 = lit.
    pub fn render(&mut self, t_ms: u32) -> [u8; FB_SIZE] {
        let mut fb = [0u8; FB_SIZE];
        // SAFETY: fb is DB_FB_SIZE bytes; state is valid.
        unsafe { ffi::db_engine_render(&mut *self.state, t_ms, fb.as_mut_ptr()) };
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
    fn animation_changes_over_time() {
        let frames = run(&presets()[0], 1);
        assert!(frames.windows(2).any(|w| w[0] != w[1]));
    }
}
