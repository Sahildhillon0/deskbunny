#!/usr/bin/env python3
"""Helpers for firmware/build.sh (python3 stdlib only).

  fwtool.py images <boards.json>
      one line per distinct image: "<image>\t<fqbn>\t<stem>" (first board wins)
  fwtool.py locate <image.hex|image.bin>
      prints the blob offset; exits 1 unless the magic occurs exactly once.
      .hex: absolute address in the HEX address space (= flash address on AVR)
      .bin: byte offset in the file
  fwtool.py manifest <boards.json> <distdir> <fwVersion>
      writes <distdir>/manifest.json (see docs/ARCHITECTURE.md)
"""
import hashlib
import json
import os
import sys

MAGIC = b"DBFACE01"
BLOB_SIZE = 64


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


def locate(path):
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


def cmd_images(boards_path):
    for image, b in distinct_images(load_boards(boards_path)).items():
        stem = os.path.splitext(image)[0]
        print("%s\t%s\t%s" % (image, b["fqbn"], stem))


def cmd_manifest(boards_path, distdir, version):
    boards = load_boards(boards_path)
    images = []
    for image in distinct_images(boards):
        path = os.path.join(distdir, image)
        if not os.path.exists(path):
            die("missing image %s" % path)
        with open(path, "rb") as fh:
            sha = hashlib.sha256(fh.read()).hexdigest()
        images.append({
            "file": image,
            "format": "hex" if image.endswith(".hex") else "bin",
            "sha256": sha,
            "blobOffset": locate(path),
        })
    manifest = {"fwVersion": version, "images": images, "boards": boards}
    with open(os.path.join(distdir, "manifest.json"), "w", newline="\n") as fh:
        json.dump(manifest, fh, indent=2)
        fh.write("\n")


def main(argv):
    if len(argv) == 3 and argv[1] == "images":
        cmd_images(argv[2])
    elif len(argv) == 3 and argv[1] == "locate":
        print(locate(argv[2]))
    elif len(argv) == 5 and argv[1] == "manifest":
        cmd_manifest(argv[2], argv[3], argv[4])
    else:
        sys.stderr.write(__doc__)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
