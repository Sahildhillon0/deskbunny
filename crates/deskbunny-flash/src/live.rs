//! Live link client (docs/ARCHITECTURE.md, "Live link v1"): one open port to a
//! running DeskBunny, driving its screen. The caller keeps the link for as long
//! as it wants control, because opening the port resets most boards.

use std::io;
use std::time::{Duration, Instant};

use crate::link::{io_err, write_all, SerialHost, SerialLink};
use crate::{FlashError, Result};

/// How long a command may take to answer (a frame upload is ~90 ms at 115200).
const REPLY_TIMEOUT: Duration = Duration::from_millis(1500);

pub struct LiveLink {
    link: Box<dyn SerialLink>,
    /// `DESKBUNNY <fw> <board> <face name>`
    pub banner: String,
    /// The board has Clawd (`!V` answered `LIVE 1 C`).
    pub has_clawd: bool,
    pending: Vec<u8>,
}

impl LiveLink {
    /// Open `port`, wait for the board to boot (it resets on open) and check
    /// that its firmware has the live link.
    pub fn open(host: &dyn SerialHost, port: &str) -> Result<LiveLink> {
        let link = host.open(port, 115200, Duration::from_millis(50))?;
        Self::start(link, Duration::from_secs(5))
    }

    fn start(link: Box<dyn SerialLink>, boot: Duration) -> Result<LiveLink> {
        let mut l = LiveLink {
            link,
            banner: String::new(),
            has_clawd: false,
            pending: Vec::new(),
        };
        let start = Instant::now();
        let mut last_ask: Option<Instant> = None;
        while start.elapsed() < boot {
            if last_ask.is_none_or(|t| t.elapsed() >= Duration::from_millis(500)) {
                write_all(l.link.as_mut(), b"?\n")?;
                last_ask = Some(Instant::now());
            }
            if let Some(line) = l.read_line(Duration::from_millis(100))? {
                if let Some(i) = line.find("DESKBUNNY ") {
                    l.banner = line[i..].to_string();
                    break;
                }
            }
        }
        if l.banner.is_empty() {
            return Err(FlashError::NoResponse(
                "The board did not answer. Is DeskBunny flashed on it, and is this the right port?"
                    .into(),
            ));
        }
        // Drop banners still queued from the repeated "?".
        while l.read_line(Duration::from_millis(150))?.is_some() {}
        let v = l.command("!V")?;
        if !v.starts_with("LIVE 1") {
            return Err(FlashError::Unsupported(
                "This board's firmware has no live mode. Flash it again with this version of DeskBunny."
                    .into(),
            ));
        }
        l.has_clawd = v.split_whitespace().any(|w| w == "C");
        Ok(l)
    }

    /// One reply line, or None if nothing complete arrived within `timeout`.
    fn read_line(&mut self, timeout: Duration) -> Result<Option<String>> {
        let start = Instant::now();
        let mut buf = [0u8; 64];
        loop {
            if let Some(i) = self.pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=i).collect();
                return Ok(Some(String::from_utf8_lossy(&line).trim().to_string()));
            }
            if start.elapsed() >= timeout {
                return Ok(None);
            }
            match self.link.read(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(2)),
                Ok(n) => {
                    self.pending.extend_from_slice(&buf[..n]);
                    if self.pending.len() > 512 {
                        // A runaway line without a newline: keep the tail only.
                        let cut = self.pending.len() - 256;
                        self.pending.drain(..cut);
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(io_err("Serial read failed", e)),
            }
        }
    }

    /// Send one command line and return the reply, skipping stray banners.
    fn command(&mut self, line: &str) -> Result<String> {
        let mut out = line.as_bytes().to_vec();
        out.push(b'\n');
        write_all(self.link.as_mut(), &out)?;
        self.reply()
    }

    fn reply(&mut self) -> Result<String> {
        let start = Instant::now();
        while start.elapsed() < REPLY_TIMEOUT {
            match self.read_line(REPLY_TIMEOUT.saturating_sub(start.elapsed()))? {
                Some(l) if l.starts_with("DESKBUNNY ") || l.is_empty() => continue,
                Some(l) => return Ok(l),
                None => break,
            }
        }
        Err(FlashError::NoResponse(
            "The board stopped answering.".into(),
        ))
    }

    fn expect_ok(&mut self, line: &str) -> Result<()> {
        match self.command(line)?.as_str() {
            "OK" => Ok(()),
            other => Err(FlashError::Protocol(format!(
                "The board refused `{line}` ({other})."
            ))),
        }
    }

    /// Keepalive. False means the board is not in live mode (it timed out or
    /// rebooted): send the screen again.
    pub fn ping(&mut self) -> Result<bool> {
        Ok(self.command("!P")? == "OK")
    }

    /// Back to the flashed face.
    pub fn home(&mut self) -> Result<()> {
        self.expect_ok("!H")
    }

    /// Engine face with this mood and overlay (enum values), optional caption.
    pub fn face(&mut self, mood: u8, overlay: u8, caption: &str) -> Result<()> {
        self.expect_ok(&with_caption(format!("!M {mood} {overlay}"), caption))
    }

    /// Clawd (anim and expression enum values), optional caption.
    pub fn clawd(&mut self, anim: u8, expr: u8, caption: &str) -> Result<()> {
        self.expect_ok(&with_caption(format!("!C {anim} {expr}"), caption))
    }

    /// A PackBits frame: whole (`!F`) or XOR-ed onto the one shown (`!X`).
    pub fn frame(&mut self, packed: &[u8], xor: bool) -> Result<()> {
        if packed.len() < 2 || packed.len() > 1200 {
            return Err(FlashError::Protocol(format!(
                "Frame of {} bytes is out of range.",
                packed.len()
            )));
        }
        let cmd = format!("!{} {}", if xor { 'X' } else { 'F' }, packed.len());
        let r = self.command(&cmd)?;
        if r != "K" {
            return Err(FlashError::Protocol(format!(
                "The board refused a frame ({r})."
            )));
        }
        write_all(self.link.as_mut(), packed)?;
        match self.reply()?.as_str() {
            "OK" => Ok(()),
            other => Err(FlashError::Protocol(format!(
                "The board rejected a frame ({other})."
            ))),
        }
    }
}

