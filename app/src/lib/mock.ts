// Browser-only stand-in for the Tauri backend, used when the page is not running
// inside Tauri (`npm run dev` in a normal browser). The drone drawing here is a
// rough JS approximation for layout work; the real app always renders with the
// C engine over IPC. Never import this directly: go through ./api.ts.

import { composeDrawing } from "./drawing";
import { MOODS, defaultFace, normalizeFace, sanitizeName, type Board, type FaceDef, type FlashProgress, type FlashReport, type Mood, type Port, type SetupStatus, type UpdateInfo, type UpdateProgress } from "./types";

const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// ---------------------------------------------------------------- presets

const PRESET_SPECS: [string, Mood, Partial<FaceDef>][] = [
  ["HOVER", "hover", { mouth: true }],
  ["PATROL", "patrol", { eyeW: 30, lookAround: false, overlay: "city" }],
  ["LIFTOFF", "takeoff", { overlay: "hills" }],
  ["STUNT", "flip", { speed: 6, mouth: true }],
  ["ORBIT", "orbit", { lookAround: false, overlay: "clouds" }],
  ["SQUADRON", "formation", { eyeGap: 12, mouth: true }],
  ["COURIER", "delivery", { overlay: "city" }],
  ["RACER", "race", { eyeW: 32, speed: 7, lookAround: false, overlay: "wind" }],
  ["SEARCH", "scan", { overlay: "grid" }],
  ["SWARM", "swarm", { lookAround: false, overlay: "stars" }],
  ["TOP VIEW", "topdown", { eyeW: 40, speed: 4 }],
  ["RECHARGE", "charge", {}],
  ["STORM", "topdown", { eyeW: 44, blinkMin: 10, cyclops: true, overlay: "rain" }],
  ["HEAVY LIFT", "hover", { eyeW: 56, eyeY: 4, speed: 3, cyclops: true, mouth: true }],
  ["AIRSHOW", "hover", { mouth: true, cycle: "random", cycleSeconds: 10 }],
  ["NIGHT SHIFT", "takeoff", { cycle: "sequence", cycleSeconds: 14, cycleMoods: ["takeoff", "patrol", "delivery", "scan", "charge"] }],
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
  return port ? "DESKBUNNY 0.1.0 atmega328p HOVER" : null;
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
    x = Math.round(x);
    y = Math.round(y);
    if (x < 0 || y < 0 || x >= W || y >= H) return;
    const i = (y >> 3) * W + x;
    if (on) this.fb[i] |= 1 << (y & 7);
    else this.fb[i] &= ~(1 << (y & 7));
  }
  line(x0: number, y0: number, x1: number, y1: number) {
    const n = Math.max(1, Math.ceil(Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0))));
    for (let i = 0; i <= n; i++) this.set(x0 + ((x1 - x0) * i) / n, y0 + ((y1 - y0) * i) / n);
  }
  ring(cx: number, cy: number, r: number) {
    for (let a = 0; a < 48; a++) {
      const t = (a / 48) * Math.PI * 2;
      this.set(cx + Math.cos(t) * r, cy + Math.sin(t) * r);
    }
  }
}

const MOOD_OVERLAY: Partial<Record<Mood, FaceDef["overlay"]>> = {
  patrol: "city",
  delivery: "city",
  takeoff: "hills",
  orbit: "clouds",
  charge: "clouds",
  race: "wind",
  scan: "grid",
  swarm: "stars",
};

/** The scene with the cycle applied at tMs (no climb-out transition in the mock). */
function cycledFace(sim: Sim, tMs: number): FaceDef {
  const f = sim.face;
  if (f.cycle === "off") return f;
  const list: readonly Mood[] = f.cycleMoods.length ? f.cycleMoods : MOODS;
  const k = Math.floor(tMs / (Math.max(1, f.cycleSeconds) * 1000));
  const mood =
    f.cycle === "sequence" ? list[k % list.length] : k === 0 ? f.mood : list[Math.floor(rand(k + sim.seed * 31) * list.length)];
  return { ...f, mood, overlay: f.cycleOverlay ? (MOOD_OVERLAY[mood] ?? "none") : f.overlay };
}

