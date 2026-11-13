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
| 38 | 1 | cycle | mood cycle: 0 off, 1 sequence (loop the list in order), 2 random (from the list) |
| 39 | 1 | cycle_secs | seconds per mood, 1..255; 0 decodes as the default 10 |
| 40 | 1 | cycle_count | moods in the list, 0..12; 0 = all 30 moods |
| 41 | 1 | cycle_flags | bit0 overlay follows the mood (table below); other bits 0 |
| 42 | 12 | cycle_moods | mood list; entries past `cycle_count` are 0 |
| 54 | 8 | reserved | must be 0 |
| 62 | 2 | crc16 | CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF) over bytes 0..61 |

Moods: `0 neutral, 1 happy, 2 angry, 3 sad, 4 sleepy, 5 surprised, 6 love,
7 suspicious, 8 excited, 9 bored, 10 scared, 11 dizzy, 12 wink, 13 laugh,
14 crying, 15 skeptical, 16 smug, 17 evil, 18 worried, 19 shy, 20 squint,
21 dead, 22 asleep, 23 starry, 24 hypno, 25 glitch, 26 scan, 27 eyeroll,
28 confused, 29 determined`. Moods 12..29 borrow the mouth, the
overlay-follows-mood choice and the glance timing of a base mood among 0..11
(`k_base` in the engine); crying, asleep, starry and shy pick their own overlay.

Mood cycle: with `cycle` != 0 the engine changes `mood` every `cycle_secs`
seconds. Sequence starts at the first list entry and wraps; random picks from
the list (never the current mood twice in a row when the list has more than one
distinct mood) and starts at `mood`. Each change closes the eyes over 150 ms,
swaps the mood and reopens them over 150 ms, whatever the blink flag says. With
bit0 of `cycle_flags` set, `overlay` is replaced on each change by the mood's
overlay: angry anger, sad tears, sleepy zzz, love hearts, suspicious question,
excited sparkles, scared sweat, every other mood none. Blobs written before the
cycle fields existed have zeros there, so they decode with the cycle off.

If magic/version/CRC is bad, the firmware falls back to `db_face_default()`
(neutral, blink + look_around + breathe, name `DESKBUNNY`) and shows it.
A blob with a valid CRC but out-of-range fields is clamped on decode (unknown
mood/overlay/cycle/list entry -> 0, cycle_count -> <= 12, cycle_secs 0 -> 10,
list entries past cycle_count -> 0, cycle_flags -> bit0 only, eye_w/eye_h -> 8..56, eye_r -> <= eye_w/2, eye_gap -> <= 40,
speed -> 1..10, name lowercase -> uppercase, other characters -> space).

## Drawing blob v2 (slot size per firmware image)

A drawing is 1..10 128x64 frames (drawn in the app or imported from a GIF),
shown instead of the face or on top of it. Every image carries
`DB_ART_BLOB` next to the face blob; its size is whatever flash the chip has
left and is stored in the slot's own header (bytes 14..15), so the flasher and
`fwtool.py` read it from the image (`artCapacity` in manifest.json):

| Image | Slot (bytes, header included) |
|-------|------:|
| atmega328p (Uno R3, Nano, Pro Mini) | 5632 |
| atmega32u4 (Leonardo, Micro) | 3200 |
| atmega2560 (Mega) | 32000 |
| Uno R4 Minima / WiFi | 61440 |

The flasher finds the magic `DBART001` (exactly one occurrence, enforced in
CI), checks the blob fits the slot and writes it over the start of the slot. The
image's own copy is "off", so a face without a drawing needs no patch.

| Off | Size | Field | Notes |
|----:|----:|-------|-------|
| 0 | 8 | magic | ASCII `DBART001` |
| 8 | 1 | version | `2` |
| 9 | 1 | mode | 0 off (face only), 1 replace (drawing only), 2 over (drawing OR-ed on top of the face) |
| 10 | 2 | frames | u16, 1..10 frames; 0 = off |
| 12 | 2 | frame_ms | u16, ms per frame, 50..10000; 0 decodes as 500 |
| 14 | 2 | capacity | u16, slot size in bytes; set by the firmware build, kept by the app |
| 16 | n | frame data | one PackBits stream per frame, back to back |

PackBits stream: token `t < 128` is followed by `t + 1` literal bytes; `t >= 128`
by one byte repeated `t - 126` times (2..129). Each stream decodes to exactly
1024 bytes in SSD1306 page layout. Replace mode: frame 0 is stored whole and
every later frame as the XOR with the frame before it (the firmware XORs it
into the framebuffer, which is its only decoder state: no extra RAM). Over mode:
every frame is stored whole and OR-ed onto the face. `db_art_frame` in the
engine is the decoder; `drawing.rs` is the encoder and `flashSize` in
`drawing.ts` mirrors its length for the app's budget meter.

No CRC: a bad magic, version, mode or frame count, or a stream that runs past
the slot, turns the drawing off (the face shows instead).

## Live link v1 (serial, app -> board)

With the app running, it can take over the screen: clock, weather, Claude
session status. The board stays autonomous: 30 s without a command and it
goes back to its flashed face (and drawing), so a closed app or a sleeping PC
never leaves a stale screen.

Opening the port resets boards with an auto-reset circuit (Uno, Nano, Pro
Mini: DTR), which costs ~1.5 s and the splash. So one process (the app) opens
the port once and keeps it; anything else that wants to change the screen
(e.g. a Claude Code hook) talks to the app, never to the port.

115200 8N1. Commands are ASCII lines ending in `\n` (`\r` is ignored), at most
39 characters; longer lines are dropped without a reply. Every command
answers one line: `OK`, `ERR`, or the reply named below. Lines that don't
start with `!` (other than `?`) are ignored silently.

| Command | Reply | Effect |
|---|---|---|
| `?` | banner | unchanged (`DESKBUNNY <fw> <board> <name>`) |
| `!V` | `LIVE 1 C` / `LIVE 1` | live link version; `C` = Clawd is built in |
| `!P` | `OK` / `ERR` | keepalive; `ERR` when not in live mode (the board timed out or rebooted: resend the screen) |
| `!H` | `OK` | back to the flashed face and drawing now |
| `!M <mood> <overlay>[ <caption>]` | `OK` | the engine face with this mood (0..29) and overlay (0..7), flashed geometry and flags, cycle off |
| `!C <anim> <expr>[ <caption>]` | `OK` | Clawd (below); `ERR` on boards without it (32u4) |
| `!F <n>` | `K`, then `OK` / `ERR` | still frame: after `K`, send `n` bytes (2..1200) of one PackBits frame (Drawing blob v2 encoding), drawn onto a clear screen |
| `!X <n>` | `K`, then `OK` / `ERR` | like `!F`, XOR-ed onto the frame shown now (deltas for a clock); `ERR` without `K` unless a frame is shown |

