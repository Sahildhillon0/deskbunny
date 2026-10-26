// Browser-only stand-in for the Tauri backend, used when the page is not running
// inside Tauri (`npm run dev` in a normal browser). The face drawing here is a
// rough JS approximation for layout work; the real app always renders with the
// C engine over IPC. Never import this directly: go through ./api.ts.

import { defaultFace, normalizeFace, sanitizeName, type Board, type FaceDef, type FlashProgress, type FlashReport, type Mood, type Port } from "./types";

const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// ---------------------------------------------------------------- presets

const PRESET_SPECS: [string, Mood, Partial<FaceDef>][] = [
  ["WATCHING", "neutral", {}],
  ["HAPPY", "happy", { mouth: true }],
  ["GRUMPY", "angry", { overlay: "anger", lookX: 10 }],
  ["BLUE", "sad", { overlay: "tears", lookAround: false }],
  ["SLEEPY", "sleepy", { overlay: "zzz", lookAround: false, speed: 3 }],
  ["WOW", "surprised", { eyeW: 32, eyeH: 40, eyeR: 16 }],
  ["SMITTEN", "love", { overlay: "hearts" }],
  ["SUS", "suspicious", { lookX: 22, lookY: 2 }],
  ["HYPED", "excited", { overlay: "sparkles", speed: 8 }],
  ["MEH", "bored", { speed: 3, lookX: 12 }],
  ["SPOOKED", "scared", { overlay: "sweat", speed: 8 }],
  ["DIZZY", "dizzy", { overlay: "question" }],
];

export async function listPresets(): Promise<FaceDef[]> {
  return PRESET_SPECS.map(([name, mood, extra]) => ({ ...defaultFace(), name, mood, ...extra }));
}

// ---------------------------------------------------------------- boards / ports

const BOARDS: Board[] = [
  { id: "uno", name: "Uno R3", mcu: "atmega328p", protocol: "stk500v1" },
  { id: "nano", name: "Nano (new bootloader)", mcu: "atmega328p", protocol: "stk500v1" },
  { id: "nano_old", name: "Nano (old bootloader)", mcu: "atmega328p", protocol: "stk500v1" },
  { id: "pro_mini", name: "Pro Mini 5V", mcu: "atmega328p", protocol: "stk500v1" },
  { id: "mega", name: "Mega 2560", mcu: "atmega2560", protocol: "stk500v2" },
  { id: "leonardo", name: "Leonardo", mcu: "atmega32u4", protocol: "avr109" },
  { id: "micro", name: "Micro", mcu: "atmega32u4", protocol: "avr109" },
  { id: "uno_r4_minima", name: "Uno R4 Minima", mcu: "RA4M1", protocol: "dfu" },
  { id: "uno_r4_wifi", name: "Uno R4 WiFi", mcu: "RA4M1", protocol: "samba" },
];

export async function listBoards(): Promise<Board[]> {
  return BOARDS;
}

// Add `?noports` to the dev URL to see the empty state.
export async function listPorts(): Promise<Port[]> {
  await delay(60);
  if (location.search.includes("noports")) return [];
  return [
    { path: "/dev/ttyUSB0", vid: "1A86", pid: "7523", product: "USB Serial (CH340)", guessedBoard: "nano" },
    { path: "/dev/ttyACM0", vid: "2341", pid: "0043", product: "Arduino Uno", serialNumber: "85736323838351F0E1C1", guessedBoard: "uno" },
  ];
}

// ---------------------------------------------------------------- flash

type ProgressCb = (p: FlashProgress) => void;
const progressListeners = new Set<ProgressCb>();

export async function onFlashProgress(cb: ProgressCb): Promise<() => void> {
  progressListeners.add(cb);
  return () => progressListeners.delete(cb);
}

const emit = (p: FlashProgress) => progressListeners.forEach((cb) => cb(p));

// Firmware image stem per board (the serial banner reports this, not the board id).
const IMAGE_STEM: Record<string, string> = {
  uno: "atmega328p",
  nano: "atmega328p",
  nano_old: "atmega328p",
  pro_mini: "atmega328p",
  mega: "atmega2560",
  leonardo: "atmega32u4",
  micro: "atmega32u4",
  uno_r4_minima: "ra4m1_minima",
  uno_r4_wifi: "ra4m1_wifi",
};

let flashing = false;

