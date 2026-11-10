#!/usr/bin/env python3
"""Helpers for firmware/build.sh (python3 stdlib only).

  fwtool.py images <boards.json>
      one line per distinct image: "<image>\t<fqbn>\t<stem>\t<sketch>" (first
      board wins); sketch is "nixxa_esp32" for esptool boards, else "deskbunny"
  fwtool.py espimage <merged.bin> <app.bin> <out.bin>
      ESP32 image: the arduino-cli merged image cut after the app (the core pads
      it to the whole flash, which would wipe app1 and the LittleFS library).
      Checks that NVS (0x9000..0xDFFF) is all 0xFF, that the app at 0x10000 is
      the app binary byte for byte and that it fits app0; prints
      "<app bytes> <app0 bytes>".
  fwtool.py locate <image.hex|image.bin> [face|art]
      prints the face (default) or drawing blob offset; exits 1 unless its
      magic occurs exactly once.
      .hex: absolute address in the HEX address space (= flash address on AVR)
      .bin: byte offset in the file
  fwtool.py manifest <boards.json> <distdir> <fwVersion>
      writes <distdir>/manifest.json (see docs/ARCHITECTURE.md). Images of
      esptool boards carry chip/flashOffset and no blob offsets.
"""
import hashlib
import json
import os
import sys

# art: the size is the slot's own capacity, read from header bytes 14..15.
BLOBS = {"face": (b"DBFACE01", 64), "art": (b"DBART001", None)}

# ESP32 images: the core's "default" 4 MB partition table.
ESP_NVS = (0x9000, 0xE000)   # nvs 0x9000/0x5000 (+ the gap up to otadata)
ESP_APP0 = (0x10000, 0x140000)  # offset, size
ESP_SKETCH = "nixxa_esp32"


def die(msg):
    sys.stderr.write("fwtool: %s\n" % msg)
    sys.exit(1)


def hex_to_flat(path):
    """Decode Intel HEX into (base_address, bytes); gaps are filled with 0xFF."""
    mem = {}
    upper = 0
    with open(path, "r") as fh:
        for lineno, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            if not line.startswith(":"):
                die("%s:%d: not an Intel HEX record" % (path, lineno))
            raw = bytes.fromhex(line[1:])
            if len(raw) < 5 or len(raw) != raw[0] + 5:
                die("%s:%d: bad record length" % (path, lineno))
            if sum(raw) & 0xFF:
                die("%s:%d: bad checksum" % (path, lineno))
            count, addr, rtype, data = raw[0], (raw[1] << 8) | raw[2], raw[3], raw[4:-1]
            if rtype == 0x00:
                base = upper + addr
                for i, b in enumerate(data):
                    mem[base + i] = b
            elif rtype == 0x01:
                break
            elif rtype == 0x02:
                upper = ((data[0] << 8) | data[1]) << 4
            elif rtype == 0x04:
                upper = ((data[0] << 8) | data[1]) << 16
            elif rtype in (0x03, 0x05):
                pass  # start address records
            else:
                die("%s:%d: unknown record type %02x" % (path, lineno, rtype))
    if not mem:
        die("%s: no data" % path)
    lo, hi = min(mem), max(mem)
    flat = bytearray(b"\xff" * (hi - lo + 1))
    for a, b in mem.items():
        flat[a - lo] = b
    return lo, bytes(flat)


def image_bytes(path):
    if path.endswith(".hex"):
        return hex_to_flat(path)
    with open(path, "rb") as fh:
        return 0, fh.read()


def art_capacity(path):
    """Drawing slot size in bytes (header included), from the image itself."""
    base, data = image_bytes(path)
    off = locate(path, "art") - base
    return data[off + 14] | (data[off + 15] << 8)


