//! Live mode (docs/ARCHITECTURE.md, "Live mode"): while enabled, a worker
//! thread owns the board's serial port and drives its screen: the flashed face,
//! designed screens (clock, weather, ...) and the Claude screen, in rotation
//! or pinned. Claude Code hooks
//! (via `server.rs`) feed `claude::Tracker`, which can take the screen over.
//!
//! Opening the port resets most boards, so the worker keeps it open. Anything
//! else that needs the port (flashing, device_info) takes a [`Pause`] first;
//! the worker sends the board home, closes the port and waits.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, Timelike};
use deskbunny_core::drawing::pack;
use deskbunny_core::layout::{self, Design};
use deskbunny_core::live::{self as screens, ClawdAnim, ClawdExpr, ClockTime};
use deskbunny_core::FB_SIZE;
use deskbunny_flash::{LiveLink, SystemHost};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::claude::{ClaudeState, ClaudeView, Tracker};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenKind {
    Face,
    Claude,
    /// A designed screen (`design` names it). "clock" is the old name of the
    /// built-in clock screen, now the design "clock".
    #[serde(alias = "clock")]
    Design,
    /// A notification on screen (not part of the rotation).
    Notify,
}

/// A screen: a kind, plus the design id for `Design`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenRef {
    pub kind: ScreenKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,
}

