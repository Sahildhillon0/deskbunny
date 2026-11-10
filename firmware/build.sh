#!/usr/bin/env bash
# Build every DeskBunny firmware image with arduino-cli.
#
#   firmware/build.sh            -> firmware/dist/<image> + manifest.json
#
# Needs arduino-cli and python3 on PATH (Linux, macOS, Git Bash on Windows).
# Installs the pinned Arduino cores if missing. Overrides:
#   ARDUINO_CLI=/path/to/arduino-cli
#   AVR_CORE_VERSION=1.8.6  RENESAS_CORE_VERSION=1.2.0  ESP32_CORE_VERSION=3.3.12
# Arduino boards build firmware/deskbunny; ESP32 boards (protocol "esptool")
# build firmware/nixxa_esp32 into a merged image flashed at 0x0.
# Contract: docs/ARCHITECTURE.md ("Boards", "ESP32 + BLE (nixxa)").
set -euo pipefail

FW_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$FW_DIR/.." && pwd)"
DIST="$FW_DIR/dist"
CLI="${ARDUINO_CLI:-arduino-cli}"
AVR_CORE_VERSION="${AVR_CORE_VERSION:-1.8.6}"
RENESAS_CORE_VERSION="${RENESAS_CORE_VERSION:-1.2.0}"
ESP32_CORE_VERSION="${ESP32_CORE_VERSION:-3.3.12}"
ESP32_INDEX_URL="https://espressif.github.io/arduino-esp32/package_esp32_index.json"
# Every arduino-cli call below sees the esp32 board manager index.
export ARDUINO_BOARD_MANAGER_ADDITIONAL_URLS="$ESP32_INDEX_URL"

# ATmega328P limits (32 KB flash with a 2 KB bootloader slot, 2 KB RAM).
MAX_FLASH_328P=30720
MIN_FREE_RAM_328P=400

die() { echo "build.sh: $*" >&2; exit 1; }

# Paths handed to arduino-cli must be native on Windows (Git Bash).
native_path() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi
}

PY=""
for cand in python3 python; do
  if command -v "$cand" >/dev/null 2>&1 &&
     "$cand" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 6) else 1)' >/dev/null 2>&1; then
    PY="$cand"
    break
  fi
done
[ -n "$PY" ] || die "python3 not found"
command -v "$CLI" >/dev/null 2>&1 || die "arduino-cli not found (set ARDUINO_CLI)"
FWTOOL="$(native_path "$FW_DIR/tools/fwtool.py")"

FW_VERSION="$(sed -n 's/^#define DB_FW_VERSION "\(.*\)".*/\1/p' "$FW_DIR/deskbunny/deskbunny.ino" | tr -d '\r')"
[ -n "$FW_VERSION" ] || die "DB_FW_VERSION not found in deskbunny.ino"
echo "== DeskBunny firmware $FW_VERSION"

# --- cores -----------------------------------------------------------------
"$CLI" core update-index
ensure_core() { # <core> <version>
  local have
  have="$("$CLI" core list | tr -d '\r' | awk -v c="$1" '$1 == c { print $2 }')"
  if [ "$have" != "$2" ]; then
    echo "== installing $1@$2 (have: ${have:-none})"
    "$CLI" core install "$1@$2"
  fi
}
ensure_core arduino:avr "$AVR_CORE_VERSION"
ensure_core arduino:renesas_uno "$RENESAS_CORE_VERSION"
ensure_core esp32:esp32 "$ESP32_CORE_VERSION"

