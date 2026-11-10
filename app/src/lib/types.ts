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
  "wink",
  "laugh",
  "crying",
  "skeptical",
  "smug",
  "evil",
  "worried",
  "shy",
  "squint",
  "dead",
  "asleep",
  "starry",
  "hypno",
  "glitch",
  "scan",
  "eyeroll",
  "confused",
  "determined",
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

export const CYCLES = ["off", "sequence", "random"] as const;
export type Cycle = (typeof CYCLES)[number];

/** Moods in a cycle list (blob limit). */
export const CYCLE_MAX = 12;

export const DRAW_MODES = ["replace", "over"] as const;
/** replace: only the drawing; over: the drawing on top of the animated face. */
export type DrawMode = (typeof DRAW_MODES)[number];

/** Most frames in a drawing. The real limit is the board's drawing slot (Board.artCapacity). */
export const DRAW_FRAMES_MAX = 10;
/** Bytes of drawing blob header, counted against the slot. */
export const ART_HEADER = 16;
export const FRAME_MS_MIN = 50;
export const FRAME_MS_MAX = 10000;

/** 128x64 frames (drawn, or from a GIF), flashed with the face. */
export interface Drawing {
  mode: DrawMode;
  /** Milliseconds per frame. */
  frameMs: number;
  /** base64 of 1024 bytes each, SSD1306 page layout. */
  frames: string[];
}

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
  /** Mood cycle: change mood every cycleSeconds, in list order or at random. */
  cycle: Cycle;
  cycleSeconds: number;
  /** Empty = every mood. Order is play order for "sequence". */
  cycleMoods: Mood[];
  /** The overlay follows the mood while cycling. */
  cycleOverlay: boolean;
  /** Absent = no drawing. */
  drawing?: Drawing;
}

/** Protocol of the ESP32 family (nixxa firmware; faces come from the phone app). */
export const ESP_PROTOCOL = "esptool";

export interface Board {
  id: string;
  name: string;
  mcu: string;
  /** "stk500v1" | "stk500v2" | "avr109" | "dfu" | "samba" | "esptool" */
  protocol: string;
  /** Bytes the board's drawing slot holds (header included); absent when unknown. */
  artCapacity?: number;
}

/** Absent keys are omitted, never null. vid/pid are 4-digit uppercase hex ("2341"). */
export interface Port {
  path: string;
  vid?: string;
  pid?: string;
  product?: string;
  serialNumber?: string;
  guessedBoard?: string;
}

export type FlashStage = "reset" | "sync" | "erase" | "write" | "verify" | "done";

export interface FlashProgress {
  stage: FlashStage;
  percent: number;
  message: string;
}

export interface FlashReport {
  protocol: string;
  /** Where the bootloader answered; may differ from the requested port after a 1200-baud touch. */
  port: string;
  baud: number | null;
  signature: string | null;
  bytesWritten: number;
  /** Flash read back and compared (false for DFU; ESP32: flash MD5 matched). */
  verified: boolean;
  durationMs: number;
  warnings: string[];
  boardId: string;
  faceName: string;
  fwVersion: string;
  blobAddress: number;
  /** The `DESKBUNNY ...` line read after the upload. */
  deviceInfo: string | null;
  /** deviceInfo ends with faceName (ESP32: deviceInfo reports boardId; faceName is ""). */
  deviceVerified: boolean;
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
    cycle: "off",
    cycleSeconds: 10,
    cycleMoods: [],
    cycleOverlay: true,
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
    eyeY: clampInt(o.eyeY, -128, 127, d.eyeY),
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
    cycle: CYCLES.includes(o.cycle as Cycle) ? (o.cycle as Cycle) : d.cycle,
    cycleSeconds: clampInt(o.cycleSeconds, 1, 255, d.cycleSeconds),
    cycleMoods: Array.isArray(o.cycleMoods)
      ? o.cycleMoods.filter((m): m is Mood => MOODS.includes(m as Mood)).slice(0, CYCLE_MAX)
      : [],
    cycleOverlay: bool("cycleOverlay"),
    ...normalizeDrawing(o.drawing),
  };
}

function normalizeDrawing(input: unknown): { drawing?: Drawing } {
  if (!input || typeof input !== "object") return {};
  const o = input as Record<string, unknown>;
  const frames = Array.isArray(o.frames) ? o.frames.filter((f): f is string => typeof f === "string").slice(0, DRAW_FRAMES_MAX) : [];
  if (!frames.length) return {};
  return {
    drawing: {
      mode: DRAW_MODES.includes(o.mode as DrawMode) ? (o.mode as DrawMode) : "replace",
      frameMs: clampInt(o.frameMs, FRAME_MS_MIN, FRAME_MS_MAX, 500),
      frames,
    },
  };
}

