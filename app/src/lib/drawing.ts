// Drawings: 128x64 frames in SSD1306 page layout (fb[(y >> 3) * 128 + x], bit
// y & 7), the pixel tools that edit them, and their JSON form (base64 frames).
// Contract: docs/ARCHITECTURE.md ("Drawing blob v1").

import type { Drawing } from "./types";

export const W = 128;
export const H = 64;
export const FRAME_BYTES = 1024;

export type Frame = Uint8Array;
export type Point = [number, number];

export const blankFrame = (): Frame => new Uint8Array(FRAME_BYTES);

export function getPx(f: Frame, x: number, y: number): boolean {
  if (x < 0 || x >= W || y < 0 || y >= H) return false;
  return (f[(y >> 3) * W + x] & (1 << (y & 7))) !== 0;
}

export function setPx(f: Frame, x: number, y: number, on: boolean) {
  if (x < 0 || x >= W || y < 0 || y >= H) return;
  const i = (y >> 3) * W + x;
  if (on) f[i] |= 1 << (y & 7);
  else f[i] &= ~(1 << (y & 7));
}

export const isBlank = (f: Frame): boolean => f.every((b) => b === 0);

// ---------------------------------------------------------------- JSON form

export function frameToB64(f: Frame): string {
  let s = "";
  for (let i = 0; i < f.length; i++) s += String.fromCharCode(f[i]);
  return btoa(s);
}

/** Decodes a base64 frame, padding or cutting it to 1024 bytes. */
export function frameFromB64(b64: string): Frame {
  const out = blankFrame();
  try {
    const s = atob(b64);
    for (let i = 0; i < Math.min(s.length, FRAME_BYTES); i++) out[i] = s.charCodeAt(i);
  } catch {
    /* not base64: blank */
  }
  return out;
}

/** The frame shown at tMs, as the firmware picks it. */
export function frameIndexAt(d: Drawing, tMs: number): number {
  return Math.floor(tMs / Math.max(1, d.frameMs)) % Math.max(1, d.frames.length);
}

/** Composites a drawing onto an upright face frame (before invert/rotate180). */
export function composeDrawing(fb: Uint8Array, d: Drawing, tMs: number) {
  if (!d.frames.length) return;
  const f = frameFromB64(d.frames[frameIndexAt(d, tMs)]);
  if (d.mode === "replace") fb.set(f);
  else for (let i = 0; i < FRAME_BYTES; i++) fb[i] |= f[i];
}

// ---------------------------------------------------------------- brushes and shapes

/** Square brush of side `size` centred on (x, y). */
export function stamp(f: Frame, x: number, y: number, size: number, on: boolean) {
  const o = Math.floor((size - 1) / 2);
  for (let dy = 0; dy < size; dy++) for (let dx = 0; dx < size; dx++) setPx(f, x - o + dx, y - o + dy, on);
}

export function linePoints([x0, y0]: Point, [x1, y1]: Point): Point[] {
  const pts: Point[] = [];
  const dx = Math.abs(x1 - x0);
  const dy = -Math.abs(y1 - y0);
  const sx = x0 < x1 ? 1 : -1;
  const sy = y0 < y1 ? 1 : -1;
  let err = dx + dy;
  for (;;) {
    pts.push([x0, y0]);
    if (x0 === x1 && y0 === y1) return pts;
    const e2 = 2 * err;
    if (e2 >= dy) {
      err += dy;
      x0 += sx;
    }
    if (e2 <= dx) {
      err += dx;
      y0 += sy;
    }
  }
}

export function rectPoints([x0, y0]: Point, [x1, y1]: Point, filled: boolean): Point[] {
  const [l, r] = [Math.min(x0, x1), Math.max(x0, x1)];
  const [t, b] = [Math.min(y0, y1), Math.max(y0, y1)];
  const pts: Point[] = [];
  for (let y = t; y <= b; y++)
    for (let x = l; x <= r; x++) if (filled || y === t || y === b || x === l || x === r) pts.push([x, y]);
  return pts;
}

/** Ellipse inscribed in the box from p0 to p1. */
export function ellipsePoints([x0, y0]: Point, [x1, y1]: Point, filled: boolean): Point[] {
  const [l, r] = [Math.min(x0, x1), Math.max(x0, x1)];
  const [t, b] = [Math.min(y0, y1), Math.max(y0, y1)];
  const cx = (l + r) / 2;
  const cy = (t + b) / 2;
  const rx = (r - l) / 2 + 0.5;
  const ry = (b - t) / 2 + 0.5;
  const inside = (x: number, y: number) => ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1;
  const pts: Point[] = [];
  for (let y = t; y <= b; y++)
    for (let x = l; x <= r; x++) {
      if (!inside(x, y)) continue;
      // Outline: a pixel inside with a 4-neighbour outside.
      if (filled || !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1)) pts.push([x, y]);
    }
  return pts;
}

