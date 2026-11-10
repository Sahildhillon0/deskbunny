#!/usr/bin/env python3
"""Drive a DeskBunny over the live link (docs/ARCHITECTURE.md, "Live link v1").

A bench tool for the firmware side; the desktop app will own the port later.
Needs pyserial (pip install pyserial).

  livelink.py PORT info                         banner + live link version
  livelink.py PORT face MOOD [OVERLAY] [-c TEXT] engine face, e.g. face happy sparkles -c "DONE"
  livelink.py PORT clawd ANIM [EXPR] [-c TEXT]  Clawd, e.g. clawd walk focus -c "EDIT main.rs"
  livelink.py PORT clock                        clock pushed as frames once a second
  livelink.py PORT frame FILE.pbm               one 128x64 P4 PBM as a still frame
  livelink.py PORT demo                         Claude states, 6 s each, forever
  livelink.py PORT home                         back to the flashed face

Every mode but info/home keeps the port open and re-sends until Ctrl-C (the
board returns to its flashed face 30 s after the last command). Opening the
port resets most Nanos (DTR), so the tool waits for the boot banner first.
"""
import argparse
import os
import re
import sys
import time

try:
    import serial
except ImportError:
    sys.exit("pyserial is missing: pip install pyserial")

MOODS = ["neutral", "happy", "angry", "sad", "sleepy", "surprised", "love", "suspicious",
         "excited", "bored", "scared", "dizzy", "wink", "laugh", "crying", "skeptical", "smug",
         "evil", "worried", "shy", "squint", "dead", "asleep", "starry", "hypno", "glitch",
         "scan", "eyeroll", "confused", "determined"]
OVERLAYS = ["none", "hearts", "zzz", "sweat", "anger", "tears", "sparkles", "question"]
ANIMS = ["idle", "walk", "jump", "shake", "sleep", "work"]
EXPRS = ["open", "happy", "closed", "wide", "squint", "dead", "worried", "focus"]

W, H, FB = 128, 64, 1024
FONT_H = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "deskbunny", "db_font5x7.h")


def index(name, table, what):
    if name.isdigit():
        return int(name)
    try:
        return table.index(name.lower())
    except ValueError:
        sys.exit(f"unknown {what} '{name}'; one of: {', '.join(table)}")


# --- frames -----------------------------------------------------------------
def packbits(data):
    """PackBits as the firmware decodes it: t < 128 -> t+1 literals, t >= 128 -> run of t-126."""
    out, i, n = bytearray(), 0, len(data)
    while i < n:
        run = 1
        while i + run < n and run < 129 and data[i + run] == data[i]:
            run += 1
        if run >= 2:
            out += bytes([run + 126, data[i]])
            i += run
            continue
        j = i
        while j < n and j - i < 128 and not (j + 1 < n and data[j + 1] == data[j]):
            j += 1
        out.append(j - i - 1)
        out += data[i:j]
        i = j
    return bytes(out)


def new_fb():
    return bytearray(FB)


def pset(fb, x, y):
    if 0 <= x < W and 0 <= y < H:
        fb[(y >> 3) * W + x] |= 1 << (y & 7)


def load_font():
    with open(FONT_H, encoding="ascii") as f:
        body = f.read().split("=", 1)[1]
    return [int(v, 16) for v in re.findall(r"0x([0-9A-Fa-f]{2})", body)]


def text(fb, font, s, x, y, scale=1):
    for ch in s:
        c = ord(ch) if 32 <= ord(ch) <= 122 else 32
        g = font[(c - 32) * 5:(c - 32) * 5 + 5]
        for col, bits in enumerate(g):
            for row in range(7):
                if bits >> row & 1:
                    for dy in range(scale):
                        for dx in range(scale):
                            pset(fb, x + col * scale + dx, y + row * scale + dy)
        x += 6 * scale


def text_width(s, scale):
    return len(s) * 6 * scale - scale


def read_pbm(path):
    with open(path, "rb") as f:
        raw = f.read()
    m = re.match(rb"P4\s+(?:#.*\s+)*(\d+)\s+(\d+)\s", raw)
    if not m or (int(m.group(1)), int(m.group(2))) != (W, H):
        sys.exit("need a 128x64 binary (P4) PBM")
    bits, fb = raw[m.end():], new_fb()
    for y in range(H):
        for x in range(W):
            if bits[y * 16 + (x >> 3)] & (0x80 >> (x & 7)):
                pset(fb, x, y)
    return fb