impl ScreenRef {
    pub fn of(kind: ScreenKind) -> ScreenRef {
        ScreenRef { kind, design: None }
    }
    #[cfg(test)]
    fn design(id: &str) -> ScreenRef {
        ScreenRef {
            kind: ScreenKind::Design,
            design: Some(id.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenCfg {
    pub kind: ScreenKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,
    pub enabled: bool,
    /// Seconds on screen per turn of the rotation, 3..600.
    pub seconds: u16,
}

impl ScreenCfg {
    pub fn screen(&self) -> ScreenRef {
        ScreenRef {
            kind: self.kind,
            design: self.design.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveConfig {
    pub enabled: bool,
    pub port: Option<String>,
    /// Rotation order.
    pub screens: Vec<ScreenCfg>,
    /// How the Claude screen draws Claude: the crab or the face.
    pub claude_style: ClaudeStyle,
    /// Keep the Claude screen up the whole time a session works or waits for
    /// you (not just the first seconds); the rotation resumes after "done".
    pub claude_hold: bool,
    pub notify: NotifyCfg,
    pub weather: crate::weather::WeatherCfg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotifyCfg {
    /// Show notifications at all (from `/notify` and the desktop).
    pub enabled: bool,
    /// Also pick up the desktop's own notifications (Linux, D-Bus).
    pub desktop: bool,
    /// Seconds each one stays up, 2..60.
    pub seconds: u16,
    /// Show the message text, not just the title (others can see the screen).
    pub show_body: bool,
    /// App names (case-insensitive) whose notifications are skipped.
    pub ignore_apps: Vec<String>,
}

impl Default for NotifyCfg {
    fn default() -> Self {
        NotifyCfg {
            enabled: true,
            desktop: true,
            seconds: 8,
            show_body: false,
            ignore_apps: Vec::new(),
        }
    }
}

/// One notification waiting for (or on) the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub app: String,
    pub title: String,
    pub body: String,
}

/// Most notifications queued; older ones are dropped.
const NOTES_MAX: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeStyle {
    #[default]
    Clawd,
    Face,
}

impl Default for LiveConfig {
    fn default() -> Self {
        LiveConfig {
            enabled: false,
            port: None,
            screens: vec![
                ScreenCfg {
                    kind: ScreenKind::Face,
                    design: None,
                    enabled: true,
                    seconds: 20,
                },
                ScreenCfg {
                    kind: ScreenKind::Design,
                    design: Some("clock".into()),
                    enabled: true,
                    seconds: 15,
                },
                ScreenCfg {
                    kind: ScreenKind::Claude,
                    design: None,
                    enabled: true,
                    seconds: 15,
                },
            ],
            claude_style: ClaudeStyle::Clawd,
            claude_hold: true,
            notify: NotifyCfg::default(),
            weather: Default::default(),
        }
    }
}

impl LiveConfig {
    /// The face and Claude screens exactly once (missing ones appended
    /// disabled), each design at most once (a design entry without an id is
    /// the old clock), first entry wins; dwell times clamped; empty port = none.
    pub fn sanitized(mut self) -> LiveConfig {
        let mut out: Vec<ScreenCfg> = Vec::new();
        for mut s in self.screens.drain(..) {
            match s.kind {
                ScreenKind::Notify => continue,
                ScreenKind::Design => {
                    let id = s.design.take().filter(|d| !d.trim().is_empty());
                    s.design = Some(id.unwrap_or_else(|| "clock".into()));
                }
                _ => s.design = None,
            }
            if out.iter().any(|o| o.screen() == s.screen()) {
                continue;
            }
            s.seconds = s.seconds.clamp(3, 600);
            out.push(s);
        }
        for k in [ScreenKind::Face, ScreenKind::Claude] {
            if !out.iter().any(|o| o.kind == k) {
                out.push(ScreenCfg {
                    kind: k,
                    design: None,
                    enabled: false,
                    seconds: 15,
                });
            }
        }
        self.screens = out;
        self.notify.seconds = self.notify.seconds.clamp(2, 60);
        self.notify.ignore_apps = self
            .notify
            .ignore_apps
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        self.port = self
            .port
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LiveState {
    #[default]
    Off,
    /// Another command (flashing) has the port.
    Paused,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub state: LiveState,
    pub port: Option<String>,
    /// `DESKBUNNY ...` line of the connected board.
    pub banner: Option<String>,
    /// The board can draw Clawd (otherwise the Claude screen uses the face).
    pub clawd: bool,
    /// Screen on the board now.
    pub screen: Option<ScreenRef>,
    pub pinned: Option<ScreenRef>,
    /// Why it is in `error` (a sentence for the user).
    pub message: Option<String>,
}

struct Inner {
    cfg: LiveConfig,
    pinned: Option<ScreenRef>,
    pauses: u32,
    /// The worker has the port open.
    holding: bool,
    status: LiveStatus,
}

pub struct Live {
    inner: Mutex<Inner>,
    claude: Mutex<Tracker>,
    /// Notifications to show, oldest first; the front one is on screen
    /// (with the time it went up) once the worker gets to it.
    notes: Mutex<(std::collections::VecDeque<Note>, Option<Instant>)>,
    /// Designed screens, saved in `<app data>/designs.json`.
    designs: Mutex<Vec<Design>>,
    designs_path: Option<PathBuf>,
    weather: Mutex<crate::weather::WeatherNow>,
    cv: Condvar,
    config_path: Option<PathBuf>,
}

/// While alive, live mode keeps its hands off the serial port.
pub struct Pause(Arc<Live>);

impl Drop for Pause {
    fn drop(&mut self) {
        let mut g = self.0.lock();
        g.pauses = g.pauses.saturating_sub(1);
        self.0.cv.notify_all();
    }
}

impl Live {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic in the worker must not take the app down with it.
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn config(&self) -> LiveConfig {
        self.lock().cfg.clone()
    }

    pub fn status(&self) -> LiveStatus {
        self.lock().status.clone()
    }

    pub fn set_config(&self, cfg: LiveConfig) -> Result<LiveConfig, String> {
        let cfg = cfg.sanitized();
        if let Some(path) = &self.config_path {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
            }
            let text = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
            std::fs::write(path, text)
                .map_err(|e| format!("Could not save {}: {e}", path.display()))?;
        }
        self.lock().cfg = cfg.clone();
        self.cv.notify_all();
        Ok(cfg)
    }

    /// One Claude Code hook payload; false if it was not one.
    pub fn claude_event(&self, payload: &serde_json::Value) -> bool {
        // When a prompt finishes, time it from the transcript (outside the lock).
        let worked = (payload.get("hook_event_name").and_then(|v| v.as_str()) == Some("Stop"))
            .then(|| payload.get("transcript_path").and_then(|v| v.as_str()))
            .flatten()
            .and_then(|p| crate::claude::prompt_age(p, std::time::SystemTime::now()));
        let ok = self
            .claude
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .event_timed(payload, Instant::now(), worked);
        if ok {
            self.cv.notify_all();
        }
        ok
    }

    pub fn claude_view(&self) -> ClaudeView {
        let mut t = self.claude.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        t.tick(now);
        t.view(now)
    }

    /// Queue a notification (dropped when notifications are off or the app is
    /// ignored).
    pub fn notify(&self, app: &str, title: &str, body: &str) {
        let cfg = self.config().notify;
        let ignored = cfg
            .ignore_apps
            .iter()
            .any(|a| a.eq_ignore_ascii_case(app.trim()));
        if !cfg.enabled || ignored || title.trim().is_empty() {
            return;
        }
        let note = Note {
            app: app.trim().to_string(),
            title: title.trim().to_string(),
            body: body.trim().to_string(),
        };
        let mut q = self.notes.lock().unwrap_or_else(|p| p.into_inner());
        if q.0.back() == Some(&note) {
            return; // the same thing twice in a row (apps love to repeat)
        }
        q.0.push_back(note);
        while q.0.len() > NOTES_MAX {
            q.0.pop_front();
            q.1 = None;
        }
        drop(q);
        self.cv.notify_all();
    }

    /// The notification to show now, retiring the current one after
    /// `seconds`.
    fn current_note(&self, seconds: u16) -> Option<Note> {
        let mut q = self.notes.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        if let Some(t) = q.1 {
            if now.duration_since(t) >= Duration::from_secs(u64::from(seconds)) {
                q.0.pop_front();
                q.1 = None;
            }
        }
        let front = q.0.front().cloned();
        if front.is_some() && q.1.is_none() {
            q.1 = Some(now);
        }
        front
    }

    pub fn designs(&self) -> Vec<Design> {
        self.designs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Replace the stored designs and write them to disk.
    pub fn set_designs(&self, designs: Vec<Design>) -> Result<(), String> {
        if let Some(path) = &self.designs_path {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
            }
            let text = serde_json::to_string_pretty(&designs).map_err(|e| e.to_string())?;
            std::fs::write(path, text)
                .map_err(|e| format!("Could not save {}: {e}", path.display()))?;
        }
        *self.designs.lock().unwrap_or_else(|p| p.into_inner()) = designs;
        self.cv.notify_all();
        Ok(())
    }

    pub fn weather_now(&self) -> crate::weather::WeatherNow {
        self.weather
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_weather(&self, w: crate::weather::WeatherNow) {
        *self.weather.lock().unwrap_or_else(|p| p.into_inner()) = w;
        self.cv.notify_all();
    }

    /// The design `id` as it goes to the board (with the Claude badge), or
    /// None if there is no such design.
    fn design_frame(&self, id: &str, v: &ClaudeView) -> Option<[u8; FB_SIZE]> {
        let d = self.designs().into_iter().find(|d| d.id == id)?;
        let (mut fb, _) = self.render_design(&d);
        badge(&mut fb, v, Local::now().second() as u8);
        Some(fb)
    }

    /// Render `d` as the board would show it now.
    pub fn render_design(&self, d: &Design) -> ([u8; FB_SIZE], Vec<layout::Bounds>) {
        let t = Local::now();
        let w = self.weather_now().weather;
        let v = self.claude_view();
        let ctx = layout::Ctx {
            time: clock_of(&t),
            year: t.year().clamp(0, 9999) as u16,
            weather: w.as_ref(),
            claude: &v.caption,
        };
        layout::render(d, &ctx)
    }

    pub fn pin(&self, screen: Option<ScreenRef>) {
        self.lock().pinned = screen;
        self.cv.notify_all();
    }

    /// Take the port away from live mode until the returned guard drops.
    /// Waits (up to 8 s: an open in progress can take 5) for the port to close.
    pub fn pause(self: &Arc<Self>) -> Pause {
        let mut g = self.lock();
        g.pauses += 1;
        self.cv.notify_all();
        let deadline = Instant::now() + Duration::from_secs(8);
        while g.holding {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                log::warn!("live: port still held after 8 s; going ahead anyway");
                break;
            }
            g = self
                .cv
                .wait_timeout(g, left)
                .map(|r| r.0)
                .unwrap_or_else(|p| p.into_inner().0);
        }
        drop(g);
        Pause(self.clone())
    }
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("live.json"))
}

/// Saved designs, or the built-in ones on first run (or if the file is bad).
fn load_designs(path: Option<&PathBuf>) -> Vec<Design> {
    path.and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Vec<Design>>(&t).ok())
        .unwrap_or_else(layout::templates)
}

fn load_config(path: Option<&PathBuf>) -> LiveConfig {
    path.and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<LiveConfig>(&t).ok())
        .unwrap_or_default()
        .sanitized()
}

/// Create the live state and start its worker.
pub fn start(app: &AppHandle) -> Arc<Live> {
    let path = config_path(app);
    let designs_path = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("designs.json"));
    let live = Arc::new(Live {
        inner: Mutex::new(Inner {
            cfg: load_config(path.as_ref()),
            pinned: None,
            pauses: 0,
            holding: false,
            status: LiveStatus::default(),
        }),
        claude: Mutex::new(Tracker::default()),
        notes: Mutex::new((std::collections::VecDeque::new(), None)),
        designs: Mutex::new(load_designs(designs_path.as_ref())),
        designs_path,
        weather: Mutex::new(Default::default()),
        cv: Condvar::new(),
        config_path: path,
    });
    let (app2, live2) = (app.clone(), live.clone());
    std::thread::Builder::new()
        .name("deskbunny-live".into())
        .spawn(move || Worker::new(app2, live2).run())
        .expect("spawn live worker");
    live
}

// --- rendering --------------------------------------------------------------

fn clock_of(t: &chrono::DateTime<Local>) -> ClockTime {
    ClockTime {
        hour: t.hour() as u8,
        minute: t.minute() as u8,
        second: t.second() as u8,
        weekday: t.weekday().num_days_from_monday() as u8,
        day: t.day() as u8,
        month: t.month() as u8,
    }
}

/// Clawd's animation and expression for a Claude state.
fn clawd_for(v: &ClaudeView) -> (ClawdAnim, ClawdExpr) {
    let verb = v.caption.split(' ').next().unwrap_or("");
    match v.state {
        ClaudeState::Idle => (ClawdAnim::Sleep, ClawdExpr::Closed),
        ClaudeState::Starting => (ClawdAnim::Idle, ClawdExpr::Wide),
        ClaudeState::NeedsYou => (ClawdAnim::Shake, ClawdExpr::Worried),
        // Celebrate while "done" holds the screen, then just look pleased.
        ClaudeState::Done if v.takeover => (ClawdAnim::Jump, ClawdExpr::Happy),
        ClaudeState::Done => (ClawdAnim::Idle, ClawdExpr::Happy),
        ClaudeState::Working => match verb {
            "EDIT" | "RUN" | "PLANNING" => (ClawdAnim::Work, ClawdExpr::Focus),
            "READ" | "SEARCH" | "FIND" | "WEB" => (ClawdAnim::Walk, ClawdExpr::Focus),
            _ => (ClawdAnim::Idle, ClawdExpr::Focus),
        },
    }
}

/// Mood and overlay (enum values) for boards without Clawd, or by choice.
fn face_for(v: &ClaudeView) -> (u8, u8) {
    let verb = v.caption.split(' ').next().unwrap_or("");
    match v.state {
        ClaudeState::Idle => (22, 2),     // asleep, zzz
        ClaudeState::Starting => (8, 6),  // excited, sparkles
        ClaudeState::NeedsYou => (18, 7), // worried, question
        ClaudeState::Done => (1, 6),      // happy, sparkles
        ClaudeState::Working => match verb {
            "READ" | "SEARCH" | "FIND" | "WEB" => (26, 0), // scan
            _ => (29, 0),                                  // determined
        },
    }
}

/// A 7x7 status mark in the top-right corner of app-drawn screens, so Claude
/// is visible from any screen: dots cycling while working, a tick when done,
/// "!" when it needs you.
fn badge(fb: &mut [u8; FB_SIZE], v: &ClaudeView, second: u8) {
    let mut c = screens::Canvas { fb: *fb };
    let (x, y) = (120, 0);
    match v.state {
        ClaudeState::Working => {
            c.rect(x, y, 8, 8, false);
            for i in 0..3 {
                let on = i32::from(second % 3) == i;
                c.rect(
                    x + 1 + i * 2,
                    y + if on { 2 } else { 4 },
                    1,
                    if on { 3 } else { 1 },
                    true,
                );
            }
        }
        ClaudeState::Done => {
            c.rect(x, y, 8, 8, false);
            for (dx, dy) in [(1, 4), (2, 5), (3, 6), (4, 5), (5, 4), (6, 3), (7, 2)] {
                c.set(x + dx, y + dy, true);
            }
        }
        ClaudeState::NeedsYou => {
            c.rect(x, y, 8, 8, true);
            c.rect(x + 3, y + 1, 2, 4, false);
            c.rect(x + 3, y + 6, 2, 1, false);
        }
        _ => return,
    }
    *fb = c.fb;
}

/// Word-wrap `s` to lines of at most `width` characters (long words are cut).
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.lines() {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let mut word = word.to_string();
            while word.chars().count() > width {
                if !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                }
                let head: String = word.chars().take(width).collect();
                word = word.chars().skip(width).collect();
                lines.push(head);
            }
            let need = cur.chars().count() + usize::from(!cur.is_empty()) + word.chars().count();
            if need > width && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(&word);
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    lines
}

/// Fit `lines` into `max` lines, marking a cut with "...".
fn clip(mut lines: Vec<String>, max: usize, width: usize) -> Vec<String> {
    if lines.len() > max {
        lines.truncate(max);
        if let Some(last) = lines.last_mut() {
            let keep: String = last.chars().take(width.saturating_sub(3)).collect();
            *last = format!("{}...", keep.trim_end());
        }
    }
    lines
}

/// Notification screen, made to be read from across the desk: a bell and
/// the app name in 2x letters, a rule, then the sender (the title) in 2x
/// letters on up to two lines. With `show_body`, a one-line title is followed
/// by two small lines of the message.
pub fn notification_frame(n: &Note, show_body: bool) -> [u8; FB_SIZE] {
    const BIG: usize = 10; // 2x glyphs across 128 px
    const SMALL: usize = 21;
    let mut c = screens::Canvas::default();
    let ascii = |s: &str| screens::normalize_caption_long(s);
    let app = if n.app.trim().is_empty() {
        "NOTIFY".to_string()
    } else {
        ascii(n.app.trim()).to_uppercase()
    };
    layout::icon(&mut c, "bell", 0, 0, 1);
    let app: String = app.chars().take(9).collect();
    c.text(&app, 20, 1, 2);
    c.rect(0, 18, 128, 1, true);
    let title = clip(wrap(&ascii(&n.title), BIG), 2, BIG);
    let mut y = 23;
    for l in &title {
        let x = (128 - screens::Canvas::text_width(l, 2)) / 2;
        c.text(l, x, y, 2);
        y += 18;
    }
    if show_body && title.len() == 1 && !n.body.trim().is_empty() {
        for l in clip(wrap(&ascii(&n.body), SMALL), 2, SMALL) {
            c.text(&l, 0, y + 1, 1);
            y += 9;
        }
    }
    c.fb
}

/// The preview of a screen at `t_ms` (the face screen is previewed by the
/// simulator, so it is not handled here).
pub fn preview(live: &Live, screen: &ScreenRef, t_ms: u32) -> [u8; FB_SIZE] {
    let cfg = live.config();
    let v = live.claude_view();
    match screen.kind {
        ScreenKind::Design => live
            .design_frame(screen.design.as_deref().unwrap_or(""), &v)
            .unwrap_or([0; FB_SIZE]),
        ScreenKind::Notify => notification_frame(
            &Note {
                app: "Slack".into(),
                title: "Ana Silva".into(),
                body: "Lunch at 1? I booked the place on the corner.".into(),
            },
            cfg.notify.show_body,
        ),
        ScreenKind::Claude | ScreenKind::Face => {
            let (a, e) = clawd_for(&v);
            let mut fb = screens::clawd_frame(a, e, t_ms);
            screens::draw_caption(&mut fb, &v.caption);
            fb
        }
    }
}

// --- worker -----------------------------------------------------------------

/// What the board is showing, as far as we know.
#[derive(Clone, PartialEq)]
enum Shown {
    Home,
    Frame(Box<[u8; FB_SIZE]>),
    Clawd(u8, u8, String),
    Face(u8, u8, String),
}

struct Worker {
    app: AppHandle,
    live: Arc<Live>,
    link: Option<LiveLink>,
    shown: Option<Shown>,
    last_cmd: Instant,
    retry_at: Instant,
    rot_idx: usize,
    rot_since: Instant,
    /// The notification on screen this tick, if any.
    note: Option<Note>,
}

const KEEPALIVE: Duration = Duration::from_secs(10);

impl Worker {
    fn new(app: AppHandle, live: Arc<Live>) -> Worker {
        let now = Instant::now();
        Worker {
            app,
            live,
            link: None,
            shown: None,
            last_cmd: now,
            retry_at: now,
            rot_idx: 0,
            rot_since: now,
            note: None,
        }
    }

    fn set_status(&self, f: impl FnOnce(&mut LiveStatus)) {
        let mut g = self.live.lock();
        let before = g.status.clone();
        f(&mut g.status);
        if g.status != before {
            let s = g.status.clone();
            drop(g);
            let _ = self.app.emit("live-status", s);
        }
    }

    fn set_holding(&self, holding: bool) {
        self.live.lock().holding = holding;
        self.live.cv.notify_all();
    }

    /// Hand the board back to its flashed face and close the port.
    fn release(&mut self) {
        if let Some(mut l) = self.link.take() {
            if self.shown != Some(Shown::Home) {
                let _ = l.home();
            }
        }
        self.shown = None;
        self.set_holding(false);
    }

    fn fail(&mut self, msg: String) {
        log::warn!("live: {msg}");
        self.link = None; // the port is in an unknown state: no home, just close
        self.shown = None;
        self.set_holding(false);
        self.retry_at = Instant::now() + Duration::from_secs(4);
        self.set_status(|s| {
            s.state = LiveState::Error;
            s.message = Some(msg);
            s.screen = None;
        });
    }

    fn run(mut self) {
        loop {
            let (cfg, pinned, paused) = {
                let g = self.live.lock();
                (g.cfg.clone(), g.pinned.clone(), g.pauses > 0)
            };
            let port = cfg.port.clone().filter(|_| cfg.enabled && !paused);
            // Port changed or live mode off/paused: let go.
            let wrong_port = self.link.is_some() && self.live.status().port != port;
            if port.is_none() || wrong_port {
                self.release();
                self.set_status(|s| {
                    *s = LiveStatus {
                        state: if paused && cfg.enabled {
                            LiveState::Paused
                        } else {
                            LiveState::Off
                        },
                        pinned: pinned.clone(),
                        ..LiveStatus::default()
                    };
                });
                if port.is_none() {
                    self.wait(Duration::from_secs(1));
                    continue;
                }
            }
            let port = port.unwrap_or_default();

            if self.link.is_none() {
                if Instant::now() < self.retry_at {
                    self.wait(self.retry_at - Instant::now());
                    continue;
                }
                self.set_status(|s| {
                    s.state = LiveState::Connecting;
                    s.port = Some(port.clone());
                    s.message = None;
                });
                self.set_holding(true);
                match LiveLink::open(&SystemHost, &port) {
                    Ok(l) => {
                        log::info!("live: connected on {port}: {}", l.banner);
                        let (banner, clawd) = (l.banner.clone(), l.has_clawd);
                        self.link = Some(l);
                        self.shown = None;
                        self.last_cmd = Instant::now();
                        self.set_status(|s| {
                            s.state = LiveState::Connected;
                            s.banner = Some(banner);
                            s.clawd = clawd;
                        });
                    }
                    Err(e) => {
                        self.fail(format!("Could not start live mode on {port}: {e}"));
                        continue;
                    }
                }
            }

            let view = self.live.claude_view();
            self.note = self.live.current_note(cfg.notify.seconds);
            let screen = self.pick(&cfg, pinned.clone(), &view);
            if let Err(e) = self.show(&screen, &cfg, &view) {
                self.fail(format!("Lost the board on {port}: {e}"));
                continue;
            }
            self.set_status(|s| {
                s.screen = Some(screen);
                s.pinned = pinned;
            });
            // A notification may have been queued meanwhile; loop round fast.
            // Wake at the next second boundary (the clock), or on a change.
            let ms = 1000 - u64::from(Local::now().timestamp_subsec_millis().min(999));
            self.wait(Duration::from_millis(ms.max(20)));
        }
    }

    fn wait(&self, d: Duration) {
        let g = self.live.lock();
        let _ = self.live.cv.wait_timeout(g, d);
    }

    /// Which screen goes on the board now.
    ///
    /// Claude needing you beats everything, a pin included. Otherwise a pin
    /// wins; then a Claude takeover (a new session, a new prompt for 5 s, done
    /// for 20 s, and with `claude_hold` the whole time a session works) if the
    /// Claude screen is enabled; then the rotation, which includes the Claude
    /// screen only while a session is busy or done.
    fn pick(&mut self, cfg: &LiveConfig, pinned: Option<ScreenRef>, v: &ClaudeView) -> ScreenRef {
        let claude_on = cfg
            .screens
            .iter()
            .any(|s| s.kind == ScreenKind::Claude && s.enabled);
        if v.state == ClaudeState::NeedsYou && claude_on {
            return ScreenRef::of(ScreenKind::Claude);
        }
        if self.note.is_some() {
            return ScreenRef::of(ScreenKind::Notify);
        }
        if let Some(p) = pinned {
            return p;
        }
        if claude_on && (v.takeover || (cfg.claude_hold && v.busy)) {
            return ScreenRef::of(ScreenKind::Claude);
        }
        let designs = self.live.designs();
        let order: Vec<&ScreenCfg> = cfg
            .screens
            .iter()
            .filter(|s| s.enabled)
            .filter(|s| s.kind != ScreenKind::Claude || v.active)
            .filter(|s| {
                // a deleted design drops out of the rotation
                s.kind != ScreenKind::Design
                    || designs.iter().any(|d| Some(&d.id) == s.design.as_ref())
            })
            .collect();
        if order.is_empty() {
            return ScreenRef::of(ScreenKind::Face);
        }
        self.rot_idx %= order.len();
        if self.rot_since.elapsed() >= Duration::from_secs(u64::from(order[self.rot_idx].seconds)) {
            self.rot_idx = (self.rot_idx + 1) % order.len();
            self.rot_since = Instant::now();
        }
        order[self.rot_idx].screen()
    }

    fn show(
        &mut self,
        screen: &ScreenRef,
        cfg: &LiveConfig,
        v: &ClaudeView,
    ) -> deskbunny_flash::Result<()> {
        let design = match screen.kind {
            ScreenKind::Design => self
                .live
                .design_frame(screen.design.as_deref().unwrap_or(""), v),
            _ => None,
        };
        let Some(link) = self.link.as_mut() else {
            return Ok(());
        };
        let want = match screen.kind {
            ScreenKind::Face => Shown::Home,
            ScreenKind::Design => match design {
                Some(fb) => Shown::Frame(Box::new(fb)),
                None => Shown::Home, // deleted while pinned
            },
            ScreenKind::Notify => match &self.note {
                Some(n) => Shown::Frame(Box::new(notification_frame(n, cfg.notify.show_body))),
                None => Shown::Home,
            },
            ScreenKind::Claude => {
                let cap = screens::normalize_caption(&v.caption);
                if link.has_clawd && cfg.claude_style == ClaudeStyle::Clawd {
                    let (a, e) = clawd_for(v);
                    Shown::Clawd(a.to_u8(), e.to_u8(), cap)
                } else {
                    let (m, o) = face_for(v);
                    Shown::Face(m, o, cap)
                }
            }
        };
        if self.shown.as_ref() == Some(&want) {
            // Same screen: keep the board from timing out.
            if want != Shown::Home && self.last_cmd.elapsed() >= KEEPALIVE {
                if !link.ping()? {
                    self.shown = None; // it fell back on its own: resend next tick
                }
                self.last_cmd = Instant::now();
            }
            return Ok(());
        }
        match &want {
            Shown::Home => link.home()?,
            Shown::Frame(fb) => {
                let whole = pack(&fb[..]);
                match &self.shown {
                    Some(Shown::Frame(prev)) => {
                        let d = pack(&screens::delta(prev, fb)[..]);
                        if d.len() < whole.len() {
                            link.frame(&d, true)?
                        } else {
                            link.frame(&whole, false)?
                        }
                    }
                    _ => link.frame(&whole, false)?,
                }
            }
            Shown::Clawd(a, e, cap) => link.clawd(*a, *e, cap)?,
            Shown::Face(m, o, cap) => link.face(*m, *o, cap)?,
        }
        self.shown = Some(want);
        self.last_cmd = Instant::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_sanitizes() {
        let cfg = LiveConfig {
            enabled: true,
            port: Some("  ".into()),
            screens: vec![
                ScreenCfg {
                    kind: ScreenKind::Design,
                    design: None,
                    enabled: true,
                    seconds: 1,
                },
                ScreenCfg {
                    kind: ScreenKind::Design,
                    design: Some("clock".into()),
                    enabled: false,
                    seconds: 99,
                },
                ScreenCfg {
                    kind: ScreenKind::Design,
                    design: Some("weather".into()),
                    enabled: true,
                    seconds: 30,
                },
                ScreenCfg {
                    kind: ScreenKind::Face,
                    design: Some("junk".into()),
                    enabled: true,
                    seconds: 30,
                },
            ],
            ..LiveConfig::default()
        }
        .sanitized();
        assert_eq!(cfg.port, None);
        let got: Vec<(ScreenRef, bool, u16)> = cfg
            .screens
            .iter()
            .map(|s| (s.screen(), s.enabled, s.seconds))
            .collect();
        assert_eq!(
            got,
            vec![
                (ScreenRef::design("clock"), true, 3),
                (ScreenRef::design("weather"), true, 30),
                (ScreenRef::of(ScreenKind::Face), true, 30),
                (ScreenRef::of(ScreenKind::Claude), false, 15),
            ]
        );
    }

    #[test]
    fn wraps_and_clips() {
        assert_eq!(wrap("the quick brown fox", 9), ["the quick", "brown fox"]);
        assert_eq!(wrap("abcdefghijkl", 5), ["abcde", "fghij", "kl"]);
        assert_eq!(wrap("a\nb", 9), ["a", "b"]);
        assert_eq!(clip(wrap("one two three four", 9), 1, 9), ["one tw..."]);
        let n = Note {
            app: "Slack".into(),
            title: "Hi".into(),
            body: "x".repeat(200),
        };
        let fb = notification_frame(&n, true);
        assert_ne!(fb, notification_frame(&n, false));
    }

    #[test]
    fn config_json_shape() {
        let v = serde_json::to_value(LiveConfig::default()).unwrap();
        assert_eq!(v["screens"][1]["kind"], "design");
        assert_eq!(v["screens"][1]["design"], "clock");
        assert!(v["screens"][0].get("design").is_none());
        // older or partial files load with defaults
        let c: LiveConfig = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(c.enabled && c.screens.len() == 3);
        // phase 2 files: {"kind":"clock"} becomes the clock design
        let old: LiveConfig = serde_json::from_str(
            r#"{"screens":[{"kind":"clock","enabled":true,"seconds":15}],"clock24":true}"#,
        )
        .unwrap();
        assert_eq!(
            old.sanitized().screens[0].screen(),
            ScreenRef::design("clock")
        );
    }
}
