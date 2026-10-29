// Minimal GIF89a/87a decoder: every frame composited to full-size RGBA, with its
// delay. Plain TS because WebKit (the Linux and macOS webview) has no ImageDecoder.

export interface GifFrame {
  /** Full canvas RGBA after this frame (disposal of earlier frames applied). */
  rgba: Uint8ClampedArray<ArrayBuffer>;
  /** Display time in ms (GIF delays under 20 ms are shown as 100 ms by browsers). */
  delayMs: number;
}

export interface Gif {
  width: number;
  height: number;
  frames: GifFrame[];
}

/** True when the bytes start with a GIF signature. */
export const isGif = (b: Uint8Array): boolean => b.length > 6 && b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46;

const MAX_FRAMES = 1000;

export function decodeGif(b: Uint8Array): Gif {
  let p = 0;
  const need = (n: number) => {
    if (p + n > b.length) throw new Error("GIF is truncated");
  };
  const u8 = () => (need(1), b[p++]);
  const u16 = () => (need(2), (p += 2), b[p - 2] | (b[p - 1] << 8));

  if (!isGif(b)) throw new Error("not a GIF");
  p = 6;
  const width = u16();
  const height = u16();
  if (!width || !height || width * height > 4096 * 4096) throw new Error("GIF has an unsupported size");
  const packed = u8();
  const bgIndex = u8();
  u8(); // aspect
  let gct: Uint8Array | null = null;
  if (packed & 0x80) {
    const n = 3 << ((packed & 7) + 1);
    need(n);
    gct = b.subarray(p, p + n);
    p += n;
  }
  void bgIndex; // background is drawn transparent, as browsers do

  const canvas: Uint8ClampedArray<ArrayBuffer> = new Uint8ClampedArray(width * height * 4);
  const frames: GifFrame[] = [];
  let delay = 0;
  let transparent = -1;
  let disposal = 0;

  const subBlocks = (): Uint8Array => {
    const parts: Uint8Array[] = [];
    let len = 0;
    for (;;) {
      const n = u8();
      if (!n) break;
      need(n);
      parts.push(b.subarray(p, p + n));
      len += n;
      p += n;
    }
    const out = new Uint8Array(len);
    let o = 0;
    for (const part of parts) {
      out.set(part, o);
      o += part.length;
    }
    return out;
  };

  while (p < b.length && frames.length < MAX_FRAMES) {
    const block = u8();
    if (block === 0x3b) break; // trailer
    if (block === 0x21) {
      const label = u8();
      if (label === 0xf9) {
        const data = subBlocks();
        if (data.length >= 4) {
          disposal = (data[0] >> 2) & 7;
          transparent = data[0] & 1 ? data[3] : -1;
          delay = (data[1] | (data[2] << 8)) * 10;
        }
      } else {
        subBlocks();
      }
      continue;
    }
    if (block !== 0x2c) throw new Error("GIF has an unknown block");

    const fx = u16();
    const fy = u16();
    const fw = u16();
    const fh = u16();
    const fpacked = u8();
    let ct = gct;
    if (fpacked & 0x80) {
      const n = 3 << ((fpacked & 7) + 1);
      need(n);
      ct = b.subarray(p, p + n);
      p += n;
    }
    const interlaced = (fpacked & 0x40) !== 0;
    const minCode = u8();
    const pixels = lzw(subBlocks(), minCode, fw * fh);
    if (!ct) throw new Error("GIF has no colour table");

    const before = disposal === 3 ? canvas.slice() : null;
    const rows = interlaced ? interlaceRows(fh) : null;
    for (let i = 0; i < fh; i++) {
      const y = fy + (rows ? rows[i] : i);
      if (y >= height) continue;
      for (let x = 0; x < fw; x++) {
        const idx = pixels[i * fw + x];
        if (idx === transparent || fx + x >= width) continue;
        const o = (y * width + fx + x) * 4;
        canvas[o] = ct[idx * 3] ?? 0;
        canvas[o + 1] = ct[idx * 3 + 1] ?? 0;
        canvas[o + 2] = ct[idx * 3 + 2] ?? 0;
        canvas[o + 3] = 255;
      }
    }
    frames.push({ rgba: canvas.slice(), delayMs: delay < 20 ? 100 : delay });

    // Disposal applies before the next frame is drawn.
    if (disposal === 2) {
      for (let y = fy; y < Math.min(height, fy + fh); y++) canvas.fill(0, (y * width + fx) * 4, (y * width + Math.min(width, fx + fw)) * 4);
    } else if (disposal === 3 && before) {
      canvas.set(before);
    }
    delay = 0;
    transparent = -1;
    disposal = 0;
  }
  if (!frames.length) throw new Error("GIF has no frames");
  return { width, height, frames };
}

/** Row order of an interlaced image: passes start 0, 4, 2, 1 with steps 8, 8, 4, 2. */
function interlaceRows(h: number): number[] {
  const rows: number[] = [];
  for (const [start, step] of [
    [0, 8],
    [4, 8],
    [2, 4],
    [1, 2],
  ])
    for (let y = start; y < h; y += step) rows.push(y);
  return rows;
}

function lzw(data: Uint8Array, minCode: number, count: number): Uint8Array {
  const out = new Uint8Array(count);
  if (minCode < 2 || minCode > 11) throw new Error("GIF has a bad LZW code size");
  const clear = 1 << minCode;
  const eoi = clear + 1;
  const prefix = new Int16Array(4096);
  const suffix = new Uint8Array(4096);
  const stack = new Uint8Array(4097);
  let size = minCode + 1;
  let next = eoi + 1;
  let old = -1;
  let first = 0;
  let bits = 0;
  let acc = 0;
  let o = 0;
  for (let i = 0; i < clear; i++) suffix[i] = i;

  for (let i = 0; i < data.length && o < count; ) {
    while (bits < size && i < data.length) {
      acc |= data[i++] << bits;
      bits += 8;
    }
    if (bits < size) break;
    const code = acc & ((1 << size) - 1);
    acc >>>= size;
    bits -= size;

    if (code === clear) {
      size = minCode + 1;
      next = eoi + 1;
      old = -1;
      continue;
    }
    if (code === eoi) break;
    if (old === -1) {
      out[o++] = suffix[code];
      old = first = code;
      continue;
    }
    let sp = 0;
    let c = code;
    if (code >= next) {
      stack[sp++] = first;
      c = old;
    }
    while (c >= clear) {
      if (sp >= 4096) throw new Error("GIF data is corrupt");
      stack[sp++] = suffix[c];
      c = prefix[c];
    }
    first = suffix[c];
    stack[sp++] = first;
    if (next < 4096) {
      prefix[next] = old;
      suffix[next] = first;
      next++;
      if (next === 1 << size && size < 12) size++;
    }
    old = code;
    while (sp && o < count) out[o++] = stack[--sp];
  }
  return out;
}

/** Which of `n` frames to keep when at most `max` fit: evenly spread, first frame included. */
export function sampleFrames(n: number, max: number): number[] {
  if (n <= max) return [...Array(n).keys()];
  return [...Array(max).keys()].map((i) => Math.floor((i * n) / max));
}