- Numbers are decimal. The caption is the rest of the line after one space, up
  to 21 characters (5x7 font, ASCII 32..122), drawn centred on rows 57..63
  after clearing rows 56..63. With a caption `!M` moves the eyes up 5 px.
  Repeating `!M` with the same mood, overlay and caption presence does not
  restart the face (blinks keep their rhythm); only the caption text changes.
- `!F`/`!X`: the board answers `K` only when it is ready to read (it is
  not pushing a frame to the panel), so the 64-byte serial buffer never
  overflows. A gap over 200 ms inside the payload, a corrupt stream or a
  wrong length answers `ERR` and shows the flashed face (the half-written
  frame is not shown). A frame is pushed to the panel once; it stays until the
  next command or the timeout.
- Every `OK` restarts the 30 s timeout. The app re-sends its screen (or `!P`)
  at least every 10 s.
- The panel's invert and rotate180 from the flashed face also apply to live
  screens (they describe the mounting), and so does its contrast.

**Clawd** (`db_clawd_render`, in the engine so the simulator matches): a
crab mascot drawn above row 55, stateless in time (32 ms ticks, power-of-two
periods). Animations: `0 idle` (bob), `1 walk` (across the screen and back,
8.2 s), `2 jump` (hop every 1.02 s, arms up, sparkles), `3 shake` (jitter,
question mark overhead), `4 sleep` (legs tucked, eyes shut, zzz), `5 work`
(arms tapping, eyes down). Expressions: `0 open`, `1 happy` (^ ^), `2 closed`,
`3 wide`, `4 squint` (> <), `5 dead` (x x), `6 worried`, `7 focus` (glancing).
Open, wide, worried and focus blink every 4.1 s; sleep forces closed. Unknown
values draw as idle/open. `engine/build/render clawd out.pbm` draws a sheet.

Flash: the live link costs ~0.9 KB and Clawd ~1.6 KB, taken from the
drawing slot (table above). The 32u4 image leaves Clawd out.
`firmware/tools/livelink.py` drives a board from the command line.

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

- Display bus, detected at boot: pulse RES (pin 8), then probe I2C 0x3C and
  0x3D on the board's hardware SDA/SCL at 400 kHz. An ACK selects I2C at that
  address; no answer selects bit-banged SPI. Same image for both wirings.
  - I2C: SDA/SCL = A4/A5 (Uno, Nano, Pro Mini, Uno R4), 20/21 (Mega),
    2/3 (Leonardo, Micro). Control byte 0x00 before commands, 0x40 before
    data. AVR drives the TWI registers directly (no Wire, one transfer per
    page); other cores use Wire. Every bus wait is bounded.
  - SPI (same on every board): CLK 13, MOSI 11, CS 10, DC 9, RES 8.
  - Init sequence from the original working sketch, for both buses.
  - I2C at 400 kHz takes ~25 ms per frame on AVR, so I2C boards run at
    ~20-25 fps instead of 30 (the frame clock resyncs when it falls behind).
- Loop: render at ~30 fps into a 1024-byte framebuffer, composite the drawing
  (Drawing blob v1), push all 8 pages. Frames are read straight from flash
  (PROGMEM on AVR): the drawing costs no RAM.
- Live link: see "Live link v1". The loop renders the live screen instead of
  the face while it is active.
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
Later: Nano Every (UPDI), Nano 33 IoT (SAM-BA). ESP32 boards: see "ESP32 + BLE (nixxa)" below.

## Tauri IPC (app/src-tauri <-> frontend)

`FaceDef` is the JSON form of the blob, camelCase:
`{ name, mood, eyeW, eyeH, eyeR, eyeGap, eyeY, blink, lookAround, breathe,
cyclops, invert, rotate180, mouth, blinkMin, blinkMax, lookMin, lookMax,
lookX, lookY, speed, contrast, overlay, seed, cycle, cycleSeconds,
cycleMoods, cycleOverlay, drawing? }` (booleans for flag bits,
`mood` and `overlay` as string enums, e.g. `"angry"`, `"hearts"`).
Mood strings are the lowercase enum names above (`"neutral"` .. `"dizzy"`).
Overlay strings: `"none" | "hearts" | "zzz" | "sweat" | "anger" | "tears" |
"sparkles" | "question"`. `cycle` is `"off" | "sequence" | "random"`,
`cycleSeconds` 1..255, `cycleMoods` an array of at most 12 mood strings (empty
= all moods), `cycleOverlay` the bit0 flag. The four cycle keys are optional on
input (faces saved before they existed): defaults `"off"`, `10`, `[]`, `true`. `drawing` is optional (absent or `null` = no drawing) and is the JSON form of
the drawing blob: `{ mode: "replace" | "over", frameMs, frames: string[] }`,
`frameMs` 50..10000, `frames` 1..10 base64 strings of 1024 bytes each (page
layout). The backend clamps `frameMs`, drops frames past 10 and pads or cuts
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

Live mode (see "Live link v1"; `app/src-tauri/src/live.rs`). A worker thread
owns the port while live mode is on; `flash` and `device_info` pause it (it
sends `!H`, closes the port, and reopens it when they finish).
- `LiveConfig = { enabled, port: string|null, screens: ScreenCfg[],
  claudeStyle: "clawd"|"face", claudeHold, notify, weather }`. `ScreenRef = { kind:
  "face"|"claude"|"design"|"notify", design?: id }`; `ScreenCfg = ScreenRef &
  { enabled, seconds }` in rotation order: face and claude exactly once, each
  design at most once, `seconds` 3..600, `notify` never (it is not rotated).
  Stored as `<app config>/live.json`; missing keys take defaults (off, face
  20 s, design "clock" 15 s, claude 15 s). The phase-2 kind `"clock"` reads as
  the design "clock".
- `live_get_config() -> LiveConfig`, `live_set_config(config) -> LiveConfig`
  (sanitised, saved, applied at once).
- `live_status() -> LiveStatus` and `live-status` events on every change:
  `{ state: "off"|"paused"|"connecting"|"connected"|"error", port, banner,
  clawd, screen, pinned, message }` (`message` explains `error`; the worker
  retries every 4 s).
