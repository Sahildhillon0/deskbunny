//! SAM-BA ("bossa") upload for the Uno R4 WiFi.
//!
//! Path: host USB -> ESP32-S3 bridge (2341:1002, never re-enumerates) -> UART ->
//! RA4M1 Arduino bootloader (github.com/arduino/arduino-renesas-bootloader,
//! src/bossa.c). The bridge turns a 1200-baud open into a double reset of the
//! RA4M1, which then runs the bootloader; every other baud is copied onto the
//! UART, so the host reopens the same port at 230400.
//!
//! The bootloader implements an Arduino "SAM-BA extended" subset. All flash
//! offsets are relative to the application base 0x4000:
//! - `N#` -> "\n\r"; `V#` -> "Arduino Bootloader (SAM-BA extended) 2.0 [Arduino:IKXYZ]\n\r"
//! - `S<idx>,<n>#` + n raw bytes -> into an 8 KiB RAM buffer at index idx (no reply)
//! - `Y<idx>,0#` -> select buffer index, "Y\n\r"
//! - `Y<off>,<n>#` -> erase the 4 KiB block at off (when 2 KiB aligned), program,
//!   "Y\n\r"
//! - `Z<off>,<n>#` (n <= 4096) -> "Z%08X#\n\r", CRC-16/XMODEM of flash
//! - `X...#` -> "X\n\r" (erases nothing), `K#` -> reset, no reply
//!
//! This mirrors the sequence Arduino's bossac 1.9.1-arduino5 sends for
//! `-U -e -w sketch.bin -R`, minus its RAM applet and register writes (which this
//! bootloader ignores), plus a CRC verify per block. Protocol facts come from the
//! bootloader and bridge sources; the end-to-end path is UNVERIFIED on hardware.

use std::sync::Arc;
use std::time::Duration;

use crate::boards::{Board, UsbRole};
use crate::link::{
    open_retry, read_n, touch_1200, wait_for_bootloader_port, write_all, SerialHost, SerialLink,
};
use crate::{FirmwareImage, FlashError, Prog, Progress, Report, Result, Stage, Uploader};

const CMD_TIMEOUT: Duration = Duration::from_millis(1000);
/// Bytes per S/Y/Z round (the bootloader's Z limit and erase granularity).
const BLOCK: usize = 4096;
/// RAM buffer index used for staging, as bossac does (past its 52-byte applet).
const BUF_INDEX: u32 = 0x34;
/// Application base on the RA4M1; sketches are linked here.
pub const APP_BASE: u32 = 0x4000;

const UNVERIFIED_NOTE: &str = "Uno R4 WiFi uploads are implemented from Arduino's bootloader sources but not yet verified on hardware. If this keeps failing, upload any sketch once with the Arduino IDE to rule out the board, and report the error text to DeskBunny.";

/// A SAM-BA session on an open link.
pub struct SambaClient<'a> {
    link: &'a mut dyn SerialLink,
    host: &'a dyn SerialHost,
    pub version: String,
}

impl<'a> SambaClient<'a> {
    /// `N#` (binary mode) then `V#` (version).
    pub fn connect(link: &'a mut dyn SerialLink, host: &'a dyn SerialHost) -> Result<Self> {
        let _ = link.clear_input();
        write_all(link, b"N#")?;
        // Expected "\n\r"; tolerate silence here, V# decides.
        let _ = read_n(link, 2, String::new);
        let mut c = SambaClient {
            link,
            host,
            version: String::new(),
        };
        write_all(c.link, b"V#")?;
        c.version = c.read_line("read version")?;
        Ok(c)
    }

    /// Read until "\n\r" (or "\r\n").
    fn read_line(&mut self, what: &str) -> Result<String> {
        let mut out = Vec::new();
        loop {
            let b = read_n(self.link, 1, || lost(what))?[0];
            out.push(b);
            if out.ends_with(b"\n\r") || out.ends_with(b"\r\n") {
                out.truncate(out.len() - 2);
                return Ok(String::from_utf8_lossy(&out).trim().to_string());
            }
            if out.len() > 256 {
                return Err(FlashError::Protocol(format!(
                    "Overlong reply from the bootloader during '{what}'."
                )));
            }
        }
    }

