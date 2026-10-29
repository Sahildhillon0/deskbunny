//! AVR109 ("butterfly") as spoken by Caterina on the Leonardo and Micro.
//! Mirrors avrdude's `avr109` programmer plus Arduino's 1200-baud touch.

use std::sync::Arc;
use std::time::Duration;

use crate::boards::{mcu_info, signature_name, Board, UsbRole};
use crate::link::{
    open_retry, read_n, touch_1200, wait_for_bootloader_port, write_all, SerialHost, SerialLink,
};
use crate::{
    check_verify, hex_bytes, FirmwareImage, FlashError, Prog, Progress, Report, Result, Stage,
    Uploader,
};

/// Caterina stays in the bootloader for 8 s after the touch.
const PORT_WAIT: Duration = Duration::from_secs(8);
const CMD_TIMEOUT: Duration = Duration::from_millis(1000);

pub struct Avr109 {
    board: Board,
    host: Arc<dyn SerialHost>,
}

fn expect_cr(link: &mut dyn SerialLink, cmd: &[u8], what: &str) -> Result<()> {
    write_all(link, cmd)?;
    let r = read_n(link, 1, || lost(what))?;
    if r[0] != b'\r' {
        return Err(FlashError::Protocol(format!(
            "The bootloader rejected '{what}' (reply 0x{:02X}).",
            r[0]
        )));
    }
    Ok(())
}

fn lost(what: &str) -> String {
    format!("The bootloader stopped responding during '{what}'. Unplug and re-plug the board and try again.")
}

fn set_address(link: &mut dyn SerialLink, byte_addr: u32) -> Result<()> {
    // Flash addresses are in words.
    let w = byte_addr / 2;
    expect_cr(link, &[b'A', (w >> 8) as u8, w as u8], "set address")
}

impl Avr109 {
    pub fn new(board: Board, host: Arc<dyn SerialHost>) -> Self {
        Avr109 { board, host }
    }

    /// Touch the app port and return the bootloader port.
    fn enter_bootloader(&self, port: &str, prog: &Prog) -> Result<String> {
        let host = self.host.as_ref();
        let before = host.list()?;
        let boot_ids: Vec<(u16, u16)> = self.board.usb_ids(UsbRole::Bootloader).collect();
        // Already in the bootloader (e.g. user double-tapped reset)? Use it directly.
        let already = before.iter().any(|p| {
            p.path == port
                && p.usb
                    .as_ref()
                    .is_some_and(|u| boot_ids.contains(&(u.vid, u.pid)))
        });
        if already {
            return Ok(port.to_string());
        }
        prog.emit(
            Stage::Reset,
            0.2,
            format!("Resetting {port} with a 1200-baud touch"),
        );
        let touch_err = touch_1200(host, port);
        prog.emit(Stage::Reset, 0.6, "Waiting for the bootloader port");
        wait_for_bootloader_port(host, &before, port, &boot_ids, PORT_WAIT)
            .map_err(|e| touch_err.unwrap_or(e))
    }
}

