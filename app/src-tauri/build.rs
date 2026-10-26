fn main() {
    // The firmware images are bundled as resources. They are built by
    // firmware/build.sh (CI); create the directory so a dev build without them
    // still compiles (the app then reports "firmware images not bundled").
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../firmware/dist");
    let _ = std::fs::create_dir_all(dist);
    tauri_build::build()
}
