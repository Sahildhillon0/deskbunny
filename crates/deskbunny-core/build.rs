//! Compile the shared C face engine (single source of truth for rendering and the
//! blob codec) plus a tiny layout shim used to check the Rust FFI structs.
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let engine = manifest.join("../../engine");
    let src = engine.join("deskbunny_engine.c");
    let hdr = engine.join("deskbunny_engine.h");
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed={}", hdr.display());
    println!("cargo:rerun-if-changed=build.rs");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let shim = out.join("layout_shim.c");
    std::fs::write(
        &shim,
        "#include \"deskbunny_engine.h\"\n\
         size_t db_sizeof_face(void) { return sizeof(db_face_t); }\n\
         size_t db_sizeof_engine(void) { return sizeof(db_engine_t); }\n",
    )
    .expect("write layout shim");

    cc::Build::new()
        .std("c99")
        .file(&src)
        .file(&shim)
        .include(&engine)
        .warnings(true)
        .compile("deskbunny_engine");
}
