//! Desktop notifications → DeskBunny (Linux). Runs `dbus-monitor` on the
//! session bus, watching `org.freedesktop.Notifications.Notify` calls (what
//! every app's popup goes through), and queues each as a notification.
//!
//! dbus-monitor ships with D-Bus on every desktop Linux and becomes a bus
//! monitor without extra rights for the user's own session bus.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::live::Live;

/// One `Notify(app_name, replaces_id, app_icon, summary, body, ...)` call.
#[derive(Debug, PartialEq, Eq)]
pub struct Call {
    pub app: String,
    pub summary: String,
    pub body: String,
}

/// Turns dbus-monitor's text output into Notify calls, one line at a time.
#[derive(Default)]
pub struct Parser {
    in_notify: bool,
    /// Top-level string arguments seen so far (app, icon, summary, body).
    strings: Vec<String>,
    /// A string argument that spans lines (a body with newlines).
    open: Option<String>,
}

/// dbus-monitor prints strings between quotes without escaping anything, so
/// an argument ends at the first line that ends with `"`. (A body whose own
/// line ends in a quote is cut there; the rest is lost, nothing worse.)
fn closes(s: &str) -> bool {
    s.ends_with('"')
}

impl Parser {
    pub fn line(&mut self, line: &str) -> Option<Call> {
        if let Some(mut acc) = self.open.take() {
            acc.push('\n');
            acc.push_str(line);
            if closes(&acc) {
                acc.pop();
                self.strings.push(acc);
                return self.maybe_done();
            }
            self.open = Some(acc);
            return None;
        }
        if line.starts_with("method call")
            || line.starts_with("signal")
            || line.starts_with("method return")
        {
            self.in_notify = line.starts_with("method call") && line.contains("member=Notify");
            self.strings.clear();
            return None;
        }
        if !self.in_notify {
            return None;
        }
        // Top-level arguments are indented three spaces; nested ones more.
        let rest = line.strip_prefix("   string \"")?;
        if closes(rest) {
            self.strings.push(rest[..rest.len() - 1].to_string());
            return self.maybe_done();
        }
        self.open = Some(rest.to_string());
        None
    }

    fn maybe_done(&mut self) -> Option<Call> {
        if self.strings.len() < 4 {
            return None;
        }
        self.in_notify = false;
        let mut s = std::mem::take(&mut self.strings).into_iter();
        let app = s.next().unwrap_or_default();
        let _icon = s.next();
        let summary = s.next().unwrap_or_default();
        let body = s.next().unwrap_or_default();
        Some(Call {
            app,
            summary,
            body: strip_markup(&body),
        })
    }
}

/// Bodies may carry a little HTML (`<b>`, `<a href>`): show the text only.
fn strip_markup(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

/// Watch the desktop's notifications while `desktop` is on in the config.
/// Restarts dbus-monitor if it exits; gives up quietly where it is missing.
pub fn start(live: Arc<Live>) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("deskbunny-desktop-notify".into())
        .spawn(move || loop {
            let cfg = live.config().notify;
            if !(cfg.enabled && cfg.desktop) {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            let child = Command::new("dbus-monitor")
                .args([
                    "--session",
                    "type='method_call',interface='org.freedesktop.Notifications',member='Notify'",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("desktop notifications: cannot run dbus-monitor: {e}");
                    return;
                }
            };
            let Some(out) = child.stdout.take() else {
                return;
            };
            let mut parser = Parser::default();
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                if let Some(call) = parser.line(&line) {
                    let cfg = live.config().notify;
                    if !(cfg.enabled && cfg.desktop) {
                        break; // switched off: stop watching
                    }
                    live.notify(&call.app, &call.summary, &call.body);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            std::thread::sleep(Duration::from_secs(2));
        });
    if let Err(e) = spawned {
        log::warn!("desktop notifications: thread failed to start: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"signal time=1.0 sender=org.freedesktop.DBus -> destination=:1.9 serial=2 path=/org/freedesktop/DBus; interface=org.freedesktop.DBus; member=NameAcquired
   string ":1.9"
method call time=2.0 sender=:1.50 -> destination=:1.20 serial=7 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "Slack"
   uint32 0
   string "slack"
   string "Message from "Ana""
   string "lunch at <b>1</b>?
see you &amp; bring c:\temp"
   array [
      string "default"
      string "Open"
   ]
   array [
      dict entry(
         string "urgency"
         variant             byte 1
      )
   ]
   int32 -1
method call time=3.0 sender=:1.51 -> destination=:1.20 serial=8 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=Notify
   string "notify-send"
   uint32 0
   string ""
   string "Build passed"
   string ""
   array [
   ]
"#;

    #[test]
    fn parses_notify_calls() {
        let mut p = Parser::default();
        let calls: Vec<Call> = SAMPLE.lines().filter_map(|l| p.line(l)).collect();
        assert_eq!(
            calls,
            vec![
                Call {
                    app: "Slack".into(),
                    summary: "Message from \"Ana\"".into(),
                    body: "lunch at 1?\nsee you & bring c:\\temp".into(),
                },
                Call {
                    app: "notify-send".into(),
                    summary: "Build passed".into(),
                    body: "".into()
                },
            ]
        );
    }
}