/** Points of a 45-degree-snapped line, or a square box, when Shift is held. */
export function constrain([x0, y0]: Point, [x1, y1]: Point, square: boolean): Point {
  const dx = x1 - x0;
  const dy = y1 - y0;
  if (square) {
    const d = Math.max(Math.abs(dx), Math.abs(dy));
    return [x0 + Math.sign(dx || 1) * d, y0 + Math.sign(dy || 1) * d];
  }
  const a = Math.atan2(dy, dx);
  const snap = Math.round(a / (Math.PI / 4)) * (Math.PI / 4);
  const len = Math.max(Math.abs(dx), Math.abs(dy));
  return [x0 + Math.round(Math.cos(snap)) * len, y0 + Math.round(Math.sin(snap)) * len];
}

export function plot(f: Frame, pts: Point[], size: number, on: boolean) {
  for (const [x, y] of pts) stamp(f, x, y, size, on);
}

/** Flood fill the 4-connected region at (x, y) with its opposite colour. */
export function floodFill(f: Frame, x: number, y: number) {
  if (x < 0 || x >= W || y < 0 || y >= H) return;
  const target = getPx(f, x, y);
  const stack: Point[] = [[x, y]];
  while (stack.length) {
    const [px, py] = stack.pop()!;
    if (px < 0 || px >= W || py < 0 || py >= H || getPx(f, px, py) !== target) continue;
    setPx(f, px, py, !target);
    stack.push([px + 1, py], [px - 1, py], [px, py + 1], [px, py - 1]);
  }
}

// ---------------------------------------------------------------- whole-frame edits

export function mapPixels(src: (x: number, y: number) => boolean): Frame {
  const out = blankFrame();
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) if (src(x, y)) setPx(out, x, y, true);
  return out;
}

export const invertFrame = (f: Frame): Frame => f.map((b) => ~b & 0xff);
export const flipH = (f: Frame): Frame => mapPixels((x, y) => getPx(f, W - 1 - x, y));
export const flipV = (f: Frame): Frame => mapPixels((x, y) => getPx(f, x, H - 1 - y));
/** Moves everything by (dx, dy); pixels pushed off the edge are lost. */
export const shift = (f: Frame, dx: number, dy: number): Frame => mapPixels((x, y) => getPx(f, x - dx, y - dy));

// ---------------------------------------------------------------- text (the firmware's 5x7 font)

const FONT_FIRST = 32;
const FONT_LAST = 90;
// prettier-ignore
const FONT = [
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x5F, 0x00, 0x00, 0x00, 0x07, 0x00, 0x07, 0x00, 0x14, 0x7F, 0x14, 0x7F, 0x14, 0x24, 0x2A, 0x7F, 0x2A, 0x12, 0x23, 0x13, 0x08, 0x64, 0x62, 0x36, 0x49, 0x55, 0x22, 0x50, 0x00, 0x05, 0x03, 0x00, 0x00,
  0x00, 0x1C, 0x22, 0x41, 0x00, 0x00, 0x41, 0x22, 0x1C, 0x00, 0x08, 0x2A, 0x1C, 0x2A, 0x08, 0x08, 0x08, 0x3E, 0x08, 0x08, 0x00, 0x50, 0x30, 0x00, 0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x60, 0x60, 0x00, 0x00, 0x20, 0x10, 0x08, 0x04, 0x02,
  0x3E, 0x51, 0x49, 0x45, 0x3E, 0x00, 0x42, 0x7F, 0x40, 0x00, 0x42, 0x61, 0x51, 0x49, 0x46, 0x21, 0x41, 0x45, 0x4B, 0x31, 0x18, 0x14, 0x12, 0x7F, 0x10, 0x27, 0x45, 0x45, 0x45, 0x39, 0x3C, 0x4A, 0x49, 0x49, 0x30, 0x01, 0x71, 0x09, 0x05, 0x03,
  0x36, 0x49, 0x49, 0x49, 0x36, 0x06, 0x49, 0x49, 0x29, 0x1E, 0x00, 0x36, 0x36, 0x00, 0x00, 0x00, 0x56, 0x36, 0x00, 0x00, 0x08, 0x14, 0x22, 0x41, 0x00, 0x14, 0x14, 0x14, 0x14, 0x14, 0x00, 0x41, 0x22, 0x14, 0x08, 0x02, 0x01, 0x51, 0x09, 0x06,
  0x32, 0x49, 0x79, 0x41, 0x3E, 0x7E, 0x11, 0x11, 0x11, 0x7E, 0x7F, 0x49, 0x49, 0x49, 0x36, 0x3E, 0x41, 0x41, 0x41, 0x22, 0x7F, 0x41, 0x41, 0x22, 0x1C, 0x7F, 0x49, 0x49, 0x49, 0x41, 0x7F, 0x09, 0x09, 0x09, 0x01, 0x3E, 0x41, 0x49, 0x49, 0x7A,
  0x7F, 0x08, 0x08, 0x08, 0x7F, 0x00, 0x41, 0x7F, 0x41, 0x00, 0x20, 0x40, 0x41, 0x3F, 0x01, 0x7F, 0x08, 0x14, 0x22, 0x41, 0x7F, 0x40, 0x40, 0x40, 0x40, 0x7F, 0x02, 0x0C, 0x02, 0x7F, 0x7F, 0x04, 0x08, 0x10, 0x7F, 0x3E, 0x41, 0x41, 0x41, 0x3E,
  0x7F, 0x09, 0x09, 0x09, 0x06, 0x3E, 0x41, 0x51, 0x21, 0x5E, 0x7F, 0x09, 0x19, 0x29, 0x46, 0x46, 0x49, 0x49, 0x49, 0x31, 0x01, 0x01, 0x7F, 0x01, 0x01, 0x3F, 0x40, 0x40, 0x40, 0x3F, 0x1F, 0x20, 0x40, 0x20, 0x1F, 0x3F, 0x40, 0x38, 0x40, 0x3F,
  0x63, 0x14, 0x08, 0x14, 0x63, 0x07, 0x08, 0x70, 0x08, 0x07, 0x61, 0x51, 0x49, 0x45, 0x43,
];

