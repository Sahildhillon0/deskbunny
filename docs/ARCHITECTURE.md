# DeskBunny architecture and contracts

DeskBunny is a desktop app (Rust + Tauri v2) that flashes an animated face onto
an Arduino driving a 0.96" 128x64 SSD1306 OLED over bit-banged SPI. No Arduino
IDE, no avrdude, no compiler on the user's machine. One face per flash: to
change faces, flash again.

This file is the contract between the parts. Change it before changing an
interface, never after.

## How it works

```
             CI (arduino-cli)                         User machine
 engine/*.c ──┬─► firmware/<board>.hex|.bin ──bundled──► Tauri app
              │      (contains a 64-byte FACE BLOB        │  1. user picks/edits a face
              │       with a magic marker)                │  2. app encodes face -> 64-byte blob
              │                                           │  3. app patches blob into the image
              └─► compiled into the app via `cc` ─────────┤  4. app uploads image (pure Rust flasher)
                  (simulator = same C code, same pixels)  │  5. bunny runs autonomously
```

- **One face engine, written once in C99** (`engine/`). Firmware runs it on the
  board. The app compiles the same files through the `cc` crate and calls it
  over FFI for the simulator, so the preview is pixel-identical to the OLED.
- **Prebuilt firmware per board** is built in CI and bundled as Tauri resources.
  Each image carries `DB_FACE_BLOB`, 64 bytes that start with the magic
  `DBFACE01`. The flasher finds the magic (exactly one occurrence, enforced in
  CI), overwrites the 64 bytes and uploads.
- **Autonomous device**: after flashing, the board needs only USB power. It
  blinks, glances around, breathes, etc. according to the blob.

## Repo layout and ownership

```
engine/                      C face engine (portable, no malloc, no floats required)
  deskbunny_engine.h         PUBLIC API (contract, below)
  deskbunny_engine.c
  tools/render.c             host tool: blob + time -> PBM/PNG frames (golden tests)
  tests/                     host tests (gcc/clang), golden frames
firmware/
  deskbunny/deskbunny.ino    sketch: SSD1306 bare-metal driver + engine + blob
  deskbunny/*.c|*.h          copies/symlinks of engine sources are NOT allowed;
                             build script copies engine/ into the sketch at build time
  boards.json                board table (below), single source of truth
  build.sh                   arduino-cli build of every board -> firmware/dist/
crates/
  deskbunny-core/            Rust: engine FFI (cc build of ../../engine), blob
                             encode/decode, FaceDef, presets
  deskbunny-flash/           Rust: Intel HEX parse/emit, blob patching, port
                             discovery, protocol backends
app/                         Tauri v2 app
  package.json, vite.config.ts, src/   frontend (Svelte 5 + TS + Vite, npm)
  src-tauri/                 Tauri crate (commands, events, resources)
Cargo.toml                   workspace: crates/*, app/src-tauri
.github/workflows/           CI + release
```

## Face blob v1 (64 bytes, little-endian)

| Off | Size | Field | Notes / default |
|----:|----:|-------|-----------------|
| 0 | 8 | magic | ASCII `DBFACE01` |
| 8 | 1 | version | `1` |
| 9 | 1 | mood | enum below |
| 10 | 12 | name | ASCII, NUL-padded, uppercase A-Z 0-9 space (font coverage) |
| 22 | 1 | eye_w | px, 8..56, default 36 |
| 23 | 1 | eye_h | px, 8..56, default 36 |
| 24 | 1 | eye_r | corner radius px, 0..eye_w/2, default 8 |
| 25 | 1 | eye_gap | px between eyes, 0..40, default 16 |
| 26 | 1 | eye_y | int8 vertical offset from centre, default 0 |
| 27 | 1 | flags | bit0 blink, bit1 look_around ("watching"), bit2 breathe, bit3 cyclops (one eye), bit4 invert, bit5 rotate180, bit6 mouth, bit7 reserved |
| 28 | 1 | blink_min | units of 100 ms, default 20 |
| 29 | 1 | blink_max | units of 100 ms, default 60 |
| 30 | 1 | look_min | units of 100 ms, default 15 |
| 31 | 1 | look_max | units of 100 ms, default 50 |
| 32 | 1 | look_x | max horizontal glance px, default 18 |
| 33 | 1 | look_y | max vertical glance px, default 8 |
| 34 | 1 | speed | 1..10 animation speed, default 5 |
| 35 | 1 | contrast | SSD1306 0x81 value, default 0xCF |
| 36 | 1 | overlay | 0 none, 1 hearts, 2 zzz, 3 sweat, 4 anger mark, 5 tears, 6 sparkles, 7 question mark |
| 37 | 1 | seed | PRNG seed salt, default 0 |
| 38 | 24 | reserved | must be 0 |
| 62 | 2 | crc16 | CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF) over bytes 0..61 |

Moods: `0 neutral, 1 happy, 2 angry, 3 sad, 4 sleepy, 5 surprised, 6 love,
7 suspicious, 8 excited, 9 bored, 10 scared, 11 dizzy`.

