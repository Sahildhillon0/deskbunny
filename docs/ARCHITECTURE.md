# DeskBunny architecture and contracts

DeskBunny is a desktop app (Rust + Tauri v2) that flashes an animated drone
scene onto an Arduino driving a 0.96" 128x64 SSD1306 OLED over bit-banged SPI.
(The `feat/drone-animations` branch draws quadcopters instead of the bunny
face. The blob layout is unchanged; only the meaning of some fields is, see
"Drone fields" below. Code and IPC keep the original field names.) No Arduino
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
| 9 | 1 | mood | flight animation ("scene"), enum below |
| 10 | 12 | name | ASCII, NUL-padded, uppercase A-Z 0-9 space (font coverage) |
| 22 | 1 | eye_w | drone size: rotor-to-rotor span px, 8..56, default 36 |
| 23 | 1 | eye_h | unused by drones, kept in range 8..56, default 36 |
| 24 | 1 | eye_r | unused by drones, kept in range 0..eye_w/2, default 8 |
| 25 | 1 | eye_gap | formation spacing px, 0..40, default 16 |
| 26 | 1 | eye_y | int8 hover altitude offset from centre (drawn clamped to -24..24), default 0 |
| 27 | 1 | flags | bit0 strobe (nav lights), bit1 drift, bit2 bob, bit3 camera gimbal, bit4 invert, bit5 rotate180, bit6 ground shadow, bit7 reserved |
| 28 | 1 | blink_min | strobe period, units of 100 ms, default 20 |
| 29 | 1 | blink_max | unused by drones, default 60 |
| 30 | 1 | look_min | drift: min time between moves, units of 100 ms, default 15 |
| 31 | 1 | look_max | drift: max time between moves, units of 100 ms, default 50 |
| 32 | 1 | look_x | drift: max horizontal move px, default 18 |
| 33 | 1 | look_y | drift: max vertical move px, default 8 |
| 34 | 1 | speed | 1..10 animation speed, default 5 |
| 35 | 1 | contrast | SSD1306 0x81 value, default 0xCF |
| 36 | 1 | overlay | sky background: 0 none, 1 clouds, 2 stars, 3 rain, 4 city, 5 hills, 6 wind, 7 grid |
| 37 | 1 | seed | PRNG seed salt, default 0 |
| 38 | 1 | cycle | mood cycle: 0 off, 1 sequence (loop the list in order), 2 random (from the list) |
| 39 | 1 | cycle_secs | seconds per mood, 1..255; 0 decodes as the default 10 |
| 40 | 1 | cycle_count | moods in the list, 0..12; 0 = all 12 moods |
| 41 | 1 | cycle_flags | bit0 overlay follows the mood (table below); other bits 0 |
| 42 | 12 | cycle_moods | mood list; entries past `cycle_count` are 0 |
| 54 | 8 | reserved | must be 0 |
| 62 | 2 | crc16 | CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF) over bytes 0..61 |

Moods (flight animations): `0 hover, 1 patrol, 2 takeoff, 3 flip, 4 orbit,
5 formation, 6 delivery, 7 race, 8 scan, 9 swarm, 10 topdown, 11 charge`.

### Drone fields

- hover: one drone holds position. patrol: flies side to side, banking into
  the turn. takeoff: lifts off a pad, hovers, lands. flip: hovers and rolls a
  full turn every few seconds. orbit: circles a beacon mast, growing as it
  comes closer. formation: three drones in a V (`eye_gap` spaces them).
  delivery: carries a swinging parcel on a tether. race: zips across with
  speed lines. scan: sweeps a searchlight over the ground. swarm: seven small
  drones on crossing paths. topdown: seen from above, four spinning rotors,
  slow yaw. charge: sits on a charging pad while a battery fills, then flies a
  short hop.
- Drift (bit1) moves the drone to a random spot within `look_x`/`look_y`
  every `look_min..look_max`, and it tilts into the move. Bob (bit2) is a
  slow hover bob. Strobe (bit0) flashes the nav lights twice every
  `blink_min` x 100 ms. Camera (bit3) hangs a gimbal under the body. Shadow
  (bit6) draws a ground shadow that shrinks with altitude.

Mood cycle: with `cycle` != 0 the engine changes `mood` every `cycle_secs`
seconds. Sequence starts at the first list entry and wraps; random picks from
the list (never the current mood twice in a row when the list has more than one
distinct mood) and starts at `mood`. Each change closes the eyes over 150 ms,
swaps the mood and reopens them over 150 ms, whatever the blink flag says.
(Drones: instead of closing eyes, the drones climb out of the top of the
screen over 300 ms, the scene swaps, and the new drones drop back in over
300 ms.) With bit0 of `cycle_flags` set, `overlay` is replaced on each change
by the mood's sky: patrol city, takeoff hills, orbit clouds, race wind, scan
grid, swarm stars, delivery city, charge clouds, every other scene none. Blobs written before the
cycle fields existed have zeros there, so they decode with the cycle off.

If magic/version/CRC is bad, the firmware falls back to `db_face_default()`
(hover, strobe + drift + bob, name `DESKDRONE`) and shows it.
A blob with a valid CRC but out-of-range fields is clamped on decode (unknown
mood/overlay/cycle/list entry -> 0, cycle_count -> <= 12, cycle_secs 0 -> 10,
list entries past cycle_count -> 0, cycle_flags -> bit0 only, eye_w/eye_h -> 8..56, eye_r -> <= eye_w/2, eye_gap -> <= 40,
speed -> 1..10, name lowercase -> uppercase, other characters -> space).

