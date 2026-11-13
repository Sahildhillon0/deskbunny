//! Designed screens (docs/ARCHITECTURE.md, "Designs"): a screen is a list of
//! elements placed anywhere on the 128x64 panel. Text elements hold tokens
//! (`{HH}:{MM}`, `{TEMP}°`, `{CLAUDE}`) filled in at render time, so a clock or
//! a weather screen is just a design. The app renders designs and sends them
//! to the board as frames; the editor previews the same pixels.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::engine::FB_SIZE;
use crate::live::{normalize_caption_long, Canvas, ClockTime};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Design {
    pub id: String,
    pub name: String,
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Font {
    /// The 5x7 font, each pixel `size` x `size` (1..4).
    #[default]
    Small,
    /// Seven-segment digits `size` px tall (10..60); other characters fall back
    /// to the 5x7 font at a matching scale.
    Seg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

fn one() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Element {
    /// `x` is the left edge, the centre or the right edge, per `align`.
    Text {
        x: i16,
        y: i16,
        text: String,
        #[serde(default)]
        font: Font,
        #[serde(default = "one")]
        size: u8,
        #[serde(default)]
        align: Align,
        /// Draw in black (on a filled box).
        #[serde(default)]
        erase: bool,
    },
    Rect {
        x: i16,
        y: i16,
        w: i16,
        h: i16,
        #[serde(default)]
        fill: bool,
        #[serde(default)]
        erase: bool,
    },
    Line {
        x1: i16,
        y1: i16,
        x2: i16,
        y2: i16,
        #[serde(default = "one")]
        thick: u8,
    },
    /// A progress bar: `value` / `max`, both may hold tokens (`{SS}` / `59`).
    Bar {
        x: i16,
        y: i16,
        w: i16,
        h: i16,
        value: String,
        max: String,
    },
    Analog {
        cx: i16,
        cy: i16,
        r: i16,
        #[serde(default)]
        seconds: bool,
    },
    /// A 16x16 icon at `size` x (1..3). `icon` is a name or a token
    /// (`{WICON}` = the weather now).
    Icon {
        x: i16,
        y: i16,
        icon: String,
        #[serde(default = "one")]
        size: u8,
    },
    /// Hand-drawn pixels: `data` is base64 of `w * h` bits, row-major, MSB first.
    Bitmap {
        x: i16,
        y: i16,
        w: i16,
        h: i16,
        data: String,
    },
}

/// Weather now, for the `{TEMP}`-style tokens and the weather icon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Weather {
    pub city: String,
    pub temp: f32,
    pub tmax: f32,
    pub tmin: f32,
    /// WMO weather code (Open-Meteo `weather_code`).
    pub code: u8,
    pub is_day: bool,
    pub humidity: u8,
    pub wind: f32,
    /// "C" or "F".
    pub unit: String,
    /// "km/h" or "mph".
    pub wind_unit: String,
    /// Air quality index now, on `aqi_scale` (None when it could not be read).
    #[serde(default)]
    pub aqi: Option<u16>,
    /// "US" (0-500, US EPA), "EU" (0-100+, European EEA) or "IN" (0-500,
    /// India's CPCB National AQI).
    #[serde(default)]
    pub aqi_scale: String,
    /// Fine particles, µg/m³.
    #[serde(default)]
    pub pm25: Option<f32>,
    #[serde(default)]
    pub pm10: Option<f32>,
}

/// The band an AQI value falls in, in capitals (fits the 5x7 font).
pub fn aqi_category(aqi: u16, scale: &str) -> &'static str {
    if scale == "IN" {
        return match aqi {
            0..=50 => "GOOD",
            51..=100 => "SATISFACTORY",
            101..=200 => "MODERATE",
            201..=300 => "POOR",
            301..=400 => "VERY POOR",
            _ => "SEVERE",
        };
    }
    if scale == "EU" {
        match aqi {
            0..=20 => "GOOD",
            21..=40 => "FAIR",
            41..=60 => "MODERATE",
            61..=80 => "POOR",
            81..=100 => "VERY POOR",
            _ => "EXTREMELY POOR",
        }
    } else {
        match aqi {
            0..=50 => "GOOD",
            51..=100 => "MODERATE",
            101..=150 => "UNHEALTHY FOR SOME",
            151..=200 => "UNHEALTHY",
            201..=300 => "VERY UNHEALTHY",
            _ => "HAZARDOUS",
        }
    }
}

/// Everything tokens can refer to.
pub struct Ctx<'a> {
    pub time: ClockTime,
    pub year: u16,
    pub weather: Option<&'a Weather>,
    /// The Claude screen's caption (`EDIT main.rs`).
    pub claude: &'a str,
    /// AI plan usage, for the `{U5H}`-style tokens (None: no harness chosen).
    pub usage: Option<&'a Usage>,
}