If magic/version/CRC is bad, the firmware falls back to `db_face_default()`
(neutral, blink + look_around + breathe, name `DESKBUNNY`) and shows it.
A blob with a valid CRC but out-of-range fields is clamped on decode (unknown
mood/overlay -> 0, eye_w/eye_h -> 8..56, eye_r -> <= eye_w/2, eye_gap -> <= 40,
speed -> 1..10, name lowercase -> uppercase, other characters -> space).

## Engine API

See `engine/deskbunny_engine.h`. Framebuffer is SSD1306 page layout:
`fb[(y >> 3) * 128 + x]`, bit `(y & 7)`, 1 = lit. Engine output is
deterministic for the same face, seed and timestamp sequence; the simulator
and golden tests rely on this. Budget: engine state <= 96 bytes RAM, code
<= 12 KB on AVR (avr-gcc -Os; measured in CI), a full render <= 15 ms on a 16 MHz ATmega328P.

`invert` and `rotate180` are applied to the framebuffer by
`db_engine_render`, so hosts (the simulator) need not do anything. The
firmware clears those two flag bits before `db_engine_init` and sets the panel
instead (0xA7 invert, 0xA0/0xC0 instead of 0xA1/0xC8 for rotate180); the
result is the same pixels.

## Firmware behaviour

- Pins (bit-banged SPI, same on every board): CLK 13, MOSI 11, CS 10, DC 9,
  RES 8. Init sequence from the original working sketch.
- Loop: render at ~30 fps into a 1024-byte framebuffer, push all 8 pages.
- Serial 115200: on boot prints `DESKBUNNY <fw_version> <board_id> <face name>`.
  Replies the same line to `?\n` (`?\r\n` also works). The app uses this to
  verify a flash. Boards sharing an image can't tell themselves apart, so
  `<board_id>` is the image stem: `atmega328p`, `atmega2560`, `atmega32u4`,
  `uno_r4_minima`, `uno_r4_wifi`. The face name may contain spaces (it is the
  rest of the line) or be empty. Native-USB boards (Leonardo, Micro, R4
  Minima) also print the line once when a host first opens the port.
- No third-party libraries. Arduino core only.

## Boards (`firmware/boards.json`)

| board_id | Name | FQBN | MCU | Protocol | Image | USB VID:PID (app / bootloader) |
|---|---|---|---|---|---|---|
| `uno` | Uno R3 | arduino:avr:uno | atmega328p | stk500v1 @115200 | hex | 2341:0043, 2341:0001, 2A03:0043 |
| `nano` | Nano (new bootloader) | arduino:avr:nano:cpu=atmega328 | atmega328p | stk500v1 @115200 | hex | clones 1A86:7523, 0403:6001 |
| `nano_old` | Nano (old bootloader) | arduino:avr:nano:cpu=atmega328old | atmega328p | stk500v1 @57600 | hex | same as nano |
| `pro_mini` | Pro Mini 5V | arduino:avr:pro:cpu=16MHzatmega328 | atmega328p | stk500v1 @57600 | hex | via FTDI/CH340 |
| `mega` | Mega 2560 | arduino:avr:mega:cpu=atmega2560 | atmega2560 | stk500v2 @115200 | hex | 2341:0042, 2341:0010 |
| `leonardo` | Leonardo | arduino:avr:leonardo | atmega32u4 | avr109 @57600, 1200-baud touch | hex | 2341:8036 / 2341:0036 |
| `micro` | Micro | arduino:avr:micro | atmega32u4 | avr109 @57600, 1200-baud touch | hex | 2341:8037 / 2341:0037 |
| `uno_r4_minima` | Uno R4 Minima | arduino:renesas_uno:minima | RA4M1 | DFU (USB, nusb), 1200-baud touch | bin | 2341:0069 / 2341:0369 |
| `uno_r4_wifi` | Uno R4 WiFi | arduino:renesas_uno:unor4wifi | RA4M1 | SAM-BA (bossa) @230400 via 1200-baud touch on the ESP32-S3 bridge (no re-enumeration) | bin | 2341:1002 |

`boards.json` shape (array): `{ "id", "name", "fqbn", "mcu", "protocol":
"stk500v1"|"stk500v2"|"avr109"|"dfu"|"samba", "baud": [115200, 57600],
"touch1200": bool, "image": "atmega328p.hex", "usb": [{ "vid": "2341",
"pid": "0043", "role": "app"|"bootloader" }] }`.

`firmware/build.sh` (needs `arduino-cli` on PATH; installs missing cores
itself) writes `firmware/dist/`: one image per distinct `image` value plus
`manifest.json` = `{ "fwVersion": "0.1.0", "images": [{ "file", "format":
"hex"|"bin", "sha256", "blobOffset" }], "boards": <copy of boards.json> }`.
`blobOffset` is where `DB_FACE_BLOB` starts: for `.hex` the absolute address in
the HEX address space (= flash address on AVR); for `.bin` the byte offset in
the file (the R4 bootloader loads the `.bin` at flash 0x4000). The build fails unless the
magic occurs exactly once in each image. The app bundles `firmware/dist/` as the
Tauri resource directory `firmware/`.