- `live_pin(screen: ScreenRef|null)`: keep one screen on (null = rotation). Not saved.
- `live_preview(screen: ScreenRef, tMs) -> ArrayBuffer` (1024 bytes): a design,
  the Claude screen or a sample notification as the board shows it (Clawd and
  caption via the engine and the firmware's font table).
- With live mode on, closing the window hides it; the tray icon's menu shows
  it again or quits. The window starts hidden (`visible: false`) and is shown
  at startup unless the app runs with `--hidden` (login autostart), which
  stays in the tray (shown anyway when there is no tray).
- `scripts/deskbunny.sh dev|build|start [--hidden]|stop|restart|status|autostart on|off`
  (Linux): `build` = `tauri build --no-bundle` into `~/.cache/deskbunny/target`;
  `autostart on` writes `~/.config/autostart/deskbunny.desktop` running the
  built app with `--hidden`.

Designs (`crates/deskbunny-core/src/layout.rs`, stored in `<app
data>/designs.json`, seeded with the built-in templates: Clock, Big digital,
Analog, Weather, Clock + weather, Air quality, Minimal; the panel offers
built-ins an install does not have yet). `Design = { id, name, elements }`,
id 1-40 of `a-z0-9-_`, name 1-32 chars, at most 64 elements drawn in order.
Elements (`type` tag, panel pixels, everything clips):
- `text { x, y, text, font: "small"|"seg", size, align: "left"|"center"|"right", erase }`:
  `small` is the 5x7 font at scale 1..4 (plus `°`); `seg` is 7-segment digits
  `size` px tall (10..60), other characters fall back to the 5x7 font; `x` is
  the left edge, centre or right edge; `erase` draws black.
- `rect { x, y, w, h, fill, erase }`, `line { x1, y1, x2, y2, thick 1..4 }`,
  `bar { x, y, w, h, value, max }` (value/max may hold tokens; 3+ px tall bars
  get a border), `analog { cx, cy, r, seconds }`, `icon { x, y, icon, size 1..3 }`
  (16 px grid; names in `ICONS`, or `{WICON}`), `bitmap { x, y, w, h, data }`
  (base64, `w*h` bits row-major MSB first, at most 128x64).
- Tokens in text, bar and icon fields: `{HH} {H} {hh} {h} {MM} {SS} {AMPM} {DAY}
  {DAYNAME} {DD} {D} {MON} {MONTH} {MO} {YYYY} {TEMP} {TMAX} {TMIN} {UNIT} {COND}
  {HUM} {WIND} {CITY} {WICON} {AQI} {AQICAT} {AQIMAX} {PM25} {PM10} {CLAUDE}` (`TOKENS` in layout.rs); without weather
  the weather ones read `--`; unknown tokens stay as written.
- `design_list() -> Design[]`, `design_catalog() -> { templates, tokens:
  [name, meaning][], icons }`, `design_save(design) -> Design[]` (insert or
  replace by id, validated), `design_delete(id) -> Design[]`,
  `design_render(design) -> { frame: number[1024], bounds: [x,y,w,h][] }`
  (unsaved designs, for the editor; bounds per element for hit testing).

Weather (`weather.rs`): Open-Meteo, no key. `LiveConfig.weather = { place:
Place|null, units: "metric"|"imperial", aqi: "us"|"eu"|"in" }`, `Place = {
name, detail, lat, lon }`. Air quality comes from Open-Meteo's air-quality API
in the same fetch (PM2.5, PM10 and the index); if only that call fails the
weather still shows and the AQI tokens read `--`. US EPA and European indexes
are Open-Meteo's own. India's National AQI is computed here the CPCB way:
PM10, PM2.5, NO2, SO2 averaged over 24 h and CO, O3 over 8 h (hourly data,
window ending at the current hour, 3/4 of it present), each mapped linearly
onto the bands 0-50/51-100/101-200/201-300/301-400/401-500 with CPCB's
breakpoints, the highest sub-index wins; it needs PM2.5 or PM10 and three
pollutants. The last band's top (no CPCB value) is taken as PM10 600, PM2.5
380, NO2 800, SO2 2100, CO 50 mg/m³, O3 1000. Categories: GOOD, SATISFACTORY,
MODERATE, POOR, VERY POOR, SEVERE. `{AQIMAX}` is 300 (US), 100 (EU), 400 (IN).
Fetched when the place or units change, every 15 min, 2 min after a failure,
timed by the wall clock (so a PC waking from suspend fetches at once). Day or
night (`isDay`, sun or moon icon) follows today's and tomorrow's sunrise and
sunset from the forecast, checked at render time; Open-Meteo's `is_day` is the
fallback.
- `weather_search(query) -> Place[]` (geocoding, up to 8; under 2 characters
  returns none).
- `weather_now() -> { weather: Weather|null, error, ageSecs }`, `Weather = {
  city, temp, tmax, tmin, code (WMO), isDay, humidity, wind, unit, windUnit,
  aqi: number|null, aqiScale: "US"|"EU"|"IN", pm25, pm10 }`.

Local endpoint (`server.rs`): `http://127.0.0.1:47807`, loopback only. Every
request must carry an `X-DeskBunny` header (any value): browsers cannot send a
custom header cross-origin without a CORS preflight, which is never answered,
so web pages cannot post here. JSON replies, `{"ok": true}` or `{"error"}`.
- `POST /claude`: a Claude Code hook payload, exactly as the hook receives it
  on stdin (needs `session_id` and `hook_event_name`).
- `POST /notify`: `{"title", "body"?, "app"?}`, or plain text (first line =
  title, the rest = body). `tools/deskbunny-notify TITLE [BODY] [APP]` wraps it.
- `POST /usage`: an AI harness's plan usage; see "AI usage" below.
- `GET /status`: `{ claude: ClaudeView, usage: UsageNow, live: LiveStatus }`.

AI usage (`usage.rs`). A plan's 5-hour and weekly usage for one AI coding
harness, chosen in the settings (`LiveConfig.usage = { harness }`, default
`claude`). Harnesses are a registry (`HARNESSES`: id, name, `short` for
`{USRC}`, setup hint); only Claude Code is supported so far. Each harness
reports to `POST /usage`; the app keeps the newest report per harness, in
memory only. Bodies:
- neutral: `{"source": "<id>", "five_hour": {"used": 42, "resets_at": unix},
  "week": {...}}` (either window may be missing; unknown sources are refused);
- Claude Code: its status line payload, or just `rate_limits`
  (`five_hour` / `seven_day`, each `{ used_percentage, resets_at }`). Claude
  Code hands these only to the status line command, so that command forwards
  its stdin: `curl -s -m 0.3 -H 'X-DeskBunny: 1' --data-binary @-
  http://127.0.0.1:47807/usage`.
Adding a harness: a `HARNESSES` entry, and either the neutral body or an
adapter in `usage::parse`. Tokens (for the chosen harness): `{USRC}` name,
`{U5H}` / `{UWK}` percent used, `{U5HIN}` / `{UWKIN}` time to reset (`2H15M`,
`45M`, `3D4H`); `--` until the first report. A window past its reset time
shows 0%. Template: "AI usage" (`usage`), a bar per window.
- `usage_harnesses() -> { id, name, short, setup }[]`.
- `usage_now() -> { harness, usage: Usage, ageSecs|null }`, `Usage = { source,
  five, fiveIn, week, weekIn }`.

Claude screen (`claude.rs`). The hooks (installed by `claude_hooks_install`
into `~/.claude/settings.json`, entries marked by the `X-DeskBunny` header in
the command, file backed up to `settings.json.deskbunny.bak`) cover
SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, Notification, Stop and
SessionEnd. Each session is idle, starting, working (caption from the tool:
`EDIT main.rs`, `RUN cargo`, `READ x`, `SEARCH p`), needs you (a permission
prompt; idle reminders are ignored) or done (`DONE 3m12s`). The busiest
session wins: needs you > done (first 20 s) > working > older done > starting
> idle, newest on a tie. Silent sessions are dropped after 30 min.
- Screen choice, highest first: Claude needs you (if the Claude screen is
  enabled) > a notification > a pin > Claude takeover (start 3 s, a new prompt
  5 s, done 20 s, and with `claudeHold` (default on) the whole time any session
  is working or needs you) > the rotation. The rotation includes the Claude screen only
  while some session is working, done or needs you.
- Clawd: idle sleep/closed, starting idle/wide, working work/focus (edit, run,
  plan) or walk/focus (read, search, web) or idle/focus, needs you
  shake/worried, done jump/happy during the takeover then idle/happy. Face
  (boards without Clawd, or `claudeStyle: "face"`): asleep+zzz, excited+
  sparkles, determined or scan, worried+question, happy+sparkles.
- App-drawn screens carry a 7x7 badge at the top right: cycling dots while
  working, a tick when done, `!` when Claude needs you.
- `claude_view() -> ClaudeView = { state: "idle"|"starting"|"working"|"done"|
  "needsYou", caption, project, sessions, takeover, active, busy }` (`active`:
  some session is working, done or needs you; `busy`: working or needs you).
- `claude_hooks_status() / claude_hooks_install() / claude_hooks_remove() ->
  { installed, settingsPath, command }`.

Notifications (`LiveConfig.notify = { enabled, desktop, seconds 2..60,
showBody, ignoreApps }`). From `/notify` and, on Linux with `desktop` on, from
the desktop: `dbus-monitor` watches `org.freedesktop.Notifications.Notify` on
the session bus. Up to 5 queue; each shows for `seconds` (app name, title in
bold, body word-wrapped unless `showBody` is off); an exact repeat of the last
one is dropped. Made to be read across a desk: a bell and the app name (2x,
9 characters), a rule, the sender/title (2x, up to two lines of 10). With
`showBody` (default off) a one-line title is followed by two small lines of
the message. `live_test_notification()` queues a sample.

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

## ESP32 + BLE (nixxa)

The ESP32 family runs a different firmware from the Arduino boards. The desktop
app flashes it once. After that a phone app (Flutter, `mobile/`, phase 2)
pairs with it over BLE by scanning a QR code, then sends faces, drawings and
phone notifications. Arduino boards keep working exactly as described above.
The ESP32 images contain **no** face/drawing blob slots and are never patched.

### Chips, wiring, images

| board_id | Chip | FQBN | I2C SDA/SCL | Image (`firmware/dist/`) |
|---|---|---|---|---|
| `esp32` | ESP32 (WROOM-32 DevKit) | `esp32:esp32:esp32` | 21 / 22 | `esp32.bin` |
| `esp32c3` | ESP32-C3 | `esp32:esp32:esp32c3:CDCOnBoot=cdc` | 8 / 9 | `esp32c3.bin` |
| `esp32s3` | ESP32-S3 | `esp32:esp32:esp32s3:CDCOnBoot=cdc` | 8 / 9 | `esp32s3.bin` |

- Display: SSD1306 128x64 over I2C only (no SPI, no RES pin). Probe 0x3C, then
  0x3D, at 400 kHz. The firmware may raise the clock to 1 MHz when the panel
  ACKs at that speed. The init sequence and control bytes are the same as the
  AVR firmware's.
- Sketch: `firmware/nixxa_esp32/`. `build.sh` copies `engine/` into it, as it
  does for the AVR sketch. Libraries: the arduino-esp32 core and its bundled
  libraries (BLE, LittleFS, Preferences, Wire, mbedtls) plus a vendored copy
  of Nayuki's MIT `qrcodegen` C encoder, with its licence header kept. Nothing
  else. The core version is pinned in `build.sh` (`ESP32_CORE_VERSION`).
- Image = the arduino-cli `*.ino.merged.bin` (bootloader, partition table,
  boot_app0 and app in one file) **cut off right after the app**, written at
  flash offset **0x0** on every chip. The core pads merged.bin to the full
  4 MB, which would also wipe app1 and the library. `fwtool.py espimage`
  checks the cut: the app is byte-identical to `.ino.bin` and fits app0,
  and 0x9000..0xDFFF is all 0xFF. Partition scheme: the core's `default` 4 MB layout (nvs 0x9000/0x5000,
  otadata 0xE000, app0 0x10000, app1, spiffs used as LittleFS). The merged
  image fills 0x9000..0xDFFF with 0xFF, so **every desktop flash erases NVS
  and resets the pairing**, which is how a lost phone is recovered. The
  LittleFS library partition is left as it was.
