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
    /// Face slot offset; `null` for images that are never patched (ESP32).
    #[serde(default)]
    pub blob_offset: Option<u32>,
    /// ESP32 images: the chip they run on ("esp32" | "esp32c3" | "esp32s3").
    #[serde(default)]
    pub chip: Option<String>,
    /// ESP32 images: flash address the file is written at (0 for merged images).
    #[serde(default)]
    pub flash_offset: Option<u32>,
    /// Drawing slot size in bytes; absent in manifests from before drawing blob v2.
    #[serde(default)]
    pub art_capacity: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub fw_version: String,
    pub images: Vec<ImageEntry>,
    pub boards: Vec<Board>,
}

#[derive(Clone)]
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
        let manifest = parse_manifest(&text)?;
        return Ok(Firmware { dir, manifest });
    }
    Err(NOT_BUNDLED.to_string())
}

pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    serde_json::from_str(text).map_err(|e| {
        format!("The bundled firmware manifest is damaged ({e}). Reinstall DeskBunny.")
    })
}

/// Drawing slot size of each board's image (None when unknown or not bundled).
pub fn art_capacities(app: &AppHandle) -> Vec<(String, Option<u32>)> {
    let Ok(fw) = load(app) else {
        return Vec::new();
    };
    fw.manifest
        .boards
        .iter()
        .map(|b| {
            let cap = fw
                .manifest
                .images
                .iter()
                .find(|i| i.file == b.image)
                .and_then(|i| i.art_capacity);
            (b.id.clone(), cap)
        })
        .collect()
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
        if let Some(chip) = &entry.chip {
            if !chip.eq_ignore_ascii_case(&board.mcu) {
                return Err(format!(
                    "Firmware {} is built for {chip}, not {} ({}). Reinstall DeskBunny.",
                    entry.file, board.name, board.mcu
                ));
            }
        }
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
            "bin" => FirmwareImage::from_bin(
                entry.flash_offset.unwrap_or_else(|| board.bin_base()),
                &bytes,
            ),
            other => return Err(format!("Unknown firmware format '{other}' in manifest.")),
        };
        Ok((image, entry))
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskbunny_flash::Protocol;

    const MANIFEST: &str = r#"{
      "fwVersion": "0.2.0",
      "images": [
        { "file": "atmega328p.hex", "format": "hex", "sha256": "aa",
          "blobOffset": 8780, "artOffset": 133, "artCapacity": 8192 },
        { "file": "uno_r4_minima.bin", "format": "bin", "sha256": "bb",
          "blobOffset": 117816, "artOffset": 54764, "artCapacity": 61440 },
        { "file": "esp32c3.bin", "format": "bin", "sha256": "cc", "chip": "esp32c3",
          "flashOffset": 0, "blobOffset": null, "artOffset": null }
      ],
      "boards": [
        { "id": "uno", "name": "Uno R3", "fqbn": "arduino:avr:uno", "mcu": "atmega328p",
          "protocol": "stk500v1", "baud": [115200], "touch1200": false,
          "image": "atmega328p.hex", "usb": [{ "vid": "2341", "pid": "0043", "role": "app" }] },
        { "id": "esp32c3", "name": "ESP32-C3", "fqbn": "esp32:esp32:esp32c3:CDCOnBoot=cdc",
          "mcu": "esp32c3", "protocol": "esptool", "baud": [460800, 115200],
          "touch1200": false, "image": "esp32c3.bin",
          "usb": [{ "vid": "303A", "pid": "1001", "role": "app" }] }
      ]
    }"#;

    #[test]
    fn parses_mixed_manifest() {
        let m = parse_manifest(MANIFEST).unwrap();
        assert_eq!(m.images[0].blob_offset, Some(8780));
        assert_eq!(m.images[0].art_capacity, Some(8192));
        assert_eq!(m.images[0].chip, None);
        assert_eq!(m.images[1].blob_offset, Some(117816));
        let esp = &m.images[2];
        assert_eq!(esp.blob_offset, None);
        assert_eq!(esp.art_capacity, None);
        assert_eq!(esp.chip.as_deref(), Some("esp32c3"));
        assert_eq!(esp.flash_offset, Some(0));
        assert_eq!(m.boards[1].protocol, Protocol::Esptool);
    }

    #[test]
    fn esp_image_loads_at_flash_offset_without_patching() {
        let dir = std::env::temp_dir().join(format!("nixxa-fw-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes: Vec<u8> = (0..4096u32).map(|i| i as u8).collect();
        std::fs::write(dir.join("esp32c3.bin"), &bytes).unwrap();
        let mut manifest = parse_manifest(MANIFEST).unwrap();
        manifest.images[2].sha256 = hex(&Sha256::digest(&bytes));
        let fw = Firmware {
            dir: dir.clone(),
            manifest,
        };
        let board = fw.board("esp32c3").unwrap();
        let (img, entry) = fw.image(&board).unwrap();
        assert_eq!(entry.blob_offset, None);
        assert_eq!(img.base(), 0);
        assert_eq!(img.flatten(0, 4096, 0xFF), bytes);

        // An image whose chip does not match the board is refused.
        let mut wrong = fw.clone();
        wrong.manifest.images[2].chip = Some("esp32s3".into());
        assert!(wrong
            .image(&board)
            .unwrap_err()
            .contains("built for esp32s3"));

        // A tampered file is refused.
        std::fs::write(dir.join("esp32c3.bin"), [1u8, 2, 3]).unwrap();
        assert!(fw.image(&board).unwrap_err().contains("integrity"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