Nano / Uno / Pro Mini share one ATmega328P image (`atmega328p.hex`); only the
upload baud differs. "Nano" auto-detect tries 115200, then 57600.
Later: Nano Every (UPDI), Nano 33 IoT (SAM-BA), ESP32 boards.

## Tauri IPC (app/src-tauri <-> frontend)

`FaceDef` is the JSON form of the blob, camelCase:
`{ name, mood, eyeW, eyeH, eyeR, eyeGap, eyeY, blink, lookAround, breathe,
cyclops, invert, rotate180, mouth, blinkMin, blinkMax, lookMin, lookMax,
lookX, lookY, speed, contrast, overlay, seed }` (booleans for flag bits,
`mood` and `overlay` as string enums, e.g. `"angry"`, `"hearts"`).
Mood strings are the lowercase enum names above (`"neutral"` .. `"dizzy"`).
Overlay strings: `"none" | "hearts" | "zzz" | "sweat" | "anger" | "tears" |
"sparkles" | "question"`. Numbers are the blob's byte types (`eyeY` is -128..127,
everything else 0..255); the backend clamps every field into its documented range
and normalises `name` (uppercase, unsupported characters -> space, max 12) before
encoding, simulating or flashing. `save_face` rejects empty or invalid names.

Commands:
- `list_presets() -> FaceDef[]`
- `list_boards() -> Board[]` where `Board = { id, name, mcu, protocol }`
  (from the bundled manifest; falls back to a built-in copy of the table)
- `list_ports() -> Port[]` where `Port = { path, vid?, pid?, product?, serialNumber?, guessedBoard? }`
  (`vid`/`pid` are 4-digit uppercase hex strings like `"2341"`; absent keys are
  omitted, not `null`; on macOS only `/dev/cu.*` ports are listed)
- `sim_create(face: FaceDef, seed: u32) -> u32` (sim id)
- `sim_set_face(id: u32, face: FaceDef)`
- `sim_frame(id: u32, tMs: u32) -> ArrayBuffer` (1024 bytes, page layout; returned as raw `tauri::ipc::Response`)
- `sim_drop(id: u32)`
- `flash(port: string, boardId: string, face: FaceDef) -> FlashReport`
  emits `flash-progress` events `{ stage: "reset"|"sync"|"erase"|"write"|"verify"|"done", percent, message }`
  (`percent` 0..100 overall and never decreasing). Only one flash runs at a time.
  `FlashReport = { protocol, port, baud: number|null, signature: string|null,
  bytesWritten, verified, durationMs, warnings: string[], boardId, faceName,
  fwVersion, blobAddress, deviceInfo: string|null, deviceVerified }`.
  `port` is where the bootloader answered (may differ from the requested port
  after a 1200-baud touch). `verified` = flash read back and compared (false for
  DFU, whose bootloader cannot read back). `deviceInfo` is the `DESKBUNNY ...`
  line read after the upload; `deviceVerified` = its face-name field equals
  `faceName`. A missing banner is a warning, not an error.
- `device_info(port: string) -> string | null` (sends `?\n`, reads the DESKBUNNY line)
- `list_saved_faces() -> FaceDef[]`, `save_face(face)`, `delete_face(name)` (app data dir, JSON)
- `encode_face(face) -> number[]` (64 bytes, for export/debug)
- `export_face(face) -> string | null`: native save dialog (default file name
  `<NAME>.deskbunny.json`), writes pretty FaceDef JSON, returns the path or
  `null` if cancelled.
- `import_face() -> FaceDef | null`: native open dialog filtered to `*.json`,
  parses, clamps and validates; `null` if cancelled; rejects bad files with a
  readable error. Both dialogs run in Rust (`tauri-plugin-dialog`); the webview
  has no dialog or filesystem permissions.

Errors are returned as strings a human can act on ("No response from bootloader
on /dev/ttyUSB0. Is this an old-bootloader Nano? Try 'Nano (old bootloader)'.").

## Build conventions

- Frontend: `app/`, npm with committed `package-lock.json`, `npm run dev`
  (Vite on port 1420), `npm run build` -> `app/dist`.
- `app/src-tauri/tauri.conf.json`: `devUrl http://localhost:1420`,
  `frontendDist ../dist`, `beforeDevCommand "npm run dev"`,
  `beforeBuildCommand "npm run build"`. Resources: `../../firmware/dist/*`
  bundled as `firmware/`.
- Linux build deps: `libwebkit2gtk-4.1-dev libudev-dev libayatana-appindicator3-dev librsvg2-dev patchelf`.
- Rust edition 2021, stable toolchain, `cargo clippy -- -D warnings` clean.
