#!/usr/bin/env python3
"""Convert a binary PBM (P4) to a grayscale PNG using only the stdlib.

usage: pbm2png.py in.pbm out.png [scale]
Lit pixels are drawn white on black, like the OLED.
"""
import struct
import sys
import zlib


def read_pbm(path):
    data = open(path, "rb").read()
    fields, pos = [], 0
    while len(fields) < 3:
        while data[pos:pos + 1].isspace():
            pos += 1
        if data[pos:pos + 1] == b"#":
            pos = data.index(b"\n", pos) + 1
            continue
        start = pos
        while not data[pos:pos + 1].isspace():
            pos += 1
        fields.append(data[start:pos])
    if fields[0] != b"P4":
        raise SystemExit("not a binary PBM")
    w, h = int(fields[1]), int(fields[2])
    pos += 1
    stride = (w + 7) // 8
    rows = []
    for y in range(h):
        row = data[pos + y * stride:pos + (y + 1) * stride]
        rows.append([(row[x >> 3] >> (7 - (x & 7))) & 1 for x in range(w)])
    return w, h, rows


def write_png(path, w, h, rows, scale):
    raw = bytearray()
    for row in rows:
        line = bytearray(b"\x00")
        for v in row:
            line += (b"\xff" if v else b"\x00") * scale
        for _ in range(scale):
            raw += line

    def chunk(tag, body):
        return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w * scale, h * scale, 8, 0, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    open(path, "wb").write(png)


def main():
    if len(sys.argv) not in (3, 4):
        raise SystemExit(__doc__)
    scale = int(sys.argv[3]) if len(sys.argv) == 4 else 2
    w, h, rows = read_pbm(sys.argv[1])
    write_png(sys.argv[2], w, h, rows, scale)


if __name__ == "__main__":
    main()