/** Pixels of `text` (uppercased; A-Z, 0-9 and ASCII punctuation) with its top-left at (x, y). */
export function textPoints(text: string, x: number, y: number, scale: number): Point[] {
  const pts: Point[] = [];
  let cx = x;
  for (const raw of text.toUpperCase()) {
    let c = raw.charCodeAt(0);
    if (c < FONT_FIRST || c > FONT_LAST) c = 32;
    const g = (c - FONT_FIRST) * 5;
    for (let col = 0; col < 5; col++) {
      const bits = FONT[g + col];
      for (let row = 0; row < 7; row++) {
        if (!(bits & (1 << row))) continue;
        for (let dy = 0; dy < scale; dy++)
          for (let dx = 0; dx < scale; dx++) pts.push([cx + col * scale + dx, y + row * scale + dy]);
      }
    }
    cx += 6 * scale;
  }
  return pts;
}

export const textWidth = (text: string, scale: number): number => Math.max(0, text.length * 6 - 1) * scale;

// ---------------------------------------------------------------- images

export interface ImportOptions {
  threshold: number; // 0..255
  dither: boolean;
  invert: boolean;
}

/** Grey levels (0..255) of an image fitted inside 128x64, centred on black. */
export function imageLuma(img: CanvasImageSource & { width: number; height: number }): Float32Array {
  const c = document.createElement("canvas");
  c.width = W;
  c.height = H;
  const ctx = c.getContext("2d", { willReadFrequently: true })!;
  ctx.fillStyle = "#000";
  ctx.fillRect(0, 0, W, H);
  const s = Math.min(W / img.width, H / img.height);
  const w = img.width * s;
  const h = img.height * s;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(img, (W - w) / 2, (H - h) / 2, w, h);
  const d = ctx.getImageData(0, 0, W, H).data;
  const luma = new Float32Array(W * H);
  for (let i = 0; i < W * H; i++) {
    const a = d[i * 4 + 3] / 255;
    luma[i] = (0.299 * d[i * 4] + 0.587 * d[i * 4 + 1] + 0.114 * d[i * 4 + 2]) * a;
  }
  return luma;
}

/** One-bit frame from grey levels: a threshold, or Floyd-Steinberg dithering around it. */
export function lumaToFrame(luma: Float32Array, o: ImportOptions): Frame {
  const g = Float32Array.from(luma, (v) => (o.invert ? 255 - v : v));
  const out = blankFrame();
  for (let y = 0; y < H; y++)
    for (let x = 0; x < W; x++) {
      const i = y * W + x;
      const on = g[i] >= o.threshold;
      if (on) setPx(out, x, y, true);
      if (!o.dither) continue;
      const err = g[i] - (on ? 255 : 0);
      if (x + 1 < W) g[i + 1] += (err * 7) / 16;
      if (y + 1 < H) {
        if (x > 0) g[i + W - 1] += (err * 3) / 16;
        g[i + W] += (err * 5) / 16;
        if (x + 1 < W) g[i + W + 1] += err / 16;
      }
    }
  return out;
}