# --- sketch copy (engine sources are copied, never symlinked) ----------------
TMP="$(mktemp -d 2>/dev/null || mktemp -d -t deskbunny)"
trap 'rm -rf "$TMP"' EXIT
SKETCH="$TMP/deskbunny"
mkdir -p "$SKETCH"
cp "$FW_DIR"/deskbunny/*.ino "$FW_DIR"/deskbunny/*.h "$SKETCH"/
cp "$ROOT/engine/deskbunny_engine.c" "$ROOT/engine/deskbunny_engine.h" "$SKETCH"/

# nixxa (ESP32): sketch root files only (tests/ stays out), the engine, and the
# release version so every image reports the same fwVersion.
NX_SKETCH="$TMP/nixxa_esp32"
mkdir -p "$NX_SKETCH"
cp "$FW_DIR"/nixxa_esp32/*.ino "$FW_DIR"/nixxa_esp32/*.h "$FW_DIR"/nixxa_esp32/*.c "$FW_DIR"/nixxa_esp32/*.cpp "$NX_SKETCH"/
cp "$ROOT/engine/deskbunny_engine.c" "$ROOT/engine/deskbunny_engine.h" "$NX_SKETCH"/
printf '#pragma once\n#define NX_FW_VERSION "%s"\n' "$FW_VERSION" > "$NX_SKETCH/nx_version.h"

rm -rf "$DIST"
mkdir -p "$DIST"

# --- one compile per distinct image ----------------------------------------
SUMMARY=""
while IFS=$'\t' read -r image fqbn stem sketch; do
  [ -n "$image" ] || continue
  echo
  echo "== $image ($fqbn)"
  out="$TMP/out-$stem"
  log="$TMP/log-$stem.txt"
  src_dir="$SKETCH"
  [ "$sketch" = "nixxa_esp32" ] && src_dir="$NX_SKETCH"
  "$CLI" compile \
    --fqbn "$fqbn" \
    --warnings default \
    --build-property "compiler.cpp.extra_flags=-DDB_BOARD_ID=$stem" \
    --output-dir "$(native_path "$out")" \
    "$(native_path "$src_dir")" 2>&1 | tee "$log"

  if [ "$sketch" = "nixxa_esp32" ]; then
    # Merged image (bootloader, partition table, boot_app0, app) cut after the
    # app; fwtool checks NVS is erased and the app fits app0.
    merged="$out/nixxa_esp32.ino.merged.bin"
    [ -f "$merged" ] || die "compiler output $merged missing"
    sizes="$("$PY" "$FWTOOL" espimage "$(native_path "$merged")" "$(native_path "$out/nixxa_esp32.ino.bin")" \
      "$(native_path "$DIST/$image")" | tr -d '\r')"
    app="${sizes%% *}"
    app0="${sizes##* }"
    ram="$(sed -n 's/.*Global variables use \([0-9]*\) bytes.*/\1/p' "$log" | tail -n 1)"
    line="$(printf '%-20s app %7s B of %7s B (%3s%%, %6s B free)  globals %6s B  image %7s B @ 0x0' \
      "$image" "$app" "$app0" "$((app * 100 / app0))" "$((app0 - app))" "${ram:-?}" \
      "$(wc -c < "$DIST/$image" | tr -d ' ')")"
    SUMMARY="$SUMMARY$line"$'\n'
    continue
  fi

  case "$image" in
    *.hex) src="$out/deskbunny.ino.hex" ;;
    *.bin) src="$out/deskbunny.ino.bin" ;;
    *) die "unknown image type: $image" ;;
  esac
  [ -f "$src" ] || die "compiler output $src missing"
  cp "$src" "$DIST/$image"

  offset="$("$PY" "$FWTOOL" locate "$(native_path "$DIST/$image")" | tr -d '\r')"
  artoffset="$("$PY" "$FWTOOL" locate "$(native_path "$DIST/$image")" art | tr -d '\r')"
  flash="$(sed -n 's/.*Sketch uses \([0-9]*\) bytes.*/\1/p' "$log" | tail -n 1)"
  ram="$(sed -n 's/.*Global variables use \([0-9]*\) bytes.*/\1/p' "$log" | tail -n 1)"
  free="$(sed -n 's/.*leaving \([0-9-]*\) bytes for local.*/\1/p' "$log" | tail -n 1)"
  line="$(printf '%-20s flash %6s B  globals %5s B  free RAM %5s B  blob @ 0x%x  art @ 0x%x' \
    "$image" "${flash:-?}" "${ram:-?}" "${free:-?}" "$offset" "$artoffset")"
  SUMMARY="$SUMMARY$line"$'\n'

  if [ "$stem" = "atmega328p" ]; then
    [ -n "$flash" ] && [ -n "$free" ] || die "could not read size report for $image"
    [ "$flash" -lt "$MAX_FLASH_328P" ] || die "$image: $flash B flash >= $MAX_FLASH_328P"
    [ "$free" -ge "$MIN_FREE_RAM_328P" ] || die "$image: only $free B RAM left for the stack"
  fi
done < <("$PY" "$FWTOOL" images "$(native_path "$FW_DIR/boards.json")" | tr -d '\r')

"$PY" "$FWTOOL" manifest "$(native_path "$FW_DIR/boards.json")" "$(native_path "$DIST")" "$FW_VERSION"

echo
echo "== summary"
printf '%s' "$SUMMARY"
echo "== wrote $DIST/manifest.json"