# --- link -------------------------------------------------------------------
class Link:
    def __init__(self, port):
        self.s = serial.Serial(port, 115200, timeout=0.1)
        self.banner = self._wait_banner()

    def _readline(self, timeout):
        end, buf = time.time() + timeout, b""
        while time.time() < end:
            buf += self.s.read(1)
            if buf.endswith(b"\n"):
                return buf.decode("ascii", "replace").strip()
        return None

    def _wait_banner(self):
        # A Nano reboots on open (~1.5 s with the splash); native-USB boards don't.
        end = time.time() + 4
        while time.time() < end:
            self.s.write(b"?\n")
            line = self._readline(0.6)
            while line is not None:
                if line.startswith("DESKBUNNY"):
                    # More "?" may still be queued on the board: let their
                    # banners arrive and drop them.
                    while self._readline(0.3) is not None:
                        pass
                    return line
                line = self._readline(0.05)
        sys.exit("no DESKBUNNY banner: wrong port, or old firmware?")

    def _reply(self):
        """Next reply line, skipping stray boot banners."""
        while True:
            line = self._readline(1.0)
            if line is None or (line and not line.startswith("DESKBUNNY")):
                return line

    def cmd(self, line, payload=None):
        self.s.write(line.encode("ascii") + b"\n")
        reply = self._reply()
        if payload is not None:
            if reply != "K":
                return reply
            self.s.write(payload)
            reply = self._reply()
        return reply

    def frame(self, fb, prev=None):
        """Send fb whole, or as an XOR delta from prev when that is shorter."""
        whole = packbits(bytes(fb))
        if prev is not None:
            delta = packbits(bytes(a ^ b for a, b in zip(fb, prev)))
            if len(delta) < len(whole):
                return self.cmd(f"!X {len(delta)}", delta)
        return self.cmd(f"!F {len(whole)}", whole)


def expect_ok(reply, what):
    if reply != "OK":
        sys.exit(f"{what}: device said {reply!r}")


def hold(link, line, every=10.0):
    expect_ok(link.cmd(line), line)
    print(f"{line}  (Ctrl-C to stop)")
    while True:
        time.sleep(every)
        expect_ok(link.cmd("!P"), "keepalive")


def clock(link):
    font, prev = load_font(), None
    print("clock  (Ctrl-C to stop)")
    while True:
        now = time.localtime()
        fb = new_fb()
        hm = time.strftime("%H:%M", now)
        text(fb, font, hm, (W - text_width(hm, 4)) // 2, 6, 4)
        for x in range(4, 4 + (W - 8) * now.tm_sec // 59):
            pset(fb, x, 40)
        date = time.strftime("%a %d %b", now).upper()
        text(fb, font, date, (W - text_width(date, 1)) // 2, 50)
        reply = link.frame(fb, prev)
        if reply != "OK":
            print(f"frame: {reply!r}, resending whole")
            prev = None
            continue
        prev = fb
        time.sleep(1.0 - (time.time() % 1.0))


def demo(link):
    clawd = "LIVE 1 C" in (link.cmd("!V") or "")
    states = [
        ("STARTING", ("excited", "sparkles"), ("idle", "wide")),
        ("EDIT main.rs", ("determined", "none"), ("work", "focus")),
        ("THINKING", ("scan", "none"), ("walk", "focus")),
        ("NEEDS YOU", ("worried", "question"), ("shake", "worried")),
        ("DONE 3m12s", ("happy", "sparkles"), ("jump", "happy")),
        ("IDLE", ("asleep", "zzz"), ("sleep", "closed")),
    ]
    print("demo  (Ctrl-C to stop)" + ("" if clawd else ": no Clawd on this board, face only"))
    while True:
        for cap, (mood, ov), (anim, ex) in states:
            for use_clawd in ([True, False] if clawd else [False]):
                if use_clawd:
                    line = f"!C {ANIMS.index(anim)} {EXPRS.index(ex)} {cap}"
                else:
                    line = f"!M {MOODS.index(mood)} {OVERLAYS.index(ov)} {cap}"
                expect_ok(link.cmd(line), line)
                print(f"  {line}")
                time.sleep(3)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("port")
    ap.add_argument("mode", choices=["info", "face", "clawd", "clock", "frame", "demo", "home"])
    ap.add_argument("args", nargs="*")
    ap.add_argument("-c", "--caption", default="", help="bottom line, up to 21 characters")
    a = ap.parse_args()

    link = Link(a.port)
    print(link.banner)
    try:
        if a.mode == "info":
            print(link.cmd("!V") or "no reply to !V (firmware without the live link)")
        elif a.mode == "home":
            expect_ok(link.cmd("!H"), "!H")
        elif a.mode in ("face", "clawd"):
            if not a.args:
                sys.exit(f"{a.mode}: name a {'mood' if a.mode == 'face' else 'animation'}")
            if a.mode == "face":
                x = index(a.args[0], MOODS, "mood")
                y = index(a.args[1], OVERLAYS, "overlay") if len(a.args) > 1 else 0
                hold(link, f"!M {x} {y} {a.caption}".rstrip())
            else:
                x = index(a.args[0], ANIMS, "animation")
                y = index(a.args[1], EXPRS, "expression") if len(a.args) > 1 else 0
                hold(link, f"!C {x} {y} {a.caption}".rstrip())
        elif a.mode == "frame":
            if not a.args:
                sys.exit("frame: give a .pbm file")
            expect_ok(link.frame(read_pbm(a.args[0])), "!F")
            hold(link, "!P")
        elif a.mode == "clock":
            clock(link)
        else:
            demo(link)
    except KeyboardInterrupt:
        link.cmd("!H")
        print()


if __name__ == "__main__":
    main()
