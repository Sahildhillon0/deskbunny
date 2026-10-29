//! USB DFU 1.1 download (Uno R4 Minima) over the pure-Rust `nusb` crate.
//!
//! Flow, matching `dfu-util -d ,2341:0369 -a 0 -D sketch.bin`: 1200-baud touch on the
//! sketch's CDC port, wait for the bootloader to enumerate as 2341:0369, then
//! DNLOAD blocks of wTransferSize with GETSTATUS polling, a zero-length DNLOAD to
//! end, manifestation, then DFU_DETACH so the bootloader resets into the sketch
//! (Arduino's dfu-util `-Q`, inferred). The Arduino RA4M1 bootloader
//! (arduino-renesas-bootloader) writes block n to 0x4000 + n * wTransferSize
//! itself (plain DFU 1.1, no DfuSe address commands), programs in 2 KiB units and
//! is manifestation tolerant. wTransferSize is 64 on that bootloader.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::boards::{Board, UsbRole};
use crate::link::{touch_1200, SerialHost};
use crate::{FirmwareImage, FlashError, Prog, Progress, Report, Result, Stage, Uploader};

const DFU_DETACH: u8 = 0;
const DFU_DNLOAD: u8 = 1;
const DFU_GETSTATUS: u8 = 3;
const DFU_CLRSTATUS: u8 = 4;
const DFU_ABORT: u8 = 6;

const STATE_DFU_IDLE: u8 = 2;
const STATE_DNBUSY: u8 = 4;
const STATE_DNLOAD_IDLE: u8 = 5;
const STATE_MANIFEST_SYNC: u8 = 6;
const STATE_MANIFEST: u8 = 7;
const STATE_MANIFEST_WAIT_RESET: u8 = 8;
const STATE_ERROR: u8 = 10;

const ENUM_WAIT: Duration = Duration::from_secs(10);

/// Control transfers to the DFU interface.
pub trait DfuTransport: Send {
    fn control_out(&self, request: u8, value: u16, data: &[u8]) -> Result<()>;
    fn control_in(&self, request: u8, value: u16, len: u16) -> Result<Vec<u8>>;
    /// wTransferSize from the DFU functional descriptor.
    fn transfer_size(&self) -> u16;
    /// bmAttributes bit 2: the device stays on the bus after manifestation.
    fn manifestation_tolerant(&self) -> bool;
}

/// Finds and opens a DFU device.
pub trait DfuBackend: Send + Sync {
    fn present(&self, ids: &[(u16, u16)]) -> bool;
    fn open(&self, ids: &[(u16, u16)], alt: u8) -> Result<Box<dyn DfuTransport>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Status {
    status: u8,
    poll_ms: u32,
    state: u8,
}

fn get_status(t: &dyn DfuTransport) -> Result<Status> {
    let r = t.control_in(DFU_GETSTATUS, 0, 6)?;
    if r.len() < 6 {
        return Err(FlashError::Protocol("Short DFU status reply.".into()));
    }
    Ok(Status {
        status: r[0],
        poll_ms: u32::from_le_bytes([r[1], r[2], r[3], 0]),
        state: r[4],
    })
}

fn status_text(s: u8) -> &'static str {
    match s {
        0x01 => "file is not for this device",
        0x02 => "file failed verification",
        0x03 => "could not write memory",
        0x04 => "memory erase failed",
        0x05 => "memory erase check failed",
        0x06 => "program memory failed",
        0x07 => "programmed memory failed verification",
        0x08 => "address out of range",
        0x09 => "received DNLOAD with length 0 but data was expected",
        0x0A => "firmware is corrupt",
        0x0B => "vendor-specific error",
        0x0C => "unexpected USB reset",
        0x0D => "unexpected power-on reset",
        0x0E => "unknown error",
        0x0F => "stalled an unexpected request",
        _ => "error",
    }
}

