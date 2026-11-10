//! Install / remove DeskBunny's Claude Code hooks in `~/.claude/settings.json`
//! (docs/ARCHITECTURE.md, "Claude screen").
//!
//! Each hook pipes Claude's payload to the local endpoint with curl. It never
//! blocks Claude: one second at most, and silence when the app is not running.
//! Our entries are recognised by the `X-DeskBunny` header in the command, so
//! install/remove leave every other hook alone. The file is backed up first.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::server::PORT;

/// Events DeskBunny listens to. Only names every current Claude Code accepts:
/// an unknown event name can make it reject the whole settings file.
const EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "Stop",
    "SessionEnd",
];

/// Tool events take a matcher; "*" is every tool.
fn has_matcher(event: &str) -> bool {
    matches!(event, "PreToolUse" | "PostToolUse")
}

const MARK: &str = "X-DeskBunny";

pub fn command() -> String {
    format!(
        "curl -s -m 1 -H '{MARK}: 1' -H 'Content-Type: application/json' --data-binary @- http://127.0.0.1:{PORT}/claude >/dev/null 2>&1 || true"
    )
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HooksStatus {
    /// Every DeskBunny hook is in place.
    pub installed: bool,
    pub settings_path: String,
    /// The command each hook runs (shown to the user before installing).
    pub command: String,
}

fn settings_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or("Could not find your home folder.")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

fn read(path: &PathBuf) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(t) if t.trim().is_empty() => Ok(json!({})),
        Ok(t) => serde_json::from_str(&t).map_err(|e| {
            format!(
                "{} is not valid JSON ({e}). Fix it first; DeskBunny will not overwrite it.",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("Could not read {}: {e}", path.display())),
    }
}

fn is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains(MARK))
            })
        })
}

/// Settings with our hooks removed (and empty event lists dropped).
fn without_ours(mut settings: Value) -> Value {
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for list in hooks.values_mut() {
            if let Some(arr) = list.as_array_mut() {
                arr.retain(|g| !is_ours(g));
            }
        }
        hooks.retain(|_, list| list.as_array().is_none_or(|a| !a.is_empty()));
        if hooks.is_empty() {
            if let Some(obj) = settings.as_object_mut() {
                obj.remove("hooks");
            }
        }
    }
    settings
}

fn with_ours(settings: Value) -> Result<Value, String> {
    let mut settings = without_ours(settings);
    let obj = settings
        .as_object_mut()
        .ok_or("Claude's settings.json is not a JSON object.")?;
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("\"hooks\" in Claude's settings.json is not an object.")?;
    for ev in EVENTS {
        let mut group = json!({
            "hooks": [{ "type": "command", "command": command(), "timeout": 2 }]
        });
        if has_matcher(ev) {
            group["matcher"] = json!("*");
        }
        let list = hooks.entry(ev).or_insert_with(|| json!([]));
        list.as_array_mut()
            .ok_or_else(|| format!("hooks.{ev} in Claude's settings.json is not a list."))?
            .push(group);
    }
    Ok(settings)
}

fn installed_in(settings: &Value) -> bool {
    EVENTS.iter().all(|ev| {
        settings
            .get("hooks")
            .and_then(|h| h.get(ev))
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(is_ours))
    })
}

pub fn status() -> Result<HooksStatus, String> {
    let path = settings_path()?;
    let installed = installed_in(&read(&path)?);
    Ok(HooksStatus {
        installed,
        settings_path: path.display().to_string(),
        command: command(),
    })
}

fn write(path: &PathBuf, settings: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    if path.exists() {
        let bak = path.with_extension("json.deskbunny.bak");
        std::fs::copy(path, &bak).map_err(|e| {
            format!(
                "Could not back up {} to {}: {e}",
                path.display(),
                bak.display()
            )
        })?;
    }
    let mut text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    text.push('\n');
    // Write a sibling and rename, so Claude never reads half a file.
    let tmp = path.with_extension("json.deskbunny.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Could not replace {}: {e}", path.display()))
}

pub fn install() -> Result<HooksStatus, String> {
    let path = settings_path()?;
    write(&path, &with_ours(read(&path)?)?)?;
    status()
}

pub fn remove() -> Result<HooksStatus, String> {
    let path = settings_path()?;
    let settings = read(&path)?;
    if settings.get("hooks").is_some() {
        write(&path, &without_ours(settings))?;
    }
    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_keeps_other_settings_and_hooks() {
        let before = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "notify-send done" }] }],
                "PreCompact": [{ "hooks": [{ "type": "command", "command": "echo hi" }] }]
            }
        });
        let after = with_ours(before.clone()).unwrap();
        assert!(installed_in(&after));
        assert_eq!(after["model"], "opus");
        assert_eq!(after["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(after["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert!(after["hooks"]["Stop"][1].get("matcher").is_none());
        // Installing twice does not duplicate.
        let again = with_ours(after.clone()).unwrap();
        assert_eq!(again, after);
        // Removing restores the original.
        assert_eq!(without_ours(after), before);
    }

    #[test]
    fn remove_drops_an_empty_hooks_block() {
        let after = with_ours(json!({})).unwrap();
        assert_eq!(without_ours(after), json!({}));
        assert!(!installed_in(&json!({})));
    }

    #[test]
    fn command_is_quiet_and_bounded() {
        let c = command();
        assert!(c.contains("-m 1") && c.contains("|| true") && c.contains("47807/claude"));
    }
}
