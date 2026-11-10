//! ESP32 family upload through the ROM serial bootloader, via the `espflash` crate
//! (library use; no CLI, no Python). Contract: docs/ARCHITECTURE.md,
//! "Desktop flashing (`deskbunny-flash`, protocol `esptool`)".
//!
//! Flow: reset into the ROM bootloader (classic DTR/RTS, or the USB-Serial/JTAG
//! sequence on 303A:1001; espflash picks it from the USB PID), sync at 115200,
//! detect the chip, load the flasher stub, raise the baud, write the merged image
//! at 0x0 compressed, compare the flash MD5 with the image, hard-reset into the app.
//!
//! The bootloader session sits behind [`EspBackend`] / [`EspSession`] so the
//! orchestration (baud fallback, chip-mismatch image switch, progress) is tested
//! without hardware; [`EspflashBackend`] is the real implementation.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::boards::{Board, Protocol};
use crate::link::{query_on_link, SerialHost};
use crate::{FirmwareImage, FlashError, Prog, Progress, Report, Result, Stage, Uploader};

/// Board ids of the ESP32 family, in table order.
pub const ESP_BOARDS: [&str; 3] = ["esp32", "esp32c3", "esp32s3"];

/// USB-Serial/JTAG peripheral of the ESP32-C3/S3 (re-enumerates on reset).
pub const USB_SERIAL_JTAG: (u16, u16) = (0x303A, 0x1001);

/// How long to keep reopening a re-enumerating port after the reset.
const REOPEN_WINDOW: Duration = Duration::from_secs(5);
/// How long to ask for the banner once the port is open.
const BANNER_WAIT: Duration = Duration::from_millis(4000);

pub fn is_esp_board(board: &Board) -> bool {
    board.protocol == Protocol::Esptool
}

/// A chip the ROM bootloader reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EspChip {
    Esp32,
    Esp32c3,
    Esp32s3,
    /// Any other Espressif chip, by espflash's lowercase name (e.g. "esp32s2").
    Other(String),
}

impl EspChip {
    /// From espflash's lowercase chip name ("esp32c3").
    pub fn from_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "esp32" => EspChip::Esp32,
            "esp32c3" => EspChip::Esp32c3,
            "esp32s3" => EspChip::Esp32s3,
            other => EspChip::Other(other.to_string()),
        }
    }

    /// The nixxa board that runs on this chip.
    pub fn board_id(&self) -> Option<&'static str> {
        match self {
            EspChip::Esp32 => Some("esp32"),
            EspChip::Esp32c3 => Some("esp32c3"),
            EspChip::Esp32s3 => Some("esp32s3"),
            EspChip::Other(_) => None,
        }
    }

    /// Marketing name: "ESP32", "ESP32-C3", "ESP32-S2", ...
    pub fn display(&self) -> String {
        let raw = match self {
            EspChip::Esp32 => "esp32",
            EspChip::Esp32c3 => "esp32c3",
            EspChip::Esp32s3 => "esp32s3",
            EspChip::Other(s) => s.as_str(),
        };
        let up = raw.to_ascii_uppercase();
        match up.strip_prefix("ESP32") {
            Some(rest) if !rest.is_empty() => format!("ESP32-{rest}"),
            _ => up,
        }
    }
}

/// Display name of a board id's chip ("esp32s3" -> "ESP32-S3").
fn board_chip_name(board_id: &str) -> String {
    EspChip::from_name(board_id).display()
}

/// Progress of one compressed write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteEvent {
    /// Erase done (the stub erases as part of the deflate begin), `chunks` to send.
    Begin { chunks: usize },
    /// `n` chunks written so far.
    Chunk(usize),
    /// All data sent; the flash MD5 is being compared with the image.
    Verifying,
}

/// One connected bootloader session.
pub trait EspSession {
    fn chip(&self) -> EspChip;
    /// MAC address as `AA:BB:CC:DD:EE:FF`, if readable.
    fn mac(&mut self) -> Option<String>;
    /// Write `data` at `addr` (compressed), check the flash MD5 against it, then
    /// hard-reset the board into its app.
    fn write_verify_reset(
        &mut self,
        addr: u32,
        data: &[u8],
        events: &mut dyn FnMut(WriteEvent),
    ) -> Result<()>;
    /// Hard-reset back into the app without writing (used on errors).
    fn hard_reset(&mut self);
}