/// Poll GETSTATUS until the device leaves dnBUSY. `sleep` is injected for tests.
fn wait_idle(t: &dyn DfuTransport, sleep: &dyn Fn(Duration), what: &str) -> Result<Status> {
    let start = Instant::now();
    loop {
        let st = get_status(t)?;
        if st.state == STATE_ERROR || st.status != 0 {
            return Err(FlashError::Protocol(format!(
                "The DFU bootloader reported an error during {what}: {} (status {}). Try again; if it persists, double-press reset and retry.",
                status_text(st.status),
                st.status
            )));
        }
        if st.state != STATE_DNBUSY {
            return Ok(st);
        }
        if start.elapsed() > Duration::from_secs(10) {
            return Err(FlashError::NoResponse(format!(
                "The DFU bootloader stayed busy during {what}."
            )));
        }
        sleep(Duration::from_millis(st.poll_ms.clamp(1, 1000) as u64));
    }
}

/// Download `data` and manifest it. Exposed for tests.
pub(crate) fn download(
    t: &dyn DfuTransport,
    data: &[u8],
    sleep: &dyn Fn(Duration),
    prog: &Prog,
) -> Result<()> {
    // Bring the device to dfuIDLE.
    let st = get_status(t)?;
    if st.state == STATE_ERROR {
        t.control_out(DFU_CLRSTATUS, 0, &[])?;
    } else if st.state != STATE_DFU_IDLE {
        t.control_out(DFU_ABORT, 0, &[])?;
    }
    let st = get_status(t)?;
    if st.state != STATE_DFU_IDLE {
        return Err(FlashError::Protocol(format!(
            "The DFU bootloader is not ready (state {}). Unplug and re-plug the board and try again.",
            st.state
        )));
    }

    let size = t.transfer_size().max(64) as usize;
    let blocks = data.chunks(size).count();
    prog.emit(
        Stage::Erase,
        1.0,
        "Bootloader erases flash as blocks arrive",
    );
    for (n, chunk) in data.chunks(size).enumerate() {
        t.control_out(DFU_DNLOAD, n as u16, chunk)?;
        let st = wait_idle(t, sleep, "download")?;
        if st.state != STATE_DNLOAD_IDLE {
            return Err(FlashError::Protocol(format!(
                "Unexpected DFU state {} after block {n}.",
                st.state
            )));
        }
        prog.emit(
            Stage::Write,
            (n + 1) as f32 / blocks as f32,
            format!(
                "Writing {} / {} bytes",
                ((n + 1) * size).min(data.len()),
                data.len()
            ),
        );
    }

    // Zero-length DNLOAD ends the transfer and starts manifestation.
    prog.emit(Stage::Verify, 0.2, "Finishing (manifestation)");
    t.control_out(DFU_DNLOAD, blocks as u16, &[])?;
    let start = Instant::now();
    loop {
        match get_status(t) {
            Ok(st) if st.state == STATE_ERROR || st.status != 0 => {
                return Err(FlashError::Protocol(format!(
                    "The DFU bootloader rejected the firmware: {}.",
                    status_text(st.status)
                )))
            }
            Ok(st) if st.state == STATE_DFU_IDLE || st.state == STATE_MANIFEST_WAIT_RESET => {
                return Ok(())
            }
            Ok(st)
                if st.state == STATE_MANIFEST_SYNC
                    || st.state == STATE_MANIFEST
                    || st.state == STATE_DNBUSY =>
            {
                sleep(Duration::from_millis(st.poll_ms.clamp(1, 1000) as u64));
            }
            Ok(st) => {
                return Err(FlashError::Protocol(format!(
                    "Unexpected DFU state {} during manifestation.",
                    st.state
                )))
            }
            // Non-manifestation-tolerant devices reset and drop off the bus here.
            Err(_) if !t.manifestation_tolerant() => return Ok(()),
            Err(e) => return Err(e),
        }
        if start.elapsed() > Duration::from_secs(10) {
            return Ok(());
        }
    }
}

pub struct Dfu {
    board: Board,
    host: Arc<dyn SerialHost>,
    backend: Arc<dyn DfuBackend>,
    alt: u8,
}

impl Dfu {
    pub fn new(board: Board, host: Arc<dyn SerialHost>) -> Self {
        Dfu {
            board,
            host,
            backend: Arc::new(NusbBackend),
            alt: 0,
        }
    }