impl Uploader for Avr109 {
    fn upload(
        &self,
        port: &str,
        image: &FirmwareImage,
        progress: &dyn Fn(Progress),
    ) -> Result<Report> {
        let prog = Prog::new(progress);
        let mcu = mcu_info(&self.board.mcu).ok_or_else(|| {
            FlashError::Unsupported(format!("Unknown MCU '{}' for AVR109", self.board.mcu))
        })?;
        let max = self.board.max_app_size();
        if image.end() > max {
            return Err(FlashError::Image(format!(
                "The firmware ends at {} bytes but the {} has room for {max} bytes.",
                image.end(),
                self.board.name
            )));
        }

        let boot_port = if self.board.touch1200 {
            self.enter_bootloader(port, &prog)?
        } else {
            port.to_string()
        };
        let baud = self.board.baud.first().copied().unwrap_or(57600);
        let mut link = open_retry(
            self.host.as_ref(),
            &boot_port,
            baud,
            CMD_TIMEOUT,
            Duration::from_secs(2),
        )?;
        let link = link.as_mut();
        let _ = link.clear_input();

        prog.emit(
            Stage::Sync,
            0.0,
            format!("Talking to the bootloader on {boot_port}"),
        );
        write_all(link, b"S")?;
        let id = read_n(link, 7, || {
            let linux = if cfg!(target_os = "linux") {
                " On Linux, ModemManager can grab the port right after it appears; stop it (sudo systemctl stop ModemManager) and retry."
            } else {
                ""
            };
            format!("No response from bootloader on {boot_port}. Unplug and re-plug the board; if it keeps failing, double-press the reset button and upload right away.{linux}")
        })?;
        let mut warnings = Vec::new();
        if !id.starts_with(b"CATERIN") {
            warnings.push(format!(
                "Bootloader identifies as '{}', not Caterina; continuing with standard AVR109.",
                String::from_utf8_lossy(&id)
            ));
        }
        write_all(link, b"V")?;
        let _ver = read_n(link, 2, || lost("read version"))?;
        write_all(link, b"p")?;
        let _ptype = read_n(link, 1, || lost("read programmer type"))?;
        write_all(link, b"a")?;
        let auto = read_n(link, 1, || lost("check auto-increment"))?;
        write_all(link, b"b")?;
        let b = read_n(link, 1, || lost("check block mode"))?;
        if b[0] != b'Y' || auto[0] != b'Y' {
            return Err(FlashError::Unsupported(
                "This bootloader does not support block mode with auto-increment; it is not a standard Leonardo/Micro bootloader.".into(),
            ));
        }
        let bs = read_n(link, 2, || lost("read block size"))?;
        let block = (u16::from_be_bytes([bs[0], bs[1]]) as usize).clamp(2, 1024) & !1;

        expect_cr(link, b"P", "enter programming mode")?;
        write_all(link, b"s")?;
        let s = read_n(link, 3, || lost("read signature"))?;
        // Caterina sends the signature high byte first.
        let sig = [s[2], s[1], s[0]];
        if sig != mcu.signature {
            let _ = expect_cr(link, b"L", "leave programming mode");
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

        // Write whole flash pages (Caterina erases the page at the block start);
        // blocks never exceed the reported buffer size.
        let page = mcu.page_size;
        let pages = image.pages(page);
        let total = pages.len() * page as usize;
        let chunk = block.min(page as usize);
        let mut done = 0usize;
        for (addr, data) in &pages {
            set_address(link, *addr)?;
            for part in data.chunks(chunk) {
                let mut cmd = vec![b'B', (part.len() >> 8) as u8, part.len() as u8, b'F'];
                cmd.extend_from_slice(part);
                expect_cr(link, &cmd, "write flash block")?;
            }
            done += data.len();
            prog.emit(
                Stage::Write,
                done as f32 / total as f32,
                format!("Writing {done} / {total} bytes"),
            );
        }

        done = 0;
        for (addr, data) in &pages {
            set_address(link, *addr)?;
            let mut back = Vec::with_capacity(data.len());
            for part in data.chunks(chunk) {
                write_all(
                    link,
                    &[b'g', (part.len() >> 8) as u8, part.len() as u8, b'F'],
                )?;
                back.extend(read_n(link, part.len(), || lost("read flash block"))?);
            }
            check_verify(*addr, data, &back)?;
            done += data.len();
            prog.emit(
                Stage::Verify,
                done as f32 / total as f32,
                format!("Verifying {done} / {total} bytes"),
            );
        }

        expect_cr(link, b"L", "leave programming mode")?;
        // 'E' makes Caterina start the sketch; the board re-enumerates on the app port.
        expect_cr(link, b"E", "exit bootloader")?;
        prog.emit(Stage::Done, 1.0, "Upload complete");
        Ok(Report {
            protocol: "avr109".into(),
            port: boot_port,
            baud: Some(baud),
            signature: Some(hex_bytes(&sig)),
            bytes_written: total,
            verified: true,
            duration_ms: prog.elapsed_ms(),
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::builtin_boards;
    use crate::mock::{test_image, Device, Flash, MockHost, Pipe, ProgressLog};
    use crate::ports::{PortInfo, UsbInfo};

    /// Caterina simulator. The app port is COM5 (2341:8036); after a 1200-baud
    /// touch the bootloader appears on COM6 (2341:0036).
    struct Caterina {
        pipe: Pipe,
        open_path: String,
        open_baud: u32,
        touched: bool,
        addr: usize,
        flash: Flash,
    }

    impl Caterina {
        fn new() -> Self {
            Caterina {
                pipe: Pipe::default(),
                open_path: String::new(),
                open_baud: 0,
                touched: false,
                addr: 0,
                flash: Flash::new(32 * 1024),
            }
        }

        fn step(&mut self) -> bool {
            let inp = &self.pipe.input;
            if inp.is_empty() {
                return false;
            }
            let need = match inp[0] {
                b'A' => 3,
                b'B' | b'g' if inp.len() >= 3 => {
                    let n = (inp[1] as usize) << 8 | inp[2] as usize;
                    if inp[0] == b'B' {
                        4 + n
                    } else {
                        4
                    }
                }
                b'B' | b'g' => return false,
                _ => 1,
            };
            if inp.len() < need {
                return false;
            }
            let cmd: Vec<u8> = self.pipe.input.drain(..need).collect();
            match cmd[0] {
                b'S' => self.pipe.reply(b"CATERIN"),
                b'V' => self.pipe.reply(b"10"),
                b'p' => self.pipe.reply(b"S"),
                b'a' => self.pipe.reply(b"Y"),
                b'b' => self.pipe.reply(&[b'Y', 0x00, 0x80]),
                b's' => self.pipe.reply(&[0x87, 0x95, 0x1E]),
                b'P' | b'L' | b'E' => self.pipe.reply(b"\r"),
                b'A' => {
                    self.addr = ((cmd[1] as usize) << 8 | cmd[2] as usize) * 2;
                    self.pipe.reply(b"\r");
                }
                b'B' => {
                    assert_eq!(cmd[3], b'F');
                    let data = cmd[4..].to_vec();
                    assert!(data.len() <= 128);
                    self.flash.write(self.addr, &data);
                    self.addr += data.len();
                    self.pipe.reply(b"\r");
                }
                b'g' => {
                    let n = (cmd[1] as usize) << 8 | cmd[2] as usize;
                    let d = self.flash.read(self.addr, n).to_vec();
                    self.addr += n;
                    self.pipe.reply(&d);
                }
                _ => self.pipe.reply(b"?"),
            }
            true
        }
    }

    impl Device for Caterina {
        fn open(&mut self, path: &str, baud: u32) -> std::result::Result<(), String> {
            if path == "COM6" && !self.touched {
                return Err("COM6 does not exist".into());
            }
            self.open_path = path.into();
            self.open_baud = baud;
            Ok(())
        }
        fn set_dtr(&mut self, level: bool) {
            if !level && self.open_baud == 1200 && self.open_path == "COM5" {
                self.touched = true;
            }
        }
        fn write(&mut self, data: &[u8]) {
            if self.open_path != "COM6" {
                return; // the sketch ignores us
            }
            self.pipe.input.extend_from_slice(data);
            while self.step() {}
        }
        fn read(&mut self, buf: &mut [u8]) -> usize {
            self.pipe.read(buf)
        }
        fn flash_snapshot(&self) -> Option<Vec<u8>> {
            Some(self.flash.mem.clone())
        }
    }

    fn port(path: &str, pid: u16) -> PortInfo {
        PortInfo {
            path: path.into(),
            usb: Some(UsbInfo {
                vid: 0x2341,
                pid,
                ..Default::default()
            }),
        }
    }

    #[test]
    fn touch_then_upload_on_new_port() {
        let host = Arc::new(MockHost::new(Box::new(Caterina::new())));
        host.push_listing(vec![port("COM5", 0x8036)]); // before touch
        host.push_listing(vec![]); // app port gone
        host.set_default_listing(vec![port("COM6", 0x0036)]);
        let board = builtin_boards()
            .into_iter()
            .find(|b| b.id == "leonardo")
            .unwrap();
        let up = Avr109::new(board, host.clone());
        let img = test_image(0, 5000);
        let log = ProgressLog::default();
        let r = up.upload("COM5", &img, &log.cb()).unwrap();
        log.assert_sane();
        assert_eq!(r.port, "COM6");
        assert_eq!(r.signature.as_deref(), Some("1E9587"));
        let flash = host.flash();
        assert_eq!(&flash[..5000], &img.flatten(0, 5000, 0xFF)[..]);
        let opens = host.opens.lock().unwrap();
        assert_eq!(opens[0], ("COM5".to_string(), 1200));
        assert_eq!(opens[1].0, "COM6");
    }

    #[test]
    fn missing_bootloader_port_times_out_helpfully() {
        // The touch is ignored (wrong board): the app port vanishes and nothing appears.
        let host = MockHost::new(Box::new(Caterina::new()));
        let before = vec![port("COM5", 0x8037)];
        let e = wait_for_bootloader_port(
            &host,
            &before,
            "COM5",
            &[(0x2341, 0x0037)],
            Duration::from_millis(30),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("did not appear"), "{e}");
    }
}
