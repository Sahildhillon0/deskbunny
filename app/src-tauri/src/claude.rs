//! Claude Code sessions, as told by its hooks (docs/ARCHITECTURE.md, "Claude
//! screen"). Each hook posts its JSON payload to the app; this module keeps one
//! record per session and reduces them to what the Claude screen shows.
//!
//! Pure state: every method takes `now`, so it is tested without clocks.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How long "done" holds the screen before the rotation resumes.
pub const DONE_HOLD: Duration = Duration::from_secs(20);
/// How long a new session or a new prompt holds the screen.
pub const START_HOLD: Duration = Duration::from_secs(3);
pub const WORK_HOLD: Duration = Duration::from_secs(5);
/// Sessions with no event for this long are forgotten (a closed terminal
/// never sends SessionEnd).
pub const STALE: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeState {
    // Ordered by priority, lowest first.
    Idle,
    Starting,
    Working,
    Done,
    NeedsYou,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub project: String,
    pub state: ClaudeState,
    /// Short "what": `EDIT main.rs`, `RUN cargo`, `ALLOW Bash?`.
    pub detail: String,
    /// When the current state began.
    pub since: Instant,
    /// When the current prompt started (for "DONE 3m12s").
    pub work_start: Option<Instant>,
    pub last_event: Instant,
}

/// What the Claude screen should show now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeView {
    pub state: ClaudeState,
    pub caption: String,
    pub project: String,
    pub sessions: usize,
    /// The screen should take over the rotation right now.
    pub takeover: bool,
    /// Something is going on: the Claude screen joins the rotation.
    pub active: bool,
    /// Some session is working or waiting for you (not just done or idle).
    pub busy: bool,
}

#[derive(Default)]
pub struct Tracker {
    sessions: HashMap<String, Session>,
}