    pub fn with_backend(
        board: Board,
        host: Arc<dyn SerialHost>,
        backend: Arc<dyn DfuBackend>,
    ) -> Self {
        Dfu {
            board,
            host,
            backend,
            alt: 0,
        }
    }
}

impl Uploader for Dfu {
    fn upload(
        &self,
        port: &str,
        image: &FirmwareImage,
        progress: &dyn Fn(Progress),
    ) -> Result<Report> {
        let prog = Prog::new(progress);
        let max = self.board.max_app_size();
        let base = self.board.bin_base();
        if image.base() < base || image.end() - base > max {
            return Err(FlashError::Image(format!(
                "The firmware does not fit the {} application area.",
                self.board.name
            )));
        }
        // The bootloader always flushes whole 2 KiB pages at manifestation; pad so
        // the tail is erased-flash 0xFF rather than stale buffer bytes.
        let mut data = image.flatten(image.base(), image.end(), 0xFF);
        data.resize(data.len().div_ceil(2048) * 2048, 0xFF);
        let boot: Vec<(u16, u16)> = self.board.usb_ids(UsbRole::Bootloader).collect();

        if !self.backend.present(&boot) {
            prog.emit(
                Stage::Reset,
                0.2,
                format!("Resetting {port} with a 1200-baud touch"),
            );
            let touch_err = touch_1200(self.host.as_ref(), port);
            prog.emit(Stage::Reset, 0.6, "Waiting for the DFU bootloader");
            let start = Instant::now();
            while !self.backend.present(&boot) {
                if start.elapsed() > ENUM_WAIT {
                    return Err(touch_err.unwrap_or_else(|| FlashError::NoResponse(format!(
                        "The {} did not enter its DFU bootloader after resetting {port}. Double-press the reset button (the LED fades in and out), then try again.",
                        self.board.name
                    ))));
                }
                self.host.sleep(Duration::from_millis(200));
            }
            // Let the OS finish enumerating before claiming the interface.
            self.host.sleep(Duration::from_millis(300));
        }

        prog.emit(Stage::Sync, 0.5, "Opening the DFU interface");
        let t = self.backend.open(&boot, self.alt)?;
        let host = self.host.clone();
        let sleep = move |d: Duration| host.sleep(d);
        download(t.as_ref(), &data, &sleep, &prog)?;
        // Leave DFU mode: the bootloader resets ~100 ms after DETACH. The device
        // may vanish before acknowledging, so errors here are expected.
        let _ = t.control_out(DFU_DETACH, 1000, &[]);
        prog.emit(Stage::Done, 1.0, "Upload complete");
        Ok(Report {
            protocol: "dfu".into(),
            port: format!("USB {:04X}:{:04X}", boot[0].0, boot[0].1),
            baud: None,
            signature: None,
            bytes_written: data.len(),
            verified: false,
            duration_ms: prog.elapsed_ms(),
            warnings: vec![
                "The DFU bootloader does not support reading flash back; the upload was checked by the bootloader's status replies only.".into(),
            ],
        })
    }
}

/// (bmAttributes, wTransferSize) from the DFU functional descriptor (type 0x21),
/// searched across the class descriptors of every alt setting of the interface.
/// Falls back to (0, 64): 64 is the smallest valid full-speed EP0 size, and a
/// block larger than the device buffer is stalled.
fn dfu_functional(alts: &[Vec<Vec<u8>>]) -> (u8, u16) {
    alts.iter()
        .flatten()
        .find(|b| b.len() >= 7 && b[1] == 0x21)
        .map(|b| (b[2], u16::from_le_bytes([b[5], b[6]])))
        .unwrap_or((0, 64))
}

/// Real USB access through nusb.
pub struct NusbBackend;

struct NusbTransport {
    iface: nusb::Interface,
    number: u8,
    transfer_size: u16,
    tolerant: bool,
}

const USB_TIMEOUT: Duration = Duration::from_secs(5);