    /// The "[Arduino:...XYZ]" capability tag: chip erase, buffer write, checksum.
    pub fn is_extended(&self) -> bool {
        let caps = self
            .version
            .split("[Arduino:")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .unwrap_or("");
        ['X', 'Y', 'Z'].iter().all(|c| caps.contains(*c))
    }

    fn expect_ack(&mut self, cmd: u8, what: &str) -> Result<()> {
        let r = read_n(self.link, 3, || lost(what))?;
        if r != [cmd, b'\n', b'\r'] {
            return Err(FlashError::Protocol(format!(
                "Unexpected reply from the bootloader during '{what}'."
            )));
        }
        Ok(())
    }

    /// Stage bytes in the bootloader's RAM buffer at `index`.
    pub fn send_buffer(&mut self, index: u32, data: &[u8]) -> Result<()> {
        write_all(
            self.link,
            format!("S{index:08X},{:08X}#", data.len()).as_bytes(),
        )?;
        // The bootloader switches to raw receive in its main loop; bytes arriving
        // before that would be parsed as command text.
        self.host.sleep(Duration::from_millis(10));
        write_all(self.link, data)
    }

    /// `X`: accepted for protocol parity; this bootloader erases per block in `Y`.
    pub fn chip_erase(&mut self) -> Result<()> {
        write_all(self.link, b"X00000000#")?;
        self.expect_ack(b'X', "erase flash")
    }

    /// Program `size` bytes from buffer `index` to flash offset `off` (from 0x4000).
    pub fn write_flash(&mut self, index: u32, off: u32, size: u32) -> Result<()> {
        write_all(self.link, format!("Y{index:08X},0#").as_bytes())?;
        self.expect_ack(b'Y', "select buffer")?;
        write_all(self.link, format!("Y{off:08X},{size:08X}#").as_bytes())?;
        self.expect_ack(b'Y', "write flash")
    }

    /// CRC-16/XMODEM of `size` (<= 4096) bytes of flash at offset `off`.
    pub fn checksum(&mut self, off: u32, size: u32) -> Result<u16> {
        write_all(self.link, format!("Z{off:08X},{size:08X}#").as_bytes())?;
        let line = self.read_line("checksum")?;
        let malformed =
            || FlashError::Protocol("Malformed checksum reply from the bootloader.".into());
        let hex = line
            .strip_prefix('Z')
            .and_then(|s| s.strip_suffix('#'))
            .ok_or_else(malformed)?;
        u32::from_str_radix(hex, 16)
            .map(|v| v as u16)
            .map_err(|_| malformed())
    }

    /// `K#`: reset into the sketch (no reply).
    pub fn reset(&mut self) -> Result<()> {
        write_all(self.link, b"K#")
    }
}

fn lost(what: &str) -> String {
    format!("The bootloader stopped responding during '{what}'. Unplug and re-plug the board and try again.")
}

/// CRC-16/XMODEM (poly 0x1021, init 0), as the bootloader's `Z` computes it.
pub fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub struct Samba {
    board: Board,
    host: Arc<dyn SerialHost>,
}

impl Samba {
    pub fn new(board: Board, host: Arc<dyn SerialHost>) -> Self {
        Samba { board, host }
    }