// Add `?flasherror` to the dev URL to see the error panel, `?nobanner` for a
// flash that succeeds without the DESKBUNNY reply.
export async function flash(port: string, boardId: string, face: FaceDef): Promise<FlashReport> {
  if (flashing) throw "Another upload is already running.";
  flashing = true;
  try {
    const started = performance.now();
    emit({ stage: "reset", percent: 0, message: `Resetting board on ${port}` });
    await delay(400);
    emit({ stage: "sync", percent: 4, message: "Syncing with bootloader" });
    await delay(500);
    if (location.search.includes("flasherror")) {
      throw `No response from bootloader on ${port}. Is this an old-bootloader Nano? Try 'Nano (old bootloader)'.`;
    }
    emit({ stage: "erase", percent: 8, message: "Erasing" });
    await delay(300);
    for (let p = 10; p <= 80; p += 5) {
      emit({ stage: "write", percent: p, message: `Writing ${Math.round(((p - 10) / 70) * 9216)} / 9216 bytes` });
      await delay(110);
    }
    for (let p = 82; p <= 98; p += 4) {
      emit({ stage: "verify", percent: p, message: "Verifying" });
      await delay(90);
    }
    emit({ stage: "done", percent: 100, message: "Done" });
    const name = sanitizeName(face.name);
    const banner = !location.search.includes("nobanner");
    const deviceInfo = banner ? `DESKBUNNY 0.1.0 ${IMAGE_STEM[boardId] ?? boardId} ${name}` : null;
    return {
      protocol: BOARDS.find((b) => b.id === boardId)?.protocol ?? "stk500v1",
      port,
      baud: 115200,
      signature: "1e950f",
      bytesWritten: 9216,
      verified: true,
      durationMs: Math.round(performance.now() - started),
      warnings: banner ? [] : [`Flashed, but no DESKBUNNY reply from ${port}. If the face is showing, you're set.`],
      boardId,
      faceName: name,
      fwVersion: "0.1.0",
      blobAddress: 0x1f40,
      deviceInfo,
      deviceVerified: !!deviceInfo && deviceInfo.endsWith(name),
    };
  } finally {
    flashing = false;
  }
}

export async function deviceInfo(port: string): Promise<string | null> {
  await delay(200);
  return port ? "DESKBUNNY 0.1.0 atmega328p WATCHING" : null;
}

// ---------------------------------------------------------------- saved faces

const STORE_KEY = "deskbunny.mock.saved";

function readSaved(): FaceDef[] {
  try {
    const raw = JSON.parse(localStorage.getItem(STORE_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.map(normalizeFace) : [];
  } catch {
    return [];
  }
}

function writeSaved(faces: FaceDef[]) {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(faces));
  } catch {
    // storage unavailable (private window): saved faces just won't persist
  }
}

export async function listSavedFaces(): Promise<FaceDef[]> {
  return readSaved();
}

export async function saveFace(face: FaceDef): Promise<void> {
  const name = face.name.trim();
  if (!name || name !== sanitizeName(name)) throw "Face names use A-Z, 0-9 and spaces, up to 12 characters.";
  const rest = readSaved().filter((f) => f.name !== name);
  writeSaved([...rest, { ...face, name }].sort((a, b) => a.name.localeCompare(b.name)));
}

export async function deleteFace(name: string): Promise<void> {
  writeSaved(readSaved().filter((f) => f.name !== name));
}

export async function encodeFace(face: FaceDef): Promise<number[]> {
  // Debug only: magic + version + mood, rest zero. The real codec lives in Rust.
  const out = new Array<number>(64).fill(0);
  [..."DBFACE01"].forEach((c, i) => (out[i] = c.charCodeAt(0)));
  out[8] = 1;
  out[9] = Math.max(0, ["neutral", "happy", "angry", "sad", "sleepy", "surprised", "love", "suspicious", "excited", "bored", "scared", "dizzy"].indexOf(face.mood));
  return out;
}

// ---------------------------------------------------------------- simulator

interface Sim {
  face: FaceDef;
  seed: number;
}

const sims = new Map<number, Sim>();
let nextSimId = 1;

export async function simCreate(face: FaceDef, seed: number): Promise<number> {
  const id = nextSimId++;
  sims.set(id, { face: { ...face }, seed });
  return id;
}

export async function simSetFace(id: number, face: FaceDef): Promise<void> {
  const sim = sims.get(id);
  if (sim) sim.face = { ...face };
}

export async function simDrop(id: number): Promise<void> {
  sims.delete(id);
}

export async function simFrame(id: number, tMs: number): Promise<ArrayBuffer> {
  const sim = sims.get(id);
  const fb = new Uint8Array(1024);
  if (sim) renderMock(sim, tMs, fb);
  return fb.buffer;
}

const W = 128;
const H = 64;

/** Small integer hash -> [0, 1). */
function rand(n: number): number {
  let x = (n | 0) ^ 0x9e3779b9;
  x = Math.imul(x ^ (x >>> 16), 0x85ebca6b);
  x = Math.imul(x ^ (x >>> 13), 0xc2b2ae35);
  x ^= x >>> 16;
  return (x >>> 0) / 4294967296;
}

