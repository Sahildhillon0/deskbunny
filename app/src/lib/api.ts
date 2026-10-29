// Typed facade over the Tauri commands in docs/ARCHITECTURE.md ("Tauri IPC").
// Outside Tauri (plain `npm run dev` in a browser) every call goes to ./mock.ts.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Board, FaceDef, FlashProgress, FlashReport, LogEntry, Port, SetupStatus, UpdateInfo, UpdateProgress } from "./types";

// Loaded lazily so the mock never ships in the Tauri code path.
const mock = () => import("./mock");

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export const isTauri = typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined;

/** Tauri rejects with a plain string; normalise anything else to one. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

function toBytes(raw: unknown): Uint8Array {
  if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
  if (ArrayBuffer.isView(raw)) return new Uint8Array(raw.buffer, raw.byteOffset, raw.byteLength);
  if (Array.isArray(raw)) return Uint8Array.from(raw as number[]);
  throw new Error("sim_frame returned an unexpected payload");
}

export const listPresets = (): Promise<FaceDef[]> =>
  isTauri ? invoke<FaceDef[]>("list_presets") : mock().then((m) => m.listPresets());

export const listBoards = (): Promise<Board[]> =>
  isTauri ? invoke<Board[]>("list_boards") : mock().then((m) => m.listBoards());

export const listPorts = (): Promise<Port[]> =>
  isTauri ? invoke<Port[]>("list_ports") : mock().then((m) => m.listPorts());

export const simCreate = (face: FaceDef, seed: number): Promise<number> =>
  isTauri ? invoke<number>("sim_create", { face, seed }) : mock().then((m) => m.simCreate(face, seed));

export const simSetFace = (id: number, face: FaceDef): Promise<void> =>
  isTauri ? invoke<void>("sim_set_face", { id, face }) : mock().then((m) => m.simSetFace(id, face));

/** One 1024-byte SSD1306 page-layout framebuffer. */
export async function simFrame(id: number, tMs: number): Promise<Uint8Array> {
  const t = Math.max(0, Math.floor(tMs)) >>> 0;
  return toBytes(isTauri ? await invoke<ArrayBuffer>("sim_frame", { id, tMs: t }) : await (await mock()).simFrame(id, t));
}

export const simDrop = (id: number): Promise<void> =>
  isTauri ? invoke<void>("sim_drop", { id }) : mock().then((m) => m.simDrop(id));

export const flash = (port: string, boardId: string, face: FaceDef): Promise<FlashReport> =>
  isTauri ? invoke<FlashReport>("flash", { port, boardId, face }) : mock().then((m) => m.flash(port, boardId, face));

export function onFlashProgress(cb: (p: FlashProgress) => void): Promise<() => void> {
  if (!isTauri) return mock().then((m) => m.onFlashProgress(cb));
  return listen<FlashProgress>("flash-progress", (ev) => cb(ev.payload));
}

export const deviceInfo = (port: string): Promise<string | null> =>
  isTauri ? invoke<string | null>("device_info", { port }) : mock().then((m) => m.deviceInfo(port));

export const listSavedFaces = (): Promise<FaceDef[]> =>
  isTauri ? invoke<FaceDef[]>("list_saved_faces") : mock().then((m) => m.listSavedFaces());

export const saveFace = (face: FaceDef): Promise<void> =>
  isTauri ? invoke<void>("save_face", { face }) : mock().then((m) => m.saveFace(face));

export const deleteFace = (name: string): Promise<void> =>
  isTauri ? invoke<void>("delete_face", { name }) : mock().then((m) => m.deleteFace(name));

export const encodeFace = (face: FaceDef): Promise<number[]> =>
  isTauri ? invoke<number[]>("encode_face", { face }) : mock().then((m) => m.encodeFace(face));

/** Native save dialog; resolves to the written path, or null if cancelled. */
export const exportFace = (face: FaceDef): Promise<string | null> =>
  isTauri ? invoke<string | null>("export_face", { face }) : mock().then((m) => m.exportFace(face));

/** Native open dialog; resolves to the validated face, or null if cancelled. */
export const importFace = (): Promise<FaceDef | null> =>
  isTauri ? invoke<FaceDef | null>("import_face") : mock().then((m) => m.importFace());

// ---------------------------------------------------------------- setup guide

export const setupStatus = (): Promise<SetupStatus> =>
  isTauri ? invoke<SetupStatus>("setup_status") : mock().then((m) => m.setupStatus());

/** Runs a one-click fix; resolves to a success message, rejects with a readable error. */
export const setupRun = (action: string): Promise<string> =>
  isTauri ? invoke<string>("setup_run", { action }) : mock().then((m) => m.setupRun(action));

/** Remembers that the guide was finished or skipped, so it doesn't open on launch again. */
export const setupFinish = (): Promise<void> =>
  isTauri ? invoke<void>("setup_finish") : mock().then((m) => m.setupFinish());

export const setupOpenLink = (id: string): Promise<void> =>
  isTauri ? invoke<void>("setup_open_link", { id }) : mock().then((m) => m.setupOpenLink(id));

// ---------------------------------------------------------------- updates

export const appVersion = (): Promise<string> =>
  isTauri ? invoke<string>("app_version") : mock().then((m) => m.appVersion());

/** Resolves to the newer release, or null when this is the latest. */
export const updateCheck = (): Promise<UpdateInfo | null> =>
  isTauri ? invoke<UpdateInfo | null>("update_check") : mock().then((m) => m.updateCheck());

/** Downloads, installs and restarts; only ever settles by rejecting. */
export const updateInstall = (): Promise<void> =>
  isTauri ? invoke<void>("update_install") : mock().then((m) => m.updateInstall());

export function onUpdateProgress(cb: (p: UpdateProgress) => void): Promise<() => void> {
  if (!isTauri) return mock().then((m) => m.onUpdateProgress(cb));
  return listen<UpdateProgress>("update-progress", (ev) => cb(ev.payload));
}

export const updateOpenReleases = (): Promise<void> =>
  isTauri ? invoke<void>("update_open_releases") : mock().then((m) => m.updateOpenReleases());

// ---------------------------------------------------------------- debug log

export const debugLog = (): Promise<LogEntry[]> =>
  isTauri ? invoke<LogEntry[]>("debug_log") : mock().then((m) => m.debugLog());

export const debugLogClear = (): Promise<void> =>
  isTauri ? invoke<void>("debug_log_clear") : mock().then((m) => m.debugLogClear());

/** Logs every serial port and USB device the OS reports right now. */
export const debugUsbScan = (): Promise<void> =>
  isTauri ? invoke<void>("debug_usb_scan") : mock().then((m) => m.debugUsbScan());

export function onDebugLog(cb: (e: LogEntry) => void): Promise<() => void> {
  if (!isTauri) return mock().then((m) => m.onDebugLog(cb));
  return listen<LogEntry>("debug-log", (ev) => cb(ev.payload));
}