/// AI plan usage windows as the screen shows them (the app works out the
/// "resets in" text, so this crate needs no clock).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// The harness, short and upper case (`CLAUDE`).
    pub source: String,
    /// 5-hour window, percent used.
    pub five: Option<u8>,
    /// Time until the 5-hour window resets: `2H15M`, `45M`, `--`.
    pub five_in: String,
    /// Weekly window, percent used.
    pub week: Option<u8>,
    /// Time until the weekly window resets: `3D4H`, `--`.
    pub week_in: String,
}

const DAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const DAY_NAMES: [&str; 7] = [
    "MONDAY",
    "TUESDAY",
    "WEDNESDAY",
    "THURSDAY",
    "FRIDAY",
    "SATURDAY",
    "SUNDAY",
];
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];
const MONTH_NAMES: [&str; 12] = [
    "JANUARY",
    "FEBRUARY",
    "MARCH",
    "APRIL",
    "MAY",
    "JUNE",
    "JULY",
    "AUGUST",
    "SEPTEMBER",
    "OCTOBER",
    "NOVEMBER",
    "DECEMBER",
];

/// Every token, with what it means (the editor lists these).
pub const TOKENS: &[(&str, &str)] = &[
    ("HH", "hour 00-23"),
    ("H", "hour 0-23"),
    ("hh", "hour 01-12"),
    ("h", "hour 1-12"),
    ("MM", "minute 00-59"),
    ("SS", "second 00-59"),
    ("AMPM", "AM / PM"),
    ("DAY", "MON"),
    ("DAYNAME", "MONDAY"),
    ("DD", "day 01-31"),
    ("D", "day 1-31"),
    ("MON", "NOV"),
    ("MONTH", "NOVEMBER"),
    ("MO", "month 01-12"),
    ("YYYY", "year"),
    ("TEMP", "temperature now"),
    ("TMAX", "today's high"),
    ("TMIN", "today's low"),
    ("UNIT", "C or F"),
    ("COND", "CLOUDY, RAIN, ..."),
    ("HUM", "humidity %"),
    ("WIND", "wind speed"),
    ("CITY", "weather city"),
    ("WICON", "weather icon name"),
    ("AQI", "air quality index"),
    ("AQICAT", "GOOD, MODERATE, ..."),
    ("AQIMAX", "top of the AQI scale (for bars)"),
    ("PM25", "PM2.5 µg/m³"),
    ("PM10", "PM10 µg/m³"),
    ("CLAUDE", "Claude status"),
    ("USRC", "usage: harness (CLAUDE)"),
    ("U5H", "usage: 5-hour window %"),
    ("U5HIN", "5-hour resets in (2H15M)"),
    ("UWK", "usage: weekly window %"),
    ("UWKIN", "weekly resets in (3D4H)"),
];

/// Condition text and icon name for a WMO weather code.
pub fn condition(code: u8, is_day: bool) -> (&'static str, &'static str) {
    match code {
        0 => ("CLEAR", if is_day { "sun" } else { "moon" }),
        1 | 2 => ("PARTLY CLOUDY", if is_day { "partly" } else { "moon" }),
        3 => ("CLOUDY", "cloud"),
        45 | 48 => ("FOG", "fog"),
        51..=57 => ("DRIZZLE", "drizzle"),
        61..=67 | 80..=82 => ("RAIN", "rain"),
        71..=77 | 85 | 86 => ("SNOW", "snow"),
        95..=99 => ("STORM", "storm"),
        _ => ("--", "cloud"),
    }
}

