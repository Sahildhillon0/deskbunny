//! In-app updates from GitHub Releases (tauri-plugin-updater, driven from Rust so
//! the webview needs no updater permission). Contract: docs/ARCHITECTURE.md.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};

const RELEASES_URL: &str = "https://github.com/Sahildhillon0/deskbunny/releases/latest";

/// The update found by the last `update_check`, installed by `update_install`.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current_version: String,
    date: Option<String>,
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app
        .updater()
        .map_err(|e| format!("Updates are not available in this build: {e}"))?;
    let found = updater.check().await.map_err(|e| {
        format!(
            "Could not check for updates: {e}. Are you online? You can also look at {RELEASES_URL}"
        )
    })?;
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        date: u.date.map(|d| d.date().to_string()),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
    });
    *app.state::<Pending>().0.lock().unwrap() = found;
    Ok(info)
}

/// Downloads and installs the pending update, then restarts. Returns only on error.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = app
        .state::<Pending>()
        .0
        .lock()
        .unwrap()
        .take()
        .ok_or("No update to install. Check for updates first.")?;
    let mut downloaded = 0u64;
    let emitter = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = emitter.emit("update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| {
            format!(
                "Could not install the update: {e}. Download version {} from the Releases page instead.",
                update.version
            )
        })?;
    app.restart()
}

pub fn open_releases(app: &AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(RELEASES_URL, None::<&str>)
        .map_err(|e| format!("Could not open the browser: {e}. The address is {RELEASES_URL}"))
}
