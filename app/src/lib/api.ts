// Typed facade over the Tauri commands in docs/ARCHITECTURE.md ("Tauri IPC").
// Outside Tauri (plain `npm run dev` in a browser) every call goes to ./mock.ts.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Board,
  FaceDef,
  FlashProgress,
  FlashReport,
  ClaudeView,
  Design,
  DesignCatalog,
  DesignRender,
  HooksStatus,
  LiveConfig,
  LiveStatus,
  Place,
  Port,
  ScreenRef,
  WeatherNow,
  SetupStatus,
  UpdateInfo,
  UpdateProgress,
} from "./types";

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

// ---------------------------------------------------------------- live mode

export const liveGetConfig = (): Promise<LiveConfig> =>
  isTauri ? invoke<LiveConfig>("live_get_config") : mock().then((m) => m.liveGetConfig());

/** Saves and applies; resolves to the config as the backend sanitised it. */
export const liveSetConfig = (config: LiveConfig): Promise<LiveConfig> =>
  isTauri ? invoke<LiveConfig>("live_set_config", { config }) : mock().then((m) => m.liveSetConfig(config));

export const liveStatus = (): Promise<LiveStatus> =>
  isTauri ? invoke<LiveStatus>("live_status") : mock().then((m) => m.liveStatus());

/** Keep one screen on the board (null = back to the rotation). */
export const livePin = (screen: ScreenRef | null): Promise<void> =>
  isTauri ? invoke<void>("live_pin", { screen }) : mock().then((m) => m.livePin(screen));

/** One 1024-byte frame of a screen as the board will show it (not the face). */
export async function livePreview(screen: ScreenRef, tMs: number): Promise<Uint8Array> {
  const t = Math.max(0, Math.floor(tMs)) >>> 0;
  return toBytes(isTauri ? await invoke<ArrayBuffer>("live_preview", { screen, tMs: t }) : await (await mock()).livePreview(screen, t));
}

export function onLiveStatus(cb: (s: LiveStatus) => void): Promise<() => void> {
  if (!isTauri) return mock().then((m) => m.onLiveStatus(cb));
  return listen<LiveStatus>("live-status", (ev) => cb(ev.payload));
}

export const liveTestNotification = (): Promise<void> =>
  isTauri ? invoke<void>("live_test_notification") : mock().then((m) => m.liveTestNotification());

export const claudeView = (): Promise<ClaudeView> =>
  isTauri ? invoke<ClaudeView>("claude_view") : mock().then((m) => m.claudeView());

export const claudeHooksStatus = (): Promise<HooksStatus> =>
  isTauri ? invoke<HooksStatus>("claude_hooks_status") : mock().then((m) => m.claudeHooksStatus());

/** Adds DeskBunny's hooks to ~/.claude/settings.json (backed up first). */
export const claudeHooksInstall = (): Promise<HooksStatus> =>
  isTauri ? invoke<HooksStatus>("claude_hooks_install") : mock().then((m) => m.claudeHooksInstall());

export const claudeHooksRemove = (): Promise<HooksStatus> =>
  isTauri ? invoke<HooksStatus>("claude_hooks_remove") : mock().then((m) => m.claudeHooksRemove());

// ---------------------------------------------------------------- designs

export const designList = (): Promise<Design[]> =>
  isTauri ? invoke<Design[]>("design_list") : mock().then((m) => m.designList());

export const designCatalog = (): Promise<DesignCatalog> =>
  isTauri ? invoke<DesignCatalog>("design_catalog") : mock().then((m) => m.designCatalog());

/** Insert or replace by id; resolves to every design. */
export const designSave = (design: Design): Promise<Design[]> =>
  isTauri ? invoke<Design[]>("design_save", { design }) : mock().then((m) => m.designSave(design));

export const designDelete = (id: string): Promise<Design[]> =>
  isTauri ? invoke<Design[]>("design_delete", { id }) : mock().then((m) => m.designDelete(id));

/** Render an unsaved design with each element's bounds. */
export const designRender = (design: Design): Promise<DesignRender> =>
  isTauri ? invoke<DesignRender>("design_render", { design }) : mock().then((m) => m.designRender(design));

// ---------------------------------------------------------------- weather

export const weatherSearch = (query: string): Promise<Place[]> =>
  isTauri ? invoke<Place[]>("weather_search", { query }) : mock().then((m) => m.weatherSearch(query));

export const weatherNow = (): Promise<WeatherNow> =>
  isTauri ? invoke<WeatherNow>("weather_now") : mock().then((m) => m.weatherNow());