def locate(path, kind="face"):
    MAGIC, BLOB_SIZE = BLOBS[kind]
    base, data = image_bytes(path)
    hits = []
    start = 0
    while True:
        i = data.find(MAGIC, start)
        if i < 0:
            break
        hits.append(i)
        start = i + 1
    if len(hits) != 1:
        die("%s: magic %r found %d times (need exactly 1)" % (path, MAGIC.decode(), len(hits)))
    off = hits[0]
    if BLOB_SIZE is None:
        BLOB_SIZE = data[off + 14] | (data[off + 15] << 8) if off + 16 <= len(data) else 1 << 30
        if BLOB_SIZE < 16:
            die("%s: drawing slot at 0x%x has no capacity (old firmware?)" % (path, base + off))
    if off + BLOB_SIZE > len(data):
        die("%s: blob at 0x%x runs past the end of the image" % (path, base + off))
    return base + off


def load_boards(path):
    with open(path, "r") as fh:
        boards = json.load(fh)
    if not isinstance(boards, list) or not boards:
        die("%s: expected a non-empty array" % path)
    return boards


def distinct_images(boards):
    seen = {}
    for b in boards:
        seen.setdefault(b["image"], b)
    return seen


def is_esp(board):
    return board.get("protocol") == "esptool"


def cmd_images(boards_path):
    for image, b in distinct_images(load_boards(boards_path)).items():
        stem = os.path.splitext(image)[0]
        sketch = ESP_SKETCH if is_esp(b) else "deskbunny"
        print("%s\t%s\t%s\t%s" % (image, b["fqbn"], stem, sketch))


def cmd_espimage(merged_path, app_path, out_path):
    with open(merged_path, "rb") as fh:
        merged = fh.read()
    with open(app_path, "rb") as fh:
        app = fh.read()
    off, size = ESP_APP0
    if not app or app[0] != 0xE9:
        die("%s: not an ESP app image" % app_path)
    if len(app) > size:
        die("%s: app is %d bytes, app0 holds %d" % (app_path, len(app), size))
    end = off + len(app)
    if len(merged) < end or merged[off:end] != app:
        die("%s: app at 0x%x differs from %s" % (merged_path, off, app_path))
    lo, hi = ESP_NVS
    if merged[lo:hi] != b"\xff" * (hi - lo):
        die("%s: 0x%x..0x%x is not erased (a flash would keep the pairing)" % (merged_path, lo, hi - 1))
    if merged[end:].strip(b"\xff"):
        die("%s: data after the app at 0x%x" % (merged_path, end))
    with open(out_path, "wb") as fh:
        fh.write(merged[:end])
    print("%d %d" % (len(app), size))


def cmd_manifest(boards_path, distdir, version):
    boards = load_boards(boards_path)
    images = []
    for image, board in distinct_images(boards).items():
        path = os.path.join(distdir, image)
        if not os.path.exists(path):
            die("missing image %s" % path)
        with open(path, "rb") as fh:
            sha = hashlib.sha256(fh.read()).hexdigest()
        if is_esp(board):
            # Never patched: no blob slots, so no magic-occurs-once check.
            images.append({
                "file": image,
                "format": "bin",
                "sha256": sha,
                "chip": board["mcu"],
                "flashOffset": 0,
                "blobOffset": None,
                "artOffset": None,
            })
            continue
        images.append({
            "file": image,
            "format": "hex" if image.endswith(".hex") else "bin",
            "sha256": sha,
            "blobOffset": locate(path),
            "artOffset": locate(path, "art"),
            "artCapacity": art_capacity(path),
        })
    manifest = {"fwVersion": version, "images": images, "boards": boards}
    with open(os.path.join(distdir, "manifest.json"), "w", newline="\n") as fh:
        json.dump(manifest, fh, indent=2)
        fh.write("\n")


def main(argv):
    if len(argv) == 3 and argv[1] == "images":
        cmd_images(argv[2])
    elif len(argv) in (3, 4) and argv[1] == "locate" and (len(argv) == 3 or argv[3] in BLOBS):
        print(locate(argv[2], *argv[3:]))
    elif len(argv) == 5 and argv[1] == "espimage":
        cmd_espimage(argv[2], argv[3], argv[4])
    elif len(argv) == 5 and argv[1] == "manifest":
        cmd_manifest(argv[2], argv[3], argv[4])
    else:
        sys.stderr.write(__doc__)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