/// Reaches the ROM bootloader on a port.
pub trait EspBackend: Send + Sync {
    /// Reset into the ROM bootloader, sync at 115200, detect the chip, load the stub,
    /// then switch to `baud`. A port that never answers is `FlashError::NoResponse`;
    /// a failed baud switch is any other error (the caller retries slower).
    fn connect(&self, port: &str, baud: u32) -> Result<Box<dyn EspSession>>;
    /// Enter the bootloader, read the chip, hard-reset back into the app.
    /// `Ok(None)` when nothing answers.
    fn detect(&self, port: &str) -> Result<Option<EspChip>>;
}

/// Loads the image of another ESP32 board (chip auto-switch).
pub type ImageResolver = Box<dyn Fn(&str) -> Result<FirmwareImage> + Send + Sync>;

pub struct Esptool {
    board: Board,
    host: Arc<dyn SerialHost>,
    backend: Arc<dyn EspBackend>,
    resolver: Option<ImageResolver>,
}

impl Esptool {
    /// Real bootloader access through espflash. Without a resolver a chip mismatch
    /// is an error.
    pub fn new(board: Board, host: Arc<dyn SerialHost>) -> Self {
        Esptool {
            board,
            host,
            backend: Arc::new(EspflashBackend),
            resolver: None,
        }
    }

    /// The `DESKBUNNY ...` line after an upload (see [`read_banner`]).
    pub fn banner(&self, port: &str) -> Result<Option<String>> {
        read_banner(self.host.as_ref(), port)
    }

    pub fn with_backend(mut self, backend: Arc<dyn EspBackend>) -> Self {
        self.backend = backend;
        self
    }