- `boards.json` gets these three entries with `"protocol": "esptool"`,
  `"baud": [460800, 115200]` (the fast rate first; ROM sync always happens at
  115200), `"touch1200": false` and USB IDs (all `role: "app"`):
  `esp32`: 10C4:EA60 (CP210x), 1A86:55D4 (CH9102); `esp32c3` and `esp32s3`:
  303A:1001 (USB-Serial/JTAG). VID:PID cannot tell the chips apart; see
  detection below.
- `manifest.json` image entries for these files carry `"format": "bin"`,
  `"chip": "esp32"|"esp32c3"|"esp32s3"`, `"flashOffset": 0`, and
  `"blobOffset": null, "artOffset": null`. The magic-occurs-once check in CI
  applies only to images that have blob offsets.

### Desktop flashing (`deskbunny-flash`, protocol `esptool`)

- The ROM-bootloader serial protocol is reached through the `espflash` crate
  (library use, no CLI, no Python). Reset into the bootloader with the usual
  DTR/RTS sequence, or with the USB-Serial/JTAG sequence on 303A:1001.
- Chip detection happens at sync. If the chip found differs from the selected
  board but is one of the three, the flasher uses that chip's image and adds a
  warning ("Selected ESP32-C3 but found ESP32-S3; flashed the S3 image."). Any
  other chip is an error.