/** Side-view quadcopter, s px span, tilt in radians, m = 1 facing right. */
function drone(c: Canvas1, f: FaceDef, t: number, x: number, y: number, s: number, tilt = 0, m = 1) {
  const co = Math.cos(tilt);
  const si = Math.sin(tilt);
  const L = (x0: number, y0: number, x1: number, y1: number) =>
    c.line(x + x0 * m * co - y0 * si, y + x0 * m * si + y0 * co, x + x1 * m * co - y1 * si, y + x1 * m * si + y1 * co);
  const bw = s / 4;
  const bh = Math.max(1, Math.floor(s / 14));
  if (f.mouth) for (let i = -s / 2 + (63 - y) / 3; i <= s / 2 - (63 - y) / 3; i += 2) c.set(x + i, 63);
  for (let j = -bh; j <= bh; j++) L(-bw, j, bw, j);
  L(-s / 2, -bh, s / 2, -bh);
  for (const k of [-1, 1]) {
    const pw = 1 + (s / 5) * Math.abs(Math.cos(t / 8 + k));
    L(k * s / 2, -bh, k * s / 2, -bh - 2);
    L(k * s / 2 - pw, -bh - 3, k * s / 2 + pw, -bh - 3);
    L(k * bw / 2, bh, k * (bw / 2 + s / 9), bh + s / 9);
  }
  if (f.cyclops) L(bw / 3, bh + 2, bw / 3 + 3, bh + 2);
}

