//! STK500v1 subset spoken by Optiboot (Uno, new Nano) and ATmegaBOOT (old Nano,
//! Pro Mini). Mirrors avrdude's `arduino` programmer.

use std::sync::Arc;
use std::time::Duration;

use crate::boards::{mcu_info, signature_name, Board};
use crate::link::{read_n, write_all, SerialHost, SerialLink};
use crate::{
    check_verify, hex_bytes, FirmwareImage, FlashError, Prog, Progress, Report, Result, Stage,
    Uploader,
};

const STK_OK: u8 = 0x10;
const STK_INSYNC: u8 = 0x14;
const STK_NOSYNC: u8 = 0x15;
const CRC_EOP: u8 = 0x20;
const GET_SYNC: u8 = 0x30;
const ENTER_PROGMODE: u8 = 0x50;
const LEAVE_PROGMODE: u8 = 0x51;
const LOAD_ADDRESS: u8 = 0x55;
const PROG_PAGE: u8 = 0x64;
const READ_PAGE: u8 = 0x74;
const READ_SIGN: u8 = 0x75;

/// Optiboot gives up ~1 s after reset; sync attempts must fit in that window.
const SYNC_ATTEMPTS: usize = 6;
const SYNC_TIMEOUT: Duration = Duration::from_millis(150);
const CMD_TIMEOUT: Duration = Duration::from_millis(1000);

pub struct Stk500v1 {
    board: Board,
    host: Arc<dyn SerialHost>,
}

impl Stk500v1 {
    pub fn new(board: Board, host: Arc<dyn SerialHost>) -> Self {
        Stk500v1 { board, host }
    }

    fn no_response(&self, port: &str, bauds: &[u32]) -> FlashError {
        let tried = bauds
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(" and ");
        let hint = match self.board.id.as_str() {
            "uno" => "If this is an older Nano, choose 'Nano (old bootloader)'. If it is a CH340/FTDI clone, choose 'Nano (new bootloader)'.",
            "nano" => "Check that the USB cable carries data (some are charge-only) and that this really is a Nano. Boards with a 168 chip are not supported.",
            "nano_old" => "If this Nano has the new bootloader, choose 'Nano (new bootloader)'.",
            "pro_mini" => "The Pro Mini has no USB: make sure the adapter's DTR pin is wired to the board's DTR/GRN pin, or press the board's reset button just as the upload starts.",
            _ => "Check the board selection and the USB cable.",
        };
        FlashError::NoResponse(format!(
            "No response from bootloader on {port} (tried {tried} baud). {hint} Also close any program using the port, such as the Arduino Serial Monitor."
        ))
    }

