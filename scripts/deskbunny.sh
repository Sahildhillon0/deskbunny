#!/usr/bin/env bash
# DeskBunny launcher (Linux). One place for the commands nobody remembers.
#
#   scripts/deskbunny.sh update          everything after pulling changes: build, app-menu entry,
#                                        autostart at login, restart in the tray
#   scripts/deskbunny.sh dev             run the app from source (hot reload)
#   scripts/deskbunny.sh build           build the release app (once, and after updates)
#   scripts/deskbunny.sh start           start the built app in the background, window shown
#   scripts/deskbunny.sh start --hidden  ... straight to the tray (live mode keeps running)
#   scripts/deskbunny.sh stop            stop it
#   scripts/deskbunny.sh status          is it running? where are the logs?
#   scripts/deskbunny.sh autostart on    start it hidden in the tray at every login
#   scripts/deskbunny.sh autostart off
#   scripts/deskbunny.sh menu on         add this build to the app menu (opens the window)
#   scripts/deskbunny.sh menu off
#
# Builds go to ~/.cache/deskbunny/target (override with CARGO_TARGET_DIR), not
# the repo. Logs: ~/.cache/deskbunny/deskbunny.log.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/app"
STATE="${XDG_CACHE_HOME:-$HOME/.cache}/deskbunny"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$STATE/target}"
BIN="$CARGO_TARGET_DIR/release/deskbunny-app"
PIDFILE="$STATE/deskbunny.pid"
LOG="$STATE/deskbunny.log"
DESKTOP="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/deskbunny.desktop"
MENU="${XDG_DATA_HOME:-$HOME/.local/share}/applications/deskbunny.desktop"

mkdir -p "$STATE"

die() { echo "deskbunny: $*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is not installed ($2)"; }

running_pid() {
  local pid
  pid="$(cat "$PIDFILE" 2>/dev/null || true)"
  # Only trust the pid file if that process really is DeskBunny.
  if [ -n "$pid" ] && [ "$(cat "/proc/$pid/comm" 2>/dev/null || true)" = "deskbunny-app" ]; then
    echo "$pid"
    return
  fi
  # Started some other way (autostart at login, the app menu): find it by name.
  pgrep -x -u "$(id -u)" deskbunny-app 2>/dev/null | head -n 1 || true
}

node_deps() {
  need npm "sudo apt install nodejs npm"
  if [ ! -d "$APP/node_modules" ]; then
    echo "== installing frontend packages"
    (cd "$APP" && npm ci)
  fi
}

firmware() {
  # The app bundles the firmware images: build them when they are missing, or
  # when the engine or firmware sources changed since the last build.
  local stamp="$ROOT/firmware/dist/manifest.json" changed=""
  if [ -f "$stamp" ]; then
    changed="$(find "$ROOT/engine" "$ROOT/firmware/deskbunny" "$ROOT/firmware/nixxa_esp32" \
      "$ROOT/firmware/boards.json" "$ROOT/firmware/build.sh" \
      -type f -newer "$stamp" -print -quit 2>/dev/null)"
  fi
  if ! ls "$ROOT/firmware/dist/"*.hex >/dev/null 2>&1 || [ -n "$changed" ]; then
    need arduino-cli "see firmware/build.sh"
    echo "== building firmware images"
    TMPDIR="$STATE" "$ROOT/firmware/build.sh"
  fi
}

cmd_dev() {
  need cargo "install rustup, then: rustup default stable"
  node_deps
  firmware
  cd "$APP" && exec npm run tauri dev
}

cmd_build() {
  need cargo "install rustup, then: rustup default stable"
  node_deps
  firmware
  echo "== building DeskBunny (release, a few minutes the first time)"
  (cd "$APP" && npx tauri build --no-bundle)
  [ -x "$BIN" ] || die "build finished but $BIN is missing"
  echo "== built $BIN"
}

cmd_start() {
  local pid
  pid="$(running_pid)"
  if [ -n "$pid" ]; then
    echo "DeskBunny is already running (pid $pid)."
    return 0
  fi
  [ -x "$BIN" ] || cmd_build
  # setsid: detach from this terminal so closing it does not stop the app.
  setsid "$BIN" "$@" >>"$LOG" 2>&1 < /dev/null &
  echo $! > "$PIDFILE"
  sleep 1
  pid="$(running_pid)"
  [ -n "$pid" ] || die "DeskBunny exited right away; see $LOG"
  echo "DeskBunny started (pid $pid). Logs: $LOG"
}