    /// Lets the uploader flash the matching image when the chip found differs.
    pub fn with_resolver(mut self, resolver: ImageResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    fn run(&self, port: &str, image: &FirmwareImage, prog: &Prog) -> Result<Report> {
        let mut warnings = Vec::new();
        prog.emit(
            Stage::Reset,
            0.0,
            format!("Resetting {port} into the ESP32 bootloader"),
        );
        prog.emit(Stage::Sync, 0.0, "Waiting for the ESP32 bootloader");
        let (mut session, baud) = self.connect(port, &mut warnings)?;
        let chip = session.chip();
        prog.emit(Stage::Sync, 1.0, format!("Found {}", chip.display()));

        let target = match select_board(&self.board.id, &chip, port) {
            Ok(t) => t,
            Err(e) => {
                session.hard_reset();
                return Err(e);
            }
        };
        let switched = if target.board_id == self.board.id {
            None
        } else {
            let Some(resolve) = &self.resolver else {
                session.hard_reset();
                return Err(FlashError::WrongChip(format!(
                    "Selected {} but found {} on {port}. Pick '{}' in the board list and try again.",
                    board_chip_name(&self.board.id),
                    chip.display(),
                    chip.display()
                )));
            };
            match resolve(target.board_id) {
                Ok(img) => Some(img),
                Err(e) => {
                    session.hard_reset();
                    return Err(e);
                }
            }
        };
        let image = switched.as_ref().unwrap_or(image);
        if let Some(w) = target.warning {
            log::warn!("{w}");
            warnings.push(w);
        }

        if image.is_empty() {
            session.hard_reset();
            return Err(FlashError::Image(
                "The ESP32 firmware image is empty.".into(),
            ));
        }
        let addr = image.base();
        let data = image.flatten(addr, image.end(), 0xFF);
        let max = self.board.max_app_size() as usize;
        if addr as usize + data.len() > max {
            session.hard_reset();
            return Err(FlashError::Image(format!(
                "The ESP32 firmware image is {} bytes, more than the {max} bytes the flash layout allows.",
                data.len()
            )));
        }

        let mac = session.mac();
        prog.emit(Stage::Erase, 0.0, "Erasing flash");
        let mut chunks = 1usize;
        let mut on_event = |ev: WriteEvent| match ev {
            WriteEvent::Begin { chunks: n } => {
                chunks = n.max(1);
                prog.emit(Stage::Erase, 1.0, "Erased");
                prog.emit(
                    Stage::Write,
                    0.0,
                    format!("Writing {} bytes (compressed)", data.len()),
                );
            }
            WriteEvent::Chunk(i) => prog.emit(
                Stage::Write,
                i as f32 / chunks as f32,
                format!("Writing block {i} / {chunks}"),
            ),
            WriteEvent::Verifying => prog.emit(Stage::Verify, 0.0, "Checking flash MD5"),
        };
        session.write_verify_reset(addr, &data, &mut on_event)?;
        prog.emit(Stage::Verify, 1.0, "Verified; restarting the board");
        prog.emit(Stage::Done, 1.0, "Upload complete");

        let signature = Some(match mac {
            Some(m) => format!("{} {m}", chip.display()),
            None => chip.display(),
        });
        Ok(Report {
            protocol: "esptool".into(),
            flashed_board: Some(target.board_id.to_string()),
            port: port.to_string(),
            baud: Some(baud),
            signature,
            bytes_written: data.len(),
            verified: true,
            duration_ms: prog.elapsed_ms(),
            warnings,
        })
    }

    /// Connect at the fastest rate that works. A silent bootloader stops at once:
    /// the ROM sync always runs at 115200, so other rates cannot help.
    fn connect(
        &self,
        port: &str,
        warnings: &mut Vec<String>,
    ) -> Result<(Box<dyn EspSession>, u32)> {
        let bauds: Vec<u32> = if self.board.baud.is_empty() {
            vec![115200]
        } else {
            self.board.baud.clone()
        };
        let mut last = None;
        for (i, &baud) in bauds.iter().enumerate() {
            match self.backend.connect(port, baud) {
                Ok(s) => {
                    if i > 0 {
                        warnings.push(format!(
                            "The board did not accept {} baud; flashed at {baud} instead.",
                            bauds[0]
                        ));
                    }
                    return Ok((s, baud));
                }
                Err(e @ (FlashError::NoResponse(_) | FlashError::Port(_))) => return Err(e),
                Err(e) => {
                    log::warn!("esptool: connect at {baud} baud failed: {e}");
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or_else(|| no_response(port)))
    }
}

impl Uploader for Esptool {
    fn upload(
        &self,
        port: &str,
        image: &FirmwareImage,
        progress: &dyn Fn(Progress),
    ) -> Result<Report> {
        let prog = Prog::with_ranges(progress, esp_ranges);
        self.run(port, image, &prog)
    }
}

/// Stage weights for ESP32 uploads: stub load is part of sync, the MD5 check is
/// quick, so the write gets most of the bar.
fn esp_ranges(stage: Stage) -> (u8, u8) {
    match stage {
        Stage::Reset => (0, 3),
        Stage::Sync => (3, 10),
        Stage::Erase => (10, 14),
        Stage::Write => (14, 94),
        Stage::Verify => (94, 99),
        Stage::Done => (100, 100),
    }
}

/// Which board's image to flash once the chip is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub board_id: &'static str,
    pub warning: Option<String>,
}

/// The selected board when the chip matches; another ESP32 board (with a warning)
/// when the chip is one of the three; an error otherwise.
pub fn select_board(selected: &str, found: &EspChip, port: &str) -> Result<Target> {
    let Some(found_id) = found.board_id() else {
        return Err(FlashError::WrongChip(format!(
            "Found an {} on {port}. nixxa runs on ESP32, ESP32-C3 and ESP32-S3 boards only.",
            found.display()
        )));
    };
    if found_id == selected {
        return Ok(Target {
            board_id: found_id,
            warning: None,
        });
    }
    let short = |id: &str| {
        let d = board_chip_name(id);
        d.strip_prefix("ESP32-").map(str::to_string).unwrap_or(d)
    };
    Ok(Target {
        board_id: found_id,
        warning: Some(format!(
            "Selected {} but found {}; flashed the {} image.",
            board_chip_name(selected),
            found.display(),
            short(found_id)
        )),
    })
}

/// The `DESKBUNNY ...` line after the hard reset. The USB-Serial/JTAG port
/// re-enumerates, so the same path is reopened for up to 5 s.
pub fn read_banner(host: &dyn SerialHost, port: &str) -> Result<Option<String>> {
    let start = Instant::now();
    // Let the reset finish and the boot ROM release the port.
    host.sleep(Duration::from_millis(300));
    let mut link = loop {
        match host.open(port, 115200, Duration::from_millis(100)) {
            Ok(l) => break l,
            Err(e) if start.elapsed() >= REOPEN_WINDOW => return Err(e),
            Err(_) => host.sleep(Duration::from_millis(200)),
        }
    };
    // Opening asserts DTR and RTS on most systems. Release RTS first (EN high),
    // then DTR, so the auto-reset circuit never pulls EN low or holds IO0.
    let _ = link.set_rts(false);
    let _ = link.set_dtr(false);
    query_on_link(host, link.as_mut(), BANNER_WAIT)
}

/// Enter the ROM bootloader on `port`, report the chip as a board id
/// ("esp32" | "esp32c3" | "esp32s3"), then hard-reset back into the app.
pub fn detect_board(backend: &dyn EspBackend, port: &str) -> Result<Option<&'static str>> {
    Ok(backend.detect(port)?.and_then(|c| c.board_id()))
}

/// [`detect_board`] with the real espflash backend.
pub fn detect_chip(port: &str) -> Result<Option<&'static str>> {
    detect_board(&EspflashBackend, port)
}

