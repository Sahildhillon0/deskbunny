//! Live link screens (docs/ARCHITECTURE.md, "Live link v1").
//!
//! The app renders still screens (clock, weather) itself and sends them as
//! PackBits frames; the face and Clawd are animated on the board, which the
//! app previews here pixel for pixel: same engine, same 5x7 font, same caption.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::engine::{FB_SIZE, HEIGHT, WIDTH};
use crate::ffi;

/// Longest caption the board draws (6 px per glyph across 128 px).
pub const CAPTION_MAX: usize = 21;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClawdAnim {
    Idle,
    Walk,
    Jump,
    Shake,
    Sleep,
    Work,
}

impl ClawdAnim {
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClawdExpr {
    Open,
    Happy,
    Closed,
    Wide,
    Squint,
    Dead,
    Worried,
    Focus,
}

impl ClawdExpr {
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// Clawd at `t_ms`, as `!C` draws it on the board (without the caption).
pub fn clawd_frame(anim: ClawdAnim, expr: ClawdExpr, t_ms: u32) -> [u8; FB_SIZE] {
    let mut fb = [0u8; FB_SIZE];
    // SAFETY: fb is DB_FB_SIZE bytes; the renderer is stateless.
    unsafe { ffi::db_clawd_render(anim.to_u8(), expr.to_u8(), t_ms, fb.as_mut_ptr()) };
    fb
}

/// What the board does with a caption: ASCII 32..122 kept, anything else
/// becomes a space, cut to [`CAPTION_MAX`]. Trailing spaces are dropped (the
/// board would centre them as blanks).
pub fn normalize_caption(s: &str) -> String {
    let out: String = s
        .chars()
        .take(CAPTION_MAX)
        .map(|c| if (' '..='z').contains(&c) { c } else { ' ' })
        .collect();
    out.trim_end().to_string()
}

/// Like [`normalize_caption`] without the length cap: text the 5x7 font can
/// draw, for host-drawn screens (which also draw `°`). Newlines are kept.
pub fn normalize_caption_long(s: &str) -> String {
    s.chars()
        .map(|c| {
            if (' '..='z').contains(&c) || c == '\n' || c == '\u{b0}' {
                c
            } else {
                ' '
            }
        })
        .collect()
}

// --- 5x7 font: parsed from the firmware's own table, so they cannot drift ---

const FONT_FIRST: u8 = 32;
const FONT_LAST: u8 = 122;
const FONT_SRC: &str = include_str!("../../../firmware/deskbunny/db_font5x7.h");

fn font() -> &'static [u8] {
    static FONT: OnceLock<Vec<u8>> = OnceLock::new();
    FONT.get_or_init(|| {
        let body = FONT_SRC.split_once('=').map_or("", |(_, b)| b);
        let mut out = Vec::new();
        let mut rest = body;
        while let Some(i) = rest.find("0x") {
            let hex = rest.get(i + 2..i + 4).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
            }
            rest = &rest[i + 2..];
        }
        out
    })
}

/// A 128x64 framebuffer in SSD1306 page layout with the few primitives the
/// still screens need. Everything clips.
#[derive(Clone)]
pub struct Canvas {
    pub fb: [u8; FB_SIZE],
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas { fb: [0; FB_SIZE] }
    }
}

