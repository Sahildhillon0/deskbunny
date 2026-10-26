//! Board table. The source of truth is `firmware/boards.json` (shipped inside
//! `manifest.json`); [`builtin_boards`] mirrors it so the app can list boards and
//! guess ports before firmware is bundled.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Stk500v1,
    Stk500v2,
    Avr109,
    Dfu,
    Samba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsbRole {
    App,
    Bootloader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbId {
    #[serde(serialize_with = "ser_hex16", deserialize_with = "de_hex16")]
    pub vid: u16,
    #[serde(serialize_with = "ser_hex16", deserialize_with = "de_hex16")]
    pub pid: u16,
    pub role: UsbRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub fqbn: String,
    pub mcu: String,
    pub protocol: Protocol,
    #[serde(default)]
    pub baud: Vec<u32>,
    #[serde(default)]
    pub touch1200: bool,
    pub image: String,
    #[serde(default)]
    pub usb: Vec<UsbId>,
}

impl Board {
    pub fn usb_ids(&self, role: UsbRole) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.usb
            .iter()
            .filter(move |u| u.role == role)
            .map(|u| (u.vid, u.pid))
    }

    /// Bytes available to the sketch (flash minus bootloader).
    pub fn max_app_size(&self) -> u32 {
        match self.id.as_str() {
            // Optiboot: 512-byte bootloader.
            "uno" | "nano" => 32 * 1024 - 512,
            // ATmegaBOOT: 2 KiB bootloader.
            "nano_old" | "pro_mini" => 30 * 1024,
            "mega" => 256 * 1024 - 8 * 1024,
            // Caterina: 4 KiB bootloader.
            "leonardo" | "micro" => 28 * 1024,
            // RA4M1: 256 KiB flash, first 16 KiB is the Arduino bootloader.
            "uno_r4_minima" | "uno_r4_wifi" => 256 * 1024 - 0x4000,
            _ => mcu_info(&self.mcu).map_or(u32::MAX, |m| m.flash_size - 512),
        }
    }

    /// Load address of a raw `.bin` image for this board.
    pub fn bin_base(&self) -> u32 {
        match self.mcu.to_ascii_lowercase().as_str() {
            "ra4m1" => 0x4000,
            _ => 0,
        }
    }
}

/// Facts about AVR parts the flasher must check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McuInfo {
    pub signature: [u8; 3],
    pub flash_size: u32,
    pub page_size: u32,
}

pub fn mcu_info(mcu: &str) -> Option<McuInfo> {
    match mcu.to_ascii_lowercase().as_str() {
        "atmega328p" => Some(McuInfo {
            signature: [0x1E, 0x95, 0x0F],
            flash_size: 32 * 1024,
            page_size: 128,
        }),
        "atmega2560" => Some(McuInfo {
            signature: [0x1E, 0x98, 0x01],
            flash_size: 256 * 1024,
            page_size: 256,
        }),
        "atmega32u4" => Some(McuInfo {
            signature: [0x1E, 0x95, 0x87],
            flash_size: 32 * 1024,
            page_size: 128,
        }),
        _ => None,
    }
}

/// Name a signature for error messages ("ATmega168" is the classic wrong-board case).
pub fn signature_name(sig: [u8; 3]) -> &'static str {
    match sig {
        [0x1E, 0x95, 0x0F] => "ATmega328P",
        [0x1E, 0x95, 0x14] => "ATmega328",
        [0x1E, 0x94, 0x06] => "ATmega168",
        [0x1E, 0x94, 0x0B] => "ATmega168P",
        [0x1E, 0x98, 0x01] => "ATmega2560",
        [0x1E, 0x97, 0x03] => "ATmega1280",
        [0x1E, 0x95, 0x87] => "ATmega32U4",
        _ => "unknown chip",
    }
}

fn usb(vid: u16, pid: u16, role: UsbRole) -> UsbId {
    UsbId { vid, pid, role }
}

