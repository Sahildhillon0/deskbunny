//! DeskBunny Tauri backend. Every IPC command in docs/ARCHITECTURE.md lives here.
//! The webview is untrusted: commands take narrow, validated inputs and never
//! expose generic file or serial access.

mod claude;
mod desktop;
mod faces;
mod firmware;
mod hooks;
mod live;
mod server;
mod setup;
mod update;
mod weather;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deskbunny_core::{presets, Engine, FaceDef};
use deskbunny_flash::boards::{Board, Protocol, UsbRole};
use deskbunny_flash::esptool::{self, Esptool};
use deskbunny_flash::link::query_device_info;
use deskbunny_flash::ports::guess_board;
use deskbunny_flash::{
    art_capacity, patch_art, patch_face, uploader_for, FlashError, Report, SerialHost, SystemHost,
    Uploader,
};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

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
    /// Bytes the board's drawing slot holds; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    art_capacity: Option<u32>,
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
    let caps = firmware::art_capacities(&app);
    firmware::boards(&app)
        .into_iter()
        .map(|b| BoardDto {
            art_capacity: caps
                .iter()
                .find(|(id, _)| *id == b.id)
                .and_then(|(_, c)| *c),
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
async fn device_info(app: AppHandle, port: String) -> CmdResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let _pause = app.state::<Arc<live::Live>>().pause();
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
    if esptool::is_esp_board(&board) {
        // ESP32 images carry no face slot: faces are sent from the phone app.
        return flash_esp(app, fw, board, port).await;
    }
    let (mut image, entry) = fw.image(&board)?;
    let face = face.sanitized();
    let blob_address = patch_face(&mut image, &face.encode()).map_err(|e| e.to_string())?;
    // The image ships with the drawing off; only a face with a drawing needs the slot.
    if let Some(drawing) = &face.drawing {
        let cap = art_capacity(&image).map_err(|e| {
            format!("{e}. This firmware predates long animations; update DeskBunny to flash one.")
        })?;
        let blob = drawing.encode(cap)?;
        patch_art(&mut image, &blob).map_err(|e| e.to_string())?;
    }
    let mut warnings = Vec::new();
    // manifest blobOffset: HEX address for .hex, file offset for .bin.
    let expected = entry.blob_offset.map(|off| match entry.format.as_str() {
        "bin" => board.bin_base() + off,
        _ => off,
    });
    if let Some(expected) = expected.filter(|e| *e != blob_address) {
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
        // Live mode lets go of the port (and resumes when this returns).
        let _pause = app2.state::<Arc<live::Live>>().pause();
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

/// Flash the nixxa firmware onto an ESP32-family board (docs/ARCHITECTURE.md,
/// "Desktop flashing"). The face is ignored; the image is never patched.
async fn flash_esp(
    app: AppHandle,
    fw: firmware::Firmware,
    board: Board,
    port: String,
) -> CmdResult<FlashReport> {
    let (image, _entry) = fw.image(&board)?;
    let fw_version = fw.manifest.fw_version.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let flash_lock = app.state::<FlashLock>();
        let _guard = flash_lock
            .0
            .try_lock()
            .map_err(|_| "Another upload is already running.".to_string())?;
        let host: Arc<dyn SerialHost> = Arc::new(SystemHost);
        // Chip auto-switch: load another ESP32 board's image on demand.
        let fw2 = fw.clone();
        let uploader = Esptool::new(board.clone(), host).with_resolver(Box::new(move |id| {
            let b = fw2.board(id).map_err(FlashError::Image)?;
            fw2.image(&b).map(|(img, _)| img).map_err(FlashError::Image)
        }));
        let emit = |p: deskbunny_flash::Progress| {
            let _ = app.emit("flash-progress", p);
        };
        log::info!(
            "flash: {} ({}) on {port}, protocol esptool, nixxa firmware {fw_version}, DeskBunny {} on {} {}",
            board.name,
            board.id,
            app.package_info().version,
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let mut upload = uploader.upload(&port, &image, &emit).map_err(|e| {
            log::error!("flash failed: {e}");
            e.to_string()
        })?;
        let board_id = upload.flashed_board.clone().unwrap_or_else(|| board.id.clone());
        log::info!(
            "upload done: {} bytes in {} ms via {} ({board_id})",
            upload.bytes_written,
            upload.duration_ms,
            upload.port
        );
        let device_info = uploader.banner(&port).unwrap_or_else(|e| {
            upload
                .warnings
                .push(format!("Could not read the device banner: {e}"));
            None
        });
        log::info!("device banner after flashing: {device_info:?}");
        // ESP32 banners carry the board id; there is no flashed face to compare.
        let device_verified = device_info
            .as_deref()
            .and_then(banner_board_id)
            .is_some_and(|id| id == board_id);
        if device_info.is_none() {
            upload.warnings.push(
                "The board did not report a nixxa banner after flashing. The upload was verified; if the screen stays dark, check the OLED wiring (SDA/SCL).".into(),
            );
        } else if !device_verified {
            upload
                .warnings
                .push("The board is running nixxa but reported a different board type.".into());
        }
        Ok(FlashReport {
            upload,
            board_id,
            face_name: String::new(),
            fw_version,
            blob_address: 0,
            device_info,
            device_verified,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Enter the ESP32 ROM bootloader on `port`, report the chip as a board id and
/// reset back into the app. Only ports whose USB ID belongs to an ESP32 board are
/// touched; anything else is `None` without opening the port.
#[tauri::command]
async fn detect_chip(app: AppHandle, port: String) -> CmdResult<Option<String>> {
    let esp_ids: Vec<(u16, u16)> = firmware::boards(&app)
        .iter()
        .filter(|b| b.protocol == Protocol::Esptool)
        .flat_map(|b| b.usb.iter().map(|u| (u.vid, u.pid)))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let flash_lock = app.state::<FlashLock>();
        let _guard = flash_lock
            .0
            .try_lock()
            .map_err(|_| "An upload is running; try again when it has finished.".to_string())?;
        let ports = deskbunny_flash::list_ports().map_err(|e| e.to_string())?;
        let is_esp = ports.iter().any(|p| {
            p.path == port
                && p.usb
                    .as_ref()
                    .is_some_and(|u| esp_ids.contains(&(u.vid, u.pid)))
        });
        if !is_esp {
            log::info!("detect_chip: {port} is not an ESP32 USB port; skipped");
            return Ok(None);
        }
        esptool::detect_chip(&port)
            .map(|c| c.map(str::to_string))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Board id from `DESKBUNNY <fw_version> <board_id> ...`.
fn banner_board_id(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("DESKBUNNY ")?;
    rest.split(' ').nth(1).filter(|s| !s.is_empty())
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

#[tauri::command]
fn live_get_config(live: State<Arc<live::Live>>) -> live::LiveConfig {
    live.config()
}

#[tauri::command]
fn live_set_config(
    live: State<Arc<live::Live>>,
    config: live::LiveConfig,
) -> CmdResult<live::LiveConfig> {
    live.set_config(config)
}

#[tauri::command]
fn live_status(live: State<Arc<live::Live>>) -> live::LiveStatus {
    live.status()
}

#[tauri::command]
fn live_pin(live: State<Arc<live::Live>>, screen: Option<live::ScreenRef>) {
    live.pin(screen);
}

#[tauri::command]
fn live_preview(
    live: State<Arc<live::Live>>,
    screen: live::ScreenRef,
    t_ms: u32,
) -> tauri::ipc::Response {
    let fb = live::preview(&live, &screen, t_ms);
    tauri::ipc::Response::new(fb.to_vec())
}

// --- designs ------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesignRender {
    frame: Vec<u8>,
    bounds: Vec<deskbunny_core::layout::Bounds>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesignCatalog {
    templates: Vec<deskbunny_core::layout::Design>,
    tokens: Vec<(String, String)>,
    icons: Vec<String>,
}

/// Limits that keep a design (and designs.json) sane; the editor stays inside them.
fn check_design(d: &deskbunny_core::layout::Design) -> CmdResult<()> {
    use deskbunny_core::layout::Element;
    let id_ok = !d.id.is_empty()
        && d.id.len() <= 40
        && d.id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !id_ok {
        return Err("A design id is 1-40 characters of a-z, 0-9, - and _.".into());
    }
    if d.name.trim().is_empty() || d.name.chars().count() > 32 {
        return Err("Give the design a name of up to 32 characters.".into());
    }
    if d.elements.len() > 64 {
        return Err("A design holds at most 64 elements.".into());
    }
    for e in &d.elements {
        match e {
            Element::Text { text, .. } if text.chars().count() > 80 => {
                return Err("A text element holds at most 80 characters.".into())
            }
            Element::Bitmap { w, h, data, .. } if *w > 128 || *h > 64 || data.len() > 2000 => {
                return Err("A drawing is at most 128x64 pixels.".into())
            }
            _ => {}
        }
    }
    Ok(())
}

#[tauri::command]
fn design_list(live: State<Arc<live::Live>>) -> Vec<deskbunny_core::layout::Design> {
    live.designs()
}

#[tauri::command]
fn design_catalog() -> DesignCatalog {
    use deskbunny_core::layout;
    DesignCatalog {
        templates: layout::templates(),
        tokens: layout::TOKENS
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        icons: layout::ICONS.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// Insert or replace (by id) and save.
#[tauri::command]
fn design_save(
    live: State<Arc<live::Live>>,
    design: deskbunny_core::layout::Design,
) -> CmdResult<Vec<deskbunny_core::layout::Design>> {
    let mut design = design;
    design.name = design.name.trim().to_string();
    check_design(&design)?;
    let mut all = live.designs();
    match all.iter_mut().find(|d| d.id == design.id) {
        Some(d) => *d = design,
        None => all.push(design),
    }
    live.set_designs(all)?;
    Ok(live.designs())
}

#[tauri::command]
fn design_delete(
    live: State<Arc<live::Live>>,
    id: String,
) -> CmdResult<Vec<deskbunny_core::layout::Design>> {
    let mut all = live.designs();
    all.retain(|d| d.id != id);
    live.set_designs(all)?;
    Ok(live.designs())
}

/// Render an unsaved design (the editor's preview), with element bounds.
#[tauri::command]
fn design_render(
    live: State<Arc<live::Live>>,
    design: deskbunny_core::layout::Design,
) -> DesignRender {
    let (fb, bounds) = live.render_design(&design);
    DesignRender {
        frame: fb.to_vec(),
        bounds,
    }
}

// --- weather ------------------------------------------------------------------

#[tauri::command]
async fn weather_search(query: String) -> CmdResult<Vec<weather::Place>> {
    tauri::async_runtime::spawn_blocking(move || weather::search(&query))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn weather_now(live: State<Arc<live::Live>>) -> weather::WeatherNow {
    let mut w = live.weather_now();
    w.age_secs = w
        .fetched
        .filter(|_| w.weather.is_some())
        .map(|t| t.elapsed().as_secs());
    w
}

#[tauri::command]
fn claude_hooks_status() -> CmdResult<hooks::HooksStatus> {
    hooks::status()
}

#[tauri::command]
fn claude_hooks_install() -> CmdResult<hooks::HooksStatus> {
    hooks::install()
}

#[tauri::command]
fn claude_hooks_remove() -> CmdResult<hooks::HooksStatus> {
    hooks::remove()
}

#[tauri::command]
fn claude_view(live: State<Arc<live::Live>>) -> claude::ClaudeView {
    live.claude_view()
}

/// Puts a sample notification on the board (the panel's "Test" button).
#[tauri::command]
fn live_test_notification(live: State<Arc<live::Live>>) {
    live.notify(
        "DeskBunny",
        "Test notification",
        "This is how a desktop notification looks on the board.",
    );
}

/// Tray icon: live mode keeps running with the window closed.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show DeskBunny", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("DeskBunny")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Bring the main window to the front (from the tray, or a second launch).
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // One copy only: the live service owns the serial port and 127.0.0.1:47807.
    // Launching again (app menu, while the tray copy runs) shows that window.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        show_main(app)
    }));
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Sims::default())
        .manage(update::Pending::default())
        .manage(FlashLock::default())
        .setup(|app| {
            let live = live::start(app.handle());
            server::start(live.clone());
            desktop::start(live.clone());
            weather::start(live.clone());
            app.manage(live);
            let tray = build_tray(app);
            if let Err(e) = &tray {
                // No tray (e.g. a desktop without one): closing then quits as before.
                log::warn!("tray icon unavailable: {e}");
            }
            // The window starts hidden (tauri.conf.json). `--hidden` (autostart
            // at login) keeps it that way and lives in the tray; without a
            // tray there would be no way back to it, so show it then.
            let hidden = std::env::args().any(|a| a == "--hidden");
            if !hidden || tray.is_err() {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // With live mode on, closing the window hides it; the tray quits.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let on = app.state::<Arc<live::Live>>().config().enabled;
                if on && app.tray_by_id("main").is_some() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
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
            detect_chip,
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
            live_get_config,
            live_set_config,
            live_status,
            live_pin,
            live_preview,
            live_test_notification,
            claude_hooks_status,
            claude_hooks_install,
            claude_hooks_remove,
            claude_view,
            design_list,
            design_catalog,
            design_save,
            design_delete,
            design_render,
            weather_search,
            weather_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DeskBunny");
}

#[cfg(test)]
mod tests {
    use super::{banner_board_id, banner_face_name};

    #[test]
    fn parses_banner_board_id() {
        assert_eq!(
            banner_board_id("DESKBUNNY 0.2.0 esp32c3 NEUTRAL"),
            Some("esp32c3")
        );
        assert_eq!(banner_board_id("DESKBUNNY 0.2.0 esp32"), Some("esp32"));
        assert_eq!(banner_board_id("DESKBUNNY 0.2.0"), None);
        assert_eq!(banner_board_id("nope"), None);
    }

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
