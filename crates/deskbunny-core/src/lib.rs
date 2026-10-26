//! DeskBunny core: the C face engine over FFI, the 64-byte face blob codec and
//! the built-in presets. Contract: docs/ARCHITECTURE.md.

pub mod engine;
pub mod face;
#[allow(non_camel_case_types)]
pub mod ffi;
pub mod presets;

pub use engine::{Engine, FB_SIZE};
pub use face::{crc16, normalize_name, FaceDef, FaceError, Mood, Overlay, BLOB_SIZE};
pub use presets::{default_preset, presets};
