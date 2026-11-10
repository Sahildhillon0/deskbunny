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
Later: Nano Every (UPDI), Nano 33 IoT (SAM-BA), ESP32 boards.

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
Fetched when the place or units change, every 15 min, 2 min after a failure.
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
- `GET /status`: `{ claude: ClaudeView, live: LiveStatus }`.

Claude screen (`claude.rs`). The hooks (installed by `claude_hooks_install`
into `~/.claude/settings.json`, entries marked by the `X-DeskBunny` header in
the command, file backed up to `settings.json.deskbunny.bak`) cover
SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, Notification, Stop and
SessionEnd. Each session is idle, starting, working (caption from the tool:
`EDIT main.rs`, `RUN cargo`, `READ x`, `SEARCH p`), needs you (a permission
prompt; idle reminders are ignored) or done (`DONE 3m12s`). The busiest
session wins: needs you > done > working > starting > idle. Silent sessions
are dropped after 30 min.
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