impl Canvas {
    pub fn set(&mut self, x: i32, y: i32, on: bool) {
        if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
            return;
        }
        let i = (y as usize >> 3) * WIDTH + x as usize;
        let m = 1u8 << (y & 7);
        if on {
            self.fb[i] |= m;
        } else {
            self.fb[i] &= !m;
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, on: bool) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, on);
            }
        }
    }

    /// Text in the 5x7 font, each pixel `scale` x `scale`, 1 column (x scale)
    /// between glyphs. Returns the x after the last glyph. Besides ASCII
    /// 32..122 it draws `°`; anything else is a space.
    pub fn text(&mut self, s: &str, x: i32, y: i32, scale: i32) -> i32 {
        self.text_ink(s, x, y, scale, true)
    }

    /// [`Canvas::text`] in either ink (`false` erases, for text on a filled box).
    pub fn text_ink(&mut self, s: &str, x: i32, y: i32, scale: i32, on: bool) -> i32 {
        const DEGREE: [u8; 5] = [0x00, 0x06, 0x09, 0x09, 0x06];
        let f = font();
        let mut cx = x;
        for c in s.chars() {
            let glyph: &[u8] = if c == '\u{b0}' {
                &DEGREE
            } else {
                let code = if (FONT_FIRST as char..=FONT_LAST as char).contains(&c) {
                    c as u8
                } else {
                    b' '
                };
                let g = (code - FONT_FIRST) as usize * 5;
                f.get(g..g + 5).unwrap_or(&[0; 5])
            };
            for (col, bits) in glyph.iter().enumerate() {
                for row in 0..7 {
                    if bits >> row & 1 == 1 {
                        self.rect(cx + col as i32 * scale, y + row * scale, scale, scale, on);
                    }
                }
            }
            cx += 6 * scale;
        }
        cx
    }

    /// Bresenham line, `thick` px square pen.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, thick: i32, on: bool) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        let t = thick.max(1);
        loop {
            self.rect(x - (t - 1) / 2, y - (t - 1) / 2, t, t, on);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Filled circle.
    pub fn disc(&mut self, cx: i32, cy: i32, r: i32, on: bool) {
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y <= r * r + r {
                    self.set(cx + x, cy + y, on);
                }
            }
        }
    }

    /// Circle outline.
    pub fn ring(&mut self, cx: i32, cy: i32, r: i32, on: bool) {
        for y in -r..=r {
            for x in -r..=r {
                let d = x * x + y * y;
                if d <= r * r + r && d > (r - 1) * (r - 1) + (r - 1) {
                    self.set(cx + x, cy + y, on);
                }
            }
        }
    }

    pub fn text_width(s: &str, scale: i32) -> i32 {
        let n = s.chars().count() as i32;
        if n == 0 {
            0
        } else {
            n * 6 * scale - scale
        }
    }

    pub fn text_centered(&mut self, s: &str, y: i32, scale: i32) {
        let x = (WIDTH as i32 - Self::text_width(s, scale)) / 2;
        self.text(s, x, y, scale);
    }
}

/// The caption exactly as the board draws it: rows 56..63 cleared, the 5x7
/// text centred at row 57 (the firmware's `drawText(caption, 57, false)`).
pub fn draw_caption(fb: &mut [u8; FB_SIZE], caption: &str) {
    let caption = normalize_caption(caption);
    if caption.is_empty() {
        return;
    }
    fb[7 * WIDTH..8 * WIDTH].fill(0);
    let mut c = Canvas { fb: *fb };
    // The firmware centres with integer division of (128 - (len * 6 - 1)) / 2.
    let len = caption.chars().count() as i32;
    c.text(&caption, (WIDTH as i32 - (len * 6 - 1)) / 2, 57, 1);
    *fb = c.fb;
}

// --- clock ---------------------------------------------------------------

/// Local wall-clock time, broken down by the caller (no time zone code here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// 0 = Monday .. 6 = Sunday
    pub weekday: u8,
    pub day: u8,
    /// 1..12
    pub month: u8,
}

const WEEKDAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Clock screen: HH:MM in 4x digits, a seconds bar and the date.
pub fn clock_frame(t: &ClockTime, h24: bool) -> [u8; FB_SIZE] {
    let mut c = Canvas::default();
    let (hour, suffix) = if h24 {
        (t.hour, "")
    } else {
        let h = t.hour % 12;
        (
            if h == 0 { 12 } else { h },
            if t.hour < 12 { "AM" } else { "PM" },
        )
    };
    let hm = if h24 {
        format!("{:02}:{:02}", hour, t.minute)
    } else {
        format!("{}:{:02}", hour, t.minute)
    };
    c.text_centered(&hm, 6, 4);
    let bar = (WIDTH as i32 - 8) * i32::from(t.second.min(59)) / 59;
    c.rect(4, 40, bar, 1, true);
    let wd = WEEKDAYS[usize::from(t.weekday % 7)];
    let mo = MONTHS[usize::from(t.month.clamp(1, 12) - 1)];
    let date = if suffix.is_empty() {
        format!("{wd} {} {mo}", t.day)
    } else {
        format!("{wd} {} {mo}  {suffix}", t.day)
    };
    c.text_centered(&date, 50, 1);
    c.fb
}

