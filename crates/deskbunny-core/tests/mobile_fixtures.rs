//! Golden fixtures for the Flutter app (mobile/test/fixtures/): the Dart
//! codecs and the nixxa_engine FFI plugin must match this crate byte for byte.
//!
//! `cargo test -p deskbunny-core --test mobile_fixtures` verifies the committed
//! files; `NIXXA_WRITE_FIXTURES=1 cargo test -p deskbunny-core --test
//! mobile_fixtures` regenerates them.

use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use deskbunny_core::{
    crc16, presets, Cycle, DrawMode, Drawing, Engine, FaceDef, Frame, Mood, Overlay, FB_SIZE,
};
use serde_json::{json, Value};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mobile/test/fixtures")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The drawing blob as the phone sends it: capacity field 0 (ESP32 ignores it).
fn phone_drawing_blob(d: &Drawing) -> Vec<u8> {
    let mut b = d.encode(usize::MAX).expect("unbounded capacity");
    b[14] = 0;
    b[15] = 0;
    b
}

fn xorshift_frame(seed: u32, sparse: bool) -> Frame {
    let mut x = seed.wrapping_mul(2654435761) | 1;
    Frame(
        (0..FB_SIZE)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if sparse && i % 7 < 3 {
                    0
                } else {
                    x as u8
                }
            })
            .collect(),
    )
}

fn dot_frame(i: usize) -> Frame {
    let mut f = vec![0u8; FB_SIZE];
    f[(i * 37) % FB_SIZE] = 0x18;
    f[(i * 91 + 5) % FB_SIZE] = 0x81;
    Frame(f)
}

fn box_frame(x0: usize, page: usize) -> Frame {
    let mut f = vec![0u8; FB_SIZE];
    for x in x0..(x0 + 20).min(128) {
        f[page * 128 + x] = 0xFF;
        f[(page + 1) * 128 + x] = 0x0F;
    }
    Frame(f)
}

fn edge_faces() -> Vec<FaceDef> {
    let d = FaceDef::default();
    vec![
        d.clone(),
        FaceDef {
            name: "  hello, world!!  ".into(),
            eye_w: 200,
            eye_h: 1,
            eye_r: 90,
            eye_gap: 99,
            eye_y: -100,
            speed: 0,
            blink_min: 50,
            blink_max: 10,
            look_min: 0,
            look_max: 0,
            contrast: 3,
            seed: 250,
            ..d.clone()
        },
        FaceDef {
            name: "éa-b_c 12345678".into(),
            mood: Mood::Determined,
            overlay: Overlay::Question,
            eye_y: 127,
            speed: 99,
            invert: true,
            rotate180: true,
            cyclops: true,
            mouth: true,
            blink: false,
            look_around: false,
            breathe: false,
            ..d.clone()
        },
        FaceDef {
            name: "".into(),
            cycle: Cycle::Sequence,
            cycle_seconds: 0,
            cycle_moods: vec![Mood::Sad; 20],
            cycle_overlay: false,
            ..d.clone()
        },
        FaceDef {
            name: "a".repeat(30),
            cycle: Cycle::Random,
            cycle_seconds: 255,
            cycle_moods: vec![Mood::Love, Mood::Dizzy, Mood::Love, Mood::Glitch],
            ..d.clone()
        },
        FaceDef {
            name: "x   y   z   w".into(),
            eye_w: 8,
            eye_h: 56,
            eye_r: 5,
            ..d
        },
    ]
}

/// Valid-CRC blobs with out-of-range fields: the decoder must clamp them.
fn damaged_blobs() -> Vec<[u8; 64]> {
    let base = FaceDef::default().encode();
    let patch = |edits: &[(usize, u8)]| {
        let mut b = base;
        for (i, v) in edits {
            b[*i] = *v;
        }
        let c = crc16(&b[..62]);
        b[62..64].copy_from_slice(&c.to_le_bytes());
        b
    };
    vec![
        patch(&[(9, 99), (36, 50), (38, 9), (39, 0), (40, 20), (41, 0xFF)]),
        patch(&[
            (10, b'h'),
            (11, b'i'),
            (12, b'!'),
            (13, 0),
            (14, b'Z'),
            (22, 200),
            (23, 2),
            (24, 255),
            (25, 77),
            (34, 0),
        ]),
        patch(&[(38, 1), (40, 3), (42, 5), (43, 77), (44, 29), (45, 7), (26, 0x80)]),
        patch(&[(34, 200), (28, 0), (29, 0), (30, 0), (31, 0)]),
    ]
}

fn faces_fixture() -> Value {
    let mut cases = Vec::new();
    for f in presets().into_iter().chain(edge_faces()) {
        let blob = f.encode();
        cases.push(json!({
            "json": f,
            "normalised": f.sanitized(),
            "blob": hex(&blob),
            "decoded": FaceDef::decode(&blob).unwrap(),
        }));
    }
    let damaged: Vec<Value> = damaged_blobs()
        .iter()
        .map(|b| json!({ "blob": hex(b), "decoded": FaceDef::decode(b).unwrap() }))
        .collect();
    json!({ "faces": cases, "decode": damaged })
}