    fn run(&self, port: &str, image: &FirmwareImage, prog: &Prog) -> Result<Report> {
        if image.base() < APP_BASE || image.end() - APP_BASE > self.board.max_app_size() {
            return Err(FlashError::Image(format!(
                "The firmware does not fit the {} application area.",
                self.board.name
            )));
        }
        let host = self.host.as_ref();
        let mut boot_port = port.to_string();
        if self.board.touch1200 {
            let before = host.list()?;
            prog.emit(
                Stage::Reset,
                0.2,
                format!("Resetting {port} with a 1200-baud touch"),
            );
            let touch_err = touch_1200(host, port);
            // The bridge pulses the RA4M1 reset twice (~400 ms) and the bootloader
            // then starts; the USB port itself stays put.
            host.sleep(Duration::from_millis(600));
            if !host.list()?.iter().any(|p| p.path == port) {
                let boot_ids: Vec<(u16, u16)> = self.board.usb_ids(UsbRole::Bootloader).collect();
                boot_port = wait_for_bootloader_port(
                    host,
                    &before,
                    port,
                    &boot_ids,
                    Duration::from_secs(8),
                )
                .map_err(|e| touch_err.unwrap_or(e))?;
            }
        }
        let baud = self.board.baud.first().copied().unwrap_or(230400);
        let mut link = open_retry(host, &boot_port, baud, CMD_TIMEOUT, Duration::from_secs(3))?;
        prog.emit(
            Stage::Sync,
            0.2,
            format!("Talking to the bootloader on {boot_port}"),
        );
        let mut c = SambaClient::connect(link.as_mut(), host).map_err(|e| match e {
            FlashError::NoResponse(_) => FlashError::NoResponse(format!(
                "No response from the SAM-BA bootloader on {boot_port}. Unplug and re-plug the board, or double-press its reset button, then try again."
            )),
            other => other,
        })?;
        if !c.is_extended() {
            return Err(FlashError::Unsupported(format!(
                "The bootloader on {boot_port} reports '{}', which lacks the Arduino SAM-BA extensions (X, Y, Z) DeskBunny needs.",
                c.version
            )));
        }
        prog.emit(Stage::Sync, 1.0, format!("Bootloader: {}", c.version));

        // Whole 4 KiB blocks: each Y erases the block it writes, so pad with 0xFF.
        let mut data = image.flatten(APP_BASE, image.end(), 0xFF);
        data.resize(data.len().div_ceil(BLOCK) * BLOCK, 0xFF);
        prog.emit(Stage::Erase, 0.5, "Erasing flash");
        c.chip_erase()?;
        prog.emit(
            Stage::Erase,
            1.0,
            "Flash erases block by block while writing",
        );

        let total = data.len();
        for (i, block) in data.chunks(BLOCK).enumerate() {
            let off = (i * BLOCK) as u32;
            c.send_buffer(BUF_INDEX, block)?;
            c.write_flash(BUF_INDEX, off, block.len() as u32)?;
            let done = (i + 1) * BLOCK;
            prog.emit(
                Stage::Write,
                done as f32 / total as f32,
                format!("Writing {done} / {total} bytes"),
            );
        }

        for (i, block) in data.chunks(BLOCK).enumerate() {
            let off = (i * BLOCK) as u32;
            let crc = c.checksum(off, block.len() as u32)?;
            let want = crc16_xmodem(block);
            if crc != want {
                return Err(FlashError::Verify(format!(
                    "Verify failed in the block at 0x{:05X} (CRC {crc:04X}, expected {want:04X}). Try uploading again.",
                    APP_BASE + off
                )));
            }
            let done = (i + 1) * BLOCK;
            prog.emit(
                Stage::Verify,
                done as f32 / total as f32,
                format!("Verifying {done} / {total} bytes"),
            );
        }

        c.reset()?;
        prog.emit(Stage::Done, 1.0, "Upload complete");
        Ok(Report {
            protocol: "samba".into(),
            port: boot_port,
            baud: Some(baud),
            signature: None,
            bytes_written: total,
            verified: true,
            duration_ms: prog.elapsed_ms(),
            warnings: vec![],
        })
    }
}

