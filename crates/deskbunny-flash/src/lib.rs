//! DeskBunny flasher: every Arduino upload protocol DeskBunny needs, in pure Rust.
//!
//! - [`image`]: Intel HEX / raw binary images and face blob patching.
//! - [`ports`]: serial port discovery and board guessing.
//! - [`boards`]: the board table (`firmware/boards.json` shape) and MCU facts.
//! - Protocol backends behind [`Uploader`]: [`stk500v1`], [`stk500v2`], [`avr109`],
//!   [`samba`], [`dfu`].
//!
//! Contract: docs/ARCHITECTURE.md.

pub mod avr109;
pub mod boards;
pub mod dfu;
pub mod image;
pub mod link;
pub mod ports;
pub mod samba;
pub mod stk500v1;
pub mod stk500v2;

#[cfg(test)]
mod mock;

use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

pub use boards::{Board, Protocol};
pub use image::{patch_art, patch_face, FirmwareImage};
pub use link::{SerialHost, SerialLink, SystemHost};
pub use ports::{list_ports, PortInfo};

/// Every error carries a sentence a user can act on.
#[derive(Debug, thiserror::Error)]
pub enum FlashError {
    #[error("{0}")]
    Image(String),
    #[error("{0}")]
    Port(String),
    #[error("{0}")]
    NoResponse(String),
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    WrongChip(String),
    #[error("{0}")]
    Verify(String),
    #[error("{0}")]
    Usb(String),
    #[error("{0}")]
    Unsupported(String),
}

pub type Result<T, E = FlashError> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Reset,
    Sync,
    Erase,
    Write,
    Verify,
    Done,
}

/// Payload of the `flash-progress` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub stage: Stage,
    /// Overall progress, 0..=100.
    pub percent: u8,
    pub message: String,
}

/// What an upload did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub protocol: String,
    /// Port the bootloader was reached on (differs from the requested port after a
    /// 1200-baud touch on some systems).
    pub port: String,
    pub baud: Option<u32>,
    /// Device signature as hex, when the protocol reports one.
    pub signature: Option<String>,
    pub bytes_written: usize,
    pub verified: bool,
    pub duration_ms: u64,
    pub warnings: Vec<String>,
}

/// A protocol backend.
pub trait Uploader: Send + Sync {
    fn upload(
        &self,
        port: &str,
        image: &FirmwareImage,
        progress: &dyn Fn(Progress),
    ) -> Result<Report>;
}

/// Pick the backend for a board.
pub fn uploader_for(board: &Board, host: Arc<dyn SerialHost>) -> Box<dyn Uploader> {
    match board.protocol {
        Protocol::Stk500v1 => Box::new(stk500v1::Stk500v1::new(board.clone(), host)),
        Protocol::Stk500v2 => Box::new(stk500v2::Stk500v2::new(board.clone(), host)),
        Protocol::Avr109 => Box::new(avr109::Avr109::new(board.clone(), host)),
        Protocol::Samba => Box::new(samba::Samba::new(board.clone(), host)),
        Protocol::Dfu => Box::new(dfu::Dfu::new(board.clone(), host)),
    }
}

/// Progress reporter that maps stage-local fractions onto one 0..100 bar.
/// The bar never moves backwards, even when a backend retries at another baud.
pub(crate) struct Prog<'a> {
    cb: &'a dyn Fn(Progress),
    started: Instant,
    last: Cell<u8>,
}

impl<'a> Prog<'a> {
    pub(crate) fn new(cb: &'a dyn Fn(Progress)) -> Self {
        Prog {
            cb,
            started: Instant::now(),
            last: Cell::new(0),
        }
    }

    fn range(stage: Stage) -> (u8, u8) {
        match stage {
            Stage::Reset => (0, 4),
            Stage::Sync => (4, 8),
            Stage::Erase => (8, 12),
            Stage::Write => (12, 70),
            Stage::Verify => (70, 99),
            Stage::Done => (100, 100),
        }
    }

    pub(crate) fn emit(&self, stage: Stage, frac: f32, message: impl Into<String>) {
        let (lo, hi) = Self::range(stage);
        let pct = (lo as f32 + (hi - lo) as f32 * frac.clamp(0.0, 1.0)).round() as u8;
        let pct = pct.max(self.last.get());
        self.last.set(pct);
        (self.cb)(Progress {
            stage,
            percent: pct,
            message: message.into(),
        });
    }

    pub(crate) fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

pub(crate) fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

/// Compare a read-back page; returns a user-facing verify error on mismatch.
pub(crate) fn check_verify(addr: u32, expected: &[u8], actual: &[u8]) -> Result<()> {
    if let Some(i) = expected.iter().zip(actual).position(|(a, b)| a != b) {
        return Err(FlashError::Verify(format!(
            "Verify failed at 0x{:05X}: wrote 0x{:02X}, read back 0x{:02X}. The board may have a faulty bootloader or flash; try uploading again.",
            addr + i as u32,
            expected[i],
            actual[i]
        )));
    }
    if expected.len() != actual.len() {
        return Err(FlashError::Verify(format!(
            "Verify failed at 0x{addr:05X}: read {} bytes, expected {}.",
            actual.len(),
            expected.len()
        )));
    }
    Ok(())
}
