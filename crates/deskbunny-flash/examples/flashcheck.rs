//! Flash a prebuilt image like the app does, then check the sketch is still
//! running by asking for its banner a second time.
//!
//!   cargo run -p deskbunny-flash --example flashcheck -- <board id> <port> <image> [--no-query]
//!
//! `--no-query` skips the app's post-upload banner query (control run).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use deskbunny_flash::boards::builtin_boards;
use deskbunny_flash::link::query_device_info;
use deskbunny_flash::{uploader_for, FirmwareImage, SerialHost, SystemHost};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: flashcheck <board id> <port> <image.hex|image.bin> [--no-query]");
        std::process::exit(2);
    }
    let (board_id, port, path) = (&args[0], &args[1], &args[2]);
    let skip_query = args.iter().any(|a| a == "--no-query");
    let board = builtin_boards()
        .into_iter()
        .find(|b| &b.id == board_id)
        .unwrap_or_else(|| panic!("unknown board {board_id}"));
    let bytes = std::fs::read(path).expect("read image");
    let image = if path.ends_with(".hex") {
        FirmwareImage::from_hex(&String::from_utf8_lossy(&bytes)).expect("parse hex")
    } else {
        FirmwareImage::from_bin(board.bin_base(), &bytes)
    };

    let host: Arc<dyn SerialHost> = Arc::new(SystemHost);
    let report = uploader_for(&board, host.clone())
        .upload(port, &image, &|p| {
            eprintln!("  {:?} {}% {}", p.stage, p.percent, p.message)
        })
        .expect("upload");
    eprintln!(
        "upload ok: {} bytes in {} ms",
        report.bytes_written, report.duration_ms
    );

    // The app waits 800 ms, then up to 8 s for the sketch's port.
    host.sleep(Duration::from_millis(800));
    let start = Instant::now();
    while !Path::new(port).exists() && start.elapsed() < Duration::from_secs(8) {
        host.sleep(Duration::from_millis(250));
    }
    eprintln!("port back after {} ms", 800 + start.elapsed().as_millis());

    if !skip_query {
        let first = query_device_info(host.as_ref(), port, Duration::from_millis(4000));
        eprintln!("post-upload query (as the app): {first:?}");
    }

    host.sleep(Duration::from_secs(3));
    let second = query_device_info(host.as_ref(), port, Duration::from_millis(3000));
    match second {
        Ok(Some(line)) => println!("ALIVE: {line}"),
        other => println!("NO ANSWER (sketch hung or not running): {other:?}"),
    }
}