fn usb_err(e: nusb::transfer::TransferError) -> FlashError {
    FlashError::Usb(format!("USB transfer to the DFU bootloader failed: {e}."))
}

impl DfuTransport for NusbTransport {
    fn control_out(&self, request: u8, value: u16, data: &[u8]) -> Result<()> {
        use nusb::transfer::{ControlOut, ControlType, Recipient};
        use nusb::MaybeFuture;
        self.iface
            .control_out(
                ControlOut {
                    control_type: ControlType::Class,
                    recipient: Recipient::Interface,
                    request,
                    value,
                    index: self.number as u16,
                    data,
                },
                USB_TIMEOUT,
            )
            .wait()
            .map_err(usb_err)
    }

    fn control_in(&self, request: u8, value: u16, len: u16) -> Result<Vec<u8>> {
        use nusb::transfer::{ControlIn, ControlType, Recipient};
        use nusb::MaybeFuture;
        self.iface
            .control_in(
                ControlIn {
                    control_type: ControlType::Class,
                    recipient: Recipient::Interface,
                    request,
                    value,
                    index: self.number as u16,
                    length: len,
                },
                USB_TIMEOUT,
            )
            .wait()
            .map_err(usb_err)
    }

    fn transfer_size(&self) -> u16 {
        self.transfer_size
    }

    fn manifestation_tolerant(&self) -> bool {
        self.tolerant
    }
}

fn open_hint() -> &'static str {
    if cfg!(windows) {
        "Windows needs Arduino's Uno R4 driver for this. Open the setup guide (the ? button, top left) and choose 'Install driver'."
    } else if cfg!(target_os = "linux") {
        "Open the setup guide (the ? button, top left) and choose 'Allow USB access', then re-plug the board."
    } else {
        "Close any other program that may be using the board and try again."
    }
}

impl DfuBackend for NusbBackend {
    fn present(&self, ids: &[(u16, u16)]) -> bool {
        use nusb::MaybeFuture;
        nusb::list_devices()
            .wait()
            .map(|mut it| it.any(|d| ids.contains(&(d.vendor_id(), d.product_id()))))
            .unwrap_or(false)
    }

    fn open(&self, ids: &[(u16, u16)], alt: u8) -> Result<Box<dyn DfuTransport>> {
        use nusb::MaybeFuture;
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| FlashError::Usb(format!("Could not list USB devices: {e}")))?
            .find(|d| ids.contains(&(d.vendor_id(), d.product_id())))
            .ok_or_else(|| {
                FlashError::NoResponse(
                    "The DFU bootloader disappeared from USB. Double-press reset and try again."
                        .into(),
                )
            })?;
        let dev = info.open().wait().map_err(|e| {
            FlashError::Usb(format!(
                "Could not open the DFU device: {e}. {}",
                open_hint()
            ))
        })?;

