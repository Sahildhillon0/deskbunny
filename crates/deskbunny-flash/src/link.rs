//! Serial transport abstraction, so every protocol can run against a mock bootloader.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use crate::ports::PortInfo;
use crate::{FlashError, Result};

/// A byte stream with modem-control lines: a real serial port or a test double.
pub trait SerialLink: Read + Write + Send {
    fn set_dtr(&mut self, level: bool) -> io::Result<()>;
    fn set_rts(&mut self, level: bool) -> io::Result<()>;
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()>;
    /// Discard anything waiting in the input buffer.
    fn clear_input(&mut self) -> io::Result<()>;
}

/// Everything a backend needs from the machine: port listing, opening and sleeping.
pub trait SerialHost: Send + Sync {
    fn list(&self) -> Result<Vec<PortInfo>>;
    fn open(&self, path: &str, baud: u32, timeout: Duration) -> Result<Box<dyn SerialLink>>;
    fn sleep(&self, d: Duration);
}

/// The real machine, via the `serialport` crate.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemHost;

struct SystemLink(Box<dyn serialport::SerialPort>);

impl Read for SystemLink {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for SystemLink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl SerialLink for SystemLink {
    fn set_dtr(&mut self, level: bool) -> io::Result<()> {
        self.0
            .write_data_terminal_ready(level)
            .map_err(io::Error::from)
    }
    fn set_rts(&mut self, level: bool) -> io::Result<()> {
        self.0.write_request_to_send(level).map_err(io::Error::from)
    }
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.0.set_timeout(timeout).map_err(io::Error::from)
    }
    fn clear_input(&mut self) -> io::Result<()> {
        self.0
            .clear(serialport::ClearBuffer::Input)
            .map_err(io::Error::from)
    }
}

impl SerialHost for SystemHost {
    fn list(&self) -> Result<Vec<PortInfo>> {
        crate::ports::list_ports()
    }

