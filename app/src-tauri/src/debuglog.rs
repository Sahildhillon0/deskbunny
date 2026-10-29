//! In-app debug log: the last lines from the `log` crate, kept in memory and
//! streamed to the Debug log panel. Contract: docs/ARCHITECTURE.md ("Debug log").

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use log::{Level, LevelFilter, Log, Metadata, Record};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const MAX_LINES: usize = 5000;

#[derive(Clone, Serialize)]
pub struct LogEntry {
    /// Milliseconds since the app started.
    t: u64,
    level: &'static str,
    target: String,
    msg: String,
}

struct DebugLog {
    start: Instant,
    lines: Mutex<VecDeque<LogEntry>>,
    app: OnceLock<AppHandle>,
}

static LOG: OnceLock<DebugLog> = OnceLock::new();

fn wanted(target: &str, level: Level) -> bool {
    // Ours and nusb's USB enumeration in full; other crates only when something is wrong.
    if target.starts_with("deskbunny") || target.starts_with("nusb") {
        level <= Level::Debug
    } else {
        level <= Level::Warn
    }
}

impl Log for DebugLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        wanted(metadata.target(), metadata.level())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let entry = LogEntry {
            t: self.start.elapsed().as_millis() as u64,
            level: match record.level() {
                Level::Error => "error",
                Level::Warn => "warn",
                Level::Info => "info",
                Level::Debug => "debug",
                Level::Trace => "trace",
            },
            target: record.target().to_string(),
            msg: record.args().to_string(),
        };
        {
            let mut lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
            if lines.len() == MAX_LINES {
                lines.pop_front();
            }
            lines.push_back(entry.clone());
        }
        // Emit outside the lock: emitting may itself log.
        if let Some(app) = self.app.get() {
            let _ = app.emit("debug-log", entry);
        }
    }

    fn flush(&self) {}
}

/// Installs the logger. Call once, before anything logs.
pub fn init() {
    let log = LOG.get_or_init(|| DebugLog {
        start: Instant::now(),
        lines: Mutex::new(VecDeque::new()),
        app: OnceLock::new(),
    });
    if log::set_logger(log).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

/// Starts streaming new lines to the webview.
pub fn attach(app: &AppHandle) {
    if let Some(log) = LOG.get() {
        let _ = log.app.set(app.clone());
    }
}

pub fn lines() -> Vec<LogEntry> {
    LOG.get()
        .map(|l| {
            l.lines
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn clear() {
    if let Some(l) = LOG.get() {
        l.lines.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}