fn drawings() -> Vec<Drawing> {
    vec![
        Drawing {
            mode: DrawMode::Replace,
            frame_ms: 250,
            frames: vec![
                Frame(vec![0x11; FB_SIZE]),
                xorshift_frame(1, true),
                xorshift_frame(2, true),
                Frame(vec![0; FB_SIZE]),
                xorshift_frame(2, true),
            ],
        },
        Drawing {
            mode: DrawMode::Over,
            frame_ms: 250,
            frames: vec![
                Frame(vec![0x11; FB_SIZE]),
                xorshift_frame(1, true),
                xorshift_frame(2, false),
            ],
        },
        Drawing {
            mode: DrawMode::Replace,
            frame_ms: 50,
            frames: (0..10).map(dot_frame).collect(),
        },
        Drawing {
            mode: DrawMode::Over,
            frame_ms: 3,
            frames: (0..12).map(|i| box_frame(i * 9, i % 6)).collect(),
        },
        Drawing {
            mode: DrawMode::Replace,
            frame_ms: 0,
            frames: vec![Frame(vec![0xAA; 10]), Frame(vec![0x55; 2000])],
        },
        Drawing {
            mode: DrawMode::Replace,
            frame_ms: 60000,
            frames: vec![Frame((0..FB_SIZE).map(|i| (i % 256) as u8).collect())],
        },
    ]
}

fn drawings_fixture() -> Value {
    let cases: Vec<Value> = drawings()
        .iter()
        .map(|d| {
            let blob = phone_drawing_blob(d);
            assert_eq!(blob.len(), d.encoded_len());
            json!({
                "json": d,
                "normalised": d.sanitized(),
                "blob": hex(&blob),
                "size": d.encoded_len(),
            })
        })
        .collect();
    json!({ "drawings": cases })
}

fn engine_fixture() -> Value {
    let p = presets();
    let mut dot = vec![0u8; FB_SIZE];
    dot[0] = 1;
    let mut runs: Vec<(FaceDef, u32)> = vec![
        (p[0].clone(), 0),
        (p[1].clone(), 7),
        (p[8].clone(), 3),
        (
            FaceDef {
                invert: true,
                rotate180: true,
                cyclops: true,
                ..p[2].clone()
            },
            5,
        ),
        (p[p.len() - 1].clone(), 11),
        (p[p.len() - 2].clone(), 1),
    ];
    let draws = drawings();
    runs.push((
        FaceDef {
            drawing: Some(draws[0].clone()),
            ..p[0].clone()
        },
        2,
    ));
    runs.push((
        FaceDef {
            drawing: Some(draws[1].clone()),
            invert: true,
            ..p[5].clone()
        },
        4,
    ));
    runs.push((
        FaceDef {
            drawing: Some(Drawing {
                mode: DrawMode::Replace,
                frame_ms: 500,
                frames: vec![Frame(dot)],
            }),
            rotate180: true,
            invert: true,
            ..p[0].clone()
        },
        1,
    ));
    let times: Vec<u32> = vec![
        0, 33, 66, 100, 500, 1000, 1500, 2033, 3000, 4500, 6000, 8000, 9000, 12000, 16500, 25000,
        40000,
    ];
    let cases: Vec<Value> = runs
        .iter()
        .map(|(face, seed)| {
            let mut e = Engine::new(face, *seed);
            let frames: Vec<String> = times.iter().map(|t| B64.encode(e.render(*t))).collect();
            json!({
                "name": face.name,
                "faceBlob": hex(&face.encode()),
                "drawingBlob": face.drawing.as_ref().map(|d| hex(&phone_drawing_blob(d))),
                "seed": seed,
                "times": times,
                "frames": frames,
            })
        })
        .collect();
    json!({ "runs": cases })
}

fn check(name: &str, value: Value) {
    let path = fixtures_dir().join(name);
    let text = serde_json::to_string_pretty(&value).unwrap() + "\n";
    if std::env::var_os("NIXXA_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(fixtures_dir()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}. Regenerate with NIXXA_WRITE_FIXTURES=1 cargo test -p deskbunny-core --test mobile_fixtures",
            path.display()
        )
    });
    let disk: Value = serde_json::from_str(&on_disk).unwrap();
    assert!(
        disk == value,
        "{} is stale. Regenerate with NIXXA_WRITE_FIXTURES=1 cargo test -p deskbunny-core --test mobile_fixtures",
        path.display()
    );
}

#[test]
fn faces() {
    check("faces.json", faces_fixture());
}

#[test]
fn drawings_blobs() {
    check("drawings.json", drawings_fixture());
}

#[test]
fn engine_frames() {
    check("engine_frames.json", engine_fixture());
}