fn no_response(port: &str) -> FlashError {
    FlashError::NoResponse(format!(
        "No response from the ESP32 bootloader on {port}. Hold BOOT, tap EN/RST, then try again."
    ))
}

// ---------------------------------------------------------------------------
// Real backend: espflash.

/// The ROM bootloader through `espflash`.
#[derive(Debug, Default, Clone, Copy)]
pub struct EspflashBackend;

mod real {
    use super::*;
    use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
    use espflash::flasher::Flasher;
    use espflash::target::ProgressCallbacks;
    use serialport::{FlowControl, SerialPortType, UsbPortInfo};

    /// The USB identity espflash needs to pick its reset sequence.
    fn usb_info(port: &str) -> Result<UsbPortInfo> {
        let ports = serialport::available_ports()
            .map_err(|e| FlashError::Port(format!("Could not list serial ports: {e}")))?;
        ports
            .into_iter()
            .find_map(|p| match p.port_type {
                SerialPortType::UsbPort(u) if p.port_name == port => Some(u),
                _ => None,
            })
            .ok_or_else(|| {
                FlashError::Port(format!(
                    "{port} is not connected as a USB serial port. Re-plug the board and refresh the port list."
                ))
            })
    }

    fn open(port: &str) -> Result<Connection> {
        let info = usb_info(port)?;
        let serial = serialport::new(port, 115_200)
            .flow_control(FlowControl::None)
            .open_native()
            .map_err(|e| crate::link::open_error(port, e))?;
        Ok(Connection::new(
            serial,
            info,
            ResetAfterOperation::HardReset,
            ResetBeforeOperation::DefaultReset,
            115_200,
        ))
    }

    pub(super) struct Session {
        flasher: Option<Flasher>,
        chip: EspChip,
        port: String,
    }

    impl Session {
        fn flasher(&mut self) -> &mut Flasher {
            self.flasher.as_mut().expect("session already closed")
        }
    }