class Canvas1 {
  constructor(public fb: Uint8Array) {}
  set(x: number, y: number, on = true) {
    x |= 0;
    y |= 0;
    if (x < 0 || y < 0 || x >= W || y >= H) return;
    const i = (y >> 3) * W + x;
    if (on) this.fb[i] |= 1 << (y & 7);
    else this.fb[i] &= ~(1 << (y & 7));
  }
  roundRect(x: number, y: number, w: number, h: number, r: number, on = true) {
    r = Math.max(0, Math.min(r, w / 2, h / 2));
    for (let j = 0; j < h; j++) {
      for (let i = 0; i < w; i++) {
        const dx = i < r ? r - i - 0.5 : i >= w - r ? i - (w - r) + 0.5 : 0;
        const dy = j < r ? r - j - 0.5 : j >= h - r ? j - (h - r) + 0.5 : 0;
        if (dx * dx + dy * dy <= r * r) this.set(x + i, y + j, on);
      }
    }
  }
  /** Clears every pixel of the rect where pred(x, y) holds. */
  carve(x: number, y: number, w: number, h: number, pred: (px: number, py: number) => boolean) {
    for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) if (pred(x + i, y + j)) this.set(x + i, y + j, false);
  }
  ring(cx: number, cy: number, r: number) {
    for (let a = 0; a < 64; a++) {
      const t = (a / 64) * Math.PI * 2;
      this.set(Math.round(cx + Math.cos(t) * r), Math.round(cy + Math.sin(t) * r));
    }
  }
  sprite(x: number, y: number, rows: string[]) {
    rows.forEach((row, j) => [...row].forEach((c, i) => c === "#" && this.set(x + i, y + j)));
  }
}

const HEART = [".##.##.", "#######", "#######", ".#####.", "..###..", "...#..."];
const SMALL_HEART = ["##.##", "#####", ".###.", "..#.."];
const Z = ["####", "..#.", ".#..", "####"];
const DROP = ["..#..", ".###.", "#####", "#####", ".###."];
const ANGER = ["#.#", "###", "#.#"];
const SPARK = ["..#..", "..#..", "#####", "..#..", "..#.."];
const QUESTION = [".###.", "#...#", "...#.", "..#..", ".....", "..#.."];

