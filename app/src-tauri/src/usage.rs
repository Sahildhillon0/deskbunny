//! AI coding plan usage (a 5-hour and a weekly window), for the usage screen
//! (docs/ARCHITECTURE.md, "Usage"). Each harness reports to `POST /usage`;
//! the app keeps the newest report per harness and the screen shows the one
//! chosen in the settings.
//!
//! Adding a harness: an entry in [`HARNESSES`], and either have it post the
//! neutral format or add an adapter in [`parse`].
//!
//! Pure state: every function takes `now` (unix seconds), so it is tested
//! without clocks.

use std::collections::HashMap;

use deskbunny_core::layout::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A tool whose plan usage the screen can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Harness {
    /// Key in reports and in the settings.
    pub id: &'static str,
    /// Name in the app.
    pub name: &'static str,
    /// `{USRC}` on the screen (upper case, short).
    pub short: &'static str,
    /// How to make it report, shown in the settings.
    pub setup: &'static str,
}

pub const CLAUDE: &str = "claude";

pub const HARNESSES: &[Harness] = &[Harness {
    id: CLAUDE,
    name: "Claude Code",
    short: "CLAUDE",
    setup: "Claude Code gives the 5-hour and weekly usage only to its status line. \
            Have your status line command forward its input: \
            curl -s -m 0.3 -H 'X-DeskBunny: 1' --data-binary @- http://127.0.0.1:47807/usage",
}];

pub fn harness(id: &str) -> Option<&'static Harness> {
    HARNESSES.iter().find(|h| h.id == id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageCfg {
    /// Which harness the usage tokens show (an id from [`HARNESSES`]).
    pub harness: String,
}

impl Default for UsageCfg {
    fn default() -> Self {
        UsageCfg {
            harness: CLAUDE.into(),
        }
    }
}

impl UsageCfg {
    pub fn sanitized(mut self) -> UsageCfg {
        if harness(&self.harness).is_none() {
            self.harness = CLAUDE.into();
        }
        self
    }
}

/// One window: percent used and when it resets (unix seconds, if known).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub used: u8,
    pub resets_at: Option<i64>,
}

/// The newest usage one harness reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub five_hour: Option<Window>,
    pub week: Option<Window>,
    /// When it arrived (unix seconds).
    pub at: i64,
}

/// The newest report per harness id.
#[derive(Debug, Default)]
pub struct Reports(HashMap<String, Report>);

impl Reports {
    pub fn get(&self, id: &str) -> Option<&Report> {
        self.0.get(id)
    }

    /// Take a `POST /usage` body; the harness id it was for, or why not.
    pub fn take(&mut self, body: &Value, now: i64) -> Result<&'static str, String> {
        let (id, report) = parse(body, now)?;
        self.0.insert(id.to_string(), report);
        Ok(id)
    }
}

fn window(w: Option<&Value>, used_key: &str) -> Option<Window> {
    let w = w?;
    let used = w.get(used_key).and_then(Value::as_f64)?;
    Some(Window {
        used: used.clamp(0.0, 100.0).round() as u8,
        resets_at: w.get("resets_at").and_then(Value::as_i64),
    })
}

/// A `POST /usage` body, in one of two shapes:
/// - neutral: `{"source": "<harness id>", "five_hour": {"used": 42,
///   "resets_at": 1794493200}, "week": {...}}` (either window may be missing);
/// - Claude Code's status line payload, or just its `rate_limits`.
pub fn parse(body: &Value, now: i64) -> Result<(&'static str, Report), String> {
    let (id, five_hour, week) = if let Some(src) = body.get("source").and_then(Value::as_str) {
        let h = harness(src).ok_or_else(|| format!("unknown source {src:?}"))?;
        (
            h.id,
            window(body.get("five_hour"), "used"),
            window(body.get("week"), "used"),
        )
    } else {
        // Claude Code: rate_limits.{five_hour, seven_day}.used_percentage.
        let rl = body.get("rate_limits").unwrap_or(body);
        (
            CLAUDE,
            window(rl.get("five_hour"), "used_percentage"),
            window(rl.get("seven_day"), "used_percentage"),
        )
    };
    if five_hour.is_none() && week.is_none() {
        return Err("no usage windows in the body".into());
    }
    Ok((
        id,
        Report {
            five_hour,
            week,
            at: now,
        },
    ))
}

