//! Local HTTP endpoint for things that want to change the screen without
//! touching the serial port (docs/ARCHITECTURE.md, "Local endpoint").
//!
//! Listens on 127.0.0.1:47807 only. Every request must carry the header
//! `X-DeskBunny`: a browser cannot send a custom header to another origin
//! without a CORS preflight, which this server never answers, so a web page
//! cannot post here. Local programs can, which is the point.
//!
//! - `POST /claude`  body: a Claude Code hook payload (JSON, as the hook gets it)
//! - `POST /notify`  body: `{"title", "body"?, "app"?}` or plain text
//! - `GET  /status`  the Claude view and live status, as JSON

use std::io::Read;
use std::sync::Arc;

use serde_json::{json, Value};
use tiny_http::{Header, Method, Response, Server};

use crate::live::Live;

pub const PORT: u16 = 47807;
/// Hook payloads include tool inputs (a whole file for Write); cap the read.
const MAX_BODY: u64 = 8 << 20;

pub fn start(live: Arc<Live>) {
    let server = match Server::http(("127.0.0.1", PORT)) {
        Ok(s) => s,
        Err(e) => {
            // Usually a second DeskBunny already has it; that one gets the hooks.
            log::warn!("local endpoint: cannot listen on 127.0.0.1:{PORT}: {e}");
            return;
        }
    };
    log::info!("local endpoint on http://127.0.0.1:{PORT}");
    let spawned = std::thread::Builder::new()
        .name("deskbunny-http".into())
        .spawn(move || {
            for req in server.incoming_requests() {
                handle(req, &live);
            }
        });
    if let Err(e) = spawned {
        log::warn!("local endpoint: thread failed to start: {e}");
    }
}

fn reply(req: tiny_http::Request, code: u16, body: Value) {
    let ct = Header::from_bytes("Content-Type", "application/json").expect("static header");
    let _ = req.respond(
        Response::from_string(body.to_string())
            .with_status_code(code)
            .with_header(ct),
    );
}

fn handle(mut req: tiny_http::Request, live: &Live) {
    let ours = req.headers().iter().any(|h| h.field.equiv("X-DeskBunny"));
    if !ours {
        return reply(req, 403, json!({"error": "missing X-DeskBunny header"}));
    }
    let method = req.method().clone();
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let mut body = String::new();
    if method == Method::Post {
        let read = req.as_reader().take(MAX_BODY).read_to_string(&mut body);
        if read.is_err() {
            return reply(req, 400, json!({"error": "body is not UTF-8"}));
        }
    }
    match (method, path.as_str()) {
        (Method::Post, "/claude") => {
            let Ok(v) = serde_json::from_str::<Value>(&body) else {
                return reply(req, 400, json!({"error": "body is not JSON"}));
            };
            if live.claude_event(&v) {
                reply(req, 200, json!({"ok": true}))
            } else {
                reply(req, 400, json!({"error": "not a Claude Code hook payload"}))
            }
        }
        (Method::Post, "/usage") => {
            let Ok(v) = serde_json::from_str::<Value>(&body) else {
                return reply(req, 400, json!({"error": "body is not JSON"}));
            };
            match live.usage_event(&v) {
                Ok(source) => reply(req, 200, json!({"ok": true, "source": source})),
                Err(e) => reply(req, 400, json!({"error": e})),
            }
        }
        (Method::Post, "/notify") => {
            let n = parse_notification(&body);
            match n {
                Some((app, title, text)) => {
                    live.notify(&app, &title, &text);
                    reply(req, 200, json!({"ok": true}))
                }
                None => reply(req, 400, json!({"error": "give a title"})),
            }
        }
        (Method::Get, "/status") => {
            let v = json!({
                "claude": live.claude_view(),
                "usage": live.usage_now(),
                "live": live.status(),
            });
            reply(req, 200, v)
        }
        _ => reply(req, 404, json!({"error": "unknown endpoint"})),
    }
}

/// `{"title", "body"?, "app"?}`, or plain text (first line title, rest body).
pub fn parse_notification(body: &str) -> Option<(String, String, String)> {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if v.is_object() {
            let s = |k: &str| {
                v.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };
            let title = s("title");
            return (!title.is_empty()).then(|| (s("app"), title, s("body")));
        }
    }
    let mut lines = body.trim().splitn(2, '\n');
    let title = lines.next().unwrap_or("").trim().to_string();
    let text = lines.next().unwrap_or("").trim().to_string();
    (!title.is_empty()).then(|| (String::new(), title, text))
}

#[cfg(test)]
mod tests {
    use super::parse_notification;

    #[test]
    fn notifications_parse() {
        assert_eq!(
            parse_notification(r#"{"title":"Build","body":"passed","app":"CI"}"#),
            Some(("CI".into(), "Build".into(), "passed".into()))
        );
        assert_eq!(
            parse_notification("Backup done\n42 GB"),
            Some(("".into(), "Backup done".into(), "42 GB".into()))
        );
        assert_eq!(parse_notification(r#"{"body":"x"}"#), None);
        assert_eq!(parse_notification("  "), None);
    }
}
