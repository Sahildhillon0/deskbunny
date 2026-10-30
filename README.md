# DeskBunny

A desk companion with moods. DeskBunny is an Arduino with a 0.96" 128x64
SSD1306 OLED that shows cute animated eyes: watching you, happy, grumpy,
sleepy, in love and more. This desktop app lets you pick or design a face,
preview it in a pixel-exact simulator, and flash it to the board. You don't
need the Arduino IDE.

- **12 moods** with blinking, glancing, breathing and overlays (hearts, zzz,
  tears, sparkles…), all editable.
- **Pixel-exact simulator.** The same C face engine runs on the board and in
  the app.
- **One-click flashing** in pure Rust: STK500v1/v2, AVR109, SAM-BA and DFU.
- **Boards:** Uno R3, Nano (old and new bootloader), Pro Mini, Mega 2560,
  Leonardo, Micro, Uno R4 Minima, Uno R4 WiFi.
- **Display driver without a library:** I2C or SPI, detected at boot. See
  [Wiring](#wiring).
- **Mood cycle:** loop through moods you pick, in order or at random, with a
  blink at every change.
- **Draw your own screen:** a pixel editor (pen, shapes, fill, text, image
  and animated GIF import) for up to four animation frames, shown instead of
  the face or on top of it. Stored in flash, so it costs the board no RAM.
- **Windows, macOS and Linux**, with a first-run setup guide and in-app updates.

## Wiring

The 0.96" SSD1306 128x64 OLED works over I2C or SPI. The firmware checks for
an I2C display at boot and falls back to SPI, so either wiring runs the same
firmware. Power the display from **5V** (or 3.3V if your module says so) and GND.

**I2C** (4-pin modules: GND VCC SCL SDA)

| Display | Uno / Nano / Pro Mini / Uno R4 | Mega 2560 | Leonardo / Micro |
|---|---|---|---|
| SDA | A4 (or the SDA header) | 20 | 2 |
| SCL | A5 (or the SCL header) | 21 | 3 |
| RES (7-pin modules only) | 8 | 8 | 8 |

Address 0x3C or 0x3D, both found automatically. A 7-pin module set to I2C
(its resistor jumpers moved) wires D0 to SCL, D1 to SDA, RES to pin 8, and DC
and CS to GND.

**SPI** (7-pin modules: GND VCC D0 D1 RES DC CS), same pins on every board:

| Display | D0 (CLK) | D1 (MOSI) | RES | DC | CS |
|---|---|---|---|---|---|
| Arduino pin | 13 | 11 | 8 | 9 | 10 |

## Get started

Download an installer from [Releases](../../releases), then follow
[docs/SETUP.md](docs/SETUP.md). The app walks you through the same steps on
first launch.

## Build from source

Needs Rust (stable), Node 20+ and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```sh
cd app && npm ci && npm run tauri dev       # run the app
cargo test --workspace && make -C engine test  # tests
bash firmware/build.sh                       # firmware images (needs arduino-cli)
```

CI builds the firmware and installers for all three platforms on every push.
Every green push to `main` publishes the next patch version on the Releases
page (`[minor]` or `[major]` in the commit message for bigger steps), and the
app offers it through its update button; see
[docs/RELEASING.md](docs/RELEASING.md).

## Layout

| Path | What |
|---|---|
| `engine/` | C99 face engine shared by the firmware and the simulator |
| `firmware/` | Arduino sketch, board table, build script |
| `crates/deskbunny-core` | Rust bindings to the engine, face format, presets |
| `crates/deskbunny-flash` | Firmware patching and upload protocols |
| `app/` | Tauri v2 app (Svelte 5 frontend) |
| `hardware/enclosure/` | 3D-printable snap-fit case for a Nano: Clawd on a chair, OLED flush in its face |
| `docs/ARCHITECTURE.md` | How the parts fit together and the contracts between them |