impl Uploader for Samba {
    fn upload(
        &self,
        port: &str,
        image: &FirmwareImage,
        progress: &dyn Fn(Progress),
    ) -> Result<Report> {
        let prog = Prog::new(progress);
        self.run(port, image, &prog).map_err(|e| match e {
            FlashError::Image(m) => FlashError::Image(m),
            FlashError::Port(m) => FlashError::Port(m),
            FlashError::NoResponse(m) => FlashError::NoResponse(format!("{m} {UNVERIFIED_NOTE}")),
            FlashError::Protocol(m) => FlashError::Protocol(format!("{m} {UNVERIFIED_NOTE}")),
            FlashError::Verify(m) => FlashError::Verify(format!("{m} {UNVERIFIED_NOTE}")),
            FlashError::Unsupported(m) => FlashError::Unsupported(format!("{m} {UNVERIFIED_NOTE}")),
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::builtin_boards;
    use crate::mock::{test_image, Device, MockHost, Pipe, ProgressLog};
    use crate::ports::{PortInfo, UsbInfo};

    /// Simulator of arduino-renesas-bootloader's bossa.c behind the USB bridge.
    struct Monitor {
        pipe: Pipe,
        in_boot: bool,
        open_baud: u32,
        buffer: Vec<u8>,
        /// Absolute flash, 256 KiB; the app area starts at 0x4000.
        flash: Vec<u8>,
        copy_index: usize,
        pending_s: Option<(usize, usize)>,
        version: &'static str,
        corrupt_at: Option<usize>,
    }

    impl Monitor {
        fn new() -> Self {
            Monitor {
                pipe: Pipe::default(),
                in_boot: false,
                open_baud: 0,
                buffer: vec![0; 8192],
                flash: vec![0xAA; 256 * 1024], // old sketch everywhere
                copy_index: 0,
                pending_s: None,
                version: "Arduino Bootloader (SAM-BA extended) 2.0 [Arduino:IKXYZ]",
                corrupt_at: None,
            }
        }

        /// First argument is exactly 8 hex digits; the second starts at index 10.
        fn args(cmd: &str) -> (usize, usize) {
            assert!(cmd.len() >= 9, "{cmd}");
            let a = usize::from_str_radix(&cmd[1..9], 16).unwrap();
            let b = if cmd.len() > 10 {
                assert_eq!(&cmd[9..10], ",");
                usize::from_str_radix(&cmd[10..], 16).unwrap()
            } else {
                0
            };
            (a, b)
        }

        fn step(&mut self) -> bool {
            if let Some((idx, n)) = self.pending_s {
                if self.pipe.input.len() < n {
                    return false;
                }
                let data: Vec<u8> = self.pipe.input.drain(..n).collect();
                self.buffer[idx..idx + n].copy_from_slice(&data);
                self.pending_s = None;
                return true;
            }
            let Some(end) = self.pipe.input.iter().position(|b| *b == b'#') else {
                return false;
            };
            let raw: Vec<u8> = self.pipe.input.drain(..=end).collect();
            let cmd = String::from_utf8(raw[..raw.len() - 1].to_vec()).unwrap();
            match &cmd[..1] {
                "N" => self.pipe.reply(b"\n\r"),
                "V" => {
                    let v = format!("{}\n\r", self.version);
                    self.pipe.reply(v.as_bytes());
                }
                "I" => self.pipe.reply(b"nRF52840-QIAA\n\r"),
                "S" => {
                    let (idx, n) = Self::args(&cmd);
                    assert!(idx + n <= 8192);
                    self.pending_s = Some((idx, n));
                }
                "X" => self.pipe.reply(b"X\n\r"),
                "Y" => {
                    let (a, n) = Self::args(&cmd);
                    if n == 0 {
                        self.copy_index = a;
                    } else {
                        let dst = 0x4000 + a;
                        if dst % 2048 == 0 {
                            self.flash[dst..dst + 4096].fill(0xFF);
                        }
                        let data = self.buffer[self.copy_index..self.copy_index + n].to_vec();
                        self.flash[dst..dst + n].copy_from_slice(&data);
                        if let Some(c) = self.corrupt_at {
                            if (dst..dst + n).contains(&c) {
                                self.flash[c] ^= 0x10;
                            }
                        }
                    }
                    self.pipe.reply(b"Y\n\r");
                }
                "Z" => {
                    let (a, n) = Self::args(&cmd);
                    assert!(n <= 4096);
                    let crc = crc16_xmodem(&self.flash[0x4000 + a..0x4000 + a + n]);
                    let r = format!("Z{crc:08X}#\n\r");
                    self.pipe.reply(r.as_bytes());
                }
                "K" => self.in_boot = false,
                _ => {} // R, W, w, G, O, o: silently dropped by this bootloader
            }
            true
        }
    }

    impl Device for Monitor {
        fn open(&mut self, _path: &str, baud: u32) -> std::result::Result<(), String> {
            self.open_baud = baud;
            Ok(())
        }
        fn set_dtr(&mut self, level: bool) {
            if !level && self.open_baud == 1200 {
                self.in_boot = true;
            }
        }
        fn write(&mut self, data: &[u8]) {
            // The bridge forwards UART data at the opened baud; 230400 is required.
            if !self.in_boot || self.open_baud != 230400 {
                return;
            }
            self.pipe.input.extend_from_slice(data);
            while self.step() {}
        }
        fn read(&mut self, buf: &mut [u8]) -> usize {
            self.pipe.read(buf)
        }
        fn flash_snapshot(&self) -> Option<Vec<u8>> {
            Some(self.flash.clone())
        }
    }

    fn wifi() -> Board {
        builtin_boards()
            .into_iter()
            .find(|b| b.id == "uno_r4_wifi")
            .unwrap()
    }

    fn setup(dev: Monitor, board: Board) -> (Arc<MockHost>, Samba) {
        let host = Arc::new(MockHost::new(Box::new(dev)));
        host.set_default_listing(vec![PortInfo {
            path: "COM9".into(),
            usb: Some(UsbInfo {
                vid: 0x2341,
                pid: 0x1002,
                ..Default::default()
            }),
        }]);
        let up = Samba::new(board, host.clone());
        (host, up)
    }

    #[test]
    fn crc_xmodem_check_value() {
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
    }

    #[test]
    fn uploads_through_extended_samba() {
        let (host, up) = setup(Monitor::new(), wifi());
        let img = test_image(0x4000, 10_000);
        let log = ProgressLog::default();
        let r = up.upload("COM9", &img, &log.cb()).unwrap();
        log.assert_sane();
        assert!(r.verified);
        assert_eq!(r.port, "COM9");
        assert_eq!(r.baud, Some(230400));
        assert_eq!(r.bytes_written, 12_288);
        let flash = host.flash();
        assert_eq!(
            &flash[0x4000..0x4000 + 10_000],
            &img.flatten(0x4000, 0x4000 + 10_000, 0xFF)[..]
        );
        assert!(flash[0x4000 + 10_000..0x4000 + 12_288]
            .iter()
            .all(|b| *b == 0xFF));
        assert!(
            flash[..0x4000].iter().all(|b| *b == 0xAA),
            "bootloader untouched"
        );
        let opens = host.opens.lock().unwrap();
        assert_eq!(opens[0], ("COM9".to_string(), 1200));
        assert_eq!(opens[1], ("COM9".to_string(), 230400));
    }

    #[test]
    fn crc_mismatch_fails_verify() {
        let mut dev = Monitor::new();
        dev.corrupt_at = Some(0x4000 + 5000);
        let (_, up) = setup(dev, wifi());
        let e = up
            .upload("COM9", &test_image(0x4000, 10_000), &|_| {})
            .unwrap_err();
        assert!(matches!(e, FlashError::Verify(_)));
        assert!(e.to_string().contains("0x05000"), "{e}");
    }

    #[test]
    fn plain_samba_is_refused_with_explanation() {
        let mut dev = Monitor::new();
        dev.version = "v1.1 Nov 2010 14:20:38";
        let (_, up) = setup(dev, wifi());
        let e = up
            .upload("COM9", &test_image(0x4000, 100), &|_| {})
            .unwrap_err()
            .to_string();
        assert!(e.contains("lacks the Arduino SAM-BA extensions"), "{e}");
        assert!(e.contains("not yet verified on hardware"), "{e}");
    }

    #[test]
    fn silent_bootloader_gives_helpful_error() {
        // The touch never happens, so the RA4M1 is still running the sketch.
        let mut b = wifi();
        b.touch1200 = false;
        let (_, up) = setup(Monitor::new(), b);
        let e = up
            .upload("COM9", &test_image(0x4000, 100), &|_| {})
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("No response from the SAM-BA bootloader on COM9"),
            "{e}"
        );
    }
}
