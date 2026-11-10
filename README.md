# DeskBunny

A desk companion with moods. DeskBunny is a microcontroller board with a 0.96"
128x64 SSD1306 OLED that shows cute animated eyes: watching you, happy, grumpy,
sleepy, in love and more. The desktop app lets you pick or design a face,
preview it in a pixel-exact simulator, and flash it to the board. You don't
need the Arduino IDE. ESP32 boards ("nixxa") can also be driven from a phone
over Bluetooth.

- **30 eye animations (moods)** with blinking, glancing, breathing and overlays
  (hearts, zzz, tears, sparkles…), all editable.
- **Pixel-exact simulator.** The same C face engine runs on the board and in
  the app.
- **One-click flashing** in pure Rust: STK500v1/v2, AVR109, SAM-BA, DFU and the
  ESP32 ROM (esptool) protocol.
- **12 boards**: Arduino Uno R3, Nano, Pro Mini, Mega 2560, Leonardo, Micro,
  Uno R4 Minima / WiFi, and ESP32, ESP32-C3, ESP32-S3. See
  [Supported boards](#supported-boards).
- **Display driver without a library:** I2C or SPI on the Arduino boards (detected
  at boot), I2C on the ESP32 boards. See [Wiring](#wiring-and-pinouts).
- **Live screens over USB:** while the board is plugged in, the app can show a
  Claude Code status screen (Clawd), desktop notifications, a clock, weather and
  air quality, and your own designed screens, then falls back to the flashed face.
- **Phone app (ESP32 boards):** a Flutter app for Android and iOS pairs with the
  board by scanning a QR code on its screen, then sends faces, drawings and
  clocks, and forwards phone notifications and calls.
- **Mood cycle:** loop through moods you pick, in order or at random, with a
  blink at every change.
- **Draw your own screen:** a pixel editor (pen, shapes, fill, text, image and
  animated GIF import) for up to ten animation frames, shown instead of the
  face or on top of it. Stored in flash, so it costs the board no RAM.
- **Windows, macOS and Linux**, with a first-run setup guide and in-app updates.

## Contents

- [Supported boards](#supported-boards)
- [Wiring and pinouts](#wiring-and-pinouts)
- [Get started](#get-started)
- [Build from source](#build-from-source)
- [Scripts reference](#scripts-reference)
- [Layout](#layout)

## Supported boards

| Board | Chip | Flash protocol | Display bus |
|---|---|---|---|
| Uno R3 | ATmega328P | STK500v1 | I2C or SPI |
| Nano (new bootloader) | ATmega328P | STK500v1 | I2C or SPI |
| Nano (old bootloader) | ATmega328P | STK500v1 | I2C or SPI |
| Pro Mini 5V | ATmega328P | STK500v1 | I2C or SPI |
| Mega 2560 | ATmega2560 | STK500v2 | I2C or SPI |
| Leonardo | ATmega32U4 | AVR109 | I2C or SPI |
| Micro | ATmega32U4 | AVR109 | I2C or SPI |
| Uno R4 Minima | Renesas RA4M1 | DFU | I2C or SPI |
| Uno R4 WiFi | Renesas RA4M1 | SAM-BA | I2C or SPI |
| ESP32 (WROOM-32 DevKit) | ESP32 | esptool | I2C only |
| ESP32-C3 | ESP32-C3 | esptool | I2C only |
| ESP32-S3 | ESP32-S3 | esptool | I2C only |

The board list lives in [`firmware/boards.json`](firmware/boards.json).

## Wiring and pinouts

The display is a 0.96" SSD1306 128x64 OLED. 4-pin modules are I2C
(`GND VCC SCL SDA`); 7-pin modules are SPI (`GND VCC D0 D1 RES DC CS`) and can
be set to I2C by moving their resistor jumpers.

- **Arduino boards** check for an I2C display at boot (address `0x3C` or `0x3D`,
  both found automatically) and fall back to SPI, so either wiring runs the same
  firmware. Power the display from **5V** (or 3.3V if your module says so).
- **ESP32 boards** are I2C only, with no SPI and no RES pin. Their GPIOs are 3.3 V,
  so power the display from **3V3**. The address is found automatically.

### I2C pinouts, every board

| Board | SDA | SCL | RES (7-pin module only) |
|---|---|---|---|
| Uno R3 | A4 (or the SDA header) | A5 (or the SCL header) | 8 |
| Nano (both bootloaders) | A4 | A5 | 8 |
| Pro Mini 5V | A4 | A5 | 8 |
| Mega 2560 | 20 | 21 | 8 |
| Leonardo | 2 | 3 | 8 |
| Micro | 2 | 3 | 8 |
| Uno R4 Minima | A4 (or the SDA header) | A5 (or the SCL header) | 8 |
| Uno R4 WiFi | A4 (or the SDA header) | A5 (or the SCL header) | 8 |
| ESP32 (WROOM-32 DevKit) | GPIO 21 | GPIO 22 | not used |
| ESP32-C3 | GPIO 8 | GPIO 9 | not used |
| ESP32-S3 | GPIO 8 | GPIO 9 | not used |

For a 7-pin module set to I2C: wire `D0` to SCL, `D1` to SDA, `RES` to pin 8
(Arduino boards), and `DC` and `CS` to GND.

### SPI pinouts, Arduino boards

Bit-banged SPI, the same pin numbers on every Arduino board above (Uno R3, Nano,
Pro Mini, Mega 2560, Leonardo, Micro, Uno R4 Minima, Uno R4 WiFi):

| Display pin | D0 (CLK) | D1 (MOSI) | RES | DC | CS |
|---|---|---|---|---|---|
| Arduino pin | 13 | 11 | 8 | 9 | 10 |

### Quick wiring

```
OLED GND -> board GND
OLED VCC -> 5V   (Arduino)  /  3V3 (ESP32)
OLED SCL -> SCL pin from the table above
OLED SDA -> SDA pin from the table above
```

## Get started

Download an installer from [Releases](../../releases), then follow
[docs/SETUP.md](docs/SETUP.md). The app walks you through the same steps on
first launch.

For an ESP32 board: flash it from the app like any other board, then install the
phone app (see [Phone app](#phone-app-flutter)). The board shows a QR code until a
phone pairs with it; scan it from the app.

## Build from source

Needs Rust (stable), Node 22 and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS. Building
firmware also needs [arduino-cli](https://arduino.github.io/arduino-cli/) and
python3.

```sh
cd app && npm ci && npm run tauri dev           # run the desktop app
cargo test --workspace && make -C engine test   # Rust and engine tests
bash firmware/build.sh                          # all firmware images (needs arduino-cli)
```

### Phone app (Flutter)

```sh
cd mobile
flutter pub get
flutter analyze && flutter test
flutter build apk --debug                       # Android
flutter build ios --debug --no-codesign         # iOS (macOS only)
```

CI builds the firmware, the desktop installers for all three platforms and the
mobile apps on every push. Every green push to `main` publishes the next patch
version on the Releases page (`[minor]` or `[major]` in the commit message's first
line for bigger steps, `[skip release]` to skip), and the app offers it through its
update button. See [docs/RELEASING.md](docs/RELEASING.md).

## Scripts reference

### `scripts/deskbunny.sh`: run DeskBunny on Linux

One place for the desktop app's commands. Builds go to `~/.cache/deskbunny/target`
(override with `CARGO_TARGET_DIR`), logs to `~/.cache/deskbunny/deskbunny.log`.

```sh
scripts/deskbunny.sh update           # after pulling changes: build, app-menu entry,
                                      #   autostart, restart in the tray (rebuilds
                                      #   firmware only if engine/firmware sources changed)
scripts/deskbunny.sh dev              # run from source with hot reload
scripts/deskbunny.sh build            # build the release app
scripts/deskbunny.sh start            # start the built app in the background
scripts/deskbunny.sh start --hidden   # start straight into the tray (live mode keeps running)
scripts/deskbunny.sh stop             # stop it
scripts/deskbunny.sh restart          # stop, then start (takes the same --hidden flag)
scripts/deskbunny.sh status           # running? where are the logs, autostart and menu state
scripts/deskbunny.sh autostart on     # start hidden in the tray at every login
scripts/deskbunny.sh autostart off
scripts/deskbunny.sh menu on          # add this build to the app menu (opens the window)
scripts/deskbunny.sh menu off
```

Only one copy runs at a time: launching DeskBunny again just shows the running
window. Needs Node and Rust; `build` also needs `arduino-cli` the first time, for
the firmware images the app bundles.

### `firmware/build.sh`: build every firmware image

```sh
firmware/build.sh        # writes firmware/dist/<image> and firmware/dist/manifest.json
```

Builds all 8 images (3 AVR hex, 2 Uno R4 bin, 3 ESP32 bin) with arduino-cli,
installing the pinned cores if missing, and checks the ATmega328P flash and RAM
budgets. Needs `arduino-cli` and `python3`; works on Linux, macOS and Git Bash on
Windows. Overrides:

| Variable | Default | What |
|---|---|---|
| `ARDUINO_CLI` | `arduino-cli` | path to arduino-cli |
| `AVR_CORE_VERSION` | `1.8.6` | `arduino:avr` core |
| `RENESAS_CORE_VERSION` | `1.2.0` | `arduino:renesas_uno` core |
| `ESP32_CORE_VERSION` | `3.3.12` | `esp32:esp32` core |

### `scripts/bump-version.sh`: set the version everywhere

```sh
scripts/bump-version.sh 0.2.0
```

Sets the version in `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`, `package.json`
and the firmware's `DB_FW_VERSION`, so the next push to `main` releases it. Then
`git commit -am "release: v0.2.0" && git push`.

### `mobile/tool/sync_engine.sh`: keep the phone app's engine copy in sync

```sh
mobile/tool/sync_engine.sh            # copy engine/ and the clock sources into the Flutter plugin
mobile/tool/sync_engine.sh --check    # exit 1 if the copies are stale (CI runs this)
```

Run it after any change to `engine/deskbunny_engine.{c,h}` or
`firmware/nixxa_esp32/nx_clock.*`, and commit the result.

### `tools/deskbunny-notify`: show a notification on the board

```sh
tools/deskbunny-notify "Backup done" "42 GB in 12 min"
make && tools/deskbunny-notify "Build passed" || tools/deskbunny-notify "Build FAILED"
```

Arguments: `TITLE [BODY] [APP]`. The desktop app must be running; it talks to its
local endpoint on `127.0.0.1:47807` (every request needs the `X-DeskBunny`
header). Needs `curl`.

### `firmware/tools/livelink.py`: drive the board over USB from a terminal

A bench tool for the live link. Needs `pip install pyserial`. Opening the port
resets most Nanos, so it waits for the boot banner first. Every mode except
`info` and `home` keeps sending until Ctrl-C; the board returns to its flashed face
30 s after the last command.

```sh
firmware/tools/livelink.py PORT info                          # banner and live link version
firmware/tools/livelink.py PORT face happy sparkles -c "DONE" # an engine face with a caption
firmware/tools/livelink.py PORT clawd walk focus -c "EDIT main.rs"
firmware/tools/livelink.py PORT clock                         # clock pushed as frames
firmware/tools/livelink.py PORT frame image.pbm               # one 128x64 P4 PBM still
firmware/tools/livelink.py PORT demo                          # Claude states, 6 s each
firmware/tools/livelink.py PORT home                          # back to the flashed face
```

`PORT` is e.g. `/dev/ttyUSB0`, `/dev/ttyACM0` or `COM3`.

### `firmware/tools/fwtool.py`: helpers for `build.sh`

Standard-library Python used by the build script, not normally run by hand:

```sh
fwtool.py images <boards.json>                    # one line per distinct image
fwtool.py espimage <merged.bin> <app.bin> <out>   # cut and check an ESP32 image
fwtool.py locate <image.hex|.bin> [face|art]      # offset of the face or drawing blob
fwtool.py manifest <boards.json> <distdir> <ver>  # write manifest.json
```

### Engine tools (`engine/`)

```sh
make -C engine test     # gcc + clang, -Werror, ASan + UBSan (the 4700+ engine checks)
make -C engine sheet    # build/moods.pbm and build/moods.png: every mood x 4 moments
make -C engine render   # build/render, the frame renderer used below
```

```sh
engine/build/render <face> <t_start_ms> <t_end_ms> <step_ms> <outdir>  # PBM frames
engine/build/render sheet <out.pbm> [t_ms ...]                         # all moods
engine/build/render clawd <out.pbm> [t_ms ...]                         # Clawd animations
python3 engine/tools/pbm2png.py in.pbm out.png [scale]                 # PBM to PNG
```

`make -C firmware/nixxa_esp32/tests` runs the ESP32 firmware's host-side C tests.

## Layout

| Path | What |
|---|---|
| `engine/` | C99 face engine shared by the firmware and the simulator |
| `firmware/deskbunny/` | Arduino sketch for the AVR and Uno R4 boards |
| `firmware/nixxa_esp32/` | ESP32 / C3 / S3 firmware: BLE, QR pairing, notifications, clocks |
| `firmware/` | Board table (`boards.json`), build script, tools |
| `crates/deskbunny-core` | Rust bindings to the engine, face format, presets |
| `crates/deskbunny-flash` | Firmware patching and upload protocols (including esptool) |
| `app/` | Tauri v2 desktop app (Svelte 5 frontend) |
| `mobile/` | Flutter phone app for Android and iOS (ESP32 boards) |
| `scripts/` | `deskbunny.sh` launcher, `bump-version.sh` |
| `tools/` | `deskbunny-notify` |
| `docs/ARCHITECTURE.md` | How the parts fit together and the contracts between them |
| `docs/SETUP.md`, `docs/RELEASING.md` | First-run setup, how releases are made |
