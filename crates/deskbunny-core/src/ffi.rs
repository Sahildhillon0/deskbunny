//! Raw bindings to engine/deskbunny_engine.h. Layout is checked against the C
//! compiler's `sizeof` in tests.

use std::os::raw::{c_char, c_int};

pub const DB_FB_SIZE: usize = 1024;
pub const DB_BLOB_SIZE: usize = 64;

pub const DB_FLAG_BLINK: u8 = 0x01;
pub const DB_FLAG_LOOK: u8 = 0x02;
pub const DB_FLAG_BREATHE: u8 = 0x04;
pub const DB_FLAG_CYCLOPS: u8 = 0x08;
pub const DB_FLAG_INVERT: u8 = 0x10;
pub const DB_FLAG_ROTATE180: u8 = 0x20;
pub const DB_FLAG_MOUTH: u8 = 0x40;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct db_face_t {
    pub mood: u8,
    pub name: [c_char; 13],
    pub eye_w: u8,
    pub eye_h: u8,
    pub eye_r: u8,
    pub eye_gap: u8,
    pub eye_y: i8,
    pub flags: u8,
    pub blink_min: u8,
    pub blink_max: u8,
    pub look_min: u8,
    pub look_max: u8,
    pub look_x: u8,
    pub look_y: u8,
    pub speed: u8,
    pub contrast: u8,
    pub overlay: u8,
    pub seed: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct db_engine_t {
    pub face: db_face_t,
    pub rng: u32,
    pub last_ms: u32,
    pub next_blink_ms: u32,
    pub blink_start_ms: u32,
    pub next_look_ms: u32,
    pub look_cur_x: i16,
    pub look_cur_y: i16,
    pub look_tgt_x: i8,
    pub look_tgt_y: i8,
    pub blinking: u8,
    pub pad: [u8; 3],
}

extern "C" {
    pub fn db_crc16(data: *const u8, len: usize) -> u16;
    pub fn db_face_default(out: *mut db_face_t);
    pub fn db_face_decode(blob: *const u8, out: *mut db_face_t) -> c_int;
    pub fn db_face_encode(face: *const db_face_t, blob: *mut u8);
    pub fn db_engine_init(e: *mut db_engine_t, face: *const db_face_t, seed: u32);
    pub fn db_engine_render(e: *mut db_engine_t, now_ms: u32, fb: *mut u8);

    // build.rs layout shim
    pub fn db_sizeof_face() -> usize;
    pub fn db_sizeof_engine() -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_c() {
        unsafe {
            assert_eq!(std::mem::size_of::<db_face_t>(), db_sizeof_face());
            assert_eq!(std::mem::size_of::<db_engine_t>(), db_sizeof_engine());
        }
    }
}