cmd_stop() {
  local pid
  pid="$(running_pid)"
  if [ -z "$pid" ]; then
    echo "DeskBunny is not running (started outside this script? quit it from the tray)."
    return 0
  fi
  kill "$pid"
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    [ -d "/proc/$pid" ] || break
    sleep 0.3
  done
  rm -f "$PIDFILE"
  echo "DeskBunny stopped."
}

cmd_status() {
  local pid
  pid="$(running_pid)"
  if [ -n "$pid" ]; then echo "running (pid $pid)"; else echo "not running"; fi
  if [ -x "$BIN" ]; then echo "app:       $BIN"; else echo "app:       not built (scripts/deskbunny.sh build)"; fi
  echo "logs:      $LOG"
  if [ -f "$DESKTOP" ]; then echo "autostart: on ($DESKTOP)"; else echo "autostart: off"; fi
  if [ -f "$MENU" ]; then echo "app menu:  on ($MENU)"; else echo "app menu:  off"; fi
  if [ -x /usr/bin/deskbunny-app ]; then
    echo "note:      an installed package also provides /usr/bin/deskbunny-app (sudo apt remove desk-bunny)"
  fi
}

write_autostart() {
  mkdir -p "$(dirname "$DESKTOP")"
  cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=DeskBunny
Comment=Live screens for the DeskBunny OLED (clock, weather, Claude, notifications)
Exec="$BIN" --hidden
Icon=$APP/src-tauri/icons/128x128.png
Terminal=false
X-GNOME-Autostart-enabled=true
X-GNOME-Autostart-Delay=5
EOF
}

cmd_autostart() {
  case "${1:-}" in
    on)
      [ -x "$BIN" ] || cmd_build
      write_autostart
      echo "Autostart on: DeskBunny starts in the tray at every login ($DESKTOP)."
      echo "Rebuilds (scripts/deskbunny.sh build) are picked up at the next login."
      ;;
    off)
      rm -f "$DESKTOP"
      echo "Autostart off."
      ;;
    *) die "usage: deskbunny.sh autostart on|off" ;;
  esac
}

write_menu() {
  mkdir -p "$(dirname "$MENU")"
  # Launching while the tray copy runs just shows its window (single instance).
  cat > "$MENU" <<EOF
[Desktop Entry]
Type=Application
Name=DeskBunny
Comment=Animated faces and live screens for the DeskBunny OLED
Exec="$BIN"
Icon=$APP/src-tauri/icons/128x128.png
StartupWMClass=deskbunny-app
Categories=Development;
Terminal=false
EOF
  command -v update-desktop-database >/dev/null 2>&1 \
    && update-desktop-database "$(dirname "$MENU")" >/dev/null 2>&1 || true
}

cmd_menu() {
  case "${1:-}" in
    on)
      [ -x "$BIN" ] || cmd_build
      write_menu
      echo "App menu entry added: $MENU"
      ;;
    off)
      rm -f "$MENU"
      echo "App menu entry removed."
      ;;
    *) die "usage: deskbunny.sh menu on|off" ;;
  esac
}

cmd_update() {
  cmd_build
  write_menu
  echo "== app menu entry: $MENU"
  write_autostart
  echo "== autostart at login: $DESKTOP"
  echo "== restarting in the tray"
  cmd_stop
  cmd_start --hidden
  if [ -x /usr/bin/deskbunny-app ]; then
    echo "note: the old package is still installed (sudo apt remove desk-bunny)"
  fi
}

case "${1:-}" in
  update) cmd_update ;;
  dev) cmd_dev ;;
  build) cmd_build ;;
  start) shift; cmd_start "$@" ;;
  stop) cmd_stop ;;
  restart) cmd_stop; shift; cmd_start "$@" ;;
  status) cmd_status ;;
  autostart) shift; cmd_autostart "$@" ;;
  menu) shift; cmd_menu "$@" ;;
  *)
    sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