fn token(name: &str, c: &Ctx) -> Option<String> {
    let t = &c.time;
    let h12 = match t.hour % 12 {
        0 => 12,
        h => h,
    };
    let w = c.weather;
    let wx = |f: &dyn Fn(&Weather) -> String| Some(w.map_or_else(|| "--".to_string(), f));
    match name {
        "HH" => Some(format!("{:02}", t.hour)),
        "H" => Some(t.hour.to_string()),
        "hh" => Some(format!("{h12:02}")),
        "h" => Some(h12.to_string()),
        "MM" => Some(format!("{:02}", t.minute)),
        "SS" => Some(format!("{:02}", t.second)),
        "AMPM" => Some(if t.hour < 12 { "AM" } else { "PM" }.into()),
        "DAY" => Some(DAYS[usize::from(t.weekday % 7)].into()),
        "DAYNAME" => Some(DAY_NAMES[usize::from(t.weekday % 7)].into()),
        "DD" => Some(format!("{:02}", t.day)),
        "D" => Some(t.day.to_string()),
        "MON" => Some(MONTHS[usize::from(t.month.clamp(1, 12) - 1)].into()),
        "MONTH" => Some(MONTH_NAMES[usize::from(t.month.clamp(1, 12) - 1)].into()),
        "MO" => Some(format!("{:02}", t.month)),
        "YYYY" => Some(c.year.to_string()),
        "TEMP" => wx(&|w| format!("{:.0}", w.temp)),
        "TMAX" => wx(&|w| format!("{:.0}", w.tmax)),
        "TMIN" => wx(&|w| format!("{:.0}", w.tmin)),
        "UNIT" => wx(&|w| w.unit.clone()),
        "COND" => wx(&|w| condition(w.code, w.is_day).0.to_string()),
        "HUM" => wx(&|w| w.humidity.to_string()),
        "WIND" => wx(&|w| format!("{:.0}{}", w.wind, w.wind_unit)),
        "CITY" => wx(&|w| w.city.to_uppercase()),
        "WICON" => Some(w.map_or("cloud", |w| condition(w.code, w.is_day).1).into()),
        "AQI" => wx(&|w| w.aqi.map_or_else(|| "--".into(), |a| a.to_string())),
        "AQICAT" => wx(&|w| {
            w.aqi
                .map_or("--", |a| aqi_category(a, &w.aqi_scale))
                .to_string()
        }),
        // Where a bar is full: the start of the worst-but-one band.
        "AQIMAX" => Some(
            match w.map(|w| w.aqi_scale.as_str()) {
                Some("EU") => "100",
                Some("IN") => "400",
                _ => "300",
            }
            .into(),
        ),
        "PM25" => wx(&|w| w.pm25.map_or_else(|| "--".into(), |v| format!("{v:.0}"))),
        "PM10" => wx(&|w| w.pm10.map_or_else(|| "--".into(), |v| format!("{v:.0}"))),
        "CLAUDE" => Some(c.claude.to_string()),
        "USRC" => Some(c.usage.map_or_else(|| "--".into(), |u| u.source.clone())),
        "U5H" | "UWK" => {
            let p = c
                .usage
                .and_then(|u| if name == "U5H" { u.five } else { u.week });
            Some(p.map_or_else(|| "--".into(), |p| p.to_string()))
        }
        "U5HIN" | "UWKIN" => Some(c.usage.map_or_else(
            || "--".into(),
            |u| {
                if name == "U5HIN" {
                    &u.five_in
                } else {
                    &u.week_in
                }
                .clone()
            },
        )),
        _ => None,
    }
}

