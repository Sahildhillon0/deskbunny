//! Contact sheet of the built-in designs and every icon, as a PBM:
//!   cargo run -p deskbunny-core --example designs -- out.pbm
//!   python3 engine/tools/pbm2png.py out.pbm out.png 3
use deskbunny_core::layout::{render, templates, Ctx, Design, Element, Weather, ICONS};
use deskbunny_core::live::ClockTime;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "designs.pbm".into());
    let w = Weather {
        city: "Pune".into(),
        temp: 27.6,
        tmax: 31.0,
        tmin: 18.4,
        code: 2,
        is_day: true,
        humidity: 40,
        wind: 7.2,
        unit: "C".into(),
        wind_unit: "km/h".into(),
        aqi: Some(99),
        aqi_scale: "US".into(),
        pm25: Some(25.1),
        pm10: Some(32.7),
    };
    let ctx = Ctx {
        time: ClockTime {
            hour: 10,
            minute: 42,
            second: 30,
            weekday: 1,
            day: 10,
            month: 11,
        },
        year: 2026,
        weather: Some(&w),
        claude: "EDIT main.rs",
    };
    let mut frames: Vec<[u8; 1024]> = templates().iter().map(|d| render(d, &ctx).0).collect();
    let icons = Design {
        id: "icons".into(),
        name: "icons".into(),
        elements: ICONS
            .iter()
            .enumerate()
            .map(|(i, n)| Element::Icon {
                x: (i % 7) as i16 * 18,
                y: (i / 7) as i16 * 20 + 2,
                icon: (*n).into(),
                size: 1,
            })
            .collect(),
    };
    frames.push(render(&icons, &ctx).0);
    let (cols, gap) = (2usize, 2usize);
    let rows = frames.len().div_ceil(cols);
    let (cw, ch) = (cols * (128 + gap), rows * (64 + gap));
    let mut px = vec![0u8; cw * ch];
    for (i, fb) in frames.iter().enumerate() {
        let (ox, oy) = ((i % cols) * (128 + gap), (i / cols) * (64 + gap));
        for y in 0..64 {
            for x in 0..128 {
                px[(oy + y) * cw + ox + x] = fb[(y >> 3) * 128 + x] >> (y & 7) & 1;
            }
        }
    }
    let mut data = format!("P4\n{cw} {ch}\n").into_bytes();
    for y in 0..ch {
        for x in (0..cw).step_by(8) {
            let mut b = 0u8;
            for k in 0..8 {
                if x + k < cw && px[y * cw + x + k] == 1 {
                    b |= 0x80 >> k;
                }
            }
            data.push(b);
        }
    }
    std::fs::write(&out, data).expect("write");
    println!("{} screens -> {out}", frames.len());
}