/// XOR of two frames: the `!X` payload before packing.
pub fn delta(a: &[u8; FB_SIZE], b: &[u8; FB_SIZE]) -> [u8; FB_SIZE] {
    let mut d = [0u8; FB_SIZE];
    for i in 0..FB_SIZE {
        d[i] = a[i] ^ b[i];
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::pixel;

    fn lit(fb: &[u8; FB_SIZE]) -> u32 {
        fb.iter().map(|b| b.count_ones()).sum()
    }

    #[test]
    fn font_parses_every_glyph() {
        assert_eq!(font().len(), (FONT_LAST - FONT_FIRST + 1) as usize * 5);
        // '0' is the classic 3E 51 49 45 3E
        let z = (b'0' - FONT_FIRST) as usize * 5;
        assert_eq!(&font()[z..z + 5], &[0x3E, 0x51, 0x49, 0x45, 0x3E]);
    }

    #[test]
    fn clawd_matches_engine_contract() {
        let a = clawd_frame(ClawdAnim::Walk, ClawdExpr::Focus, 1234);
        assert_eq!(a, clawd_frame(ClawdAnim::Walk, ClawdExpr::Focus, 1234));
        assert!(lit(&a) > 200);
        for y in 55..64 {
            for x in 0..128 {
                assert!(
                    !pixel(&a, x, y),
                    "Clawd drew in the caption rows at ({x},{y})"
                );
            }
        }
        assert_ne!(a, clawd_frame(ClawdAnim::Jump, ClawdExpr::Happy, 1234));
    }

    #[test]
    fn caption_normalizes_like_the_board() {
        assert_eq!(normalize_caption("Edit main.rs"), "Edit main.rs");
        assert_eq!(normalize_caption("é{|}~"), "");
        assert_eq!(normalize_caption("a\u{1F600}b"), "a b");
        assert_eq!(normalize_caption(&"X".repeat(30)).len(), CAPTION_MAX);
    }

    #[test]
    fn caption_clears_the_bottom_page() {
        let mut fb = [0xFFu8; FB_SIZE];
        draw_caption(&mut fb, "HI");
        assert!(fb[..7 * WIDTH].iter().all(|&b| b == 0xFF));
        assert!(!pixel(&fb, 0, 60) && !pixel(&fb, 127, 56));
        // "HI" is 11 px wide, starting at (128 - 11) / 2 = 58; H's first column is 0x7F.
        assert!(pixel(&fb, 58, 57) && pixel(&fb, 58, 63));
        let mut untouched = [0x55u8; FB_SIZE];
        draw_caption(&mut untouched, "   ");
        assert!(untouched.iter().all(|&b| b == 0x55));
    }

    #[test]
    fn clock_draws_and_changes_each_second() {
        let t = ClockTime {
            hour: 9,
            minute: 5,
            second: 0,
            weekday: 1,
            day: 10,
            month: 11,
        };
        let a = clock_frame(&t, true);
        let b = clock_frame(&ClockTime { second: 30, ..t }, true);
        assert!(lit(&a) > 100);
        assert_ne!(a, b);
        let d = delta(&a, &b);
        assert!(crate::drawing::pack(&d).len() < crate::drawing::pack(&b).len());
        assert_ne!(clock_frame(&t, false), a);
        // A 12-hour clock reads 12:05 at midnight and at noon (same digits,
        // pages 0..4), and only the AM/PM on the date line differs.
        let mid = clock_frame(&ClockTime { hour: 0, ..t }, false);
        let noon = clock_frame(&ClockTime { hour: 12, ..t }, false);
        assert_eq!(mid[..5 * WIDTH], noon[..5 * WIDTH]);
        assert_ne!(mid, noon);
    }
}
