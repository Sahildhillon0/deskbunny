//! Test doubles: a fake machine whose serial ports talk to a simulated bootloader.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::link::{SerialHost, SerialLink};
use crate::ports::PortInfo;
use crate::{FlashError, Result};

/// A simulated device on the far end of the serial line.
pub trait Device: Send {
    /// Called on every open; return Err to simulate a missing/busy port.
    fn open(&mut self, _path: &str, _baud: u32) -> std::result::Result<(), String> {
        Ok(())
    }
    /// Bytes from host to device.
    fn write(&mut self, data: &[u8]);
    /// Bytes from device to host; returning 0 means "nothing within the timeout".
    fn read(&mut self, buf: &mut [u8]) -> usize;
    fn set_dtr(&mut self, _level: bool) {}
    fn set_rts(&mut self, _level: bool) {}
    fn clear_input(&mut self) {}
    /// Simulated flash contents, for assertions.
    fn flash_snapshot(&self) -> Option<Vec<u8>> {
        None
    }
}

/// Input accumulator + output queue shared by the protocol simulators.
#[derive(Default)]
pub struct Pipe {
    pub input: Vec<u8>,
    pub output: VecDeque<u8>,
}

impl Pipe {
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.output.len());
        for b in buf.iter_mut().take(n) {
            *b = self.output.pop_front().unwrap();
        }
        n
    }
    pub fn reply(&mut self, bytes: &[u8]) {
        self.output.extend(bytes.iter().copied());
    }
}

/// Replies `response` whenever `trigger` has been written. Used for simple cases.
#[derive(Default)]
pub struct ScriptedDevice {
    trigger: Vec<u8>,
    response: Vec<u8>,
    pipe: Pipe,
}

impl ScriptedDevice {
    pub fn replying(trigger: &[u8], response: &[u8]) -> Self {
        ScriptedDevice {
            trigger: trigger.to_vec(),
            response: response.to_vec(),
            pipe: Pipe::default(),
        }
    }
}

impl Device for ScriptedDevice {
    fn write(&mut self, data: &[u8]) {
        self.pipe.input.extend_from_slice(data);
        if !self.trigger.is_empty()
            && self
                .pipe
                .input
                .windows(self.trigger.len())
                .any(|w| w == self.trigger)
        {
            self.pipe.input.clear();
            let r = self.response.clone();
            self.pipe.reply(&r);
        }
    }
    fn read(&mut self, buf: &mut [u8]) -> usize {
        self.pipe.read(buf)
    }
}

pub type SharedDevice = Arc<Mutex<Box<dyn Device>>>;

pub struct MockHost {
    pub dev: SharedDevice,
    listings: Mutex<VecDeque<Vec<PortInfo>>>,
    default_listing: Mutex<Vec<PortInfo>>,
    pub opens: Mutex<Vec<(String, u32)>>,
}

impl MockHost {
    pub fn new(dev: Box<dyn Device>) -> Self {
        MockHost {
            dev: Arc::new(Mutex::new(dev)),
            listings: Mutex::new(VecDeque::new()),
            default_listing: Mutex::new(Vec::new()),
            opens: Mutex::new(Vec::new()),
        }
    }
    /// Queue one result for the next `list()` call.
    pub fn push_listing(&self, ports: Vec<PortInfo>) {
        self.listings.lock().unwrap().push_back(ports);
    }
    pub fn flash(&self) -> Vec<u8> {
        self.dev
            .lock()
            .unwrap()
            .flash_snapshot()
            .expect("device has no flash")
    }
    /// Result for `list()` once the queue is empty.
    pub fn set_default_listing(&self, ports: Vec<PortInfo>) {
        *self.default_listing.lock().unwrap() = ports;
    }
}

impl SerialHost for MockHost {
    fn list(&self) -> Result<Vec<PortInfo>> {
        if let Some(l) = self.listings.lock().unwrap().pop_front() {
            return Ok(l);
        }
        Ok(self.default_listing.lock().unwrap().clone())
    }

    fn open(&self, path: &str, baud: u32, _timeout: Duration) -> Result<Box<dyn SerialLink>> {
        self.opens.lock().unwrap().push((path.to_string(), baud));
        self.dev
            .lock()
            .unwrap()
            .open(path, baud)
            .map_err(FlashError::Port)?;
        Ok(Box::new(MockLink {
            dev: self.dev.clone(),
        }))
    }

    fn sleep(&self, _d: Duration) {}
}

pub struct MockLink {
    dev: SharedDevice,
}

impl Read for MockLink {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.dev.lock().unwrap().read(buf) {
            0 => Err(io::Error::new(io::ErrorKind::TimedOut, "mock timeout")),
            n => Ok(n),
        }
    }
}

impl Write for MockLink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.dev.lock().unwrap().write(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SerialLink for MockLink {
    fn set_dtr(&mut self, level: bool) -> io::Result<()> {
        self.dev.lock().unwrap().set_dtr(level);
        Ok(())
    }
    fn set_rts(&mut self, level: bool) -> io::Result<()> {
        self.dev.lock().unwrap().set_rts(level);
        Ok(())
    }
    fn set_timeout(&mut self, _timeout: Duration) -> io::Result<()> {
        Ok(())
    }
    fn clear_input(&mut self) -> io::Result<()> {
        self.dev.lock().unwrap().clear_input();
        Ok(())
    }
}

/// Simulated flash memory with page-granular writes.
pub struct Flash {
    pub mem: Vec<u8>,
}

impl Flash {
    pub fn new(size: usize) -> Self {
        Flash {
            mem: vec![0xFF; size],
        }
    }
    pub fn write(&mut self, addr: usize, data: &[u8]) {
        self.mem[addr..addr + data.len()].copy_from_slice(data);
    }
    pub fn read(&self, addr: usize, len: usize) -> &[u8] {
        &self.mem[addr..addr + len]
    }
}

/// A recognisable test image: `len` bytes at `base` with a face blob somewhere inside.
pub fn test_image(base: u32, len: usize) -> crate::FirmwareImage {
    let mut data: Vec<u8> = (0..len).map(|i| (i * 31 + 7) as u8).collect();
    let at = len / 3;
    data[at..at + 8].copy_from_slice(crate::image::FACE_MAGIC);
    crate::FirmwareImage::from_bin(base, &data)
}

/// Collects progress events for assertions.
#[derive(Default)]
pub struct ProgressLog(pub Mutex<Vec<crate::Progress>>);

impl ProgressLog {
    pub fn cb(&self) -> impl Fn(crate::Progress) + '_ {
        move |p| self.0.lock().unwrap().push(p)
    }
    pub fn assert_sane(&self) {
        let v = self.0.lock().unwrap();
        assert!(!v.is_empty());
        assert!(
            v.windows(2).all(|w| w[0].percent <= w[1].percent),
            "progress went backwards"
        );
        let last = v.last().unwrap();
        assert_eq!(last.stage, crate::Stage::Done);
        assert_eq!(last.percent, 100);
    }
}
