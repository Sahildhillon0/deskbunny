//! STK500v2 as spoken by the Arduino Mega 2560 "wiring" bootloader (stk500boot.c).
//! Mirrors avrdude's `wiring` programmer.

use std::sync::Arc;
use std::time::Duration;

use crate::boards::{mcu_info, signature_name, Board};
use crate::link::{read_n, write_all, SerialHost, SerialLink};
use crate::{
    check_verify, hex_bytes, FirmwareImage, FlashError, Prog, Progress, Report, Result, Stage,
    Uploader,
};

const MESSAGE_START: u8 = 0x1B;
const TOKEN: u8 = 0x0E;
const STATUS_CMD_OK: u8 = 0x00;

const CMD_SIGN_ON: u8 = 0x01;
const CMD_LOAD_ADDRESS: u8 = 0x06;
const CMD_ENTER_PROGMODE_ISP: u8 = 0x10;
const CMD_LEAVE_PROGMODE_ISP: u8 = 0x11;
const CMD_PROGRAM_FLASH_ISP: u8 = 0x13;
const CMD_READ_FLASH_ISP: u8 = 0x14;
const CMD_READ_SIGNATURE_ISP: u8 = 0x1B;

const SIGN_ON_ATTEMPTS: usize = 8;
const SIGN_ON_TIMEOUT: Duration = Duration::from_millis(200);
const CMD_TIMEOUT: Duration = Duration::from_millis(1000);
/// Largest body the wiring bootloader accepts (275 bytes buffer).
const MAX_BODY: usize = 275;

pub struct Stk500v2 {
    board: Board,
    host: Arc<dyn SerialHost>,
}

/// Framing state: the sequence number increments per message.
struct Session<'a> {
    link: &'a mut dyn SerialLink,
    seq: u8,
}

impl Session<'_> {
    fn send(&mut self, body: &[u8]) -> Result<()> {
        debug_assert!(body.len() <= MAX_BODY);
        let mut msg = Vec::with_capacity(body.len() + 6);
        msg.extend_from_slice(&[
            MESSAGE_START,
            self.seq,
            (body.len() >> 8) as u8,
            body.len() as u8,
            TOKEN,
        ]);
        msg.extend_from_slice(body);
        msg.push(msg.iter().fold(0, |a, b| a ^ b));
        write_all(self.link, &msg)
    }

    fn recv(&mut self, what: &str) -> Result<Vec<u8>> {
        let lost = || {
            format!("The bootloader stopped responding during '{what}'. Unplug and re-plug the board and try again.")
        };
        // Skip noise until MESSAGE_START (bounded).
        let mut skipped = 0;
        loop {
            let b = read_n(self.link, 1, lost)?[0];
            if b == MESSAGE_START {
                break;
            }
            skipped += 1;
            if skipped > 300 {
                return Err(FlashError::Protocol(format!(
                    "Garbled reply from the bootloader during '{what}'."
                )));
            }
        }
        let hdr = read_n(self.link, 4, lost)?;
        let (seq, len, token) = (hdr[0], (hdr[1] as usize) << 8 | hdr[2] as usize, hdr[3]);
        if token != TOKEN || len > MAX_BODY + 4 {
            return Err(FlashError::Protocol(format!(
                "Malformed reply from the bootloader during '{what}'."
            )));
        }
        let mut rest = read_n(self.link, len + 1, lost)?;
        let checksum = rest.pop().unwrap();
        let mut x = MESSAGE_START ^ hdr.iter().fold(0, |a, b| a ^ b);
        x = rest.iter().fold(x, |a, b| a ^ b);
        if x != checksum {
            return Err(FlashError::Protocol(format!(
                "Checksum error in reply from the bootloader during '{what}'. Try a different USB cable or port."
            )));
        }
        if seq != self.seq {
            return Err(FlashError::Protocol(format!(
                "Out-of-sequence reply from the bootloader during '{what}'."
            )));
        }
        Ok(rest)
    }

    /// Send a command, check the echoed command id and STATUS_CMD_OK.
    fn command(&mut self, body: &[u8], what: &str) -> Result<Vec<u8>> {
        self.send(body)?;
        let r = self.recv(what)?;
        self.seq = self.seq.wrapping_add(1);
        if r.len() < 2 || r[0] != body[0] {
            return Err(FlashError::Protocol(format!(
                "Unexpected reply from the bootloader during '{what}'."
            )));
        }
        if r[1] != STATUS_CMD_OK {
            return Err(FlashError::Protocol(format!(
                "The bootloader refused '{what}' (status 0x{:02X}).",
                r[1]
            )));
        }
        Ok(r)
    }
}

