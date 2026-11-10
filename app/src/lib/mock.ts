// Browser-only stand-in for the Tauri backend, used when the page is not running
// inside Tauri (`npm run dev` in a normal browser). The face drawing here is a
// rough JS approximation for layout work; the real app always renders with the
// C engine over IPC. Never import this directly: go through ./api.ts.

import { composeDrawing } from "./drawing";
import { MOODS, defaultFace, normalizeFace, sanitizeName, type Board, type FaceDef, type FlashProgress, type FlashReport, type ClaudeView, type Design, type DesignCatalog, type DesignRender, type HooksStatus, type LiveConfig, type LiveStatus, type Mood, type Place, type Port, type ScreenRef, type WeatherNow, type SetupStatus, type UpdateInfo, type UpdateProgress } from "./types";

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
  ["WINKY", "wink", { mouth: true }],
  ["LOL", "laugh", { mouth: true, overlay: "sparkles", speed: 7 }],
  ["WEEPY", "crying", { overlay: "tears", lookAround: false }],
  ["REALLY", "skeptical", { overlay: "question" }],
  ["SMUG", "smug", { mouth: true, speed: 4 }],
  ["VILLAIN", "evil", { overlay: "anger" }],
  ["NERVOUS", "worried", { overlay: "sweat", speed: 7 }],
  ["BASHFUL", "shy", { overlay: "hearts", lookAround: false }],
  ["SQUINT", "squint", { speed: 4 }],
  ["KO", "dead", { mouth: true, lookAround: false }],
  ["ASLEEP", "asleep", { overlay: "zzz", speed: 3 }],
  ["STARSTRUCK", "starry", { overlay: "sparkles" }],
  ["HYPNO", "hypno", { lookAround: false }],
  ["GLITCH", "glitch", { speed: 6 }],
  ["SCANNER", "scan", { lookAround: false, speed: 4 }],
  ["EYE ROLL", "eyeroll", { lookAround: false, speed: 4 }],
  ["HUH", "confused", { overlay: "question" }],
  ["FOCUSED", "determined", { speed: 6 }],
  ["MOODY", "neutral", { mouth: true, cycle: "random", cycleSeconds: 8 }],
  ["MY DAY", "neutral", { mouth: true, cycle: "sequence", cycleSeconds: 12, cycleMoods: ["neutral", "happy", "excited", "bored", "sleepy"] }],
];

export async function listPresets(): Promise<FaceDef[]> {
  return PRESET_SPECS.map(([name, mood, extra]) => ({ ...defaultFace(), name, mood, ...extra }));
}

// ---------------------------------------------------------------- boards / ports

const BOARDS: Board[] = [
  { id: "uno", name: "Uno R3", mcu: "atmega328p", protocol: "stk500v1", artCapacity: 5632 },
  { id: "nano", name: "Nano (new bootloader)", mcu: "atmega328p", protocol: "stk500v1", artCapacity: 5632 },
  { id: "nano_old", name: "Nano (old bootloader)", mcu: "atmega328p", protocol: "stk500v1", artCapacity: 5632 },
  { id: "pro_mini", name: "Pro Mini 5V", mcu: "atmega328p", protocol: "stk500v1", artCapacity: 5632 },
  { id: "mega", name: "Mega 2560", mcu: "atmega2560", protocol: "stk500v2", artCapacity: 32000 },
  { id: "leonardo", name: "Leonardo", mcu: "atmega32u4", protocol: "avr109", artCapacity: 3200 },
  { id: "micro", name: "Micro", mcu: "atmega32u4", protocol: "avr109", artCapacity: 3200 },
  { id: "uno_r4_minima", name: "Uno R4 Minima", mcu: "RA4M1", protocol: "dfu", artCapacity: 61440 },
  { id: "uno_r4_wifi", name: "Uno R4 WiFi", mcu: "RA4M1", protocol: "samba", artCapacity: 61440 },
  { id: "esp32", name: "ESP32", mcu: "esp32", protocol: "esptool" },
  { id: "esp32c3", name: "ESP32-C3", mcu: "esp32c3", protocol: "esptool" },
  { id: "esp32s3", name: "ESP32-S3", mcu: "esp32s3", protocol: "esptool" },
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
    // VID:PID guesses C3; detectChip() finds the S3 on it.
    { path: "/dev/ttyACM1", vid: "303A", pid: "1001", product: "USB JTAG/serial debug unit", guessedBoard: "esp32c3" },
  ];
}