- Stages map onto `flash-progress`: `reset`, `sync` (connect + chip detect),
  `erase`, `write` (compressed), `verify` (flash MD5 vs image), then a hard
  reset into the app and `done`. `FlashReport.protocol = "esptool"`,
  `signature` = chip name + MAC (e.g. `"ESP32-C3 34:85:18:AA:BB:CC"`),
  `blobAddress` = 0, `verified` = MD5 matched, `faceName` = `""`,
  `boardId` = the board actually flashed.
- After the reset the flasher reads the banner as it does for AVR. On
  303A:1001 the port re-enumerates, so it reopens the same path, retrying for
  up to 5 s.
- `flash(port, boardId, face)` ignores `face` for ESP32 boards. For ESP32
  boards `deviceVerified` = the banner's board-id field equals the flashed
  board, since there is no face name to compare. Images larger than the end
  of app0 (0x150000) are refused before writing. A manifest image whose
  `chip` doesn't match the board is refused. If the 460800 switch fails, the
  flasher reconnects at 115200 and adds a warning. Only USB serial ports are
  accepted, because the reset sequence is chosen from the USB PID.
- Linux: the udev rule also grants `uaccess` on 10c4:ea60, 1a86:55d4 and
  303a:1001. The setup check treats a rule without 303a as missing, so the
  user is asked to reinstall it.
- New command `detect_chip(port: string) -> string | null`: enters the ROM
  bootloader, reads the chip and returns `"esp32"|"esp32c3"|"esp32s3"`
  (`null` for anything else or no answer), then hard-resets the board back
  into its app. Ports without an ESP VID:PID from the board table return
  `null` without being opened, so the webview can't reset arbitrary devices.
  It shares the flash lock. The frontend calls it when a port with an ESP VID:PID is
  selected, to pick the board automatically.
- Frontend: when the selected board is an ESP32 board, the flash button reads
  "Flash nixxa firmware". The face editor still works as a preview, with a
  note that faces are sent from the phone app. On success it shows "Scan the
  QR code on the screen with the nixxa app".

### Firmware behaviour (ESP32)

- Serial 115200 (UART0, and also USB CDC on C3/S3). The boot banner and the
  `?` reply are `DESKBUNNY <fw_version> <board_id> <active face name>`, so
  `device_info` keeps working. Debug commands, one per line, for bench testing
  without a phone:
  `N <app>|<title>|<body>` fakes a message notification,
  `C <caller>` fakes an incoming call, `D` dismisses the call, `Q` shows the
  QR screen for 30 s, `K <0-20>` picks the idle screen (0 = the face, 1..20 =
  that clock, same as op 0x25), `U` unpairs (same as op 0x7F), `S` prints the
  state (paired, active slot, used slots, BLE connected, clock).
- First boot: generate the 16-byte secret `K` with the hardware RNG
  (`esp_fill_random` with the radio on) and store it in NVS namespace `nixxa`,
  key `k`. Device ID = the last 3 bytes of the BT MAC, as 6 uppercase hex
  characters, e.g. `A1B2C3`.
- **Unpaired**: the screen shows the QR (left, 2 px per module) and to its
  right the text `NIXXA` / `<ID>` / `SCAN ME`. The device advertises as
  connectable to anyone, with name `NIXXA-<ID>` and the service UUID.
- **Paired**: the device shows the active slot's face/drawing (the engine,
  with the same compositing as on AVR). It advertises connectable to anyone,
  exactly like unpaired (no filter accept list: after a reboot the controller
  did not match the owner's resolvable private address against it, so the
  owner could not reconnect). AUTH (K) decides who is the owner, so the
  owner's phone reconnects by itself on power-up.
- Storage: LittleFS. `/s/NN.bin` (NN = 00..15) holds one slot: 64-byte face
  blob v1, then an optional drawing blob v2 (its `capacity` field ignored,
  max 12285 bytes, which is what fits in `maxPayload` next to the slot,
  face and artLen fields). `/state.bin` holds the active slot and the notification
  config. On first boot, slot 00 = `db_face_default()` and it is active.
  Corrupt or missing files fall back to the default face.
- Idle screen: the face, or the clock picked with SET_CLOCK (op 0x25). The
  clock is one of 20 styles drawn by `nx_clock.c` from the time set with
  SET_TIME (`--:--` until then). The style is persisted in `/clock.bin`.
- Notifications: see op 0x21. When a notification passes the filter, the
  logo replaces the idle screen for 1.5 s: the app's 32x32 logo centred, its
  name under it (a generic bell, doubled, when the phone sent no logo). Then a
  card replaces it for `cardSecs` (default 6 s), then the idle screen returns.
  The card shows a 16x16 version of the logo (or the bell) plus the app name on
  the top row, a rule, the title (one line, truncated) and the body word-wrapped
  below it, scrolling vertically if it doesn't fit. The per-app mood no longer
  changes the screen. The queue holds 4 cards; older ones are dropped. A call
  shows the caller with a ringing animation until DISMISS, a kind-2 NOTIFY,
  or 45 s. Text: printable ASCII 0x20..0x7E in a full 5x7 font, anything else
  rendered as `?` (the phone app transliterates first). Nothing is shown
  during quiet hours or while DND is on (calls included, unless
  `callsInQuiet` is set).

### Pairing and security

- QR payload (alphanumeric mode, ECC M, fits version 3 = 29x29):
  `NIXXA1:<ID>:<K as 32 uppercase hex>`.
- The phone connects and bonds with LE Secure Connections (Just Works:
  encryption, with no MITM protection from BLE itself). It then reads INFO,
  which carries a fresh 16-byte nonce per connection, and must send AUTH =
  first 16 bytes of HMAC-SHA256(K, nonce || "NIXXA-AUTH") within 10 s of the
  link being encrypted (30 s from connect if it never encrypts; Android's
  pairing dialog can take a while). Only
  someone who scanned the QR knows K, and that is what gives the MITM
  protection.