/// Replace every `{TOKEN}`; unknown ones stay as written.
pub fn expand(s: &str, c: &Ctx) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find('}') {
            Some(j) => match token(&tail[1..j], c) {
                Some(v) => {
                    out.push_str(&v);
                    rest = &tail[j + 1..];
                }
                None => {
                    out.push('{');
                    rest = &tail[1..];
                }
            },
            None => {
                out.push_str(tail);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

// --- seven-segment font ---------------------------------------------------

/// Segments a..g as bits 0..6 (a top, b top right, c bottom right, d bottom,
/// e bottom left, f top left, g middle).
fn seg_bits(c: char) -> Option<u8> {
    Some(match c {
        '0' => 0x3F,
        '1' => 0x06,
        '2' => 0x5B,
        '3' => 0x4F,
        '4' => 0x66,
        '5' => 0x6D,
        '6' => 0x7D,
        '7' => 0x07,
        '8' => 0x7F,
        '9' => 0x6F,
        '-' => 0x40,
        ' ' => 0x00,
        _ => return None,
    })
}

struct Seg {
    h: i32,
    w: i32,
    t: i32,
}

impl Seg {
    fn new(size: u8) -> Seg {
        let h = i32::from(size.clamp(10, 60));
        Seg {
            h,
            w: (h * 11 + 10) / 20,
            t: (h / 8).max(2),
        }
    }
    /// Advance for `c` (narrow for ':' and '.').
    fn advance(&self, c: char) -> i32 {
        match c {
            ':' | '.' => self.t * 2,
            _ if seg_bits(c).is_some() => self.w + self.t,
            _ => self.fallback_scale() * 6,
        }
    }
    fn fallback_scale(&self) -> i32 {
        (self.h / 7).clamp(1, 4)
    }
    fn width(&self, s: &str) -> i32 {
        let w: i32 = s.chars().map(|c| self.advance(c)).sum();
        (w - self.t).max(0)
    }
    fn draw(&self, cv: &mut Canvas, s: &str, x: i32, y: i32, on: bool) {
        let (h, w, t) = (self.h, self.w, self.t);
        let mid = y + (h - t) / 2;
        let mut cx = x;
        for c in s.chars() {
            match c {
                ':' => {
                    cv.rect(cx, y + h / 4, t, t, on);
                    cv.rect(cx, y + h * 3 / 4 - t, t, t, on);
                }
                '.' => cv.rect(cx, y + h - t, t, t, on),
                _ => match seg_bits(c) {
                    Some(b) => {
                        let seg = |cv: &mut Canvas, i: u8, rx: i32, ry: i32, rw: i32, rh: i32| {
                            if b >> i & 1 == 1 {
                                cv.rect(rx, ry, rw, rh, on);
                            }
                        };
                        // Segments meet with a 1 px gap so the digit reads as segments.
                        seg(cv, 0, cx + 1, y, w - 2, t);
                        seg(cv, 1, cx + w - t, y + 1, t, mid - y);
                        seg(cv, 2, cx + w - t, mid + t, t, y + h - 1 - (mid + t));
                        seg(cv, 3, cx + 1, y + h - t, w - 2, t);
                        seg(cv, 4, cx, mid + t, t, y + h - 1 - (mid + t));
                        seg(cv, 5, cx, y + 1, t, mid - y);
                        seg(cv, 6, cx + 1, mid, w - 2, t);
                    }
                    None => {
                        let k = self.fallback_scale();
                        cv.text_ink(&c.to_string(), cx, y, k, on);
                    }
                },
            }
            cx += self.advance(c);
        }
    }
}

// --- icons ----------------------------------------------------------------

/// Icon names `Element::Icon` draws.
pub const ICONS: &[&str] = &[
    "sun", "moon", "partly", "cloud", "fog", "drizzle", "rain", "snow", "storm", "air", "bell",
    "heart", "check", "claude",
];

/// Draw icon `name` (see [`ICONS`]) on a 16x16 grid scaled by `k` at (x, y).
pub fn icon(cv: &mut Canvas, name: &str, x: i32, y: i32, k: i32) {
    // Drawn on a 16x16 grid scaled by k.
    let p = |v: i32| v * k;
    let cloud = |cv: &mut Canvas, dy: i32| {
        cv.disc(x + p(5), y + p(10 + dy), p(3), true);
        cv.disc(x + p(9), y + p(8 + dy), p(4), true);
        cv.disc(x + p(12), y + p(10 + dy), p(3), true);
        cv.rect(x + p(5), y + p(10 + dy), p(7), p(3) + 1, true);
    };
    match name {
        "sun" => {
            cv.disc(x + p(8), y + p(8), p(3), true);
            for (dx, dy) in [
                (0, -1),
                (1, -1),
                (1, 0),
                (1, 1),
                (0, 1),
                (-1, 1),
                (-1, 0),
                (-1, -1),
            ] {
                let (a, b) = (
                    if dx * dy != 0 { 4 } else { 5 },
                    if dx * dy != 0 { 6 } else { 7 },
                );
                cv.line(
                    x + p(8 + dx * a),
                    y + p(8 + dy * a),
                    x + p(8 + dx * b),
                    y + p(8 + dy * b),
                    k,
                    true,
                );
            }
        }
        "moon" => {
            cv.disc(x + p(8), y + p(8), p(6), true);
            cv.disc(x + p(11), y + p(6), p(5), false);
        }
        "partly" => {
            cv.disc(x + p(6), y + p(6), p(3), true);
            for (dx, dy) in [(0, -1), (-1, 0), (-1, -1), (1, -1)] {
                cv.line(
                    x + p(6 + dx * 4),
                    y + p(6 + dy * 4),
                    x + p(6 + dx * 5),
                    y + p(6 + dy * 5),
                    k,
                    true,
                );
            }
            // Cloud in front, outlined in black so the sun stays separate.
            for (cx, cy, r) in [(7, 11, 3), (11, 9, 4), (14, 11, 2)] {
                cv.disc(x + p(cx), y + p(cy), p(r) + 1, false);
            }
            for (cx, cy, r) in [(7, 11, 3), (11, 9, 4), (14, 11, 2)] {
                cv.disc(x + p(cx), y + p(cy), p(r), true);
            }
            cv.rect(x + p(7), y + p(11), p(7), p(3) + 1, true);
        }
        "cloud" => cloud(cv, 0),
        "fog" => {
            for (i, row) in [4, 7, 10, 13].iter().enumerate() {
                let off = if i % 2 == 0 { 1 } else { 3 };
                cv.rect(x + p(off), y + p(*row), p(12), k, true);
            }
        }
        "drizzle" | "rain" => {
            cloud(cv, -3);
            let n = if name == "rain" {
                [4, 8, 12].as_slice()
            } else {
                [6, 10].as_slice()
            };
            for &dx in n {
                cv.line(x + p(dx), y + p(12), x + p(dx - 1), y + p(15), k, true);
            }
        }
        "snow" => {
            cloud(cv, -3);
            for (dx, dy) in [(4, 13), (8, 15), (12, 13)] {
                cv.rect(x + p(dx) - k / 2, y + p(dy) - k, k + 1, k + 1, true);
            }
        }
        "storm" => {
            cloud(cv, -3);
            cv.line(x + p(9), y + p(10), x + p(7), y + p(13), k, true);
            cv.line(x + p(7), y + p(13), x + p(10), y + p(13), k, true);
            cv.line(x + p(10), y + p(13), x + p(8), y + p(16), k, true);
        }
        "air" => {
            // Three gusts with curled ends.
            for (row, len) in [(4, 10), (8, 13), (12, 8)] {
                cv.rect(x + p(1), y + p(row), p(len), k, true);
                cv.ring(x + p(len + 1), y + p(row - 1), k.max(1), true);
            }
        }
        "bell" => {
            cv.disc(x + p(8), y + p(7), p(5), true);
            cv.rect(x + p(3), y + p(7), p(10) + 1, p(5), true);
            cv.rect(x + p(2), y + p(12), p(12) + 1, k, true);
            cv.disc(x + p(8), y + p(14), k, true);
        }
        "heart" => {
            cv.disc(x + p(5), y + p(6), p(3), true);
            cv.disc(x + p(11), y + p(6), p(3), true);
            for r in 0..p(7) {
                cv.rect(x + p(2) + r, y + p(7) + r, p(12) - 2 * r + 1, 1, true);
            }
        }
        "check" => {
            cv.line(x + p(2), y + p(8), x + p(6), y + p(12), k * 2, true);
            cv.line(x + p(6), y + p(12), x + p(14), y + p(4), k * 2, true);
        }
        "claude" => {
            // A tiny Clawd: body, eyes, legs.
            cv.rect(x + p(2), y + p(4), p(12), p(7), true);
            cv.rect(x, y + p(6), p(2), p(3), true);
            cv.rect(x + p(14), y + p(6), p(2), p(3), true);
            cv.rect(x + p(5), y + p(6), k, p(2), false);
            cv.rect(x + p(10), y + p(6), k, p(2), false);
            for lx in [3, 6, 9, 12] {
                cv.rect(x + p(lx), y + p(11), k, p(3), true);
            }
        }
        _ => {
            cv.rect(x, y, p(16), p(16), true);
            cv.rect(x + 1, y + 1, p(16) - 2, p(16) - 2, false);
        }
    }
}

// --- rendering ------------------------------------------------------------

/// A pixel box `[x, y, w, h]` per element, for the editor's hit testing.
pub type Bounds = [i16; 4];

fn num(s: &str) -> f32 {
    s.trim().parse::<f32>().unwrap_or(0.0)
}

fn clamp16(v: i32) -> i16 {
    v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn bitmap_bits(data: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .unwrap_or_default()
}

/// Draw `d` and return the framebuffer and each element's bounds.
pub fn render(d: &Design, c: &Ctx) -> ([u8; FB_SIZE], Vec<Bounds>) {
    let mut cv = Canvas::default();
    let mut bounds = Vec::with_capacity(d.elements.len());
    for el in &d.elements {
        let b: [i32; 4] = match el {
            Element::Text {
                x,
                y,
                text,
                font,
                size,
                align,
                erase,
            } => {
                let s = normalize_caption_long(&expand(text, c)).replace('\n', " ");
                let (w, h) = match font {
                    Font::Small => {
                        let k = i32::from((*size).clamp(1, 4));
                        (Canvas::text_width(&s, k), 7 * k)
                    }
                    Font::Seg => {
                        let sg = Seg::new(*size);
                        (sg.width(&s), sg.h)
                    }
                };
                let x0 = match align {
                    Align::Left => i32::from(*x),
                    Align::Center => i32::from(*x) - w / 2,
                    Align::Right => i32::from(*x) - w,
                };
                let y0 = i32::from(*y);
                match font {
                    Font::Small => {
                        cv.text_ink(&s, x0, y0, i32::from((*size).clamp(1, 4)), !erase);
                    }
                    Font::Seg => Seg::new(*size).draw(&mut cv, &s, x0, y0, !erase),
                }
                [x0, y0, w.max(1), h]
            }
            Element::Rect {
                x,
                y,
                w,
                h,
                fill,
                erase,
            } => {
                let (x, y, w, h) = (i32::from(*x), i32::from(*y), i32::from(*w), i32::from(*h));
                if *fill {
                    cv.rect(x, y, w, h, !erase);
                } else {
                    cv.rect(x, y, w, 1, !erase);
                    cv.rect(x, y + h - 1, w, 1, !erase);
                    cv.rect(x, y, 1, h, !erase);
                    cv.rect(x + w - 1, y, 1, h, !erase);
                }
                [x, y, w, h]
            }
            Element::Line {
                x1,
                y1,
                x2,
                y2,
                thick,
            } => {
                let (a, b, cc, dd) = (
                    i32::from(*x1),
                    i32::from(*y1),
                    i32::from(*x2),
                    i32::from(*y2),
                );
                cv.line(a, b, cc, dd, i32::from((*thick).clamp(1, 4)), true);
                [a.min(cc), b.min(dd), (a - cc).abs() + 1, (b - dd).abs() + 1]
            }
            Element::Bar {
                x,
                y,
                w,
                h,
                value,
                max,
            } => {
                let (x, y, w, h) = (i32::from(*x), i32::from(*y), i32::from(*w), i32::from(*h));
                let (v, m) = (num(&expand(value, c)), num(&expand(max, c)));
                let frac = if m > 0.0 {
                    (v / m).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if h >= 3 {
                    cv.rect(x, y, w, 1, true);
                    cv.rect(x, y + h - 1, w, 1, true);
                    cv.rect(x, y, 1, h, true);
                    cv.rect(x + w - 1, y, 1, h, true);
                    cv.rect(
                        x + 1,
                        y + 1,
                        ((w - 2) as f32 * frac).round() as i32,
                        h - 2,
                        true,
                    );
                } else {
                    cv.rect(x, y, (w as f32 * frac).round() as i32, h, true);
                }
                [x, y, w, h]
            }
            Element::Analog { cx, cy, r, seconds } => {
                let (cx, cy, r) = (i32::from(*cx), i32::from(*cy), i32::from(*r).max(4));
                cv.ring(cx, cy, r, true);
                let at = |frac: f32, len: f32| {
                    let a = frac * std::f32::consts::TAU;
                    (
                        cx + (a.sin() * len).round() as i32,
                        cy - (a.cos() * len).round() as i32,
                    )
                };
                for i in 0..12 {
                    let (x0, y0) = at(i as f32 / 12.0, r as f32 - 1.0);
                    let (x1, y1) = at(
                        i as f32 / 12.0,
                        r as f32 - if i % 3 == 0 { 4.0 } else { 2.0 },
                    );
                    cv.line(x0, y0, x1, y1, 1, true);
                }
                let t = &c.time;
                let m = (f32::from(t.minute) + f32::from(t.second) / 60.0) / 60.0;
                let h = (f32::from(t.hour % 12) + m) / 12.0;
                let (hx, hy) = at(h, r as f32 * 0.5);
                cv.line(cx, cy, hx, hy, 2, true);
                let (mx, my) = at(m, r as f32 * 0.8);
                cv.line(cx, cy, mx, my, 1, true);
                if *seconds {
                    let (sx, sy) = at(f32::from(t.second) / 60.0, r as f32 * 0.9);
                    cv.line(cx, cy, sx, sy, 1, true);
                }
                cv.disc(cx, cy, 1, true);
                [cx - r, cy - r, 2 * r + 1, 2 * r + 1]
            }
            Element::Icon {
                x,
                y,
                icon: name,
                size,
            } => {
                let k = i32::from((*size).clamp(1, 3));
                let n = expand(name, c);
                icon(&mut cv, n.trim(), i32::from(*x), i32::from(*y), k);
                [i32::from(*x), i32::from(*y), 16 * k, 16 * k]
            }
            Element::Bitmap { x, y, w, h, data } => {
                let (x, y, w, h) = (
                    i32::from(*x),
                    i32::from(*y),
                    i32::from(*w).max(0),
                    i32::from(*h).max(0),
                );
                let bits = bitmap_bits(data);
                for py in 0..h {
                    for px in 0..w {
                        let i = (py * w + px) as usize;
                        if bits.get(i / 8).is_some_and(|b| b >> (7 - i % 8) & 1 == 1) {
                            cv.set(x + px, y + py, true);
                        }
                    }
                }
                [x, y, w, h]
            }
        };
        bounds.push(b.map(clamp16));
    }
    (cv.fb, bounds)
}

// --- built-in designs -------------------------------------------------------

fn text(x: i16, y: i16, s: &str, font: Font, size: u8, align: Align) -> Element {
    Element::Text {
        x,
        y,
        text: s.into(),
        font,
        size,
        align,
        erase: false,
    }
}

/// The designs a new install starts with (also the "new from template" list).
pub fn templates() -> Vec<Design> {
    use Align::*;
    use Font::*;
    vec![
        Design {
            id: "clock".into(),
            name: "Clock".into(),
            elements: vec![
                text(64, 6, "{HH}:{MM}", Small, 4, Center),
                Element::Bar {
                    x: 4,
                    y: 40,
                    w: 120,
                    h: 1,
                    value: "{SS}".into(),
                    max: "59".into(),
                },
                text(64, 50, "{DAY} {D} {MON}", Small, 1, Center),
            ],
        },
        Design {
            id: "segclock".into(),
            name: "Big digital".into(),
            elements: vec![
                text(64, 6, "{HH}:{MM}", Seg, 36, Center),
                text(64, 50, "{DAYNAME} {D} {MON}", Small, 1, Center),
            ],
        },
        Design {
            id: "analog".into(),
            name: "Analog".into(),
            elements: vec![
                Element::Analog {
                    cx: 31,
                    cy: 31,
                    r: 30,
                    seconds: true,
                },
                text(97, 14, "{DAY}", Small, 2, Center),
                text(97, 34, "{D} {MON}", Small, 1, Center),
                text(97, 48, "{HH}:{MM}", Small, 1, Center),
            ],
        },
        Design {
            id: "weather".into(),
            name: "Weather".into(),
            elements: vec![
                Element::Icon {
                    x: 2,
                    y: 4,
                    icon: "{WICON}".into(),
                    size: 3,
                },
                text(126, 4, "{TEMP}\u{b0}", Seg, 30, Right),
                text(126, 38, "{TMAX}\u{b0} / {TMIN}\u{b0}", Small, 1, Right),
                text(64, 56, "{COND}", Small, 1, Center),
            ],
        },
        Design {
            id: "clockweather".into(),
            name: "Clock + weather".into(),
            elements: vec![
                text(2, 2, "{HH}:{MM}", Seg, 26, Left),
                Element::Icon {
                    x: 94,
                    y: 0,
                    icon: "{WICON}".into(),
                    size: 2,
                },
                Element::Line {
                    x1: 0,
                    y1: 33,
                    x2: 127,
                    y2: 33,
                    thick: 1,
                },
                text(2, 38, "{TEMP}\u{b0}{UNIT}  {COND}", Small, 1, Left),
                text(2, 50, "{DAY} {D} {MON}", Small, 1, Left),
                text(126, 50, "{HUM}%", Small, 1, Right),
            ],
        },
        Design {
            id: "air".into(),
            name: "Air quality".into(),
            elements: vec![
                Element::Icon {
                    x: 0,
                    y: 2,
                    icon: "air".into(),
                    size: 2,
                },
                text(126, 2, "{AQI}", Seg, 30, Right),
                text(36, 6, "AQI", Small, 2, Left),
                Element::Bar {
                    x: 0,
                    y: 38,
                    w: 128,
                    h: 5,
                    value: "{AQI}".into(),
                    max: "{AQIMAX}".into(),
                },
                text(64, 46, "{AQICAT}", Small, 1, Center),
                text(64, 56, "PM2.5 {PM25}  PM10 {PM10}", Small, 1, Center),
            ],
        },
        Design {
            id: "usage".into(),
            name: "AI usage".into(),
            elements: vec![
                text(0, 4, "{USRC} USAGE", Small, 1, Left),
                text(0, 19, "5H {U5H}%", Small, 1, Left),
                text(127, 19, "{U5HIN}", Small, 1, Right),
                Element::Bar {
                    x: 0,
                    y: 28,
                    w: 128,
                    h: 6,
                    value: "{U5H}".into(),
                    max: "100".into(),
                },
                text(0, 41, "WEEK {UWK}%", Small, 1, Left),
                text(127, 41, "{UWKIN}", Small, 1, Right),
                Element::Bar {
                    x: 0,
                    y: 50,
                    w: 128,
                    h: 6,
                    value: "{UWK}".into(),
                    max: "100".into(),
                },
            ],
        },
        Design {
            id: "minimal".into(),
            name: "Minimal".into(),
            elements: vec![
                text(64, 20, "{h}:{MM}", Small, 3, Center),
                text(64, 50, "{AMPM}", Small, 1, Center),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(w: Option<&Weather>) -> Ctx<'_> {
        Ctx {
            time: ClockTime {
                hour: 13,
                minute: 5,
                second: 9,
                weekday: 0,
                day: 3,
                month: 11,
            },
            year: 2026,
            weather: w,
            claude: "EDIT main.rs",
            usage: None,
        }
    }

    fn sunny() -> Weather {
        Weather {
            city: "Pune".into(),
            temp: 27.6,
            tmax: 31.0,
            tmin: 18.4,
            code: 0,
            is_day: true,
            humidity: 40,
            wind: 7.2,
            unit: "C".into(),
            wind_unit: "km/h".into(),
            aqi: Some(99),
            aqi_scale: "US".into(),
            pm25: Some(25.1),
            pm10: Some(32.7),
        }
    }

    #[test]
    fn tokens_expand() {
        let w = sunny();
        let c = ctx(Some(&w));
        assert_eq!(expand("{HH}:{MM}:{SS} {h}{AMPM}", &c), "13:05:09 1PM");
        assert_eq!(
            expand("{DAY} {DD} {MON} {MO}/{YYYY}", &c),
            "MON 03 NOV 11/2026"
        );
        assert_eq!(
            expand("{TEMP}{UNIT} {COND} {WICON} {CITY}", &c),
            "28C CLEAR sun PUNE"
        );
        assert_eq!(expand("{CLAUDE}", &c), "EDIT main.rs");
        assert_eq!(
            expand("{USRC} {U5H}% {U5HIN} {UWK}% {UWKIN}", &c),
            "-- --% -- --% --"
        );
        let u = Usage {
            source: "CLAUDE".into(),
            five: Some(42),
            five_in: "2H15M".into(),
            week: Some(7),
            week_in: "3D4H".into(),
        };
        let c = Ctx {
            usage: Some(&u),
            ..c
        };
        assert_eq!(
            expand("{USRC} {U5H}% {U5HIN} {UWK}% {UWKIN}", &c),
            "CLAUDE 42% 2H15M 7% 3D4H"
        );
        assert_eq!(
            expand("{AQI} {AQICAT} {PM25}/{PM10} {AQIMAX}", &c),
            "99 MODERATE 25/33 300"
        );
        assert_eq!(aqi_category(79, "EU"), "POOR");
        assert_eq!(aqi_category(151, "IN"), "MODERATE");
        assert_eq!(aqi_category(99, "IN"), "SATISFACTORY");
        assert_eq!(expand("{AQI} {AQICAT}", &ctx(None)), "-- --");
        assert_eq!(expand("{nope} {", &c), "{nope} {");
        assert_eq!(expand("{TEMP}", &ctx(None)), "--");
        assert_eq!(normalize_caption_long("21\u{b0}C é"), "21\u{b0}C  ");
    }

    #[test]
    fn every_template_renders_inside_the_panel() {
        let w = sunny();
        for d in templates() {
            let (fb, b) = render(&d, &ctx(Some(&w)));
            assert_eq!(b.len(), d.elements.len());
            let lit: u32 = fb.iter().map(|x| x.count_ones()).sum();
            assert!(lit > 50, "{} draws almost nothing", d.id);
            // Templates are laid out on the panel (no clipping by accident).
            for (i, [x, y, w, h]) in b.iter().enumerate() {
                assert!(
                    *x >= 0 && *y >= 0 && x + w <= 128 && y + h <= 64,
                    "{} element {i}: {:?}",
                    d.id,
                    b[i]
                );
            }
        }
    }

    #[test]
    fn json_round_trip_and_defaults() {
        let d = &templates()[0];
        let back: Design = serde_json::from_str(&serde_json::to_string(d).unwrap()).unwrap();
        assert_eq!(&back, d);
        let el: Element =
            serde_json::from_str(r#"{"type":"text","x":1,"y":2,"text":"hi"}"#).unwrap();
        assert_eq!(
            el,
            Element::Text {
                x: 1,
                y: 2,
                text: "hi".into(),
                font: Font::Small,
                size: 1,
                align: Align::Left,
                erase: false
            }
        );
    }

    #[test]
    fn seg_digits_differ_and_fit() {
        let s = Seg::new(20);
        assert_eq!((s.h, s.w, s.t), (20, 11, 2));
        let mut a = Canvas::default();
        let mut b = Canvas::default();
        s.draw(&mut a, "8", 0, 0, true);
        s.draw(&mut b, "1", 0, 0, true);
        assert_ne!(a.fb, b.fb);
        assert_eq!(s.width("12:34"), 4 * 13 + 4 - 2);
    }

    #[test]
    fn bitmap_and_icons_draw() {
        // 3x2 bitmap: 101 / 010
        let d = Design {
            id: "t".into(),
            name: "t".into(),
            elements: vec![Element::Bitmap {
                x: 0,
                y: 0,
                w: 3,
                h: 2,
                data: "qA==".into(),
            }],
        };
        let (fb, _) = render(&d, &ctx(None));
        let px = |x: usize, y: usize| fb[(y >> 3) * 128 + x] >> (y & 7) & 1 == 1;
        assert!(px(0, 0) && !px(1, 0) && px(2, 0) && !px(0, 1) && px(1, 1));
        for name in ICONS {
            let mut c = Canvas::default();
            icon(&mut c, name, 0, 0, 1);
            assert!(c.fb.iter().any(|b| *b != 0), "{name} is empty");
        }
    }
}