    struct Events<'a> {
        cb: &'a mut dyn FnMut(WriteEvent),
    }

    impl ProgressCallbacks for Events<'_> {
        fn init(&mut self, _addr: u32, total: usize) {
            (self.cb)(WriteEvent::Begin { chunks: total });
        }
        fn update(&mut self, current: usize) {
            (self.cb)(WriteEvent::Chunk(current));
        }
        fn verifying(&mut self) {
            (self.cb)(WriteEvent::Verifying);
        }
        fn finish(&mut self, _skipped: bool) {}
    }

    impl EspSession for Session {
        fn chip(&self) -> EspChip {
            self.chip.clone()
        }

        fn mac(&mut self) -> Option<String> {
            let f = self.flasher();
            let chip = f.chip();
            chip.mac_address(f.connection())
                .map(|m| m.to_ascii_uppercase())
                .map_err(|e| log::warn!("esptool: could not read MAC: {e}"))
                .ok()
        }

        fn write_verify_reset(
            &mut self,
            addr: u32,
            data: &[u8],
            events: &mut dyn FnMut(WriteEvent),
        ) -> Result<()> {
            let port = self.port.clone();
            let mut cb = Events { cb: events };
            // verify = true: espflash compares the flash MD5 with the image before
            // FLASH_END; HardReset after the write is the connection's after-op.
            self.flasher()
                .write_bin_to_flash(addr, data, &mut cb)
                .map_err(|e| {
                    log::error!("esptool: write failed: {e:?}");
                    match e {
                        espflash::Error::VerifyFailed => FlashError::Verify(
                            "Verify failed: the flash MD5 does not match the firmware. Try again with a shorter or better USB cable.".into(),
                        ),
                        other => FlashError::Protocol(format!(
                            "Writing to the ESP32 on {port} failed ({other}). Re-plug the board and try again; if it keeps failing, hold BOOT, tap EN/RST, then try again."
                        )),
                    }
                })?;
            // The board is running the new app; drop the port so it can re-enumerate.
            self.flasher = None;
            Ok(())
        }

        fn hard_reset(&mut self) {
            if let Some(mut f) = self.flasher.take() {
                if let Err(e) = f.connection().reset() {
                    log::warn!("esptool: hard reset failed: {e}");
                }
            }
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // Never leave a board parked in the bootloader after an error.
            self.hard_reset();
        }
    }

    impl EspBackend for EspflashBackend {
        fn connect(&self, port: &str, baud: u32) -> Result<Box<dyn EspSession>> {
            let conn = open(port)?;
            log::info!("esptool: connecting on {port}");
            let mut flasher = Flasher::connect(conn, true, true, false, None, None).map_err(|e| {
                log::warn!("esptool: connect on {port} failed: {e:?}");
                match e {
                    espflash::Error::Connection(_) | espflash::Error::RomError(_) => {
                        no_response(port)
                    }
                    other => FlashError::Protocol(format!(
                        "The ESP32 bootloader on {port} answered but setup failed ({other}). Re-plug the board and try again."
                    )),
                }
            })?;
            let chip = EspChip::from_name(&flasher.chip().to_string());
            log::info!("esptool: found {} on {port}", chip.display());
            if baud > 115_200 {
                if let Err(e) = flasher.change_baud(baud) {
                    // Leave the board in its app; the caller reconnects slower.
                    let _ = flasher.connection().reset();
                    return Err(FlashError::Protocol(format!(
                        "Could not switch the ESP32 bootloader to {baud} baud: {e}"
                    )));
                }
            }
            Ok(Box::new(Session {
                flasher: Some(flasher),
                chip,
                port: port.to_string(),
            }))
        }

        fn detect(&self, port: &str) -> Result<Option<EspChip>> {
            let mut conn = open(port)?;
            if let Err(e) = conn.begin() {
                log::info!("detect_chip: no bootloader answer on {port}: {e:?}");
                let _ = conn.reset();
                return Ok(None);
            }
            let found = conn
                .set_timeout(Duration::from_secs(3))
                .and_then(|_| conn.detect_chip(false));
            if let Err(e) = conn.reset() {
                log::warn!("detect_chip: hard reset on {port} failed: {e}");
            }
            match found {
                Ok(chip) => {
                    let chip = EspChip::from_name(&chip.to_string());
                    log::info!("detect_chip: {} on {port}", chip.display());
                    Ok(Some(chip))
                }
                Err(e) => {
                    log::info!("detect_chip: unknown chip on {port}: {e:?}");
                    Ok(None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::builtin_boards;
    use crate::mock::{MockHost, ProgressLog, ScriptedDevice};
    use std::sync::Mutex;

    fn board(id: &str) -> Board {
        builtin_boards().into_iter().find(|b| b.id == id).unwrap()
    }

    /// What a fake bootloader saw.
    #[derive(Default)]
    struct Log {
        connects: Vec<u32>,
        writes: Vec<(u32, Vec<u8>)>,
        resets: usize,
    }

    struct FakeEsp {
        chip: Option<EspChip>,
        /// Bauds whose switch fails.
        bad_bauds: Vec<u32>,
        verify_fails: bool,
        log: Arc<Mutex<Log>>,
    }

    impl FakeEsp {
        fn new(chip: EspChip) -> Self {
            FakeEsp {
                chip: Some(chip),
                bad_bauds: vec![],
                verify_fails: false,
                log: Arc::default(),
            }
        }
    }

    struct FakeSession {
        chip: EspChip,
        verify_fails: bool,
        log: Arc<Mutex<Log>>,
    }

    impl EspSession for FakeSession {
        fn chip(&self) -> EspChip {
            self.chip.clone()
        }
        fn mac(&mut self) -> Option<String> {
            Some("34:85:18:AA:BB:CC".into())
        }
        fn write_verify_reset(
            &mut self,
            addr: u32,
            data: &[u8],
            events: &mut dyn FnMut(WriteEvent),
        ) -> Result<()> {
            let chunks = data.len().div_ceil(1024);
            events(WriteEvent::Begin { chunks });
            for i in 1..=chunks {
                events(WriteEvent::Chunk(i));
            }
            events(WriteEvent::Verifying);
            if self.verify_fails {
                return Err(FlashError::Verify("Verify failed: MD5".into()));
            }
            let mut log = self.log.lock().unwrap();
            log.writes.push((addr, data.to_vec()));
            log.resets += 1;
            Ok(())
        }
        fn hard_reset(&mut self) {
            self.log.lock().unwrap().resets += 1;
        }
    }

    impl EspBackend for FakeEsp {
        fn connect(&self, port: &str, baud: u32) -> Result<Box<dyn EspSession>> {
            self.log.lock().unwrap().connects.push(baud);
            let Some(chip) = self.chip.clone() else {
                return Err(no_response(port));
            };
            if self.bad_bauds.contains(&baud) {
                return Err(FlashError::Protocol(format!("no {baud}")));
            }
            Ok(Box::new(FakeSession {
                chip,
                verify_fails: self.verify_fails,
                log: self.log.clone(),
            }))
        }
        fn detect(&self, _port: &str) -> Result<Option<EspChip>> {
            Ok(self.chip.clone())
        }
    }

    fn img(tag: u8, len: usize) -> FirmwareImage {
        let data: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(13) ^ tag).collect();
        FirmwareImage::from_bin(0, &data)
    }

    fn uploader(id: &str, fake: FakeEsp) -> (Esptool, Arc<Mutex<Log>>) {
        let log = fake.log.clone();
        let host: Arc<dyn SerialHost> =
            Arc::new(MockHost::new(Box::new(ScriptedDevice::default())));
        let up = Esptool::new(board(id), host).with_backend(Arc::new(fake));
        (up, log)
    }

    #[test]
    fn chip_names() {
        assert_eq!(EspChip::from_name("esp32c3"), EspChip::Esp32c3);
        assert_eq!(EspChip::Esp32.display(), "ESP32");
        assert_eq!(EspChip::Esp32c3.display(), "ESP32-C3");
        assert_eq!(EspChip::Esp32s3.display(), "ESP32-S3");
        assert_eq!(EspChip::from_name("esp32s2").display(), "ESP32-S2");
        assert_eq!(EspChip::from_name("esp32s2").board_id(), None);
    }

    #[test]
    fn select_board_rules() {
        let t = select_board("esp32c3", &EspChip::Esp32c3, "p").unwrap();
        assert_eq!(t.board_id, "esp32c3");
        assert!(t.warning.is_none());

        let t = select_board("esp32c3", &EspChip::Esp32s3, "p").unwrap();
        assert_eq!(t.board_id, "esp32s3");
        assert_eq!(
            t.warning.as_deref(),
            Some("Selected ESP32-C3 but found ESP32-S3; flashed the S3 image.")
        );

        let t = select_board("esp32s3", &EspChip::Esp32, "p").unwrap();
        assert_eq!(t.board_id, "esp32");
        assert_eq!(
            t.warning.as_deref(),
            Some("Selected ESP32-S3 but found ESP32; flashed the ESP32 image.")
        );

        let e = select_board("esp32", &EspChip::from_name("esp32c6"), "/dev/ttyACM0")
            .unwrap_err()
            .to_string();
        assert!(e.contains("ESP32-C6") && e.contains("/dev/ttyACM0"), "{e}");
    }

    #[test]
    fn flashes_matching_chip_at_fast_baud() {
        let (up, log) = uploader("esp32c3", FakeEsp::new(EspChip::Esp32c3));
        let progress = ProgressLog::default();
        let image = img(1, 5000);
        let r = up.upload("/dev/ttyACM0", &image, &progress.cb()).unwrap();
        progress.assert_sane();
        assert_eq!(r.protocol, "esptool");
        assert_eq!(r.baud, Some(460800));
        assert_eq!(r.signature.as_deref(), Some("ESP32-C3 34:85:18:AA:BB:CC"));
        assert_eq!(r.flashed_board.as_deref(), Some("esp32c3"));
        assert_eq!(r.bytes_written, 5000);
        assert!(r.verified);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let log = log.lock().unwrap();
        assert_eq!(log.connects, vec![460800]);
        assert_eq!(log.writes.len(), 1);
        assert_eq!(log.writes[0].0, 0, "merged image goes to 0x0");
        assert_eq!(log.writes[0].1, image.flatten(0, 5000, 0xFF));

        // Every stage shows up, in order.
        let stages: Vec<Stage> = progress.0.lock().unwrap().iter().map(|p| p.stage).collect();
        let mut seen = stages.clone();
        seen.dedup();
        assert_eq!(
            seen,
            vec![
                Stage::Reset,
                Stage::Sync,
                Stage::Erase,
                Stage::Write,
                Stage::Verify,
                Stage::Done
            ]
        );
    }

    #[test]
    fn mismatched_chip_switches_image_with_warning() {
        let (up, log) = uploader("esp32c3", FakeEsp::new(EspChip::Esp32s3));
        let up = up.with_resolver(Box::new(|id| {
            assert_eq!(id, "esp32s3");
            Ok(img(9, 3000))
        }));
        let progress = ProgressLog::default();
        let r = up
            .upload("/dev/ttyACM0", &img(1, 5000), &progress.cb())
            .unwrap();
        progress.assert_sane();
        assert_eq!(r.flashed_board.as_deref(), Some("esp32s3"));
        assert_eq!(
            r.warnings,
            vec!["Selected ESP32-C3 but found ESP32-S3; flashed the S3 image.".to_string()]
        );
        assert_eq!(
            log.lock().unwrap().writes[0].1,
            img(9, 3000).flatten(0, 3000, 0xFF)
        );
    }

    #[test]
    fn mismatch_without_resolver_or_foreign_chip_is_an_error() {
        let (up, log) = uploader("esp32", FakeEsp::new(EspChip::Esp32c3));
        let e = up
            .upload("COM4", &img(1, 100), &|_| {})
            .unwrap_err()
            .to_string();
        assert!(e.contains("Selected ESP32 but found ESP32-C3"), "{e}");
        assert!(log.lock().unwrap().writes.is_empty());
        assert_eq!(
            log.lock().unwrap().resets,
            1,
            "board is sent back to its app"
        );

        let (up, log) = uploader("esp32", FakeEsp::new(EspChip::from_name("esp32s2")));
        let up = up.with_resolver(Box::new(|_| panic!("must not resolve")));
        let e = up.upload("COM4", &img(1, 100), &|_| {}).unwrap_err();
        assert!(matches!(e, FlashError::WrongChip(_)), "{e}");
        assert!(log.lock().unwrap().writes.is_empty());
    }

    #[test]
    fn falls_back_to_slow_baud_but_not_on_silence() {
        let mut fake = FakeEsp::new(EspChip::Esp32);
        fake.bad_bauds = vec![460800];
        let (up, log) = uploader("esp32", fake);
        let progress = ProgressLog::default();
        let r = up
            .upload("/dev/ttyUSB0", &img(1, 2048), &progress.cb())
            .unwrap();
        progress.assert_sane();
        assert_eq!(r.baud, Some(115200));
        assert_eq!(log.lock().unwrap().connects, vec![460800, 115200]);
        assert!(r.warnings[0].contains("460800"), "{:?}", r.warnings);

        let mut silent = FakeEsp::new(EspChip::Esp32);
        silent.chip = None;
        let (up, log) = uploader("esp32", silent);
        let e = up
            .upload("/dev/ttyUSB0", &img(1, 2048), &|_| {})
            .unwrap_err()
            .to_string();
        assert_eq!(
            e,
            "No response from the ESP32 bootloader on /dev/ttyUSB0. Hold BOOT, tap EN/RST, then try again."
        );
        assert_eq!(
            log.lock().unwrap().connects,
            vec![460800],
            "no retry on silence"
        );
    }

    #[test]
    fn verify_failure_is_reported() {
        let mut fake = FakeEsp::new(EspChip::Esp32s3);
        fake.verify_fails = true;
        let (up, _) = uploader("esp32s3", fake);
        let e = up.upload("COM9", &img(1, 4096), &|_| {}).unwrap_err();
        assert!(matches!(e, FlashError::Verify(_)), "{e}");
    }

    #[test]
    fn progress_ranges_are_monotonic_and_cover_the_bar() {
        let order = [
            Stage::Reset,
            Stage::Sync,
            Stage::Erase,
            Stage::Write,
            Stage::Verify,
            Stage::Done,
        ];
        let mut last = 0;
        for s in order {
            let (lo, hi) = esp_ranges(s);
            assert!(lo >= last && hi >= lo && hi <= 100, "{s:?}");
            last = hi;
        }
        assert_eq!(esp_ranges(Stage::Done), (100, 100));
        assert_eq!(esp_ranges(Stage::Reset).0, 0);
    }

    #[test]
    fn detect_maps_to_board_ids() {
        assert_eq!(
            detect_board(&FakeEsp::new(EspChip::Esp32s3), "p").unwrap(),
            Some("esp32s3")
        );
        assert_eq!(
            detect_board(&FakeEsp::new(EspChip::from_name("esp32h2")), "p").unwrap(),
            None
        );
        let mut silent = FakeEsp::new(EspChip::Esp32);
        silent.chip = None;
        assert_eq!(detect_board(&silent, "p").unwrap(), None);
    }

    #[test]
    fn banner_is_read_after_reenumeration() {
        struct Flaky {
            fails: usize,
            inner: ScriptedDevice,
        }
        impl crate::mock::Device for Flaky {
            fn open(&mut self, _p: &str, _b: u32) -> std::result::Result<(), String> {
                if self.fails > 0 {
                    self.fails -= 1;
                    return Err("gone".into());
                }
                Ok(())
            }
            fn write(&mut self, d: &[u8]) {
                self.inner.write(d)
            }
            fn read(&mut self, b: &mut [u8]) -> usize {
                self.inner.read(b)
            }
        }
        let dev = Flaky {
            fails: 3,
            inner: ScriptedDevice::replying(b"?\n", b"DESKBUNNY 0.2.0 esp32c3 \r\n"),
        };
        let host = MockHost::new(Box::new(dev));
        let line = read_banner(&host, "/dev/ttyACM0").unwrap();
        assert_eq!(line.as_deref(), Some("DESKBUNNY 0.2.0 esp32c3"));
        assert_eq!(host.opens.lock().unwrap().len(), 4);
    }
}