    fn open(&self, path: &str, baud: u32, timeout: Duration) -> Result<Box<dyn SerialLink>> {
        log::debug!("open {path} at {baud} baud");
        let port = serialport::new(path, baud)
            .timeout(timeout)
            .open()
            .map_err(|e| {
                log::warn!(
                    "open {path} at {baud} baud failed: {e} (kind {:?})",
                    e.kind()
                );
                open_error(path, e)
            })?;
        Ok(Box::new(SystemLink(port)))
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// WCH's USB vendor id (CH340/CH341 USB-serial chips).
const WCH_VID: u16 = 0x1A86;

/// True when `path` is a WCH (CH340/CH341) USB-serial adapter.
fn is_wch_port(path: &str) -> bool {
    serialport::available_ports()
        .unwrap_or_default()
        .iter()
        .any(|p| {
            p.port_name.eq_ignore_ascii_case(path)
                && matches!(&p.port_type, serialport::SerialPortType::UsbPort(u) if u.vid == WCH_VID)
        })
}

fn open_error(path: &str, e: serialport::Error) -> FlashError {
    let detail = e.to_string();
    // WCH's Windows driver 3.8+ fails every open on many clone CH340 chips with
    // ERROR_GEN_FAILURE (31), which serialport reports as Io(Other); missing and
    // busy ports are NoDevice. Match the vendor id, not the (localised) text.
    if cfg!(windows) && matches!(e.kind(), serialport::ErrorKind::Io(_)) && is_wch_port(path) {
        log::warn!("{path} is a WCH (CH340) port: likely the broken WCH driver 3.8/3.9");
        return FlashError::Port(format!(
            "Could not open {path}: {detail}. This board's USB chip is a CH340, and the Windows CH340 driver from WCH (3.8 or newer) doesn't work with it. Install CH340 driver 3.5.2019.1 instead (setup guide, the ? button, top left), then re-plug the board."
        ));
    }
    let hint = match e.kind() {
        // Windows reports a busy port (access denied) as NoDevice too.
        serialport::ErrorKind::NoDevice if cfg!(windows) => "The board was unplugged, the port name changed, or another program (Arduino IDE, a serial monitor) is using the port. Close other programs, re-plug the board and refresh the port list.".to_string(),
        serialport::ErrorKind::NoDevice => "The board was unplugged or the port name changed. Re-plug it and refresh the port list.".to_string(),
        serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied) => {
            if cfg!(target_os = "linux") {
                "DeskBunny isn't allowed to use USB devices yet. Open the setup guide (the ? button, top left) and choose 'Allow USB access'.".to_string()
            } else {
                "Permission denied. Another program may be using the port.".to_string()
            }
        }
        _ if detail.to_ascii_lowercase().contains("busy") || detail.contains("Access is denied") => {
            "The port is in use. Close the Arduino IDE Serial Monitor or any other program using it.".to_string()
        }
        _ => "Check that the board is plugged in and the right port is selected.".to_string(),
    };
    FlashError::Port(format!("Could not open {path}: {detail}. {hint}"))
}

/// Read exactly `n` bytes, mapping a timeout to a protocol-specific message.
pub(crate) fn read_n(
    link: &mut dyn SerialLink,
    n: usize,
    on_timeout: impl FnOnce() -> String,
) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut got = 0;
    while got < n {
        match link.read(&mut buf[got..]) {
            Ok(0) => return Err(FlashError::NoResponse(on_timeout())),
            Ok(k) => got += k,
            Err(e)
                if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock =>
            {
                return Err(FlashError::NoResponse(on_timeout()))
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(io_err("Serial read failed", e)),
        }
    }
    Ok(buf)
}

pub(crate) fn write_all(link: &mut dyn SerialLink, data: &[u8]) -> Result<()> {
    link.write_all(data)
        .and_then(|_| link.flush())
        .map_err(|e| io_err("Serial write failed", e))
}

pub(crate) fn io_err(what: &str, e: io::Error) -> FlashError {
    FlashError::Port(format!(
        "{what}: {e}. The board may have been unplugged; re-plug it and try again."
    ))
}

/// Open the port at 1200 baud and drop DTR: the "touch" that makes Caterina,
/// the R4 bootloaders and the R4 WiFi bridge enter their bootloader.
///
/// A failed open is returned, not raised: on Windows the R4 resets as soon as
/// `SetCommState` sends the 1200-baud line coding, so the open itself can fail
/// ("A device attached to the system is not functioning") although the touch
/// worked. Callers wait for the bootloader anyway and report this error only if
/// it never shows up, like arduino-cli's "TOUCH: error during reset".
pub fn touch_1200(host: &dyn SerialHost, port: &str) -> Option<FlashError> {
    let mut link = match host.open(port, 1200, Duration::from_millis(100)) {
        Ok(link) => link,
        Err(e) => {
            log::warn!("1200-baud touch on {port}: open failed, waiting for the bootloader anyway");
            return Some(e);
        }
    };
    // Some drivers only latch the line coding when DTR toggles.
    if let Err(e) = link.set_dtr(true) {
        log::debug!("1200-baud touch on {port}: DTR on failed: {e}");
    }
    host.sleep(Duration::from_millis(50));
    if let Err(e) = link.set_dtr(false) {
        log::debug!("1200-baud touch on {port}: DTR off failed: {e}");
    }
    drop(link);
    log::info!("1200-baud touch on {port} done");
    None
}

/// After a 1200-baud touch, find the port the bootloader shows up on.
///
/// Port names are not stable across re-enumeration: Windows usually assigns a new
/// COM number, macOS a new `/dev/cu.usbmodemXXXX`, Linux often reuses `ttyACM0`.
/// Accept, in order: a port with a bootloader VID:PID, a port that was not there
/// before, or the original port after it disappeared and came back. If nothing
/// changes before the timeout and the original port still exists, use it (some
/// bootloaders keep the same USB identity, e.g. the R4 WiFi bridge).
pub fn wait_for_bootloader_port(
    host: &dyn SerialHost,
    before: &[PortInfo],
    original: &str,
    bootloader_ids: &[(u16, u16)],
    timeout: Duration,
) -> Result<String> {
    let start = Instant::now();
    let mut original_gone = false;
    loop {
        let now = host.list()?;
        if let Some(p) = now.iter().find(|p| {
            p.usb
                .as_ref()
                .is_some_and(|u| bootloader_ids.contains(&(u.vid, u.pid)))
        }) {
            return Ok(p.path.clone());
        }
        if let Some(p) = now
            .iter()
            .find(|p| !before.iter().any(|b| b.path == p.path))
        {
            return Ok(p.path.clone());
        }
        let has_original = now.iter().any(|p| p.path == original);
        if !has_original {
            original_gone = true;
        } else if original_gone {
            return Ok(original.to_string());
        }
        if start.elapsed() >= timeout {
            if has_original {
                return Ok(original.to_string());
            }
            return Err(FlashError::NoResponse(format!(
                "The bootloader port did not appear within {} s after resetting {original}. Unplug and re-plug the board, or double-press its reset button and start the upload right away.",
                timeout.as_secs()
            )));
        }
        host.sleep(Duration::from_millis(100));
    }
}

/// Open a port that has just enumerated; the OS may need a moment to grant access.
pub(crate) fn open_retry(
    host: &dyn SerialHost,
    path: &str,
    baud: u32,
    timeout: Duration,
    total: Duration,
) -> Result<Box<dyn SerialLink>> {
    let start = Instant::now();
    loop {
        match host.open(path, baud, timeout) {
            Ok(l) => return Ok(l),
            Err(e) if start.elapsed() >= total => return Err(e),
            Err(_) => host.sleep(Duration::from_millis(200)),
        }
    }
}

/// Ask a running DeskBunny firmware for its identity line
/// (`DESKBUNNY <fw_version> <board_id> <face name>`).
///
/// Opening the port resets boards with auto-reset (Uno, Nano, Mega), so this keeps
/// sending `?\n` and reading lines until `total` elapses.
pub fn query_device_info(
    host: &dyn SerialHost,
    port: &str,
    total: Duration,
) -> Result<Option<String>> {
    let mut link = host.open(port, 115200, Duration::from_millis(100))?;
    let start = Instant::now();
    let mut line = Vec::new();
    let mut last_ask: Option<Instant> = None;
    let mut buf = [0u8; 64];
    while start.elapsed() < total {
        if last_ask.is_none_or(|t| t.elapsed() >= Duration::from_millis(400)) {
            write_all(link.as_mut(), b"?\n")?;
            last_ask = Some(Instant::now());
        }
        match link.read(&mut buf) {
            Ok(0) => host.sleep(Duration::from_millis(20)),
            Ok(n) => {
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        let s = String::from_utf8_lossy(&line).trim().to_string();
                        line.clear();
                        if let Some(idx) = s.find("DESKBUNNY ") {
                            return Ok(Some(s[idx..].to_string()));
                        }
                    } else if line.len() < 128 {
                        line.push(b);
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(io_err("Serial read failed", e)),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MockHost, ScriptedDevice};
    use crate::ports::UsbInfo;

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
    fn finds_new_port_by_bootloader_pid() {
        let host = MockHost::new(Box::new(ScriptedDevice::default()));
        host.push_listing(vec![]);
        host.push_listing(vec![port("COM7", 0x0036)]);
        let before = vec![port("COM5", 0x8036)];
        let p = wait_for_bootloader_port(
            &host,
            &before,
            "COM5",
            &[(0x2341, 0x0036)],
            Duration::from_secs(8),
        )
        .unwrap();
        assert_eq!(p, "COM7");
    }

    #[test]
    fn same_name_reappearing_is_accepted() {
        let host = MockHost::new(Box::new(ScriptedDevice::default()));
        let before = vec![port("/dev/ttyACM0", 0x8036)];
        host.push_listing(vec![]);
        host.push_listing(vec![port("/dev/ttyACM0", 0x9999)]);
        let p = wait_for_bootloader_port(
            &host,
            &before,
            "/dev/ttyACM0",
            &[(0x2341, 0x0036)],
            Duration::from_secs(8),
        )
        .unwrap();
        assert_eq!(p, "/dev/ttyACM0");
    }

    #[test]
    fn times_out_with_advice_when_port_vanishes() {
        let host = MockHost::new(Box::new(ScriptedDevice::default()));
        host.set_default_listing(vec![]);
        let before = vec![port("COM5", 0x8036)];
        let e = wait_for_bootloader_port(
            &host,
            &before,
            "COM5",
            &[(0x2341, 0x0036)],
            Duration::from_millis(50),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("reset button"), "{e}");
    }

    #[test]
    fn device_info_reads_identity_line() {
        let dev = ScriptedDevice::replying(b"?\n", b"garbage\r\nDESKBUNNY 0.1.0 uno WATCHER\r\n");
        let host = MockHost::new(Box::new(dev));
        let line = query_device_info(&host, "/dev/ttyUSB0", Duration::from_millis(500)).unwrap();
        assert_eq!(line.as_deref(), Some("DESKBUNNY 0.1.0 uno WATCHER"));
    }
}