    fn session(&self, port: &str, baud: u32, image: &FirmwareImage, prog: &Prog) -> Result<Report> {
        let mcu = mcu_info(&self.board.mcu).ok_or_else(|| {
            FlashError::Unsupported(format!("Unknown MCU '{}' for STK500v1", self.board.mcu))
        })?;
        let mut link = self.host.open(port, baud, SYNC_TIMEOUT)?;
        let link = link.as_mut();

        prog.emit(Stage::Reset, 0.5, format!("Resetting the board on {port}"));
        reset(self.host.as_ref(), link)?;

        prog.emit(
            Stage::Sync,
            0.0,
            format!("Waiting for the bootloader at {baud} baud"),
        );
        sync(self.host.as_ref(), link)?;
        link.set_timeout(CMD_TIMEOUT)
            .map_err(|e| crate::link::io_err("Serial setup failed", e))?;

        command(
            link,
            &[ENTER_PROGMODE, CRC_EOP],
            0,
            "enter programming mode",
        )?;
        let sig = command(link, &[READ_SIGN, CRC_EOP], 3, "read signature")?;
        let sig = [sig[0], sig[1], sig[2]];
        if sig != mcu.signature {
            let _ = command(
                link,
                &[LEAVE_PROGMODE, CRC_EOP],
                0,
                "leave programming mode",
            );
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

        for (i, (addr, data)) in pages.iter().enumerate() {
            load_address(link, *addr)?;
            let mut cmd = vec![PROG_PAGE, (data.len() >> 8) as u8, data.len() as u8, b'F'];
            cmd.extend_from_slice(data);
            cmd.push(CRC_EOP);
            command(link, &cmd, 0, "write flash page")?;
            prog.emit(
                Stage::Write,
                (i + 1) as f32 / pages.len() as f32,
                format!("Writing {} / {} bytes", (i + 1) * page as usize, total),
            );
        }

        for (i, (addr, data)) in pages.iter().enumerate() {
            load_address(link, *addr)?;
            let back = command(
                link,
                &[
                    READ_PAGE,
                    (data.len() >> 8) as u8,
                    data.len() as u8,
                    b'F',
                    CRC_EOP,
                ],
                data.len(),
                "read flash page",
            )?;
            check_verify(*addr, data, &back)?;
            prog.emit(
                Stage::Verify,
                (i + 1) as f32 / pages.len() as f32,
                format!("Verifying {} / {} bytes", (i + 1) * page as usize, total),
            );
        }

        command(
            link,
            &[LEAVE_PROGMODE, CRC_EOP],
            0,
            "leave programming mode",
        )?;
        Ok(Report {
            protocol: "stk500v1".into(),
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

impl Uploader for Stk500v1 {
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
        let bauds = if self.board.baud.is_empty() {
            vec![115200]
        } else {
            self.board.baud.clone()
        };
        for baud in &bauds {
            match self.session(port, *baud, image, &prog) {
                Ok(report) => {
                    prog.emit(Stage::Done, 1.0, "Upload complete");
                    return Ok(report);
                }
                Err(FlashError::NoResponse(_)) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(self.no_response(port, &bauds))
    }
}

/// Pulse DTR/RTS like avrdude: deassert, wait for the reset capacitor to discharge,
/// assert (the edge resets the board), then give the bootloader a moment.
pub(crate) fn reset(host: &dyn SerialHost, link: &mut dyn SerialLink) -> Result<()> {
    let e = |e| crate::link::io_err("Could not toggle DTR/RTS", e);
    link.set_dtr(false).map_err(e)?;
    link.set_rts(false).map_err(e)?;
    host.sleep(Duration::from_millis(250));
    link.set_dtr(true).map_err(e)?;
    link.set_rts(true).map_err(e)?;
    host.sleep(Duration::from_millis(50));
    link.clear_input().map_err(e)?;
    Ok(())
}

fn sync(host: &dyn SerialHost, link: &mut dyn SerialLink) -> Result<()> {
    for _ in 0..SYNC_ATTEMPTS {
        let _ = link.clear_input();
        write_all(link, &[GET_SYNC, CRC_EOP])?;
        match read_n(link, 2, String::new) {
            Ok(r) if r == [STK_INSYNC, STK_OK] => {
                // Optiboot may have queued replies to earlier attempts; drop them.
                host.sleep(Duration::from_millis(10));
                let _ = link.clear_input();
                return Ok(());
            }
            Ok(_) | Err(FlashError::NoResponse(_)) => host.sleep(Duration::from_millis(20)),
            Err(e) => return Err(e),
        }
    }
    Err(FlashError::NoResponse("no sync".into()))
}

fn load_address(link: &mut dyn SerialLink, byte_addr: u32) -> Result<()> {
    // STK500v1 addresses flash in 16-bit words, little-endian.
    let word = byte_addr / 2;
    command(
        link,
        &[LOAD_ADDRESS, word as u8, (word >> 8) as u8, CRC_EOP],
        0,
        "set address",
    )
    .map(|_| ())
}

/// Send a command and read `INSYNC <payload> OK`.
fn command(link: &mut dyn SerialLink, cmd: &[u8], payload: usize, what: &str) -> Result<Vec<u8>> {
    write_all(link, cmd)?;
    let lost = || {
        format!("The bootloader stopped responding during '{what}'. Unplug and re-plug the board and try again.")
    };
    let first = read_n(link, 1, lost)?;
    match first[0] {
        STK_INSYNC => {}
        STK_NOSYNC => {
            return Err(FlashError::Protocol(format!(
                "The bootloader lost sync during '{what}'. Try the upload again."
            )))
        }
        other => {
            return Err(FlashError::Protocol(format!(
                "Unexpected reply 0x{other:02X} from the bootloader during '{what}'."
            )))
        }
    }
    let mut rest = read_n(link, payload + 1, lost)?;
    if rest.pop() != Some(STK_OK) {
        return Err(FlashError::Protocol(format!(
            "The bootloader did not confirm '{what}'."
        )));
    }
    Ok(rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::builtin_boards;
    use crate::mock::{test_image, Device, Flash, MockHost, Pipe, ProgressLog};

    /// Optiboot / ATmegaBOOT simulator.
    pub struct Optiboot {
        baud: u32,
        open_baud: u32,
        signature: [u8; 3],
        pipe: Pipe,
        dtr: bool,
        active: bool,
        addr: usize,
        pub flash: Flash,
        /// Simulate a flash cell that will not program.
        pub stuck_bit: Option<usize>,
    }

    impl Optiboot {
        pub fn new(baud: u32, signature: [u8; 3]) -> Self {
            Optiboot {
                baud,
                open_baud: 0,
                signature,
                pipe: Pipe::default(),
                dtr: false,
                active: false,
                addr: 0,
                flash: Flash::new(32 * 1024),
                stuck_bit: None,
            }
        }

        fn step(&mut self) -> bool {
            let inp = &self.pipe.input;
            if inp.is_empty() {
                return false;
            }
            let need = match inp[0] {
                GET_SYNC | ENTER_PROGMODE | LEAVE_PROGMODE | READ_SIGN => 2,
                LOAD_ADDRESS => 4,
                READ_PAGE => 5,
                PROG_PAGE if inp.len() >= 3 => 5 + ((inp[1] as usize) << 8 | inp[2] as usize),
                PROG_PAGE => return false,
                _ => {
                    self.pipe.input.remove(0);
                    return true;
                }
            };
            if inp.len() < need {
                return false;
            }
            let cmd: Vec<u8> = self.pipe.input.drain(..need).collect();
            assert_eq!(*cmd.last().unwrap(), CRC_EOP, "missing CRC_EOP");
            self.pipe.reply(&[STK_INSYNC]);
            match cmd[0] {
                READ_SIGN => {
                    let s = self.signature;
                    self.pipe.reply(&s);
                }
                LOAD_ADDRESS => self.addr = (cmd[1] as usize | (cmd[2] as usize) << 8) * 2,
                PROG_PAGE => {
                    assert_eq!(cmd[3], b'F');
                    let data = &cmd[4..cmd.len() - 1];
                    assert_eq!(data.len(), 128);
                    assert_eq!(self.addr % 128, 0, "page not aligned");
                    self.flash.write(self.addr, data);
                    if let Some(a) = self.stuck_bit {
                        if (self.addr..self.addr + 128).contains(&a) {
                            self.flash.mem[a] ^= 0x01;
                        }
                    }
                }
                READ_PAGE => {
                    let n = (cmd[1] as usize) << 8 | cmd[2] as usize;
                    let d = self.flash.read(self.addr, n).to_vec();
                    self.pipe.reply(&d);
                }
                // Optiboot jumps to the sketch after LEAVE_PROGMODE.
                LEAVE_PROGMODE => self.active = false,
                _ => {}
            }
            self.pipe.reply(&[STK_OK]);
            true
        }
    }

    impl Device for Optiboot {
        fn open(&mut self, _path: &str, baud: u32) -> std::result::Result<(), String> {
            self.open_baud = baud;
            self.active = false;
            Ok(())
        }
        fn set_dtr(&mut self, level: bool) {
            if level && !self.dtr {
                self.active = true; // reset edge: bootloader starts
                self.pipe = Pipe::default();
            }
            self.dtr = level;
        }
        fn write(&mut self, data: &[u8]) {
            if !self.active || self.open_baud != self.baud {
                return; // app running or wrong baud: silence
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

    fn board(id: &str) -> Board {
        builtin_boards().into_iter().find(|b| b.id == id).unwrap()
    }

    fn run(board_id: &str, dev: Optiboot) -> (Arc<MockHost>, Result<Report>) {
        let host = Arc::new(MockHost::new(Box::new(dev)));
        let up = Stk500v1::new(board(board_id), host.clone());
        let log = ProgressLog::default();
        let img = test_image(0, 3000);
        let r = up.upload("/dev/ttyUSB0", &img, &log.cb());
        if r.is_ok() {
            log.assert_sane();
        }
        (host, r)
    }

    #[test]
    fn uploads_and_verifies_uno() {
        let (host, r) = run("uno", Optiboot::new(115200, [0x1E, 0x95, 0x0F]));
        let r = r.unwrap();
        assert!(r.verified);
        assert_eq!(r.baud, Some(115200));
        assert_eq!(r.signature.as_deref(), Some("1E950F"));
        assert_eq!(r.bytes_written, 3072);
        let img = test_image(0, 3000);
        let flash = host.flash();
        assert_eq!(&flash[..3000], &img.flatten(0, 3000, 0xFF)[..]);
        assert!(flash[3000..].iter().all(|b| *b == 0xFF));
    }

    #[test]
    fn nano_falls_back_to_57600() {
        let (host, r) = run("nano", Optiboot::new(57600, [0x1E, 0x95, 0x0F]));
        assert_eq!(r.unwrap().baud, Some(57600));
        let opens = host.opens.lock().unwrap();
        assert_eq!(
            opens.iter().map(|o| o.1).collect::<Vec<_>>(),
            vec![115200, 57600]
        );
    }

    #[test]
    fn no_sync_explains_old_bootloader() {
        let (_, r) = run("uno", Optiboot::new(57600, [0x1E, 0x95, 0x0F]));
        let e = r.unwrap_err();
        assert!(matches!(e, FlashError::NoResponse(_)));
        let msg = e.to_string();
        assert!(
            msg.contains("No response from bootloader on /dev/ttyUSB0"),
            "{msg}"
        );
        assert!(msg.contains("Nano (old bootloader)"), "{msg}");
    }

    #[test]
    fn wrong_signature_is_reported() {
        let (_, r) = run("nano_old", Optiboot::new(57600, [0x1E, 0x94, 0x06]));
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("ATmega168"), "{msg}");
    }

    #[test]
    fn oversized_image_is_rejected() {
        let host = Arc::new(MockHost::new(Box::new(Optiboot::new(
            115200,
            [0x1E, 0x95, 0x0F],
        ))));
        let up = Stk500v1::new(board("uno"), host);
        let img = test_image(0, 32 * 1024);
        let e = up.upload("p", &img, &|_| {}).unwrap_err().to_string();
        assert!(e.contains("room for 32256"), "{e}");
    }

    #[test]
    fn verify_catches_bad_flash() {
        let mut dev = Optiboot::new(115200, [0x1E, 0x95, 0x0F]);
        dev.stuck_bit = Some(0x123);
        let (_, r) = run("uno", dev);
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("Verify failed at 0x00123"), "{msg}");
    }
}