/// Built-in copy of the board table in docs/ARCHITECTURE.md.
pub fn builtin_boards() -> Vec<Board> {
    use UsbRole::{App, Bootloader};
    let clones = || {
        vec![
            usb(0x1A86, 0x7523, App), // CH340
            usb(0x0403, 0x6001, App), // FTDI FT232R
        ]
    };
    let b = |id: &str,
             name: &str,
             fqbn: &str,
             mcu: &str,
             protocol: Protocol,
             baud: &[u32],
             touch1200: bool,
             image: &str,
             usb: Vec<UsbId>| Board {
        id: id.into(),
        name: name.into(),
        fqbn: fqbn.into(),
        mcu: mcu.into(),
        protocol,
        baud: baud.to_vec(),
        touch1200,
        image: image.into(),
        usb,
    };
    vec![
        b(
            "uno",
            "Uno R3",
            "arduino:avr:uno",
            "atmega328p",
            Protocol::Stk500v1,
            &[115200],
            false,
            "atmega328p.hex",
            vec![
                usb(0x2341, 0x0043, App),
                usb(0x2341, 0x0001, App),
                usb(0x2A03, 0x0043, App),
            ],
        ),
        b(
            "nano",
            "Nano (new bootloader)",
            "arduino:avr:nano:cpu=atmega328",
            "atmega328p",
            Protocol::Stk500v1,
            &[115200, 57600],
            false,
            "atmega328p.hex",
            clones(),
        ),
        b(
            "nano_old",
            "Nano (old bootloader)",
            "arduino:avr:nano:cpu=atmega328old",
            "atmega328p",
            Protocol::Stk500v1,
            &[57600],
            false,
            "atmega328p.hex",
            clones(),
        ),
        b(
            "pro_mini",
            "Pro Mini 5V",
            "arduino:avr:pro:cpu=16MHzatmega328",
            "atmega328p",
            Protocol::Stk500v1,
            &[57600],
            false,
            "atmega328p.hex",
            vec![],
        ),
        b(
            "mega",
            "Mega 2560",
            "arduino:avr:mega:cpu=atmega2560",
            "atmega2560",
            Protocol::Stk500v2,
            &[115200],
            false,
            "atmega2560.hex",
            vec![usb(0x2341, 0x0042, App), usb(0x2341, 0x0010, App)],
        ),
        b(
            "leonardo",
            "Leonardo",
            "arduino:avr:leonardo",
            "atmega32u4",
            Protocol::Avr109,
            &[57600],
            true,
            "atmega32u4.hex",
            vec![usb(0x2341, 0x8036, App), usb(0x2341, 0x0036, Bootloader)],
        ),
        b(
            "micro",
            "Micro",
            "arduino:avr:micro",
            "atmega32u4",
            Protocol::Avr109,
            &[57600],
            true,
            "atmega32u4.hex",
            vec![usb(0x2341, 0x8037, App), usb(0x2341, 0x0037, Bootloader)],
        ),
        b(
            "uno_r4_minima",
            "Uno R4 Minima",
            "arduino:renesas_uno:minima",
            "ra4m1",
            Protocol::Dfu,
            &[],
            true,
            "uno_r4_minima.bin",
            vec![usb(0x2341, 0x0069, App), usb(0x2341, 0x0369, Bootloader)],
        ),
        b(
            "uno_r4_wifi",
            "Uno R4 WiFi",
            "arduino:renesas_uno:unor4wifi",
            "ra4m1",
            Protocol::Samba,
            &[230400],
            true,
            "uno_r4_wifi.bin",
            vec![usb(0x2341, 0x1002, App)],
        ),
    ]
}

fn ser_hex16<S: Serializer>(v: &u16, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{v:04X}"))
}

fn de_hex16<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum HexOrNum {
        S(String),
        N(u16),
    }
    match HexOrNum::deserialize(d)? {
        HexOrNum::N(n) => Ok(n),
        HexOrNum::S(s) => {
            let t = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            u16::from_str_radix(t, 16).map_err(serde::de::Error::custom)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_boards_json_shape() {
        let json = r#"[{ "id": "uno", "name": "Uno R3", "fqbn": "arduino:avr:uno",
            "mcu": "atmega328p", "protocol": "stk500v1", "baud": [115200],
            "touch1200": false, "image": "atmega328p.hex",
            "usb": [{ "vid": "2341", "pid": "0043", "role": "app" },
                    { "vid": "0x2a03", "pid": "0043", "role": "app" }] }]"#;
        let boards: Vec<Board> = serde_json::from_str(json).unwrap();
        assert_eq!(boards[0].usb[1].vid, 0x2A03);
        assert_eq!(boards[0].protocol, Protocol::Stk500v1);
        let back = serde_json::to_string(&boards[0].usb[0]).unwrap();
        assert!(back.contains("\"2341\""));
    }

    #[test]
    fn builtin_table_is_consistent() {
        let boards = builtin_boards();
        assert_eq!(boards.len(), 9);
        for b in &boards {
            if matches!(
                b.protocol,
                Protocol::Stk500v1 | Protocol::Stk500v2 | Protocol::Avr109
            ) {
                assert!(mcu_info(&b.mcu).is_some(), "{}", b.id);
                assert!(!b.baud.is_empty(), "{}", b.id);
            }
        }
        let nano = boards.iter().find(|b| b.id == "nano").unwrap();
        assert_eq!(nano.baud, vec![115200, 57600]);
    }
}
