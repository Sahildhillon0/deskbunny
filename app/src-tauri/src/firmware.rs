//! Bundled firmware: `manifest.json` + images from `firmware/dist/` (resource dir
//! `firmware/`). See docs/ARCHITECTURE.md, "Boards".

use std::path::{Path, PathBuf};

use deskbunny_flash::boards::{builtin_boards, Board};
use deskbunny_flash::FirmwareImage;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

pub const NOT_BUNDLED: &str = "Firmware images are not bundled with this build. Run firmware/build.sh (needs arduino-cli) and rebuild the app.";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageEntry {
    pub file: String,
    pub format: String,
    pub sha256: String,
    pub blob_offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub fw_version: String,
    pub images: Vec<ImageEntry>,
    pub boards: Vec<Board>,
}

pub struct Firmware {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

/// Where firmware may live: the bundled resource dir, then (dev builds) the
/// source tree's firmware/dist.
fn candidate_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        dirs.push(res.join("firmware"));
    }
    if cfg!(debug_assertions) {
        dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../firmware/dist"));
    }
    dirs
}

pub fn load(app: &AppHandle) -> Result<Firmware, String> {
    for dir in candidate_dirs(app) {
        let path = dir.join("manifest.json");
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        let manifest: Manifest = serde_json::from_str(&text).map_err(|e| {
            format!("The bundled firmware manifest is damaged ({e}). Reinstall DeskBunny.")
        })?;
        return Ok(Firmware { dir, manifest });
    }
    Err(NOT_BUNDLED.to_string())
}

/// Boards from the manifest when firmware is bundled, else the built-in table.
pub fn boards(app: &AppHandle) -> Vec<Board> {
    match load(app) {
        Ok(fw) if !fw.manifest.boards.is_empty() => fw.manifest.boards,
        _ => builtin_boards(),
    }
}

impl Firmware {
    pub fn board(&self, id: &str) -> Result<Board, String> {
        self.manifest
            .boards
            .iter()
            .find(|b| b.id == id)
            .cloned()
            .ok_or_else(|| format!("Unknown board '{id}'. Pick a board from the list."))
    }

    /// Load and integrity-check the image for a board.
    pub fn image(&self, board: &Board) -> Result<(FirmwareImage, &ImageEntry), String> {
        let entry = self
            .manifest
            .images
            .iter()
            .find(|i| i.file == board.image)
            .ok_or_else(|| {
                format!(
                    "No firmware image '{}' for {} in this build. {NOT_BUNDLED}",
                    board.image, board.name
                )
            })?;
        let path = self.dir.join(&entry.file);
        let bytes = std::fs::read(&path).map_err(|e| {
            format!(
                "Could not read firmware {}: {e}. {NOT_BUNDLED}",
                path.display()
            )
        })?;
        let digest = hex(&Sha256::digest(&bytes));
        if !digest.eq_ignore_ascii_case(&entry.sha256) {
            return Err(format!(
                "Firmware {} failed its integrity check (sha256 mismatch). Reinstall DeskBunny.",
                entry.file
            ));
        }
        let image = match entry.format.as_str() {
            "hex" => {
                let text = String::from_utf8(bytes)
                    .map_err(|_| format!("Firmware {} is not a text HEX file.", entry.file))?;
                FirmwareImage::from_hex(&text).map_err(|e| e.to_string())?
            }
            "bin" => FirmwareImage::from_bin(board.bin_base(), &bytes),
            other => return Err(format!("Unknown firmware format '{other}' in manifest.")),
        };
        Ok((image, entry))
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