- If AUTH succeeds while unpaired, this bond becomes the owner, every other
  bond is deleted, `paired = 1` is saved, and the face shows. If AUTH fails or
  times out, the device disconnects and deletes the bonds made during that
  connection, never the ones that existed when it started (the owner's).
  `paired` and K live in NVS,
  so a desktop reflash resets both. Every
  connection, the owner's included, must AUTH before any other op
  (`NOT_AUTHED` otherwise).
- UNPAIR (op 0x7F), serial `U`, or a desktop reflash: delete all bonds,
  generate a new K, `paired = 0`, show the QR. The library slots are kept.

### BLE GATT

Service `4e580001-6e69-4878-a100-6e6978786100`. Characteristics share the
base, with the first group changed:

| UUID | Name | Props | Security |
|---|---|---|---|
| `4e580002-…` | INFO | read | encrypted |
| `4e580003-…` | RX (phone → device) | write, write-without-response | encrypted |
| `4e580004-…` | TX (device → phone) | notify | encrypted |

The device offers an MTU of 517. NimBLE (C3/S3) starts the exchange itself;
Bluedroid (ESP32) only answers one, so the phone must request it (Android
`requestMtu(517)`; iOS negotiates by itself). The BLE stack is fixed per
chip by the core: Bluedroid on ESP32, NimBLE on C3/S3. Security is LE
Secure Connections only (legacy pairing is refused). The name goes in the
scan response, since UUID-128 plus the name don't fit in 31 bytes. The ESP32
advertises from a resolvable private address; C3/S3 use the public one.

INFO (41 bytes): `0 proto u8 = 1`, `1 chip u8 (0 esp32, 1 esp32c3, 2 esp32s3)`,
`2 paired u8`, `3 slots u8 = 16`, `4 maxPayload u16 LE = 12352`,
`6 id[3]`, `9 nonce[16]`, `25 fwVersion[16]` ASCII NUL-padded.

Messages (both directions, little-endian): `op u8, seq u8, len u16, payload[len]`.
A message may span several GATT writes or notifications; the receiver
reassembles it by `len`. The phone sends the next message only after it gets
the reply to the previous one (stop-and-wait), so no reordering is possible.
Replies use `op | 0x80`, echo `seq`, and start with a status byte: `0 OK,
1 NOT_AUTHED, 2 BAD_MSG, 3 BAD_SLOT, 4 NO_SPACE, 5 BUSY, 6 AUTH_FAILED`. A
reassembled message larger than `maxPayload + 4` makes the device reply
`BAD_MSG` and drop the bytes until the next write that starts a message (a
write that arrives 300 ms after the previous one starts a new message).

| Op | Name | Payload (phone → device) | Reply payload after status |
|---|---|---|---|
| 0x01 | AUTH | tag[16] | – |
| 0x02 | GET_STATE | – | activeSlot u8, usedMask u16, notifEnabled u8, dnd u8 |
| 0x03 | SET_TIME | unixSecs u32, tzOffsetMin i16 | – |
| 0x10 | LIST_SLOTS | – | count u8, then per used slot: slot u8, name[12] |
| 0x11 | PUT_SLOT | slot u8, face[64], artLen u16, art[artLen] | – |
| 0x12 | GET_SLOT | slot u8 | face[64], artLen u16, art[artLen] |
| 0x13 | DELETE_SLOT | slot u8 (the active slot can't be deleted → BAD_SLOT) | – |
| 0x14 | ACTIVATE | slot u8 (shown at once and persisted) | – |
| 0x20 | SET_NOTIF_CONFIG | see below | – |
| 0x21 | NOTIFY | see below | – |
| 0x22 | DISMISS | notifId u32 | – |
| 0x23 | SET_DND | on u8 | – |
| 0x24 | LIST_SEEN_APPS | – | count u8, then per app idLen u8, appId, nameLen u8, name (iOS) |
| 0x25 | SET_CLOCK | style u8 (0 = the face, 1..20 = that clock; else BAD_MSG) | – (persisted) |
| 0x7F | UNPAIR | – | – (the device then disconnects) |

PUT_SLOT checks the face CRC (BAD_MSG if it's wrong). LIST_SLOTS reports a
slot whose face name is empty as `SLOT NN`. DELETE_SLOT of an empty slot
replies OK. A write that arrives while the previous message is still being
handled gets BUSY. Defaults: notifications enabled, cardSecs 6, defaultMood 5
(surprised). `enabled = 0` suppresses calls too. DND is persisted in
`/state.bin`. Cards and calls also show while unpaired, so the serial `N`/`C`
commands work on the bench.

SET_NOTIF_CONFIG: `enabled u8, cardSecs u8 (2..30), defaultMood u8,
quietStartMin u16, quietEndMin u16 (equal = no quiet hours; a range may wrap
past midnight), callsInQuiet u8, ruleCount u8 (<= 32)`, then per rule:
`idLen u8, appId[idLen] (Android package name or iOS bundle id, ASCII, <= 64),
mood u8, flags u8 (bit0 show)`. Quiet hours use the time from SET_TIME. With
no time set, quiet hours are ignored. Android filters on the phone and only
sends what should be shown; the device-side rules matter on iOS (ANCS, phase
3), where the device itself sees every notification.

NOTIFY: `kind u8 (0 message, 1 incoming call, 2 call ended), notifId u32,
mood u8 (0xFF = rule or default mood), appIdLen u8, appId, appNameLen u8,
appName, titleLen u8, title, bodyLen u16, body, iconFlag u8, icon`. The icon
flag is 0 (no icon), 1 (icon = 16x16, 32 bytes, from older phones) or 2 (icon
= 32x32, 128 bytes, the app logo). Both are 1 bpp, row-major, MSB first,
1 = lit. The device doubles a 16x16 icon. Strings are UTF-8 and have no NUL
terminator. Bodies past 512 bytes are cut off.

iOS (phase 3): the device also acts as an ANCS client on the owner's
connection. It applies the rules by AppIdentifier and maps ANCS
`IncomingCall` to kind 1. The GATT service above doesn't change.

## Mobile app (nixxa, `mobile/`)

Flutter app, package `com.zenithratech.nixxa`. Android ships first (phase 2);
iOS follows in phase 3 with a Swift link and ANCS on the device.

### Layout and ownership

```
mobile/
  lib/model/        FaceDef (JSON identical to the desktop FaceDef), Drawing
  lib/protocol/     blob v1/v2 codecs, pairing QR parser, Bunny (slot ops)
  lib/link/         device_link.dart = the Dart contract for the native link
                    device_link_channel.dart = the platform-channel implementation
                    fake_device_link.dart = in-memory fake for UI work and tests
  lib/ui/, lib/app/ screens and state (provider)
  packages/nixxa_engine/   FFI plugin: compiles engine/*.c (copied by a script,
                    never symlinked) for Android and the Linux host; EngineSim
                    gives 1024-byte frames identical to the desktop simulator
  android/          Kotlin: NixxaLinkService, notification listener, channels
```

- Codecs must be byte-identical to `crates/deskbunny-core`. Fixtures are
  generated by a Rust test in that crate into `mobile/test/fixtures/`.
- The preview always goes through `EngineSim` (the C engine). No Dart
  re-implementation of the renderer.

### Android link (native, Kotlin)

`NixxaLinkService` is a foreground service (type `connectedDevice`, with a
persistent "nixxa connected" notification). It owns the one BLE connection,
so the link and notification forwarding survive the UI being closed or
killed. The UI only talks to it.

- Pairing: parse the QR, scan for the name `NIXXA-<ID>` (service-UUID
  filter), connect, `createBond()`, `requestMtu(517)`, read INFO, write
  AUTH (HMAC-SHA256 as in "Pairing and security"), then SET_TIME and
  SET_NOTIF_CONFIG. On success it persists `{address, deviceId, K}` in
  EncryptedSharedPreferences.
- Reconnect: `connectGatt(autoConnect = true)` to the bonded address. It is
  restarted on boot (`BOOT_COMPLETED`) and on Bluetooth turning on. Every
  connection redoes MTU, INFO, AUTH, SET_TIME, SET_NOTIF_CONFIG and SET_DND
  (when followDnd).
- Messages: one FIFO queue, stop-and-wait per the GATT section, 5 s reply
  timeout, split into writes of MTU-3 bytes with write-with-response.
  NOTIFY from the listener and `request()` from the UI share the queue.
  NOTIFY is dropped (not queued) while disconnected.
- Notifications: a `NotificationListenerService`.
  - Forward only when config.enabled and the package has a rule with `show`
    set. Ongoing notifications, group summaries, our own package and
    duplicate updates of the same key within 2 s are skipped.
  - mood = the rule's mood (0xFF → device default).
  - Text: title = EXTRA_TITLE; body = EXTRA_BIG_TEXT, else EXTRA_TEXT.
    Accents are stripped and the rest is transliterated to ASCII.
  - Icon: the app's launcher icon rendered to 32x32 and thresholded to 1 bpp
    (Otsu on luminance; the minority class is the lit glyph). Falls back to the
    notification's small icon, then to none (the device's bell).
  - Calls: a notification with `CATEGORY_CALL` from the default dialer or a
    telephony UI package (Samsung's in-call UI posts its own), or with
    `EXTRA_CALL_TYPE` 1..3, when `calls` is set, is sent as NOTIFY kind 1
    (title = caller). Its removal sends kind 2. Every call decision is logged
    under `nixxa.listener`.
  - Bonds: when setup fails with a GATT insufficient-auth/encryption status
    (the bunny dropped its keys, e.g. after a power cut), the phone removes its
    stale bond, bonds again on a fresh link and sets up on the new bond.
  - Quiet hours are enforced on the device (it gets the time from SET_TIME).
    DND: when followDnd, interruption-filter changes are sent as SET_DND.
- Notifications removed on the phone send DISMISS (notifId = the key hash).

Channels: MethodChannel `nixxa/link`, EventChannel `nixxa/link/status`
(emits the `LinkStatus.toMap()` map on every change and once on listen).

| Method | Args | Result |
|---|---|---|
| `status` | – | LinkStatus map |
| `pair` | `{qr: String}` | LinkStatus map |
| `forget` | – | null |
| `request` | `{op: int, payload: Uint8List}` | Uint8List (after the status byte) |
| `getNotifConfig` / `setNotifConfig` | – / NotifConfig map | NotifConfig map / null |
| `hasNotificationAccess` | – | bool |
| `openNotificationAccessSettings` | – | null |
| `requestBatteryExemption` | – | null |
| `listApps` | – | `[{appId, label, iconPng}]`: launcher apps plus apps seen posting, sorted by label |
| `sendTestNotification` | – | null |

Errors are PlatformExceptions whose `code` is one of the `LinkException`
codes in `device_link.dart`; DEVICE_ERROR carries `details = status byte`.
Permissions: BLUETOOTH_SCAN (neverForLocation), BLUETOOTH_CONNECT, CAMERA,
POST_NOTIFICATIONS, FOREGROUND_SERVICE(_CONNECTED_DEVICE),
RECEIVE_BOOT_COMPLETED, REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, plus a
`<queries>` entry for launcher apps (no QUERY_ALL_PACKAGES). minSdk 26,
target the latest SDK.

### App screens (black theme, same feel as the desktop app)

1. Pair: camera QR scan (mobile_scanner) with a manual-entry fallback,
   progress through scan → bond → auth, and readable errors.
2. Home: link state, chip and firmware, the active face (live EngineSim
   preview), quick actions.
3. Library: 16 slots on the device (LIST_SLOTS); tap to activate; edit,
   delete, and a local library of saved faces. Import and export of
   `.deskbunny.json`.
4. Face editor: every FaceDef field, all 30 moods, overlays and the mood
   cycle, with a live preview. "Send to bunny" = PUT_SLOT plus ACTIVATE.
5. Drawing editor: 128x64 pixel canvas, 1..10 frames, frame time,
   replace/over, GIF import (package `image`, dithered to 1 bpp), and a
   size meter against `maxArtBytes`.
6. Notifications: master switch, notification-access status, an app picker
   with a per-app mood, quiet hours, follow DND, calls, card seconds, default
   mood, and a test button.
7. Device: firmware/chip info, battery-optimisation exemption, and Forget
   device (UNPAIR).

### Android implementation notes (as built)

- K is encrypted with an Android Keystore AES-256-GCM key (alias
  `nixxa_link_key`) and kept in SharedPreferences `nixxa_link`.
  EncryptedSharedPreferences is deprecated. `allowBackup="false"`, so a
  pairing is never restored onto another phone.
- On Android 8–11, BLE scanning needs ACCESS_FINE_LOCATION, plus
  BLUETOOTH/BLUETOOTH_ADMIN (all with maxSdkVersion 30). POST_NOTIFICATIONS
  is requested, but denying it doesn't fail pairing.
- Writes are chunked to `min(MTU-3, 512)`, since 512 is the maximum
  attribute length.
- SET_NOTIF_CONFIG puts shown rules first (max 32). A rule mood of 0xFF is
  sent as `defaultMood`, because the firmware clamps unknown moods to 0. On
  iOS, the device-side rules must therefore always carry a real mood.
- Calls from the default dialer are forwarded whenever `calls` is on, with
  no rule needed. Answering (Android 12+), declining or removing the call
  sends kind 2. DISMISS is sent only for forwarded notifications.
- AUTH_FAILED on reconnect means the bunny was reflashed or unpaired
  elsewhere. The phone then forgets the pairing locally and the status
  becomes `unpaired`. `removeBond` goes through reflection; if it fails,
  the user removes the bond in Bluetooth settings.
- `sendTestNotification` sends NOTIFY straight over the link, since the
  listener skips the app's own package.
- Link states: `connecting` covers the GATT connection through auth and
  setup. `disconnected` means waiting on autoConnect. The reconnect backoff
  runs from 1 to 30 s. SET_TIME is resent when the time zone or the clock
  changes.
- `./gradlew` must run with the JDK Flutter is configured with (21); the
  system default (27) fails jlink. `flutter build` picks the right one by
  itself.

## iOS (phase 3)

iOS doesn't let an app read other apps' notifications, so on an iPhone the
**ESP32 itself is the ANCS client**. The iOS app pairs the bunny, sends faces
and drawings, and pushes the notification rules. Notifications then flow from
iOS straight to the bunny, with or without the app running.

### Firmware: ANCS client (`firmware/nixxa_esp32/`)

- Advertising (owner-only and open alike) carries the ANCS service
  solicitation UUID `7905F431-B5CE-4E99-A40F-4B1E122D00D0`, so iOS offers
  "Share System Notifications" when bonding.
- After AUTH succeeds, the device discovers ANCS on the owner's connection.
  When it isn't there (Android), ANCS stays off for that connection. When it
  is: subscribe to Data Source then Notification Source. On each Added event
  (skip `PreExisting` and `Silent`), fetch through the Control Point:
  - Notification attributes: AppIdentifier, Title (64), Message (255).
  - App attribute DisplayName, cached for the 32 most recent app IDs.
- Filtering uses the stored SET_NOTIF_CONFIG: `enabled`, a rule with `show`
  for the AppIdentifier, and quiet hours. The mood is the rule's mood.
- `CategoryIDIncomingCall` maps to kind 1 (title = caller) when the calls
  flag is set; its Removed event maps to kind 2. Other Removed events
  dismiss a shown card. NotificationUID is used as the `notifId`. ANCS
  notifications go through the same queue and card renderer as NOTIFY.
- Rule mood `0xFF` = the config's `defaultMood`. This is a fix: it used to
  clamp to 0. Unknown moods still clamp to 0.
- SET_NOTIF_CONFIG gets an **optional trailing byte** after the rules:
  `ext u8`, bit0 = forward calls (default 1 when the byte is absent). Old
  payloads stay valid. Android and Dart encoders append it.
- New op **0x24 LIST_SEEN_APPS**: no payload; reply `count u8`, then per app
  `idLen u8, appId, nameLen u8, name`. These are the cached ANCS apps,
  newest first. It's empty on Android connections and after a reboot (RAM
  only).
- The ESP32 (Bluedroid) image must stay within app0 (0x140000). If ANCS
  doesn't fit, report it before changing the partition scheme.

### iOS app (`mobile/ios/`, Swift)

- `NixxaLink` (CoreBluetooth) implements the same `nixxa/link` MethodChannel
  and EventChannel as Android, so the Dart side is unchanged.
  - `CBCentralManager` uses a restore identifier, and the app declares
    `UIBackgroundModes: bluetooth-central`.
  - Pairing: scan for the service UUID and the name `NIXXA-<ID>`, connect,
    then read INFO. Reading INFO triggers the system pairing dialog. Then
    AUTH, SET_TIME and SET_NOTIF_CONFIG.
  - Reconnect: `connect()` to the stored peripheral identifier, which iOS
    keeps pending with no timeout, plus state restoration.
  - K and the identifier live in the Keychain
    (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`).
  - Same queue, timeouts and framing rules as Android. iOS negotiates the
    MTU itself; use `maximumWriteValueLength(for: .withResponse)`.
- Method mapping on iOS:
  - `hasNotificationAccess` = `peripheral.ancsAuthorized`.
  - `openNotificationAccessSettings` opens the app's Settings page. The UI
    explains: Settings › Bluetooth › NIXXA-… › Share System Notifications.
  - `requestBatteryExemption` is a no-op.
  - `listApps` = LIST_SEEN_APPS from the device (when connected) merged with
    a built-in catalogue of common bundle IDs (Messages, Phone, Mail,
    WhatsApp, Telegram, Signal, Gmail, Outlook, Slack, Teams, Discord,
    Instagram, Messenger, Snapchat, X, LinkedIn, Calendar), with
    `iconPng = null`.
  - `followDnd`: the iOS app sends SET_DND from `INFocusStatusCenter` while
    it runs (Focus Status permission). Otherwise DND isn't mirrored.
  - `sendTestNotification` sends NOTIFY directly.
- Info.plist: NSBluetoothAlwaysUsageDescription, NSCameraUsageDescription,
  NSFocusStatusUsageDescription, the background mode. Minimum iOS 15.
- UI (Dart, shared screens): on iOS the Notifications screen hides the
  battery button and shows the Bluetooth-settings hint. The app picker adds
  a "bundle ID" manual entry. Platform checks are local to the screens.

### CI

`.github/workflows/mobile.yml`:
- ubuntu job: `flutter analyze`, `flutter test`, Kotlin unit tests (JDK 21),
  `flutter build apk --debug`.
- macos job: `flutter build ios --debug --no-codesign`, which is the only
  place the Swift code compiles. This machine has no Xcode.

### Phase 3 implementation notes (as built)

- Advertising: the service UUID, the ANCS solicitation and the name don't fit
  together in 31 + 31 bytes. The scan response always carries the full name,
  and the advertising data alternates every 1 s (`NX_ADV_ROTATE_MS`) between
  flags + service UUID and flags + ANCS solicitation. Scans that filter on
  the service UUID see the bunny half the time.
- ANCS: Data Source is subscribed before Notification Source. One Control
  Point request is in flight at a time, with a 4 s timeout. A 32-entry LRU
  maps app ID to DisplayName (names up to 32 bytes; it's cleared on UNPAIR).
  If a name lookup fails, the card shows the bundle ID. ANCS cards need a
  rule with `show`. NOTIFY from Android still shows unless a rule hides it.
  The `ext` calls bit only gates ANCS calls. Service Changed is ignored; a
  late ANCS appearance is picked up at the next connection.
- On the ESP32, the ANCS client and the LRU (~4.5 KB) are allocated at boot,
  because static DRAM is tight. If that allocation fails, ANCS is off.
  `/state.bin` keeps the magic `NXS1` and always ends with `ext`. Older
  firmware rejects the new file and falls back to its defaults.
- Old firmware rejects the `ext` byte with BAD_MSG. The Kotlin and Swift
  links then resend SET_NOTIF_CONFIG without it and remember that for the
  rest of the connection.
- iOS:
  - Write chunks are `min(maximumWriteValueLength(.withoutResponse), 512)`,
    sent with response. This avoids prepared writes.
  - Pairing first checks `retrieveConnectedPeripherals`, since iOS may hold
    the bunny connected for ANCS, in which case it doesn't advertise.
  - A denied Bluetooth permission shows as `bluetoothOff`, and `pair`
    fails with PERMISSION.
  - `hasNotificationAccess` is false while the bunny is disconnected.
  - The iOS-only UI sits on the Device screen (the battery button is hidden
    and the Share System Notifications hint shown) and the Notifications
    screen (access card, manual bundle-ID entry, Focus hint).
- Not verified anywhere yet: the CoreBluetooth and Flutter bridge Swift
  code. It compiles only in the macOS CI job or on a Mac.