impl Stk500v2 {
    pub fn new(board: Board, host: Arc<dyn SerialHost>) -> Self {
        Stk500v2 { board, host }
    }

    fn session(&self, port: &str, baud: u32, image: &FirmwareImage, prog: &Prog) -> Result<Report> {
        let mcu = mcu_info(&self.board.mcu).ok_or_else(|| {
            FlashError::Unsupported(format!("Unknown MCU '{}' for STK500v2", self.board.mcu))
        })?;
        let mut link = self.host.open(port, baud, SIGN_ON_TIMEOUT)?;

        prog.emit(Stage::Reset, 0.5, format!("Resetting the board on {port}"));
        // avrdude `wiring`: drop DTR/RTS 50 ms, raise them, wait 50 ms.
        {
            let e = |e| crate::link::io_err("Could not toggle DTR/RTS", e);
            link.set_dtr(false).map_err(e)?;
            link.set_rts(false).map_err(e)?;
            self.host.sleep(Duration::from_millis(50));
            link.set_dtr(true).map_err(e)?;
            link.set_rts(true).map_err(e)?;
            self.host.sleep(Duration::from_millis(50));
            link.clear_input().map_err(e)?;
        }

        let mut s = Session {
            link: link.as_mut(),
            seq: 1,
        };
        prog.emit(
            Stage::Sync,
            0.0,
            format!("Waiting for the bootloader at {baud} baud"),
        );
        let mut signed_on = false;
        for _ in 0..SIGN_ON_ATTEMPTS {
            let _ = s.link.clear_input();
            match s.command(&[CMD_SIGN_ON], "sign on") {
                Ok(_) => {
                    signed_on = true;
                    break;
                }
                Err(FlashError::NoResponse(_)) | Err(FlashError::Protocol(_)) => {
                    self.host.sleep(Duration::from_millis(30));
                }
                Err(e) => return Err(e),
            }
        }
        if !signed_on {
            return Err(FlashError::NoResponse(format!(
                "No response from bootloader on {port}. Check that this is a Mega 2560 (a Mega 1280 is not supported), that the USB cable carries data, and close any program using the port."
            )));
        }
        s.link
            .set_timeout(CMD_TIMEOUT)
            .map_err(|e| crate::link::io_err("Serial setup failed", e))?;

        s.command(
            &[
                CMD_ENTER_PROGMODE_ISP,
                200,
                100,
                25,
                32,
                0,
                0x53,
                3,
                0xAC,
                0x53,
                0x00,
                0x00,
            ],
            "enter programming mode",
        )?;
        let mut sig = [0u8; 3];
        for (i, b) in sig.iter_mut().enumerate() {
            let r = s.command(
                &[CMD_READ_SIGNATURE_ISP, 4, 0x30, 0x00, i as u8, 0x00],
                "read signature",
            )?;
            *b = *r.get(2).ok_or_else(|| {
                FlashError::Protocol("Short signature reply from the bootloader.".into())
            })?;
        }
        if sig != mcu.signature {
            let _ = s.command(&[CMD_LEAVE_PROGMODE_ISP, 1, 1], "leave programming mode");
            return Err(FlashError::WrongChip(format!(
                "This board has an {} (signature {}), but '{}' expects an {} ({}). Choose the matching board.",
                signature_name(sig),
                hex_bytes(&sig),
                self.board.name,
                signature_name(mcu.signature),
                hex_bytes(&mcu.signature)
            )));
        }
        prog.emit(
            Stage::Sync,
            1.0,
            format!("Found {} bootloader", signature_name(sig)),
        );
        prog.emit(
            Stage::Erase,
            1.0,
            "Bootloader erases each page as it is written",
        );

        let page = mcu.page_size;
        let pages = image.pages(page);
        let total = pages.len() * page as usize;
        // Parts with >64 KiB words of address space need the "extended address" bit.
        let ext = if mcu.flash_size > 128 * 1024 {
            0x8000_0000
        } else {
            0
        };

        for (i, (addr, data)) in pages.iter().enumerate() {
            load_address(&mut s, ext | (addr / 2))?;
            let mut body = vec![
                CMD_PROGRAM_FLASH_ISP,
                (data.len() >> 8) as u8,
                data.len() as u8,
                0xC1, // page mode, write page, wait for RDY
                10,   // delay
                0x40, // load page low byte
                0x4C, // write page
                0x20, // read low byte (poll)
                0x00,
                0x00,
            ];
            body.extend_from_slice(data);
            s.command(&body, "write flash page")?;
            prog.emit(
                Stage::Write,
                (i + 1) as f32 / pages.len() as f32,
                format!("Writing {} / {} bytes", (i + 1) * page as usize, total),
            );
        }

        for (i, (addr, data)) in pages.iter().enumerate() {
            load_address(&mut s, ext | (addr / 2))?;
            let r = s.command(
                &[
                    CMD_READ_FLASH_ISP,
                    (data.len() >> 8) as u8,
                    data.len() as u8,
                    0x20,
                ],
                "read flash page",
            )?;
            // Reply: cmd, STATUS_OK, data..., STATUS_OK
            if r.len() != data.len() + 3 || r[r.len() - 1] != STATUS_CMD_OK {
                return Err(FlashError::Protocol(
                    "Malformed flash read reply from the bootloader.".into(),
                ));
            }
            check_verify(*addr, data, &r[2..r.len() - 1])?;
            prog.emit(
                Stage::Verify,
                (i + 1) as f32 / pages.len() as f32,
                format!("Verifying {} / {} bytes", (i + 1) * page as usize, total),
            );
        }

        s.command(&[CMD_LEAVE_PROGMODE_ISP, 1, 1], "leave programming mode")?;
        Ok(Report {
            protocol: "stk500v2".into(),
            port: port.to_string(),
            baud: Some(baud),
            signature: Some(hex_bytes(&sig)),
            bytes_written: total,
            verified: true,
            duration_ms: prog.elapsed_ms(),
            warnings: vec![],
        })
    }
}

