//! Serial port discovery and board guessing.

use crate::boards::Board;
use crate::{FlashError, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsbInfo {
    pub vid: u16,
    pub pid: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub path: String,
    pub usb: Option<UsbInfo>,
}

/// List serial ports. On macOS only the `/dev/cu.*` callout device of each pair is
/// returned (`/dev/tty.*` blocks on open until carrier detect).
pub fn list_ports() -> Result<Vec<PortInfo>> {
    let ports = serialport::available_ports()
        .map_err(|e| FlashError::Port(format!("Could not list serial ports: {e}")))?;
    let mut out: Vec<PortInfo> = ports
        .into_iter()
        .map(|p| PortInfo {
            path: p.port_name,
            usb: match p.port_type {
                serialport::SerialPortType::UsbPort(u) => Some(UsbInfo {
                    vid: u.vid,
                    pid: u.pid,
                    manufacturer: u.manufacturer,
                    product: u.product,
                    serial_number: u.serial_number,
                }),
                _ => None,
            },
        })
        .collect();
    out = prefer_callout(out);
    out.sort_by(|a, b| {
        // USB ports first, then natural name order.
        b.usb
            .is_some()
            .cmp(&a.usb.is_some())
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(out)
}

/// Drop `/dev/tty.X` when `/dev/cu.X` exists.
pub fn prefer_callout(ports: Vec<PortInfo>) -> Vec<PortInfo> {
    let cu: Vec<String> = ports
        .iter()
        .filter_map(|p| p.path.strip_prefix("/dev/cu.").map(str::to_string))
        .collect();
    ports
        .into_iter()
        .filter(|p| match p.path.strip_prefix("/dev/tty.") {
            Some(rest) => !cu.iter().any(|c| c == rest),
            None => true,
        })
        .collect()
}

/// Guess the board on a port from its USB IDs.
///
/// Genuine Arduino IDs are unique. CH340/FTDI clone IDs match several boards (Nano,
/// old-bootloader Nano, ...): guess "nano", which auto-falls-back to 57600 baud, and
/// let the user override.
pub fn guess_board(port: &PortInfo, boards: &[Board]) -> Option<String> {
    let usb = port.usb.as_ref()?;
    let matches: Vec<&Board> = boards
        .iter()
        .filter(|b| b.usb.iter().any(|u| u.vid == usb.vid && u.pid == usb.pid))
        .collect();
    match matches.as_slice() {
        [] => None,
        [one] => Some(one.id.clone()),
        many => many
            .iter()
            .find(|b| b.id == "nano")
            .or_else(|| many.first())
            .map(|b| b.id.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::builtin_boards;

    fn usb_port(path: &str, vid: u16, pid: u16) -> PortInfo {
        PortInfo {
            path: path.into(),
            usb: Some(UsbInfo {
                vid,
                pid,
                ..Default::default()
            }),
        }
    }

    #[test]
    fn guesses() {
        let boards = builtin_boards();
        let g = |vid, pid| guess_board(&usb_port("x", vid, pid), &boards);
        assert_eq!(g(0x2341, 0x0043).as_deref(), Some("uno"));
        assert_eq!(g(0x1A86, 0x7523).as_deref(), Some("nano"));
        assert_eq!(g(0x0403, 0x6001).as_deref(), Some("nano"));
        assert_eq!(g(0x2341, 0x0042).as_deref(), Some("mega"));
        assert_eq!(g(0x2341, 0x8036).as_deref(), Some("leonardo"));
        assert_eq!(g(0x2341, 0x0036).as_deref(), Some("leonardo"));
        assert_eq!(g(0x2341, 0x0069).as_deref(), Some("uno_r4_minima"));
        assert_eq!(g(0x2341, 0x1002).as_deref(), Some("uno_r4_wifi"));
        assert_eq!(g(0x1234, 0x5678), None);
        let plain = PortInfo {
            path: "/dev/ttyS0".into(),
            usb: None,
        };
        assert_eq!(guess_board(&plain, &boards), None);
    }

    #[test]
    fn macos_prefers_cu() {
        let ports = vec![
            usb_port("/dev/tty.usbmodem1101", 0x2341, 0x0043),
            usb_port("/dev/cu.usbmodem1101", 0x2341, 0x0043),
            usb_port("/dev/tty.Bluetooth-Incoming-Port", 0, 0),
        ];
        let kept: Vec<String> = prefer_callout(ports).into_iter().map(|p| p.path).collect();
        assert_eq!(
            kept,
            vec!["/dev/cu.usbmodem1101", "/dev/tty.Bluetooth-Incoming-Port"]
        );
    }
}