/// `2H15M`, `45M`, `3D4H`; under a minute is `0M`.
pub fn span(secs: i64) -> String {
    let m = secs.max(0) / 60;
    let (d, h, m) = (m / 1440, m / 60 % 24, m % 60);
    if d > 0 {
        format!("{d}D{h}H")
    } else if h > 0 {
        format!("{h}H{m:02}M")
    } else {
        format!("{m}M")
    }
}

/// What the tokens show at `now` for `harness`. A window whose reset time has
/// passed is back to 0% (the next one starts with the next request).
pub fn view(h: &Harness, r: Option<&Report>, now: i64) -> Usage {
    let show = |w: Option<Window>| match w {
        None => (None, "--".to_string()),
        Some(w) => match w.resets_at {
            Some(t) if t <= now => (Some(0), "--".to_string()),
            Some(t) => (Some(w.used), span(t - now)),
            None => (Some(w.used), "--".to_string()),
        },
    };
    let (five, five_in) = show(r.and_then(|r| r.five_hour));
    let (week, week_in) = show(r.and_then(|r| r.week));
    Usage {
        source: h.short.into(),
        five,
        five_in,
        week,
        week_in,
    }
}

/// For the settings: the chosen harness's usage and how old its report is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageNow {
    pub harness: &'static str,
    pub usage: Usage,
    /// Seconds since the last report; None if none came yet.
    pub age_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claude() -> &'static Harness {
        harness(CLAUDE).unwrap()
    }

    #[test]
    fn reads_claude_status_line_payload() {
        let p = json!({"session_id": "x", "rate_limits": {
            "five_hour": {"used_percentage": 41.6, "resets_at": 1000 + 2 * 3600 + 15 * 60},
            "seven_day": {"used_percentage": 4, "resets_at": 1000 + 3 * 86400 + 4 * 3600}
        }});
        let mut rs = Reports::default();
        assert_eq!(rs.take(&p, 1000), Ok(CLAUDE));
        let r = rs.get(CLAUDE).unwrap();
        assert_eq!((r.five_hour.unwrap().used, r.at), (42, 1000));
        let u = view(claude(), Some(r), 1000);
        assert_eq!(
            (
                u.source.as_str(),
                u.five,
                u.five_in.as_str(),
                u.week,
                u.week_in.as_str()
            ),
            ("CLAUDE", Some(42), "2H15M", Some(4), "3D4H")
        );
        // After the 5-hour reset: 0% until the harness reports again.
        let u = view(claude(), Some(r), 1000 + 3 * 3600);
        assert_eq!((u.five, u.five_in.as_str()), (Some(0), "--"));
        assert_eq!(u.week, Some(4));
    }

    #[test]
    fn neutral_format() {
        let (id, r) = parse(
            &json!({"source": "claude", "five_hour": {"used": 12, "resets_at": 600}}),
            0,
        )
        .unwrap();
        assert_eq!(id, CLAUDE);
        let u = view(claude(), Some(&r), 0);
        assert_eq!(
            (u.five, u.five_in.as_str(), u.week, u.week_in.as_str()),
            (Some(12), "10M", None, "--")
        );
        assert!(parse(&json!({"source": "nope", "week": {"used": 1}}), 0)
            .unwrap_err()
            .contains("unknown source"));
    }

    #[test]
    fn missing_and_settings() {
        assert!(parse(&json!({"session_id": "x"}), 0).is_err());
        assert!(parse(&json!({"rate_limits": {}}), 0).is_err());
        let u = view(claude(), None, 0);
        assert_eq!((u.five, u.five_in.as_str(), u.week), (None, "--", None));
        let cfg = UsageCfg {
            harness: "gone".into(),
        };
        assert_eq!(cfg.sanitized().harness, CLAUDE);
    }

    #[test]
    fn spans() {
        assert_eq!(span(30), "0M");
        assert_eq!(span(45 * 60), "45M");
        assert_eq!(span(2 * 3600 + 5 * 60), "2H05M");
        assert_eq!(span(86400 + 3600), "1D1H");
        assert_eq!(span(-5), "0M");
    }
}
