// IPC contract types. Source of truth: docs/ARCHITECTURE.md ("Tauri IPC").

export const MOODS = [
  "neutral",
  "happy",
  "angry",
  "sad",
  "sleepy",
  "surprised",
  "love",
  "suspicious",
  "excited",
  "bored",
  "scared",
  "dizzy",
] as const;
export type Mood = (typeof MOODS)[number];

export const OVERLAYS = [
  "none",
  "hearts",
  "zzz",
  "sweat",
  "anger",
  "tears",
  "sparkles",
  "question",
] as const;
export type Overlay = (typeof OVERLAYS)[number];

export interface FaceDef {
  name: string;
  mood: Mood;
  eyeW: number;
  eyeH: number;
  eyeR: number;
  eyeGap: number;
  eyeY: number;
  blink: boolean;
  lookAround: boolean;
  breathe: boolean;
  cyclops: boolean;
  invert: boolean;
  rotate180: boolean;
  mouth: boolean;
  blinkMin: number;
  blinkMax: number;
  lookMin: number;
  lookMax: number;
  lookX: number;
  lookY: number;
  speed: number;
  contrast: number;
  overlay: Overlay;
  seed: number;
}

export interface Board {
  id: string;
  name: string;
  mcu: string;
  protocol: string;
}

export interface Port {
  path: string;
  vid?: string | null;
  pid?: string | null;
  product?: string | null;
  serialNumber?: string | null;
  guessedBoard?: string | null;
}

export type FlashStage = "reset" | "sync" | "erase" | "write" | "verify" | "done";

export interface FlashProgress {
  stage: FlashStage;
  percent: number;
  message: string;
}

/** Shape not pinned down in ARCHITECTURE.md; every field is read defensively. */
export interface FlashReport {
  port?: string;
  boardId?: string;
  bytesWritten?: number;
  durationMs?: number;
  deviceLine?: string | null;
}

export const NAME_MAX = 12;

/** Uppercases and strips characters the OLED font cannot draw. */
export function sanitizeName(raw: string): string {
  return raw
    .toUpperCase()
    .replace(/[^A-Z0-9 ]/g, "")
    .slice(0, NAME_MAX);
}

export function defaultFace(): FaceDef {
  return {
    name: "BUNNY",
    mood: "neutral",
    eyeW: 36,
    eyeH: 36,
    eyeR: 8,
    eyeGap: 16,
    eyeY: 0,
    blink: true,
    lookAround: true,
    breathe: true,
    cyclops: false,
    invert: false,
    rotate180: false,
    mouth: false,
    blinkMin: 20,
    blinkMax: 60,
    lookMin: 15,
    lookMax: 50,
    lookX: 18,
    lookY: 8,
    speed: 5,
    contrast: 0xcf,
    overlay: "none",
    seed: 0,
  };
}

const clampInt = (v: unknown, lo: number, hi: number, dflt: number): number => {
  const n = typeof v === "number" && Number.isFinite(v) ? Math.round(v) : dflt;
  return Math.min(hi, Math.max(lo, n));
};

/** Coerces untrusted JSON (imports, mock storage) into a valid FaceDef. */
export function normalizeFace(input: unknown): FaceDef {
  const d = defaultFace();
  const o = (input && typeof input === "object" ? input : {}) as Record<string, unknown>;
  const bool = (k: keyof FaceDef) => (typeof o[k] === "boolean" ? (o[k] as boolean) : (d[k] as boolean));
  const eyeW = clampInt(o.eyeW, 8, 56, d.eyeW);
  const eyeH = clampInt(o.eyeH, 8, 56, d.eyeH);
  const blinkMin = clampInt(o.blinkMin, 1, 255, d.blinkMin);
  const lookMin = clampInt(o.lookMin, 1, 255, d.lookMin);
  return {
    name: sanitizeName(typeof o.name === "string" ? o.name : d.name) || d.name,
    mood: MOODS.includes(o.mood as Mood) ? (o.mood as Mood) : d.mood,
    eyeW,
    eyeH,
    eyeR: clampInt(o.eyeR, 0, Math.floor(eyeW / 2), d.eyeR),
    eyeGap: clampInt(o.eyeGap, 0, 40, d.eyeGap),
    eyeY: clampInt(o.eyeY, -20, 20, d.eyeY),
    blink: bool("blink"),
    lookAround: bool("lookAround"),
    breathe: bool("breathe"),
    cyclops: bool("cyclops"),
    invert: bool("invert"),
    rotate180: bool("rotate180"),
    mouth: bool("mouth"),
    blinkMin,
    blinkMax: clampInt(o.blinkMax, blinkMin, 255, Math.max(blinkMin, d.blinkMax)),
    lookMin,
    lookMax: clampInt(o.lookMax, lookMin, 255, Math.max(lookMin, d.lookMax)),
    lookX: clampInt(o.lookX, 0, 40, d.lookX),
    lookY: clampInt(o.lookY, 0, 20, d.lookY),
    speed: clampInt(o.speed, 1, 10, d.speed),
    contrast: clampInt(o.contrast, 0, 255, d.contrast),
    overlay: OVERLAYS.includes(o.overlay as Overlay) ? (o.overlay as Overlay) : d.overlay,
    seed: clampInt(o.seed, 0, 255, d.seed),
  };
}