fn load_address(s: &mut Session, word_addr: u32) -> Result<()> {
    let b = word_addr.to_be_bytes();
    s.command(&[CMD_LOAD_ADDRESS, b[0], b[1], b[2], b[3]], "set address")
        .map(|_| ())
}

impl Uploader for Stk500v2 {
    fn upload(
        &self,
        port: &str,
        image: &FirmwareImage,
        progress: &dyn Fn(Progress),
    ) -> Result<Report> {
        let prog = Prog::new(progress);
        let max = self.board.max_app_size();
        if image.end() > max {
            return Err(FlashError::Image(format!(
                "The firmware ends at {} bytes but the {} has room for {max} bytes.",
                image.end(),
                self.board.name
            )));
        }
        let baud = self.board.baud.first().copied().unwrap_or(115200);
        let r = self.session(port, baud, image, &prog)?;
        prog.emit(Stage::Done, 1.0, "Upload complete");
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::builtin_boards;
    use crate::mock::{test_image, Device, Flash, MockHost, Pipe, ProgressLog};

    /// Wiring (stk500boot.c) simulator for an ATmega2560.
    struct Wiring {
        pipe: Pipe,
        dtr: bool,
        active: bool,
        addr: usize,
        flash: Flash,
        silent: bool,
    }

    impl Wiring {
        fn new() -> Self {
            Wiring {
                pipe: Pipe::default(),
                dtr: false,
                active: false,
                addr: 0,
                flash: Flash::new(256 * 1024),
                silent: false,
            }
        }

        fn respond(&mut self, seq: u8, body: &[u8]) {
            let mut msg = vec![
                MESSAGE_START,
                seq,
                (body.len() >> 8) as u8,
                body.len() as u8,
                TOKEN,
            ];
            msg.extend_from_slice(body);
            msg.push(msg.iter().fold(0, |a, b| a ^ b));
            self.pipe.reply(&msg);
        }

        fn step(&mut self) -> bool {
            let inp = &self.pipe.input;
            if inp.len() < 6 {
                return false;
            }
            assert_eq!(inp[0], MESSAGE_START);
            let len = (inp[2] as usize) << 8 | inp[3] as usize;
            if inp.len() < len + 6 {
                return false;
            }
            let msg: Vec<u8> = self.pipe.input.drain(..len + 6).collect();
            assert_eq!(
                msg.iter().fold(0, |a, b| a ^ b),
                0,
                "bad checksum from host"
            );
            let seq = msg[1];
            let body = &msg[5..5 + len];
            match body[0] {
                CMD_SIGN_ON => self.respond(
                    seq,
                    &[
                        CMD_SIGN_ON,
                        0,
                        8,
                        b'A',
                        b'V',
                        b'R',
                        b'I',
                        b'S',
                        b'P',
                        b'_',
                        b'2',
                    ],
                ),
                CMD_ENTER_PROGMODE_ISP | CMD_LEAVE_PROGMODE_ISP => self.respond(seq, &[body[0], 0]),
                CMD_READ_SIGNATURE_ISP => {
                    let sig = [0x1E, 0x98, 0x01][body[4] as usize];
                    self.respond(seq, &[body[0], 0, sig, 0]);
                }
                CMD_LOAD_ADDRESS => {
                    let a = u32::from_be_bytes([body[1], body[2], body[3], body[4]]);
                    assert!(a & 0x8000_0000 != 0, "2560 needs the extended address bit");
                    self.addr = ((a & 0x7FFF_FFFF) * 2) as usize;
                    self.respond(seq, &[body[0], 0]);
                }
                CMD_PROGRAM_FLASH_ISP => {
                    let n = (body[1] as usize) << 8 | body[2] as usize;
                    assert_eq!(n, 256);
                    let data = body[10..10 + n].to_vec();
                    self.flash.write(self.addr, &data);
                    self.respond(seq, &[body[0], 0]);
                }
                CMD_READ_FLASH_ISP => {
                    let n = (body[1] as usize) << 8 | body[2] as usize;
                    let mut r = vec![body[0], 0];
                    r.extend_from_slice(self.flash.read(self.addr, n));
                    r.push(0);
                    self.respond(seq, &r);
                }
                other => panic!("unexpected command {other:02X}"),
            }
            true
        }
    }

    impl Device for Wiring {
        fn set_dtr(&mut self, level: bool) {
            if level && !self.dtr {
                self.active = true;
            }
            self.dtr = level;
        }
        fn write(&mut self, data: &[u8]) {
            if !self.active || self.silent {
                return;
            }
            self.pipe.input.extend_from_slice(data);
            while self.step() {}
        }
        fn read(&mut self, buf: &mut [u8]) -> usize {
            self.pipe.read(buf)
        }
        fn clear_input(&mut self) {
            self.pipe.output.clear();
        }
        fn flash_snapshot(&self) -> Option<Vec<u8>> {
            Some(self.flash.mem.clone())
        }
    }

    fn mega() -> Board {
        builtin_boards()
            .into_iter()
            .find(|b| b.id == "mega")
            .unwrap()
    }

    #[test]
    fn uploads_above_64k_with_extended_addressing() {
        let host = Arc::new(MockHost::new(Box::new(Wiring::new())));
        let up = Stk500v2::new(mega(), host.clone());
        // Two segments: low code and a block above 128 KiB.
        let low = test_image(0, 2000);
        let mut segs = low.segments().to_vec();
        segs.push(crate::image::Segment {
            addr: 0x2_0010,
            data: vec![0x5A; 300],
        });
        let img = FirmwareImage::from_segments(segs).unwrap();
        let log = ProgressLog::default();
        let r = up.upload("/dev/ttyACM0", &img, &log.cb()).unwrap();
        log.assert_sane();
        assert!(r.verified);
        assert_eq!(r.signature.as_deref(), Some("1E9801"));
        let flash = host.flash();
        assert_eq!(&flash[..2000], &low.flatten(0, 2000, 0xFF)[..]);
        assert!(flash[0x2_0010..0x2_0010 + 300].iter().all(|b| *b == 0x5A));
        assert_eq!(flash[0x2_000F], 0xFF);
    }

    #[test]
    fn silent_bootloader_gives_helpful_error() {
        let mut dev = Wiring::new();
        dev.silent = true;
        let host = Arc::new(MockHost::new(Box::new(dev)));
        let up = Stk500v2::new(mega(), host);
        let e = up
            .upload("COM5", &test_image(0, 100), &|_| {})
            .unwrap_err()
            .to_string();
        assert!(e.contains("No response from bootloader on COM5"), "{e}");
    }
}
