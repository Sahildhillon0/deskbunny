//! DeskBunny Tauri backend. Every IPC command in docs/ARCHITECTURE.md lives here.
//! The webview is untrusted: commands take narrow, validated inputs and never
//! expose generic file or serial access.

mod debuglog;
mod faces;
mod firmware;
mod setup;
mod update;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deskbunny_core::{presets, Engine, FaceDef};
use deskbunny_flash::boards::{Board, UsbRole};
use deskbunny_flash::link::query_device_info;
use deskbunny_flash::ports::guess_board;
use deskbunny_flash::{patch_face, uploader_for, Report, SerialHost, SystemHost};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

#[derive(Default)]
struct Sims {
    next: AtomicU32,
    engines: Mutex<HashMap<u32, Engine>>,
}

/// Serialises flashes: two uploads at once would fight over ports.
#[derive(Default)]
struct FlashLock(Mutex<()>);

/// Most simulators the UI may keep alive (guards against a leaking frontend).
const MAX_SIMS: usize = 64;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BoardDto {
    id: String,
    name: String,
    mcu: String,
    protocol: deskbunny_flash::Protocol,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PortDto {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    vid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    product: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    serial_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    guessed_board: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FlashReport {
    #[serde(flatten)]
    upload: Report,
    board_id: String,
    face_name: String,
    fw_version: String,
    /// Flash address the face blob was written to.
    blob_address: u32,
    /// The `DESKBUNNY ...` line read back after the upload, if any.
    device_info: Option<String>,
    /// True when the device reported the face that was just flashed.
    device_verified: bool,
}

#[tauri::command]
fn list_presets() -> Vec<FaceDef> {
    presets()
}

#[tauri::command]
fn list_boards(app: AppHandle) -> Vec<BoardDto> {
    firmware::boards(&app)
        .into_iter()
        .map(|b| BoardDto {
            id: b.id,
            name: b.name,
            mcu: b.mcu,
            protocol: b.protocol,
        })
        .collect()
}

#[tauri::command]
async fn list_ports(app: AppHandle) -> CmdResult<Vec<PortDto>> {
    let boards = firmware::boards(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let ports = deskbunny_flash::list_ports().map_err(|e| e.to_string())?;
        Ok(ports
            .into_iter()
            .map(|p| {
                let guessed_board = guess_board(&p, &boards);
                let usb = p.usb;
                PortDto {
                    path: p.path,
                    vid: usb.as_ref().map(|u| format!("{:04X}", u.vid)),
                    pid: usb.as_ref().map(|u| format!("{:04X}", u.pid)),
                    product: usb.as_ref().and_then(|u| u.product.clone()),
                    serial_number: usb.as_ref().and_then(|u| u.serial_number.clone()),
                    guessed_board,
                }
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn sim_create(sims: State<Sims>, face: FaceDef, seed: u32) -> CmdResult<u32> {
    let mut engines = sims
        .engines
        .lock()
        .map_err(|_| "simulator state poisoned")?;
    if engines.len() >= MAX_SIMS {
        return Err("Too many face previews are open; close some and try again.".into());
    }
    let id = sims.next.fetch_add(1, Ordering::Relaxed) + 1;
    engines.insert(id, Engine::new(&face, seed));
    Ok(id)
}

#[tauri::command]
fn sim_set_face(sims: State<Sims>, id: u32, face: FaceDef) -> CmdResult<()> {
    let mut engines = sims
        .engines
        .lock()
        .map_err(|_| "simulator state poisoned")?;
    engines
        .get_mut(&id)
        .ok_or_else(|| format!("No preview with id {id}"))?
        .set_face(&face);
    Ok(())
}

#[tauri::command]
fn sim_frame(sims: State<Sims>, id: u32, t_ms: u32) -> CmdResult<tauri::ipc::Response> {
    let mut engines = sims
        .engines
        .lock()
        .map_err(|_| "simulator state poisoned")?;
    let fb = engines
        .get_mut(&id)
        .ok_or_else(|| format!("No preview with id {id}"))?
        .render(t_ms);
    Ok(tauri::ipc::Response::new(fb.to_vec()))
}

#[tauri::command]
fn sim_drop(sims: State<Sims>, id: u32) -> CmdResult<()> {
    sims.engines
        .lock()
        .map_err(|_| "simulator state poisoned")?
        .remove(&id);
    Ok(())
}

#[tauri::command]
fn encode_face(face: FaceDef) -> Vec<u8> {
    face.encode().to_vec()
}

#[tauri::command]
fn list_saved_faces(app: AppHandle) -> CmdResult<Vec<FaceDef>> {
    faces::list(&app)
}

#[tauri::command]
fn save_face(app: AppHandle, face: FaceDef) -> CmdResult<()> {
    faces::save(&app, &face)
}

#[tauri::command]
fn delete_face(app: AppHandle, name: String) -> CmdResult<()> {
    faces::delete(&app, &name)
}

#[tauri::command]
async fn export_face(app: AppHandle, face: FaceDef) -> CmdResult<Option<String>> {
    // Dialogs block until closed; keep them off the async workers.
    tauri::async_runtime::spawn_blocking(move || faces::export(&app, &face))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn import_face(app: AppHandle) -> CmdResult<Option<FaceDef>> {
    tauri::async_runtime::spawn_blocking(move || faces::import(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn device_info(port: String) -> CmdResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        query_device_info(&SystemHost, &port, Duration::from_millis(3500))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn flash(
    app: AppHandle,
    port: String,
    board_id: String,
    face: FaceDef,
) -> CmdResult<FlashReport> {
    let fw = firmware::load(&app)?;
    let board = fw.board(&board_id)?;
    let (mut image, entry) = fw.image(&board)?;
    let face = face.sanitized();
    let blob_address = patch_face(&mut image, &face.encode()).map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    // manifest blobOffset: HEX address for .hex, file offset for .bin.
    let expected = match entry.format.as_str() {
        "bin" => board.bin_base() + entry.blob_offset,
        _ => entry.blob_offset,
    };
    if blob_address != expected {
        warnings.push(format!(
            "Face slot found at 0x{blob_address:X}, manifest says 0x{expected:X}."
        ));
    }
    let fw_version = fw.manifest.fw_version.clone();

    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let flash_lock = app2.state::<FlashLock>();
        let _guard = flash_lock
            .0
            .try_lock()
            .map_err(|_| "Another upload is already running.".to_string())?;
        let host: Arc<dyn SerialHost> = Arc::new(SystemHost);
        let uploader = uploader_for(&board, host.clone());
        let emit = |p: deskbunny_flash::Progress| {
            let _ = app2.emit("flash-progress", p);
        };
        log::info!(
            "flash: {} ({}) on {port}, protocol {:?}, face {:?}, DeskBunny {} on {} {}",
            board.name,
            board.id,
            board.protocol,
            face.name,
            app2.package_info().version,
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let mut upload = uploader.upload(&port, &image, &emit).map_err(|e| {
            log::error!("flash failed: {e}");
            e.to_string()
        })?;
        log::info!(
            "upload done: {} bytes in {} ms via {}",
            upload.bytes_written,
            upload.duration_ms,
            upload.port
        );
        upload.warnings.extend(warnings);

        let info_port = app_port_after_upload(host.as_ref(), &board, &port);
        let device_info = match info_port {
            Some(p) => query_device_info(host.as_ref(), &p, Duration::from_millis(4000))
                .unwrap_or_else(|e| {
                    upload.warnings.push(format!("Could not read the device banner: {e}"));
                    None
                }),
            None => None,
        };
        log::info!("device banner after flashing: {device_info:?}");
        let device_verified = device_info
            .as_deref()
            .and_then(banner_face_name)
            .is_some_and(|n| n == face.name);
        if device_info.is_none() {
            upload.warnings.push(
                "The board did not report a DeskBunny banner after flashing. The upload was verified; if the screen stays dark, check the OLED wiring.".into(),
            );
        } else if !device_verified {
            upload
                .warnings
                .push("The board is running DeskBunny but reported a different face name.".into());
        }
        Ok(FlashReport {
            upload,
            board_id: board.id.clone(),
            face_name: face.name.clone(),
            fw_version,
            blob_address,
            device_info,
            device_verified,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Face name from `DESKBUNNY <fw_version> <image_stem> <face name>`; the name is
/// the rest of the line and may contain spaces or be empty.
fn banner_face_name(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("DESKBUNNY ")?;
    let mut parts = rest.splitn(3, ' ');
    let _version = parts.next()?;
    let _stem = parts.next()?;
    Some(parts.next().unwrap_or("").trim())
}

/// After an upload, find the port the sketch talks on. Boards with native USB
/// re-enumerate (and may get a new name on Windows/macOS).
fn app_port_after_upload(host: &dyn SerialHost, board: &Board, original: &str) -> Option<String> {
    if !board.touch1200 {
        return Some(original.to_string());
    }
    let app_ids: Vec<(u16, u16)> = board.usb_ids(UsbRole::App).collect();
    let start = Instant::now();
    // Give the board time to leave the bootloader before looking.
    host.sleep(Duration::from_millis(800));
    while start.elapsed() < Duration::from_secs(8) {
        if let Ok(ports) = host.list() {
            if ports.iter().any(|p| p.path == original) {
                return Some(original.to_string());
            }
            if let Some(p) = ports.iter().find(|p| {
                p.usb
                    .as_ref()
                    .is_some_and(|u| app_ids.contains(&(u.vid, u.pid)))
            }) {
                return Some(p.path.clone());
            }
        }
        host.sleep(Duration::from_millis(250));
    }
    None
}

#[tauri::command]
fn debug_log() -> Vec<debuglog::LogEntry> {
    debuglog::lines()
}

#[tauri::command]
fn debug_log_clear() {
    debuglog::clear();
}

#[tauri::command]
async fn debug_usb_scan() {
    let _ = tauri::async_runtime::spawn_blocking(deskbunny_flash::ports::log_system_scan).await;
}

#[tauri::command]
async fn setup_status(app: AppHandle) -> setup::SetupStatus {
    // Checks shell out (pnputil, sw_vers) and read rule files: keep them off the UI thread.
    tauri::async_runtime::spawn_blocking(move || setup::status(&app))
        .await
        .expect("setup status task panicked")
}

#[tauri::command]
async fn setup_run(app: AppHandle, action: String) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || setup::run(&app, &action))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn setup_finish(app: AppHandle) -> CmdResult<()> {
    setup::finish(&app)
}

#[tauri::command]
fn setup_open_link(app: AppHandle, id: String) -> CmdResult<()> {
    setup::open_link(&app, &id)
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
async fn update_check(app: AppHandle) -> CmdResult<Option<update::UpdateInfo>> {
    update::check(&app).await
}

#[tauri::command]
async fn update_install(app: AppHandle) -> CmdResult<()> {
    update::install(&app).await
}

#[tauri::command]
fn update_open_releases(app: AppHandle) -> CmdResult<()> {
    update::open_releases(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    debuglog::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Sims::default())
        .manage(update::Pending::default())
        .manage(FlashLock::default())
        .setup(|app| {
            debuglog::attach(app.handle());
            log::info!(
                "DeskBunny {} started on {} {}",
                app.package_info().version,
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_presets,
            list_boards,
            list_ports,
            sim_create,
            sim_set_face,
            sim_frame,
            sim_drop,
            flash,
            device_info,
            list_saved_faces,
            save_face,
            delete_face,
            encode_face,
            export_face,
            import_face,
            setup_status,
            setup_run,
            setup_finish,
            setup_open_link,
            app_version,
            update_check,
            update_install,
            update_open_releases,
            debug_log,
            debug_log_clear,
            debug_usb_scan,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DeskBunny");
}

#[cfg(test)]
mod tests {
    use super::banner_face_name;

    #[test]
    fn parses_banner() {
        assert_eq!(
            banner_face_name("DESKBUNNY 0.1.0 atmega328p MY BUNNY"),
            Some("MY BUNNY")
        );
        assert_eq!(banner_face_name("DESKBUNNY 0.1.0 uno_r4_wifi"), Some(""));
        assert_eq!(banner_face_name("DESKBUNNY 0.1.0 atmega328p "), Some(""));
        assert_eq!(banner_face_name("hello"), None);
    }
}
