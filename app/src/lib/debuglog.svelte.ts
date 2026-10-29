// Debug log panel state. Lines stream in from the backend from launch, so the
// panel already shows what happened when it is opened after a failed flash.

import * as api from "./api";
import type { LogEntry } from "./types";

const MAX_LINES = 5000;

class DebugLog {
  open = $state(false);
  lines = $state<LogEntry[]>([]);
  private started = false;

  /** Loads what the backend already has and follows new lines. */
  async start() {
    if (this.started) return;
    this.started = true;
    const pending: LogEntry[] = [];
    let loaded = false;
    await api.onDebugLog((e) => {
      if (!loaded) pending.push(e);
      else this.push(e);
    });
    try {
      this.lines = await api.debugLog();
    } catch {
      this.lines = [];
    }
    loaded = true;
    // Lines that arrived while loading may already be in the snapshot.
    const last = this.lines.at(-1)?.t ?? -1;
    pending.filter((e) => e.t > last).forEach((e) => this.push(e));
  }

  private push(e: LogEntry) {
    this.lines.push(e);
    if (this.lines.length > MAX_LINES) this.lines.splice(0, this.lines.length - MAX_LINES);
  }

  async clear() {
    await api.debugLogClear().catch(() => {});
    this.lines = [];
  }

  scan() {
    return api.debugUsbScan();
  }

  show() {
    this.open = true;
  }

  close() {
    this.open = false;
  }
}

export const debugLog = new DebugLog();

export function formatTime(ms: number): string {
  const s = ms / 1000;
  return s.toFixed(3).padStart(9, " ");
}

/** Plain text for "Copy": one line per entry, continuation lines indented. */
export function logText(lines: LogEntry[]): string {
  return lines
    .map((e) => `${formatTime(e.t)} ${e.level.toUpperCase().padEnd(5)} ${e.msg.replace(/\n/g, "\n                ")}`)
    .join("\n");
}