function renderMock(sim: Sim, tMs: number, fb: Uint8Array) {
  const f = cycledFace(sim, tMs);
  const c = new Canvas1(fb);
  const t = (tMs * f.speed) / 5;
  const salt = sim.seed * 7919 + f.seed;

  // Drift: hold a random target per segment, ease into it.
  let gx = 0;
  let gy = 0;
  if (f.lookAround) {
    const seg = Math.max(300, ((f.lookMin + f.lookMax) / 2) * 100);
    const k = Math.floor(t / seg);
    const tx = (k: number) => (rand(k * 2 + salt) * 2 - 1) * f.lookX;
    const ty = (k: number) => (rand(k * 2 + 1 + salt) * 2 - 1) * f.lookY;
    const e = Math.min(1, (t - k * seg) / 800);
    const s = e * e * (3 - 2 * e);
    gx = tx(k - 1) + (tx(k) - tx(k - 1)) * s;
    gy = ty(k - 1) + (ty(k) - ty(k - 1)) * s;
  }

  // Sky (rough).
  const sc = t / 100;
  switch (f.overlay) {
    case "stars":
      for (let i = 0; i < 18; i++) c.set(rand(i + salt) * W, rand(i + 99 + salt) * 44);
      break;
    case "clouds":
      for (let i = 0; i < 3; i++) c.ring(((150 - sc - i * 67) % 190 + 190) % 190 - 30, 6 + i * 6, 4);
      break;
    case "rain":
      for (let i = 0; i < 16; i++) {
        const y = ((t / 6) * (1 + (i & 1) / 2) + rand(i) * 72) % 72 - 4;
        c.line(rand(i + 7) * W, y, rand(i + 7) * W - 1, y + 3);
      }
      break;
    case "city":
    case "hills":
      for (let x = 0; x < W; x++) {
        const b = Math.floor((x + sc) / 12);
        if (f.overlay === "city") c.set(x, 55 - Math.floor(rand(b + salt) * 14));
        else c.set(x, 55 + Math.sin((x + sc) / 20) * 4);
      }
      break;
    case "wind":
      for (let i = 0; i < 6; i++) {
        const x = 140 - ((t / 5 + rand(i) * 180) % 180);
        c.line(x, 4 + i * 10, x + 8, 4 + i * 10);
      }
      break;
    case "grid":
      c.line(0, 44, W - 1, 44);
      for (let i = -4; i <= 4; i++) c.line(64 + i * 6, 45, 64 + i * 34, 63);
      break;
  }

  const s = f.eyeW;
  const bob = f.breathe ? Math.sin(t / 520) * 3 : 0;
  const x = W / 2 + gx;
  const y = H / 2 + f.eyeY + gy + bob;
  const ground = 61 - Math.max(1, Math.floor(s / 14)) - Math.max(2, Math.floor(s / 9));
  const wave = (period: number) => Math.sin((t / period) * Math.PI * 2);
  switch (f.mood) {
    case "patrol":
      drone(c, f, t, x + wave(3300) * ((W - s) / 2 - 2), y, s, 0.3 * Math.cos((t / 3300) * Math.PI * 2), Math.cos((t / 3300) * Math.PI * 2) < 0 ? -1 : 1);
      break;
    case "takeoff":
    case "charge": {
      const alt = Math.max(0, Math.min(1, wave(6500) * 1.5 + (f.mood === "charge" ? -0.8 : 0.3)));
      c.line(W / 2 - s / 2 - 4, 62, W / 2 + s / 2 + 4, 62);
      if (f.mood === "charge") {
        c.line(3, 2, 21, 2), c.line(3, 10, 21, 10), c.line(3, 2, 3, 10), c.line(21, 2, 21, 10);
      }
      drone(c, f, t, W / 2 + (x - W / 2) * alt, ground + (y - ground) * alt, s);
      break;
    }
    case "flip":
      drone(c, f, t, x, y, s, (t % 6500) > 5000 ? (((t % 6500) - 5000) / 1500) * Math.PI * 2 : 0);
      break;
    case "orbit": {
      const a = (t / 3300) * Math.PI * 2;
      c.line(63, 28, 63, 63);
      drone(c, f, t, x + Math.cos(a) * 44, y + Math.sin(a) * 6, Math.max(8, s * (0.7 + Math.sin(a) * 0.3)));
      break;
    }
    case "formation":
      for (const k of [-1, 0, 1]) drone(c, f, t, x + k * Math.min(f.eyeGap + s * 0.66, 50), y - 6 + Math.abs(k) * 12, s * 0.66);
      break;
    case "delivery": {
      const dx = x + wave(3300) * 14;
      const px = dx + wave(3300) * 4;
      c.line(dx, y - 8, px, y + 6);
      for (let i = -4; i <= 3; i++) c.line(px + i, y + 6, px + i, y + 12);
      drone(c, f, t, dx, y - 10, s);
      break;
    }
    case "race": {
      const rx = ((t / 6.4) % 256) - 64;
      for (let k = 0; k < 3; k++) c.line(rx - s / 2 - 12 - k * 5, y - 4 + k * 4, rx - s / 2 - 4, y - 4 + k * 4);
      drone(c, f, t, rx, y, s, 0.4);
      break;
    }
    case "scan": {
      const sx = x + wave(3300) * 34;
      c.line(x - 1, y - 10, sx - 9, 62);
      c.line(x + 1, y - 10, sx + 9, 62);
      drone(c, f, t, x, y - 14, s);
      break;
    }
    case "swarm":
      for (let k = 0; k < 7; k++)
        drone(c, f, t, W / 2 + Math.sin(t / (500 + k * 90) + k) * 54, H / 2 + Math.sin(t / (800 + k * 70) + k * 2) * 22, Math.max(8, s / 3));
      break;
    case "topdown": {
      const len = Math.min((s * 3) / 5, 26);
      for (let k = 0; k < 4; k++) {
        const a = t / 2000 + Math.PI / 4 + (k * Math.PI) / 2;
        c.line(x, y, x + Math.cos(a) * len, y + Math.sin(a) * len);
        c.ring(x + Math.cos(a) * len, y + Math.sin(a) * len, Math.max(2, len / 3));
      }
      c.ring(x, y, Math.max(2, len / 4));
      break;
    }
    default:
      drone(c, f, t, x, y, s);
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