fn basename(p: &str) -> &str {
    p.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(p)
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

/// `EDIT main.rs`, `READ lib.rs`, `RUN cargo`, ... from a tool call.
pub fn tool_detail(tool: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    let file = || basename(s("file_path")).to_string();
    match tool {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => format!("EDIT {}", file()),
        "Read" => format!("READ {}", file()),
        "Bash" => format!("RUN {}", basename(first_word(s("command")))),
        "Grep" => format!("SEARCH {}", s("pattern")),
        "Glob" => format!("FIND {}", s("pattern")),
        "WebFetch" | "WebSearch" => "WEB".into(),
        "Task" | "Agent" => "AGENT".into(),
        "TodoWrite" => "PLANNING".into(),
        t if t.starts_with("mcp__") => format!("TOOL {}", t.rsplit("__").next().unwrap_or(t)),
        t => format!("TOOL {t}"),
    }
}

fn mmss(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, s / 60 % 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

impl Tracker {
    /// Apply one hook payload. Returns false for payloads that are not
    /// Claude Code hooks.
    #[cfg(test)]
    pub fn event(&mut self, payload: &Value, now: Instant) -> bool {
        self.event_timed(payload, now, None)
    }

    /// [`Tracker::event`], with how long the prompt that just finished ran
    /// according to the transcript (for `Stop`); it beats the app's own
    /// clock, which misses prompts sent before the app started.
    pub fn event_timed(&mut self, payload: &Value, now: Instant, worked: Option<Duration>) -> bool {
        let Some(id) = payload.get("session_id").and_then(Value::as_str) else {
            return false;
        };
        let Some(ev) = payload.get("hook_event_name").and_then(Value::as_str) else {
            return false;
        };
        if ev == "SessionEnd" {
            self.sessions.remove(id);
            return true;
        }
        let cwd = payload.get("cwd").and_then(Value::as_str).unwrap_or("");
        let s = self
            .sessions
            .entry(id.to_string())
            .or_insert_with(|| Session {
                project: String::new(),
                state: ClaudeState::Idle,
                detail: String::new(),
                since: now,
                work_start: None,
                last_event: now,
            });
        if !cwd.is_empty() {
            s.project = basename(cwd).to_string();
        }
        s.last_event = now;
        let set = |s: &mut Session, st: ClaudeState, detail: String| {
            if s.state != st {
                s.since = now;
            }
            s.state = st;
            s.detail = detail;
        };
        match ev {
            "SessionStart" => set(s, ClaudeState::Starting, "HELLO".into()),
            "UserPromptSubmit" => {
                // A message typed while Claude is busy joins the running
                // task; the clock keeps going.
                let busy = matches!(s.state, ClaudeState::Working | ClaudeState::NeedsYou);
                if !busy || s.work_start.is_none() {
                    s.work_start = Some(now);
                }
                set(s, ClaudeState::Working, "THINKING".into());
            }
            "PreToolUse" => {
                let tool = payload
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
                s.work_start.get_or_insert(now);
                set(s, ClaudeState::Working, tool_detail(tool, &input));
            }
            "PostToolUse" | "PostToolUseFailure" => {
                // Permission was granted (or the tool ran): back to work.
                if s.state == ClaudeState::NeedsYou {
                    set(s, ClaudeState::Working, "THINKING".into());
                }
            }
            "PermissionRequest" => {
                let tool = payload
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let d = if tool.is_empty() {
                    "NEEDS YOU".into()
                } else {
                    format!("ALLOW {tool}?")
                };
                set(s, ClaudeState::NeedsYou, d);
            }
            "Notification" => {
                let kind = payload
                    .get("notification_type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let msg = payload.get("message").and_then(Value::as_str).unwrap_or("");
                // Idle reminders ("waiting for your input") come a minute after
                // Stop: they are not news. Permission prompts are.
                let permission = kind == "permission_prompt"
                    || (kind.is_empty() && msg.to_ascii_lowercase().contains("permission"));
                if permission {
                    let tool = msg.rsplit(' ').next().unwrap_or("").trim_end_matches('.');
                    let d = if tool.is_empty() {
                        "NEEDS YOU".into()
                    } else {
                        format!("ALLOW {tool}?")
                    };
                    set(s, ClaudeState::NeedsYou, d);
                }
            }
            "Stop" => {
                // The longer of the two: the transcript survives app restarts, the
                // app's clock survives follow-ups typed mid-task (which the
                // transcript records as new prompts).
                let own = s.work_start.map(|t| now.duration_since(t));
                let d = match (worked, own) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
                s.work_start = None;
                let detail = match d {
                    Some(d) => format!("DONE {}", mmss(d)),
                    None => "DONE".into(),
                };
                set(s, ClaudeState::Done, detail);
            }
            _ => {}
        }
        true
    }

    /// Forget sessions that went quiet; `starting` decays to idle.
    pub fn tick(&mut self, now: Instant) {
        self.sessions
            .retain(|_, s| now.duration_since(s.last_event) < STALE);
        for s in self.sessions.values_mut() {
            if s.state == ClaudeState::Starting && now.duration_since(s.since) >= START_HOLD {
                s.state = ClaudeState::Idle;
                s.detail = "IDLE".into();
                s.since = now;
            }
        }
    }

    pub fn view(&self, now: Instant) -> ClaudeView {
        let top = self.sessions.values().max_by_key(|s| (s.state, s.since));
        let Some(s) = top else {
            return ClaudeView {
                state: ClaudeState::Idle,
                caption: "NO SESSIONS".into(),
                project: String::new(),
                sessions: 0,
                takeover: false,
                active: false,
                busy: false,
            };
        };
        let age = now.duration_since(s.since);
        let takeover = match s.state {
            ClaudeState::NeedsYou => true,
            ClaudeState::Done => age < DONE_HOLD,
            ClaudeState::Starting => age < START_HOLD,
            ClaudeState::Working => age < WORK_HOLD,
            ClaudeState::Idle => false,
        };
        let active = self
            .sessions
            .values()
            .any(|s| s.state >= ClaudeState::Working);
        let busy = self
            .sessions
            .values()
            .any(|s| matches!(s.state, ClaudeState::Working | ClaudeState::NeedsYou));
        let mut caption = s.detail.clone();
        if s.state == ClaudeState::Idle {
            caption = if s.project.is_empty() {
                "IDLE".into()
            } else {
                s.project.clone()
            };
        }
        ClaudeView {
            state: s.state,
            caption,
            project: s.project.clone(),
            sessions: self.sessions.len(),
            takeover,
            active,
            busy,
        }
    }
}

/// How long ago the last prompt you typed was sent, from a Claude Code
/// transcript (JSONL; the hook payload's `transcript_path`). Only files under
/// `~/.claude/` ending in `.jsonl` are read, and only their tail.
pub fn prompt_age(transcript: &str, now: SystemTime) -> Option<Duration> {
    let path = Path::new(transcript).canonicalize().ok()?;
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let claude_dir = Path::new(&home).join(".claude").canonicalize().ok()?;
    if !path.starts_with(&claude_dir) || path.extension().is_none_or(|e| e != "jsonl") {
        return None;
    }
    const TAIL: u64 = 4 << 20;
    let mut f = std::fs::File::open(&path).ok()?;
    let len = f.metadata().ok()?.len();
    let from = len.saturating_sub(TAIL);
    f.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines().rev().collect::<Vec<_>>();
    if from > 0 {
        lines.pop(); // the first line is cut
    }
    let started = lines.into_iter().find_map(prompt_time)?;
    let now_ms = now.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
    let age = now_ms - started;
    (0..48 * 3600 * 1000)
        .contains(&age)
        .then(|| Duration::from_millis(age as u64))
}

/// Milliseconds since the epoch of a transcript line that is a prompt you
/// typed (not a tool result, not a meta or compaction line).
fn prompt_time(line: &str) -> Option<i64> {
    if !line.contains("\"user\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "user" {
        return None;
    }
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag("isMeta") || flag("isCompactSummary") || flag("isSidechain") {
        return None;
    }
    let content = v.get("message")?.get("content")?;
    let typed = match content {
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(items) => {
            !items.is_empty()
                && items
                    .iter()
                    .all(|i| i.get("type").and_then(Value::as_str) != Some("tool_result"))
        }
        _ => false,
    };
    if !typed {
        return None;
    }
    let ts = v.get("timestamp")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(t: &mut Tracker, now: Instant, v: Value) {
        assert!(t.event(&v, now));
    }

    #[test]
    fn a_prompt_runs_to_done() {
        let t0 = Instant::now();
        let mut t = Tracker::default();
        ev(
            &mut t,
            t0,
            json!({"session_id":"a","hook_event_name":"SessionStart","cwd":"/home/me/deskbunny"}),
        );
        let v = t.view(t0);
        assert_eq!(
            (v.state, v.takeover, v.project.as_str()),
            (ClaudeState::Starting, true, "deskbunny")
        );
        t.tick(t0 + START_HOLD);
        assert_eq!(t.view(t0 + START_HOLD).state, ClaudeState::Idle);
        assert!(!t.view(t0 + START_HOLD).active);

        let t1 = t0 + Duration::from_secs(10);
        ev(
            &mut t,
            t1,
            json!({"session_id":"a","hook_event_name":"UserPromptSubmit"}),
        );
        ev(
            &mut t,
            t1,
            json!({"session_id":"a","hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"/x/src/main.rs"}}),
        );
        let v = t.view(t1);
        assert_eq!(
            (v.state, v.caption.as_str(), v.takeover, v.active),
            (ClaudeState::Working, "EDIT main.rs", true, true)
        );
        assert!(
            !t.view(t1 + WORK_HOLD).takeover,
            "a long task gives the screen back"
        );
        assert!(t.view(t1 + WORK_HOLD).busy, "...but it is still busy");

        let t2 = t1 + Duration::from_secs(192);
        ev(
            &mut t,
            t2,
            json!({"session_id":"a","hook_event_name":"Stop"}),
        );
        let v = t.view(t2);
        assert_eq!(
            (v.state, v.caption.as_str(), v.takeover),
            (ClaudeState::Done, "DONE 3m12s", true)
        );
        assert!(!t.view(t2 + DONE_HOLD).takeover);
        assert!(!t.view(t2).busy && t.view(t2).active);
        // The idle reminder a minute later changes nothing.
        ev(
            &mut t,
            t2 + Duration::from_secs(60),
            json!({"session_id":"a","hook_event_name":"Notification","notification_type":"idle_prompt","message":"Claude is waiting for your input"}),
        );
        assert_eq!(
            t.view(t2 + Duration::from_secs(61)).state,
            ClaudeState::Done
        );
    }

    #[test]
    fn permission_wins_over_everything_until_answered() {
        let t0 = Instant::now();
        let mut t = Tracker::default();
        ev(
            &mut t,
            t0,
            json!({"session_id":"a","hook_event_name":"Stop"}),
        );
        ev(
            &mut t,
            t0,
            json!({"session_id":"b","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"/usr/bin/cargo test"}}),
        );
        // a's fresh "done" outranks b working.
        assert_eq!(t.view(t0).caption, "DONE");
        ev(
            &mut t,
            t0,
            json!({"session_id":"b","hook_event_name":"Notification","message":"Claude needs your permission to use Bash"}),
        );
        let v = t.view(t0 + Duration::from_secs(600));
        assert_eq!(
            (v.state, v.caption.as_str(), v.takeover, v.sessions),
            (ClaudeState::NeedsYou, "ALLOW Bash?", true, 2)
        );
        ev(
            &mut t,
            t0,
            json!({"session_id":"b","hook_event_name":"PostToolUse","tool_name":"Bash"}),
        );
        assert_eq!(
            t.view(t0).state,
            ClaudeState::Done,
            "session a's done outranks b working"
        );
    }

    #[test]
    fn sessions_end_and_go_stale() {
        let t0 = Instant::now();
        let mut t = Tracker::default();
        assert!(!t.event(&json!({"hello":1}), t0));
        ev(
            &mut t,
            t0,
            json!({"session_id":"a","hook_event_name":"UserPromptSubmit"}),
        );
        ev(
            &mut t,
            t0,
            json!({"session_id":"b","hook_event_name":"UserPromptSubmit"}),
        );
        ev(
            &mut t,
            t0,
            json!({"session_id":"a","hook_event_name":"SessionEnd"}),
        );
        assert_eq!(t.view(t0).sessions, 1);
        t.tick(t0 + STALE);
        assert_eq!(t.view(t0 + STALE).caption, "NO SESSIONS");
    }

    #[test]
    fn transcript_gives_the_prompt_time() {
        let prompt = r#"{"type":"user","message":{"role":"user","content":"fix the clock"},"timestamp":"2026-11-10T07:00:00.000Z"}"#;
        let blocks = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"and this"}]},"timestamp":"2026-11-10T07:01:00Z"}"#;
        let result = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]},"timestamp":"2026-11-10T07:03:00Z"}"#;
        let meta = r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<command>"},"timestamp":"2026-11-10T07:03:30Z"}"#;
        let reply = r#"{"type":"assistant","message":{"role":"assistant","content":"done"},"timestamp":"2026-11-10T07:04:00Z"}"#;
        assert_eq!(prompt_time(prompt), Some(1_794_294_000_000));
        assert_eq!(prompt_time(blocks), Some(1_794_294_060_000));
        assert_eq!(prompt_time(result), None);
        assert_eq!(prompt_time(meta), None);
        assert_eq!(prompt_time(reply), None);
        assert_eq!(prompt_time("not json \"user\""), None);
        // Files outside ~/.claude are never read.
        assert_eq!(prompt_age("/etc/passwd", SystemTime::now()), None);
    }

    #[test]
    fn done_uses_the_transcript_time_and_follow_ups_keep_the_clock() {
        let t0 = Instant::now();
        let mut t = Tracker::default();
        ev(
            &mut t,
            t0,
            json!({"session_id":"a","hook_event_name":"UserPromptSubmit"}),
        );
        // A follow-up typed while it works does not restart the clock.
        ev(
            &mut t,
            t0 + Duration::from_secs(200),
            json!({"session_id":"a","hook_event_name":"UserPromptSubmit"}),
        );
        ev(
            &mut t,
            t0 + Duration::from_secs(240),
            json!({"session_id":"a","hook_event_name":"Stop"}),
        );
        assert_eq!(t.view(t0 + Duration::from_secs(240)).caption, "DONE 4m00s");
        // The app started late: its own clock would say 40 s, the transcript 4 min.
        let mut t = Tracker::default();
        ev(
            &mut t,
            t0,
            json!({"session_id":"b","hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{}}),
        );
        let stop = json!({"session_id":"b","hook_event_name":"Stop"});
        assert!(t.event_timed(
            &stop,
            t0 + Duration::from_secs(40),
            Some(Duration::from_secs(245))
        ));
        assert_eq!(t.view(t0 + Duration::from_secs(40)).caption, "DONE 4m05s");
    }

    #[test]
    fn tool_details() {
        assert_eq!(
            tool_detail("Grep", &json!({"pattern":"fn main"})),
            "SEARCH fn main"
        );
        assert_eq!(
            tool_detail("mcp__github__create_issue", &json!({})),
            "TOOL create_issue"
        );
        assert_eq!(tool_detail("Bash", &json!({})), "RUN ");
        assert_eq!(mmss(Duration::from_secs(3725)), "1h02m");
    }
}