// ---------------------------------------------------------------- setup guide

export type CheckState = "ok" | "action" | "warn" | "info";

export interface SetupCheck {
  id: string;
  title: string;
  detail: string;
  state: CheckState;
  /** One-click fix run by the backend (may show an OS permission prompt). */
  action?: { id: string; label: string };
  /** Manual fallback commands the user can copy. */
  commands?: string[];
  /** Allowlisted external page, opened by id. */
  link?: { id: string; label: string };
}

export interface SetupStatus {
  os: "linux" | "windows" | "macos";
  osLabel: string;
  firstRun: boolean;
  checks: SetupCheck[];
}

// ---------------------------------------------------------------- updates

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  /** YYYY-MM-DD */
  date: string | null;
  /** Release notes (markdown), if any. */
  notes: string | null;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

// ---------------------------------------------------------------- live mode

export type ScreenKind = "face" | "claude" | "design" | "notify";

/** A screen: a kind, plus the design id for "design". */
export interface ScreenRef {
  kind: ScreenKind;
  design?: string;
}

export interface ScreenCfg extends ScreenRef {
  enabled: boolean;
  /** Seconds per turn in the rotation, 3..600. */
  seconds: number;
}

export interface LiveConfig {
  enabled: boolean;
  port: string | null;
  /** Rotation order: the face and Claude once each, designs at most once each. */
  screens: ScreenCfg[];
  claudeStyle: "clawd" | "face";
  /** Stay on the Claude screen the whole time a session works or needs you. */
  claudeHold: boolean;
  notify: NotifyCfg;
  weather: WeatherCfg;
}

export interface Place {
  name: string;
  /** Region and country. */
  detail: string;
  lat: number;
  lon: number;
}

export interface WeatherCfg {
  place: Place | null;
  units: "metric" | "imperial";
  /** US EPA (0-500), European (0-100+) or India's CPCB (0-500) air quality index. */
  aqi: "us" | "eu" | "in";
}

export interface Weather {
  city: string;
  temp: number;
  tmax: number;
  tmin: number;
  code: number;
  isDay: boolean;
  humidity: number;
  wind: number;
  unit: string;
  windUnit: string;
  aqi: number | null;
  /** "US", "EU" or "IN" */
  aqiScale: string;
  pm25: number | null;
  pm10: number | null;
}

export interface WeatherNow {
  weather: Weather | null;
  error: string | null;
  ageSecs: number | null;
}

// ---------------------------------------------------------------- designs

export type Align = "left" | "center" | "right";
export type DesignFont = "small" | "seg";

export type Element =
  | { type: "text"; x: number; y: number; text: string; font: DesignFont; size: number; align: Align; erase: boolean }
  | { type: "rect"; x: number; y: number; w: number; h: number; fill: boolean; erase: boolean }
  | { type: "line"; x1: number; y1: number; x2: number; y2: number; thick: number }
  | { type: "bar"; x: number; y: number; w: number; h: number; value: string; max: string }
  | { type: "analog"; cx: number; cy: number; r: number; seconds: boolean }
  | { type: "icon"; x: number; y: number; icon: string; size: number }
  | { type: "bitmap"; x: number; y: number; w: number; h: number; data: string };

export type ElementType = Element["type"];

export interface Design {
  id: string;
  name: string;
  elements: Element[];
}

export interface DesignCatalog {
  templates: Design[];
  /** [token, meaning] */
  tokens: [string, string][];
  icons: string[];
}

export interface DesignRender {
  frame: number[];
  /** [x, y, w, h] per element */
  bounds: [number, number, number, number][];
}

export interface NotifyCfg {
  enabled: boolean;
  /** Also show the desktop's own notifications (Linux). */
  desktop: boolean;
  /** Seconds each one stays up, 2..60. */
  seconds: number;
  showBody: boolean;
  ignoreApps: string[];
}

export interface HooksStatus {
  installed: boolean;
  settingsPath: string;
  command: string;
}

export type ClaudeState = "idle" | "starting" | "working" | "done" | "needsYou";

export interface ClaudeView {
  state: ClaudeState;
  caption: string;
  project: string;
  sessions: number;
  takeover: boolean;
  active: boolean;
}

export type LiveState = "off" | "paused" | "connecting" | "connected" | "error";

export interface LiveStatus {
  state: LiveState;
  port: string | null;
  banner: string | null;
  /** The board draws Clawd itself; otherwise the Claude screen uses the face. */
  clawd: boolean;
  screen: ScreenRef | null;
  pinned: ScreenRef | null;
  message: string | null;
}