function renderMock(sim: Sim, tMs: number, fb: Uint8Array) {
  const f = sim.face;
  const c = new Canvas1(fb);
  const t = (tMs * f.speed) / 5;
  const salt = sim.seed * 7919 + f.seed;

  // Glance: hold a random target per segment, ease into it.
  let gx = 0;
  let gy = 0;
  if (f.lookAround) {
    const seg = Math.max(300, ((f.lookMin + f.lookMax) / 2) * 100);
    const k = Math.floor(t / seg);
    const tx = (k: number) => (rand(k * 2 + salt) * 2 - 1) * f.lookX;
    const ty = (k: number) => (rand(k * 2 + 1 + salt) * 2 - 1) * f.lookY;
    const e = Math.min(1, (t - k * seg) / 220);
    const s = e * e * (3 - 2 * e);
    gx = tx(k - 1) + (tx(k) - tx(k - 1)) * s;
    gy = ty(k - 1) + (ty(k) - ty(k - 1)) * s;
  }

  // Blink: one 160 ms close/open per period.
  let lid = 0;
  if (f.blink && f.mood !== "dizzy") {
    const period = Math.max(400, ((f.blinkMin + f.blinkMax) / 2) * 100);
    const k = Math.floor(t / period);
    const at = (t - k * period) - rand(k + salt) * (period - 200);
    if (at >= 0 && at < 160) lid = 1 - Math.abs(at - 80) / 80;
  }

  const breathe = f.breathe ? Math.round(Math.sin(t / 900) * 1.5) : 0;
  let w = f.eyeW;
  let h = f.eyeH + breathe;
  let r = f.eyeR;
  let bob = 0;
  let jitter = 0;
  switch (f.mood) {
    case "surprised":
      h = Math.round(h * 1.12);
      r = Math.max(r, Math.floor(w / 2) - 2);
      break;
    case "scared":
      w = Math.round(w * 0.7);
      h = Math.round(h * 0.8);
      jitter = Math.round(Math.sin(t / 40) * 1);
      break;
    case "excited":
      bob = Math.round(Math.abs(Math.sin(t / 180)) * -4);
      break;
    case "dizzy":
      gx = gy = 0;
      break;
  }
  const n = f.cyclops ? 1 : 2;
  const total = n * w + (n - 1) * f.eyeGap;
  const x0 = Math.round((W - total) / 2 + gx) + jitter;
  const y0 = Math.round((H - h) / 2 + f.eyeY + gy + bob - (f.mouth ? 5 : 0));
  const visH = Math.max(2, Math.round(h * (1 - lid)));

  for (let i = 0; i < n; i++) {
    const ex = x0 + i * (w + f.eyeGap);
    const ey = y0 + Math.round((h - visH) / 2);
    const left = i === 0 && n === 2;
    const cx = ex + w / 2;
    const cy = y0 + h / 2;

    if (f.mood === "love") {
      const s = Math.max(1, Math.floor(Math.min(w / 7, h / 6)));
      HEART.forEach((row, j) => [...row].forEach((ch, k) => {
        if (ch === "#") for (let a = 0; a < s; a++) for (let b = 0; b < s; b++) c.set(cx - 3.5 * s + k * s + a, cy - 3 * s + j * s + b);
      }));
      continue;
    }
    if (f.mood === "dizzy") {
      const spin = t / 300;
      for (let rr = 3; rr < Math.min(w, h) / 2; rr += 4) c.ring(cx + Math.cos(spin + rr) * 1.5, cy + Math.sin(spin + rr) * 1.5, rr);
      continue;
    }

    c.roundRect(ex, ey, w, visH, r);
    const top = ey;
    const bottom = ey + visH;
    switch (f.mood) {
      case "happy":
      case "excited":
        c.carve(ex, ey, w, visH, (px, py) => {
          const dx = (px - cx) / (w * 0.75);
          const dy = (py - (bottom + h * 0.15)) / (h * 0.55);
          return dx * dx + dy * dy < 1;
        });
        break;
      case "angry":
        c.carve(ex, ey, w, visH, (px, py) => {
          const u = (px - ex) / w;
          const slope = left ? u : 1 - u;
          return py < top + slope * h * 0.45;
        });
        break;
      case "sad":
        c.carve(ex, ey, w, visH, (px, py) => {
          const u = (px - ex) / w;
          const slope = left ? 1 - u : u;
          return py < top + slope * h * 0.4;
        });
        break;
      case "sleepy":
        c.carve(ex, ey, w, visH, (_px, py) => py < top + h * 0.55 + Math.sin(t / 1400) * 2);
        break;
      case "bored":
        c.carve(ex, ey, w, visH, (_px, py) => py < top + h * 0.45);
        break;
      case "suspicious":
        if (left) c.carve(ex, ey, w, visH, (_px, py) => py < top + h * 0.35 || py > bottom - h * 0.2);
        else c.carve(ex, ey, w, visH, (_px, py) => py < top + h * 0.15);
        break;
    }
  }

  if (f.mouth) {
    const my = y0 + h + 5;
    const happyish = f.mood === "happy" || f.mood === "excited" || f.mood === "love";
    const sadish = f.mood === "sad" || f.mood === "scared" || f.mood === "angry";
    for (let i = -6; i <= 6; i++) {
      const curve = Math.round((i * i) / 12);
      c.set(W / 2 + i + gx * 0.5, my + (happyish ? -curve + 3 : sadish ? curve : 1));
    }
  }

  const fl = Math.floor(t / 400);
  switch (f.overlay) {
    case "hearts":
      c.sprite(6, 6 + (fl % 3), SMALL_HEART);
      c.sprite(114, 10 + ((fl + 1) % 3), SMALL_HEART);
      break;
    case "zzz":
      c.sprite(100, 14 - (fl % 4), Z);
      c.sprite(108, 8 - ((fl + 2) % 4), Z);
      c.sprite(116, 2 + (fl % 2), Z);
      break;
    case "sweat":
      c.sprite(110, 8 + (fl % 6), DROP);
      break;
    case "anger":
      if (fl % 2 === 0) c.sprite(112, 4, ANGER);
      break;
    case "tears":
      c.sprite(x0 + 4, y0 + h + 2 + (fl % 5), DROP);
      break;
    case "sparkles":
      c.sprite(4 + (fl % 2) * 2, 6, SPARK);
      c.sprite(118, 44 + (fl % 2), SPARK);
      break;
    case "question":
      c.sprite(116, 4, QUESTION);
      break;
  }

  if (f.invert) for (let i = 0; i < fb.length; i++) fb[i] ^= 0xff;
  if (f.rotate180) {
    const src = fb.slice();
    fb.fill(0);
    for (let y = 0; y < H; y++)
      for (let x = 0; x < W; x++)
        if (src[(y >> 3) * W + x] & (1 << (y & 7))) {
          const ry = H - 1 - y;
          fb[(ry >> 3) * W + (W - 1 - x)] |= 1 << (ry & 7);
        }
  }
}
