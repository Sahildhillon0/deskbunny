//! Saved faces: one JSON file per face in `<app data>/faces/`.

use std::path::PathBuf;

use deskbunny_core::{normalize_name, FaceDef};
use tauri::{AppHandle, Manager};

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not find the app data folder: {e}"))?
        .join("faces");
    std::fs::create_dir_all(&d).map_err(|e| format!("Could not create {}: {e}", d.display()))?;
    Ok(d)
}

/// File name for a face. Names are restricted to A-Z, 0-9 and spaces, so this
/// cannot escape the faces folder.
fn file_name(name: &str) -> Result<String, String> {
    let n = normalize_name(name);
    if n.is_empty() {
        return Err("Face name is empty.".into());
    }
    Ok(format!("{}.json", n.replace(' ', "_")))
}

pub fn list(app: &AppHandle) -> Result<Vec<FaceDef>, String> {
    let d = dir(app)?;
    let mut faces = Vec::new();
    let entries =
        std::fs::read_dir(&d).map_err(|e| format!("Could not read {}: {e}", d.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // Skip unreadable or foreign files rather than failing the whole list.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(face) = serde_json::from_str::<FaceDef>(&text) else {
            continue;
        };
        faces.push(face.sanitized());
    }
    faces.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(faces)
}

pub fn save(app: &AppHandle, face: &FaceDef) -> Result<(), String> {
    face.validate().map_err(|e| e.to_string())?;
    let face = face.sanitized();
    let path = dir(app)?.join(file_name(&face.name)?);
    let json = serde_json::to_string_pretty(&face).map_err(|e| e.to_string())?;
    // Write then rename so a crash never leaves a half-written face.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("Could not save face: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Could not save face: {e}"))
}

pub fn delete(app: &AppHandle, name: &str) -> Result<(), String> {
    let path = dir(app)?.join(file_name(name)?);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not delete face: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_confined() {
        assert_eq!(file_name("my bunny").unwrap(), "MY_BUNNY.json");
        assert_eq!(
            file_name("../../etc/passwd").unwrap(),
            "ETC PASSWD".replace(' ', "_") + ".json"
        );
        assert!(file_name("///").is_err());
    }
}
