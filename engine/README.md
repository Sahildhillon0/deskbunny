# DeskBunny face engine

Portable C99 (integer only, no malloc, no stdio) that draws the face into a
1024-byte SSD1306 page-layout framebuffer. The same two files run on the
Arduino (copied into the sketch by `firmware/build.sh`) and in the desktop
simulator (compiled by the `cc` crate), so the preview is pixel-identical.
API and blob format: `deskbunny_engine.h`, `docs/ARCHITECTURE.md`.

## Build and test on the host

```sh
cd engine
make test     # gcc + clang, -std=c99 -Wall -Wextra -Wpedantic -Wconversion -Werror, ASan + UBSan
make sheet    # build/moods.pbm + build/moods.png: every mood x 4 moments
```

Without make:

```sh
mkdir -p build
cc -std=c99 -O2 -o build/render tools/render.c deskbunny_engine.c
cc -std=c99 -g -fsanitize=address,undefined -o build/test tests/test_engine.c deskbunny_engine.c && ./build/test
```

## Renderer

```sh
build/render <face> <t_start_ms> <t_end_ms> <step_ms> <outdir>   # frame_NNNNN.pbm + strip.pbm
build/render sheet <out.pbm> [t_ms ...]                            # contact sheet of all moods
build/render blob <face> <out.bin>                                 # encode a 64-byte blob
build/render blobhdr > ../firmware/deskbunny/db_default_blob.h     # regenerate the default blob
python3 tools/pbm2png.py in.pbm out.png [scale]                    # stdlib-only PNG conversion
```

`<face>` is a `*.bin` blob or a spec `mood[:overlay][+mouth][+cyclops][+invert][+rotate][+noblink][+nolook]`,
e.g. `love:hearts+mouth`, `sleepy:zzz`, `dizzy+cyclops`. The clock always runs
from 0 in 33 ms steps up to `t_start_ms`, as on the device.

## Notes

- Every eye is drawn as one vertical span per column (base shape, then lids,
  then blink scaling), which maps directly onto SSD1306 pages.
- `int` is 16 bits on AVR: products that can exceed 32767 are done in
  `int32_t`. The engine also builds warning-free for `clang --target=avr`.
- Animation clock = `now_ms * speed`; periodic animations use power-of-two
  periods of it, so `millis()` wrap-around is seamless.
- The magic `DBFACE01` is never stored as a literal here (it is XOR-ed), so
  the firmware image holds it exactly once, in `DB_FACE_BLOB`.
- `invert` and `rotate180` are applied to the framebuffer by the engine. The
  firmware clears those two bits before `db_engine_init` and uses the panel's
  own commands instead; the pixels are the same.