/// The board takes lines of at most 39 characters; the caption is cut to fit
/// (callers normally pass one already normalised to 21 characters).
fn with_caption(mut cmd: String, caption: &str) -> String {
    let caption = caption.trim_end();
    if !caption.is_empty() {
        cmd.push(' ');
        for c in caption.chars() {
            if cmd.len() >= 39 {
                break;
            }
            cmd.push(if (' '..='z').contains(&c) { c } else { ' ' });
        }
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{Device, MockHost, Pipe};
    use std::sync::{Arc, Mutex};

    /// The firmware's live link, simulated: enough to check the client.
    #[derive(Default)]
    struct Board {
        pipe: Pipe,
        line: Vec<u8>,
        expect: usize,
        payload: Vec<u8>,
        clawd: bool,
        log: Arc<Mutex<Vec<String>>>,
        reject_frames: bool,
    }

    fn unpack(p: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < p.len() {
            let t = p[i] as usize;
            i += 1;
            if t < 128 {
                out.extend_from_slice(p.get(i..i + t + 1)?);
                i += t + 1;
            } else {
                out.extend(std::iter::repeat_n(*p.get(i)?, t - 126));
                i += 1;
            }
        }
        Some(out)
    }

    impl Device for Board {
        fn write(&mut self, data: &[u8]) {
            for &b in data {
                if self.expect > 0 {
                    self.payload.push(b);
                    self.expect -= 1;
                    if self.expect == 0 {
                        let ok = unpack(&self.payload).is_some_and(|f| f.len() == 1024);
                        self.log
                            .lock()
                            .unwrap()
                            .push(format!("frame {}", self.payload.len()));
                        self.pipe.reply(if ok && !self.reject_frames {
                            b"OK\r\n"
                        } else {
                            b"ERR\r\n"
                        });
                        self.payload.clear();
                    }
                    continue;
                }
                if b != b'\n' {
                    self.line.push(b);
                    continue;
                }
                let l = String::from_utf8_lossy(&self.line).to_string();
                self.line.clear();
                self.log.lock().unwrap().push(l.clone());
                let reply: &[u8] = match l.as_str() {
                    "?" => b"DESKBUNNY 0.1.14 atmega328p BUNNY\r\n",
                    "!V" if self.clawd => b"LIVE 1 C\r\n",
                    "!V" => b"LIVE 1\r\n",
                    "!P" | "!H" => b"OK\r\n",
                    s if s.starts_with("!M ") || s.starts_with("!C ") => b"OK\r\n",
                    s if s.starts_with("!F ") || s.starts_with("!X ") => {
                        self.expect = s[3..].parse().unwrap();
                        b"K\r\n"
                    }
                    _ => b"ERR\r\n",
                };
                self.pipe.reply(reply);
            }
        }
        fn read(&mut self, buf: &mut [u8]) -> usize {
            self.pipe.read(buf)
        }
    }

    fn board(clawd: bool, reject_frames: bool) -> (MockHost, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let b = Board {
            clawd,
            reject_frames,
            log: log.clone(),
            ..Default::default()
        };
        (MockHost::new(Box::new(b)), log)
    }

    #[test]
    fn opens_and_drives_the_screen() {
        let (host, log) = board(true, false);
        let mut l = LiveLink::open(&host, "/dev/ttyUSB0").unwrap();
        assert_eq!(l.banner, "DESKBUNNY 0.1.14 atmega328p BUNNY");
        assert!(l.has_clawd);
        l.clawd(2, 1, "DONE 3m").unwrap();
        l.face(1, 6, "").unwrap();
        // 1024 zero bytes pack to 8 runs of 129 minus a bit: 16 bytes
        let packed = [
            255u8, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 247, 0,
        ];
        l.frame(&packed, false).unwrap();
        l.frame(&packed, true).unwrap();
        assert!(l.ping().unwrap());
        l.home().unwrap();
        let log = log.lock().unwrap();
        assert!(log.contains(&"!C 2 1 DONE 3m".to_string()), "{log:?}");
        assert!(log.contains(&"!M 1 6".to_string()), "{log:?}");
        assert!(log.contains(&"!X 16".to_string()), "{log:?}");
        assert_eq!(log.iter().filter(|s| *s == "frame 16").count(), 2);
    }

    #[test]
    fn no_clawd_on_small_boards() {
        let (host, _) = board(false, false);
        assert!(!LiveLink::open(&host, "COM3").unwrap().has_clawd);
    }

    #[test]
    fn rejected_frame_is_an_error() {
        let (host, _) = board(true, true);
        let mut l = LiveLink::open(&host, "COM3").unwrap();
        let packed = [
            255u8, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 247, 0,
        ];
        let e = l.frame(&packed, false).unwrap_err().to_string();
        assert!(e.contains("rejected"), "{e}");
    }

    #[test]
    fn caption_fits_the_line_buffer() {
        let s = with_caption("!C 5 7".into(), &"W".repeat(40));
        assert_eq!(s.len(), 39);
        assert_eq!(with_caption("!M 1 2".into(), "  "), "!M 1 2");
        assert_eq!(with_caption("!M 1 2".into(), "a~b"), "!M 1 2 a b");
    }
}