        // Find the DFU interface (class FE, subclass 01) and its functional descriptor.
        let config = dev
            .active_configuration()
            .map_err(|e| FlashError::Usb(format!("Could not read the USB configuration: {e}")))?;
        let mut found = None;
        for iface in config.interfaces() {
            let alts: Vec<_> = iface.alt_settings().collect();
            if !alts
                .iter()
                .any(|a| a.class() == 0xFE && a.subclass() == 0x01 && a.alternate_setting() == alt)
            {
                continue;
            }
            // The functional descriptor follows only one alt setting (TinyUSB puts it
            // after the last), so search them all.
            let extra: Vec<Vec<Vec<u8>>> = alts
                .iter()
                .map(|a| a.descriptors().map(|d| d.as_bytes().to_vec()).collect())
                .collect();
            let (attrs, size) = dfu_functional(&extra);
            found = Some((alts[0].interface_number(), size, attrs));
        }
        let (number, transfer_size, attrs) = found.ok_or_else(|| {
            FlashError::Usb("The device has no DFU interface; is it really an Uno R4 Minima in bootloader mode?".into())
        })?;
        let iface = dev.detach_and_claim_interface(number).wait().map_err(|e| {
            FlashError::Usb(format!(
                "Could not claim the DFU interface: {e}. {}",
                open_hint()
            ))
        })?;
        if alt != 0 {
            iface.set_alt_setting(alt).wait().map_err(|e| {
                FlashError::Usb(format!("Could not select DFU alt setting {alt}: {e}"))
            })?;
        }
        Ok(Box::new(NusbTransport {
            iface,
            number,
            transfer_size,
            tolerant: attrs & 0x04 != 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::builtin_boards;
    use crate::mock::{MockHost, ProgressLog, ScriptedDevice};
    use std::sync::Mutex;

    /// DFU 1.1 device simulator.
    #[derive(Default)]
    struct SimState {
        state: u8,
        busy_polls: u8,
        expect_block: u16,
        mem: Vec<u8>,
        manifested: bool,
        detached: bool,
        fail_block: Option<u16>,
        status: u8,
    }

    struct SimDfu(Arc<Mutex<SimState>>);

    impl DfuTransport for SimDfu {
        fn control_out(&self, request: u8, value: u16, data: &[u8]) -> Result<()> {
            let mut s = self.0.lock().unwrap();
            match request {
                DFU_DNLOAD if data.is_empty() => {
                    assert_eq!(s.state, STATE_DNLOAD_IDLE);
                    s.state = STATE_MANIFEST_SYNC;
                }
                DFU_DNLOAD => {
                    assert!(s.state == STATE_DFU_IDLE || s.state == STATE_DNLOAD_IDLE);
                    assert_eq!(value, s.expect_block, "block numbers must be sequential");
                    assert!(data.len() <= 64);
                    if s.fail_block == Some(value) {
                        s.state = STATE_ERROR;
                        s.status = 0x03;
                        return Ok(());
                    }
                    s.mem.extend_from_slice(data);
                    s.expect_block += 1;
                    s.state = STATE_DNBUSY;
                    s.busy_polls = 2;
                }
                DFU_ABORT => s.state = STATE_DFU_IDLE,
                DFU_DETACH => {
                    assert_eq!(s.state, STATE_DFU_IDLE, "detach only after manifestation");
                    s.detached = true;
                }
                DFU_CLRSTATUS => {
                    s.state = STATE_DFU_IDLE;
                    s.status = 0;
                }
                r => panic!("unexpected request {r}"),
            }
            Ok(())
        }

        fn control_in(&self, request: u8, _value: u16, len: u16) -> Result<Vec<u8>> {
            assert_eq!(request, DFU_GETSTATUS);
            assert_eq!(len, 6);
            let mut s = self.0.lock().unwrap();
            let reply_state = s.state;
            match s.state {
                STATE_DNBUSY => {
                    s.busy_polls -= 1;
                    if s.busy_polls == 0 {
                        s.state = STATE_DNLOAD_IDLE;
                    }
                }
                STATE_MANIFEST_SYNC => s.state = STATE_MANIFEST,
                STATE_MANIFEST => {
                    // Manifestation tolerant: back to dfuIDLE.
                    s.manifested = true;
                    s.state = STATE_DFU_IDLE;
                }
                _ => {}
            }
            Ok(vec![s.status, 5, 0, 0, reply_state, 0])
        }

        fn transfer_size(&self) -> u16 {
            64
        }

        fn manifestation_tolerant(&self) -> bool {
            true
        }
    }

    struct SimBackend {
        state: Arc<Mutex<SimState>>,
        polls_until_present: Mutex<u32>,
    }

    impl DfuBackend for SimBackend {
        fn present(&self, ids: &[(u16, u16)]) -> bool {
            assert_eq!(ids, &[(0x2341, 0x0369)]);
            let mut n = self.polls_until_present.lock().unwrap();
            if *n == 0 {
                return true;
            }
            *n -= 1;
            false
        }
        fn open(&self, _ids: &[(u16, u16)], _alt: u8) -> Result<Box<dyn DfuTransport>> {
            Ok(Box::new(SimDfu(self.state.clone())))
        }
    }

    fn minima() -> Board {
        builtin_boards()
            .into_iter()
            .find(|b| b.id == "uno_r4_minima")
            .unwrap()
    }

    fn sim(fail_block: Option<u16>) -> (Arc<Mutex<SimState>>, Arc<SimBackend>) {
        let state = Arc::new(Mutex::new(SimState {
            state: STATE_DFU_IDLE,
            fail_block,
            ..Default::default()
        }));
        let backend = Arc::new(SimBackend {
            state: state.clone(),
            polls_until_present: Mutex::new(3),
        });
        (state, backend)
    }

    #[test]
    fn touch_download_and_manifest() {
        let (state, backend) = sim(None);
        let host = Arc::new(MockHost::new(Box::new(ScriptedDevice::default())));
        let up = Dfu::with_backend(minima(), host.clone(), backend);
        let img = crate::mock::test_image(0x4000, 1000);
        let log = ProgressLog::default();
        let r = up.upload("/dev/ttyACM0", &img, &log.cb()).unwrap();
        log.assert_sane();
        assert_eq!(r.bytes_written, 2048);
        let s = state.lock().unwrap();
        assert!(s.manifested && s.detached);
        assert_eq!(s.mem.len(), 2048);
        assert_eq!(
            &s.mem[..1000],
            &img.flatten(0x4000, 0x4000 + 1000, 0xFF)[..]
        );
        assert!(s.mem[1000..].iter().all(|b| *b == 0xFF));
        assert_eq!(
            host.opens.lock().unwrap()[0],
            ("/dev/ttyACM0".to_string(), 1200)
        );
    }

    /// Windows: the R4 resets during the 1200-baud open, so the open fails with
    /// ERROR_GEN_FAILURE even though the touch worked.
    struct ResetsDuringOpen;

    impl crate::mock::Device for ResetsDuringOpen {
        fn open(&mut self, path: &str, _baud: u32) -> std::result::Result<(), String> {
            Err(format!(
                "Could not open {path}: A device attached to the system is not functioning."
            ))
        }
        fn write(&mut self, _data: &[u8]) {}
        fn read(&mut self, _buf: &mut [u8]) -> usize {
            0
        }
    }

    #[test]
    fn touch_open_failure_still_waits_for_the_bootloader() {
        let (state, backend) = sim(None);
        let host = Arc::new(MockHost::new(Box::new(ResetsDuringOpen)));
        let up = Dfu::with_backend(minima(), host, backend);
        let img = crate::mock::test_image(0x4000, 1000);
        up.upload("COM7", &img, &|_| {}).unwrap();
        assert!(state.lock().unwrap().manifested);
    }

    #[test]
    fn device_error_is_reported() {
        let (_, backend) = sim(Some(2));
        let host = Arc::new(MockHost::new(Box::new(ScriptedDevice::default())));
        let up = Dfu::with_backend(minima(), host, backend);
        let img = crate::mock::test_image(0x4000, 1000);
        let e = up.upload("COM3", &img, &|_| {}).unwrap_err().to_string();
        assert!(e.contains("could not write memory"), "{e}");
    }

    #[test]
    fn functional_descriptor_found_after_last_alt() {
        // Uno R4 Minima bootloader (TinyUSB TUD_DFU_DESCRIPTOR): alt 0 and alt 1, with
        // the functional descriptor (attrs 0x07, detach 1000 ms, 64-byte transfers)
        // only after alt 1.
        let func = vec![0x09, 0x21, 0x07, 0xE8, 0x03, 0x40, 0x00, 0x01, 0x01];
        assert_eq!(dfu_functional(&[vec![], vec![func]]), (0x07, 64));
        // No functional descriptor at all: never guess a size above 64.
        assert_eq!(dfu_functional(&[vec![], vec![]]), (0, 64));
    }

    #[test]
    fn image_below_app_area_is_rejected() {
        let (_, backend) = sim(None);
        let host = Arc::new(MockHost::new(Box::new(ScriptedDevice::default())));
        let up = Dfu::with_backend(minima(), host, backend);
        let img = crate::mock::test_image(0, 100);
        assert!(up.upload("COM3", &img, &|_| {}).is_err());
    }
}