## Drawing blob v1 (4112 bytes)

A drawing is up to four 128x64 frames the user drew in the app, shown instead
of the face or on top of it. Every image carries `DB_ART_BLOB` next to the
face blob; the flasher finds the magic `DBART001` (exactly one occurrence,
enforced in CI) and overwrites all 4112 bytes. The image's own copy is "off",
so a face without a drawing needs no patch.

| Off | Size | Field | Notes |
|----:|----:|-------|-------|
| 0 | 8 | magic | ASCII `DBART001` |
| 8 | 1 | version | `1` |
| 9 | 1 | mode | 0 off (face only), 1 replace (drawing only), 2 over (drawing OR-ed on top of the face) |
| 10 | 1 | frames | 1..4 frames in use; 0 = off |
| 11 | 1 | reserved | 0 |
| 12 | 2 | frame_ms | u16, ms per frame, 50..10000; 0 decodes as 500 |
| 14 | 2 | reserved | 0 |
| 16 | 4096 | frame data | 4 x 1024 bytes, SSD1306 page layout (same as the framebuffer); unused frames 0 |

No CRC: a bad magic, version, mode or frame count just turns the drawing off.
At time `t` (ms since boot) the shown frame is `(t / frame_ms) % frames`. Mode
replace: the framebuffer is that frame (the engine is not run). Mode over: the
engine renders the face as usual, then each byte is OR-ed with the frame.
Contrast, invert and rotate180 still come from the face blob and apply to the
drawing too (on the panel in firmware; after compositing in the simulator).

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
- Loop: render at ~30 fps into a 1024-byte framebuffer, composite the drawing
  (Drawing blob v1), push all 8 pages. Frames are read straight from flash
  (PROGMEM on AVR): the drawing costs no RAM.
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
"hex"|"bin", "sha256", "blobOffset", "artOffset" }], "boards": <copy of boards.json> }`.
`artOffset` (same convention) is where `DB_ART_BLOB` starts.
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
lookX, lookY, speed, contrast, overlay, seed, cycle, cycleSeconds,
cycleMoods, cycleOverlay, drawing? }` (booleans for flag bits,
`mood` and `overlay` as string enums, e.g. `"angry"`, `"hearts"`).
Mood strings are the lowercase enum names above (`"hover"` .. `"charge"`).
Overlay strings: `"none" | "clouds" | "stars" | "rain" | "city" | "hills" |
"wind" | "grid"`. The boolean keys keep their face names: `blink` = strobe,
`lookAround` = drift, `breathe` = bob, `cyclops` = camera, `mouth` = shadow. `cycle` is `"off" | "sequence" | "random"`,
`cycleSeconds` 1..255, `cycleMoods` an array of at most 12 mood strings (empty
= all moods), `cycleOverlay` the bit0 flag. The four cycle keys are optional on
input (faces saved before they existed): defaults `"off"`, `10`, `[]`, `true`. `drawing` is optional (absent or `null` = no drawing) and is the JSON form of
the drawing blob: `{ mode: "replace" | "over", frameMs, frames: string[] }`,
`frameMs` 50..10000, `frames` 1..4 base64 strings of 1024 bytes each (page
layout). The backend clamps `frameMs`, drops frames past 4 and pads or cuts
each frame to 1024 bytes; an empty `frames` list means no drawing. The
simulator (`sim_*`) and `flash` both use it. Numbers are the blob's byte types (`eyeY` is -128..127,
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

Setup guide (first run; see docs/SETUP.md):
- `setup_status() -> SetupStatus` = `{ os: "linux"|"windows"|"macos", osLabel,
  firstRun, checks: SetupCheck[] }`. `SetupCheck` = `{ id, title, detail,
  state: "ok"|"action"|"warn"|"info", action?: { id, label }, commands?:
  string[], link?: { id, label } }`. `firstRun` is true until `setup_finish`.
- `setup_run(action: string) -> string`: runs a one-click fix (`install_udev`
  on Linux via pkexec; `install_r4_driver` on Windows via elevated pnputil;
  `install_ch340_driver` on Windows: downloads WCH CH340 driver 3.5.2019.1
  from the Microsoft Update Catalog, checks its SHA-256, then in one elevated
  step adds it, deletes every other `ch341ser.inf` package and binds it) and
  returns a success message, or rejects with a readable error.
- `setup_finish()`: writes the `setup-complete-v1` marker in the app config dir.
- `setup_open_link(id: string)`: opens an allowlisted URL by id; the webview
  never passes URLs.

Updates (tauri-plugin-updater, driven from Rust; the webview has no updater
permission). The endpoint is the `latest.json` attached to the latest GitHub
release; updates are signed with the minisign key whose public half is
`plugins.updater.pubkey` in tauri.conf.json (private half: the
`TAURI_SIGNING_PRIVATE_KEY` repo secret).
- `app_version() -> string`
- `update_check() -> UpdateInfo | null`, `UpdateInfo = { version, currentVersion,
  date: string|null, notes: string|null }`; `null` when up to date, including
  when the latest release has no `latest.json` (published before updates, or
  unsigned). Rejects with a readable error only when GitHub can't be reached.
- `update_install()`: downloads and installs the update found by the last
  `update_check`, emitting `update-progress` events `{ downloaded, total:
  number|null }`, then restarts the app (never resolves on success).
- `update_open_releases()`: opens the GitHub Releases page (fallback when an
  install fails).
  The frontend checks once on launch and shows a dialog when `update_check`
  returns an update; the header also has a "Check for updates" button.

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
