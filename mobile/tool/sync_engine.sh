#!/usr/bin/env bash
# Copy the C sources into the nixxa_engine FFI plugin (copies, never
# symlinks; the copies are committed so builds work without running this).
#   tool/sync_engine.sh          copy the sources into the plugin
#   tool/sync_engine.sh --check  exit 1 if the plugin's copies are stale
# Sources: engine/deskbunny_engine.{c,h} (the face engine) and the clock
# module firmware/nixxa_esp32/nx_clock.{c,h} with its font nx_font5x7.h.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="$here/../../engine"
fw="$here/../../firmware/nixxa_esp32"
dst="$here/../packages/nixxa_engine/src"
stale=0
for path in "$src/deskbunny_engine.c" "$src/deskbunny_engine.h" \
            "$fw/nx_clock.c" "$fw/nx_clock.h" "$fw/nx_font5x7.h"; do
  f="$(basename "$path")"
  if [[ "${1:-}" == "--check" ]]; then
    if ! cmp -s "$path" "$dst/$f"; then
      echo "stale: packages/nixxa_engine/src/$f (run mobile/tool/sync_engine.sh)" >&2
      stale=1
    fi
  else
    rm -f "$dst/$f"
    cp "$path" "$dst/$f"
    echo "copied $path -> packages/nixxa_engine/src/$f"
  fi
done
exit $stale