/** Mock ROM-bootloader probe: the 303A:1001 port holds an ESP32-S3. */
export async function detectChip(port: string): Promise<string | null> {
  await delay(700);
  return port === "/dev/ttyACM1" ? "esp32s3" : null;
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
  esp32: "esp32",
  esp32c3: "esp32c3",
  esp32s3: "esp32s3",
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
    if (BOARDS.find((b) => b.id === boardId)?.protocol === "esptool") return espReport(port, boardId, started);
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

// ESP32 boards: the face is ignored and the merged image is written at 0x0.
function espReport(port: string, boardId: string, started: number): FlashReport {
  const flashed = port === "/dev/ttyACM1" ? "esp32s3" : boardId;
  const name = BOARDS.find((b) => b.id === flashed)?.name ?? flashed;
  const banner = !location.search.includes("nobanner");
  const deviceInfo = banner ? `DESKBUNNY 0.2.0 ${flashed} NEUTRAL` : null;
  const warnings: string[] = [];
  if (flashed !== boardId) {
    const selected = BOARDS.find((b) => b.id === boardId)?.name ?? boardId;
    warnings.push(`Selected ${selected} but found ${name}; flashed the ${name.replace("ESP32-", "")} image.`);
  }
  if (!banner) warnings.push("The board did not report a nixxa banner after flashing. The upload was verified; if the screen stays dark, check the OLED wiring (SDA/SCL).");
  return {
    protocol: "esptool",
    port,
    baud: 460800,
    signature: `${name} 34:85:18:AA:BB:CC`,
    bytesWritten: 1_179_648,
    verified: true,
    durationMs: Math.round(performance.now() - started),
    warnings,
    boardId: flashed,
    faceName: "",
    fwVersion: "0.2.0",
    blobAddress: 0,
    deviceInfo,
    deviceVerified: !!deviceInfo,
  };
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
  out[9] = Math.max(0, (MOODS as readonly string[]).indexOf(face.mood));
  return out;
}

// ---------------------------------------------------------------- import / export
// The real app uses native dialogs in Rust; the browser falls back to a download
// link and a file picker.

export async function exportFace(face: FaceDef): Promise<string | null> {
  const file = `${sanitizeName(face.name).trim() || "FACE"}.deskbunny.json`;
  const url = URL.createObjectURL(new Blob([JSON.stringify(face, null, 2) + "\n"], { type: "application/json" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = file;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return file;
}

export function importFace(): Promise<FaceDef | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,application/json";
    input.addEventListener("cancel", () => resolve(null));
    input.addEventListener("change", async () => {
      const file = input.files?.[0];
      if (!file) return resolve(null);
      try {
        resolve(normalizeFace(JSON.parse(await file.text())));
      } catch {
        reject(`${file.name} isn't a DeskBunny face (expected JSON).`);
      }
    });
    input.click();
  });
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

const MOOD_OVERLAY: Partial<Record<Mood, FaceDef["overlay"]>> = {
  angry: "anger",
  sad: "tears",
  sleepy: "zzz",
  love: "hearts",
  suspicious: "question",
  excited: "sparkles",
  scared: "sweat",
};

/** The face with the mood cycle applied at tMs (no closed-eye transition in the mock). */
function cycledFace(sim: Sim, tMs: number): FaceDef {
  const f = sim.face;
  if (f.cycle === "off") return f;
  const list: readonly Mood[] = f.cycleMoods.length ? f.cycleMoods : MOODS;
  const k = Math.floor(tMs / (Math.max(1, f.cycleSeconds) * 1000));
  const mood =
    f.cycle === "sequence" ? list[k % list.length] : k === 0 ? f.mood : list[Math.floor(rand(k + sim.seed * 31) * list.length)];
  return { ...f, mood, overlay: f.cycleOverlay ? (MOOD_OVERLAY[mood] ?? "none") : f.overlay };
}

function renderMock(sim: Sim, tMs: number, fb: Uint8Array) {
  const f = cycledFace(sim, tMs);
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

  if (f.drawing) composeDrawing(fb, f.drawing, tMs);
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

// ---------------------------------------------------------------- setup guide
// `?setup` forces the guide open; `?setupdone` makes the USB check pass.

const SETUP_KEY = "deskbunny.mock.setupDone";
let mockUdevInstalled = new URLSearchParams(location.search).has("setupdone");

function storedDone(): boolean {
  try {
    return localStorage.getItem(SETUP_KEY) === "1";
  } catch {
    return false;
  }
}

export async function setupStatus(): Promise<SetupStatus> {
  await delay(250);
  const forced = new URLSearchParams(location.search).has("setup");
  return {
    os: "linux",
    osLabel: "Arch Linux (browser preview)",
    firstRun: forced || !storedDone(),
    checks: [
      mockUdevInstalled
        ? {
            id: "usb_access",
            title: "USB access",
            detail: "A udev rule lets you reach Arduino boards without root.",
            state: "ok",
          }
        : {
            id: "usb_access",
            title: "USB access",
            detail:
              "Linux only lets root talk to USB serial ports and bootloaders by default. DeskBunny installs a small udev rule so your user can, with no group changes and no logging out. You will be asked for your password.",
            state: "action",
            action: { id: "install_udev", label: "Allow USB access" },
            commands: [
              "sudo tee /etc/udev/rules.d/70-deskbunny.rules >/dev/null <<'EOF'\n...\nEOF",
              "sudo udevadm control --reload-rules && sudo udevadm trigger",
            ],
          },
      {
        id: "firmware",
        title: "Firmware",
        detail: "DeskBunny firmware 0.1.0 is built in for 9 boards.",
        state: "ok",
      },
    ],
  };
}

export async function setupRun(action: string): Promise<string> {
  await delay(900);
  if (action !== "install_udev") throw `Unknown setup action '${action}'.`;
  mockUdevInstalled = true;
  return "USB access allowed. If your board is plugged in, unplug it and plug it back in.";
}

export async function setupFinish(): Promise<void> {
  try {
    localStorage.setItem(SETUP_KEY, "1");
  } catch {
    /* private mode: the guide just shows again next time */
  }
}

export async function setupOpenLink(id: string): Promise<void> {
  console.info("[mock] open link", id);
}

// ---------------------------------------------------------------- updates

// Add `?update` to the dev URL to see the update dialog.
export async function appVersion(): Promise<string> {
  return "0.1.0";
}

export async function updateCheck(): Promise<UpdateInfo | null> {
  await delay(500);
  if (!location.search.includes("update")) return null;
  return { version: "0.1.1", currentVersion: "0.1.0", date: "2026-10-27", notes: "Mood cycle animation and in-app updates." };
}

const updateListeners = new Set<(p: UpdateProgress) => void>();

export async function onUpdateProgress(cb: (p: UpdateProgress) => void): Promise<() => void> {
  updateListeners.add(cb);
  return () => updateListeners.delete(cb);
}

export async function updateInstall(): Promise<void> {
  const total = 8_000_000;
  for (let d = 0; d <= total; d += 400_000) {
    await delay(60);
    updateListeners.forEach((cb) => cb({ downloaded: d, total }));
  }
  throw "The browser preview cannot install updates.";
}

export async function updateOpenReleases(): Promise<void> {
  window.open("https://github.com/Sahildhillon0/deskbunny/releases/latest", "_blank");
}

// ---------------------------------------------------------------- live mode

let liveCfg: LiveConfig = {
  enabled: false,
  port: null,
  screens: [
    { kind: "face", enabled: true, seconds: 20 },
    { kind: "design", design: "clock", enabled: true, seconds: 15 },
    { kind: "claude", enabled: true, seconds: 15 },
  ],
  claudeStyle: "clawd",
  claudeHold: true,
  weather: { place: null, units: "metric", aqi: "us" },
  notify: { enabled: true, desktop: true, seconds: 8, showBody: false, ignoreApps: [] },
};
let liveStat: LiveStatus = { state: "off", port: null, banner: null, clawd: false, screen: null, pinned: null, message: null };
const liveListeners = new Set<(s: LiveStatus) => void>();

function liveEmit(patch: Partial<LiveStatus>) {
  liveStat = { ...liveStat, ...patch };
  for (const cb of liveListeners) cb(liveStat);
}

export async function liveGetConfig(): Promise<LiveConfig> {
  return structuredClone(liveCfg);
}

export async function liveSetConfig(config: LiveConfig): Promise<LiveConfig> {
  liveCfg = structuredClone(config);
  const on = config.enabled && !!config.port;
  liveEmit(
    on
      ? { state: "connected", port: config.port, banner: "DESKBUNNY 0.1.14 atmega328p MOCK", clawd: true, screen: { kind: "face" } }
      : { state: "off", port: null, banner: null, screen: null },
  );
  return structuredClone(liveCfg);
}

export async function liveStatus(): Promise<LiveStatus> {
  return { ...liveStat };
}

export async function livePin(screen: ScreenRef | null): Promise<void> {
  liveEmit({ pinned: screen, screen: screen ?? liveStat.screen });
}

/** Mock preview: a frame with the screen's name in big blocks (no font here). */
export async function livePreview(screen: ScreenRef, tMs: number): Promise<ArrayBuffer> {
  const fb = new Uint8Array(1024);
  const set = (x: number, y: number) => {
    if (x >= 0 && x < 128 && y >= 0 && y < 64) fb[(y >> 3) * 128 + x] |= 1 << (y & 7);
  };
  if (screen.kind === "design") {
    const s = new Date().getSeconds();
    for (let x = 4; x < 4 + Math.round((120 * s) / 59); x++) set(x, 40);
  } else {
    const bob = Math.floor(tMs / 700) % 2;
    for (let x = 42; x < 86; x++) for (let y = 25 + bob; y < 49 + bob; y++) set(x, y);
  }
  return fb.buffer;
}

export async function onLiveStatus(cb: (s: LiveStatus) => void): Promise<() => void> {
  liveListeners.add(cb);
  return () => liveListeners.delete(cb);
}

export async function liveTestNotification(): Promise<void> {
  liveEmit({ screen: { kind: "notify" } });
}

export async function claudeView(): Promise<ClaudeView> {
  return { state: "idle", caption: "NO SESSIONS", project: "", sessions: 0, takeover: false, active: false };
}

let mockHooks = false;
const hooksStatus = (): HooksStatus => ({
  installed: mockHooks,
  settingsPath: "~/.claude/settings.json",
  command: "curl -s -m 1 -H 'X-DeskBunny: 1' ... http://127.0.0.1:47807/claude || true",
});

export async function claudeHooksStatus(): Promise<HooksStatus> {
  return hooksStatus();
}

export async function claudeHooksInstall(): Promise<HooksStatus> {
  mockHooks = true;
  return hooksStatus();
}

export async function claudeHooksRemove(): Promise<HooksStatus> {
  mockHooks = false;
  return hooksStatus();
}

// ---------------------------------------------------------------- designs (mock: boxes only)

const MOCK_TEMPLATES: Design[] = [
  {
    id: "clock",
    name: "Clock",
    elements: [
      { type: "text", x: 64, y: 6, text: "{HH}:{MM}", font: "small", size: 4, align: "center", erase: false },
      { type: "bar", x: 4, y: 40, w: 120, h: 1, value: "{SS}", max: "59" },
      { type: "text", x: 64, y: 50, text: "{DAY} {D} {MON}", font: "small", size: 1, align: "center", erase: false },
    ],
  },
];
let mockDesigns: Design[] = structuredClone(MOCK_TEMPLATES);

export async function designList(): Promise<Design[]> {
  return structuredClone(mockDesigns);
}

export async function designCatalog(): Promise<DesignCatalog> {
  return {
    templates: structuredClone(MOCK_TEMPLATES),
    tokens: [
      ["HH", "hour 00-23"],
      ["MM", "minute 00-59"],
      ["SS", "second 00-59"],
      ["TEMP", "temperature now"],
    ],
    icons: ["sun", "moon", "cloud", "rain", "bell", "heart"],
  };
}

export async function designSave(d: Design): Promise<Design[]> {
  const i = mockDesigns.findIndex((x) => x.id === d.id);
  if (i >= 0) mockDesigns[i] = structuredClone(d);
  else mockDesigns.push(structuredClone(d));
  return designList();
}

export async function designDelete(id: string): Promise<Design[]> {
  mockDesigns = mockDesigns.filter((d) => d.id !== id);
  return designList();
}

/** Mock render: each element's box outlined (the real renderer is in Rust). */
export async function designRender(d: Design): Promise<DesignRender> {
  const fb = new Array<number>(1024).fill(0);
  const set = (x: number, y: number) => {
    if (x >= 0 && x < 128 && y >= 0 && y < 64) fb[(y >> 3) * 128 + x] |= 1 << (y & 7);
  };
  const bounds: [number, number, number, number][] = d.elements.map((e) => {
    switch (e.type) {
      case "text": {
        const w = e.text.length * 6 * e.size;
        const x = e.align === "center" ? e.x - w / 2 : e.align === "right" ? e.x - w : e.x;
        return [Math.round(x), e.y, w, 7 * e.size];
      }
      case "line":
        return [Math.min(e.x1, e.x2), Math.min(e.y1, e.y2), Math.abs(e.x1 - e.x2) + 1, Math.abs(e.y1 - e.y2) + 1];
      case "analog":
        return [e.cx - e.r, e.cy - e.r, 2 * e.r + 1, 2 * e.r + 1];
      case "icon":
        return [e.x, e.y, 16 * e.size, 16 * e.size];
      default:
        return [e.x, e.y, e.w, e.h];
    }
  });
  for (const [x, y, w, h] of bounds) {
    for (let i = 0; i < w; i++) {
      set(x + i, y);
      set(x + i, y + h - 1);
    }
    for (let j = 0; j < h; j++) {
      set(x, y + j);
      set(x + w - 1, y + j);
    }
  }
  return { frame: fb, bounds };
}

export async function weatherSearch(q: string): Promise<Place[]> {
  return q.length < 2 ? [] : [{ name: "Pune", detail: "Maharashtra, India", lat: 18.52, lon: 73.86 }];
}

export async function weatherNow(): Promise<WeatherNow> {
  return liveCfg.weather.place
    ? {
        weather: {
          city: "Pune",
          temp: 27.6,
          tmax: 31,
          tmin: 18,
          code: 2,
          isDay: true,
          humidity: 40,
          wind: 7,
          unit: "C",
          windUnit: "km/h",
          aqi: 99,
          aqiScale: "US",
          pm25: 25,
          pm10: 33,
        },
        error: null,
        ageSecs: 60,
      }
    : { weather: null, error: null, ageSecs: null };
}
