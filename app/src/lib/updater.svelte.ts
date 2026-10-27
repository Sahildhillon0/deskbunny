// In-app updates. App.svelte checks once on launch and opens the dialog only when
// a newer release exists; the library header button checks on demand.

import * as api from "./api";
import type { UpdateInfo } from "./types";

type Phase = "idle" | "checking" | "available" | "latest" | "installing" | "error";

class Updater {
  open = $state(false);
  phase = $state<Phase>("idle");
  info = $state<UpdateInfo | null>(null);
  current = $state("");
  error = $state("");
  downloaded = $state(0);
  total = $state<number | null>(null);

  /** Silent: failures (offline, no release yet) never interrupt the user. */
  async checkOnLaunch() {
    api.appVersion().then((v) => (this.current = v)).catch(() => {});
    try {
      const info = await api.updateCheck();
      if (info) {
        this.info = info;
        this.phase = "available";
        this.open = true;
      }
    } catch {
      // The header button is still there.
    }
  }

  async check() {
    if (this.phase === "checking" || this.phase === "installing") return;
    this.open = true;
    this.phase = "checking";
    this.error = "";
    try {
      this.info = await api.updateCheck();
      this.phase = this.info ? "available" : "latest";
    } catch (e) {
      this.error = api.errorText(e);
      this.phase = "error";
    }
  }

  async install() {
    this.phase = "installing";
    this.error = "";
    this.downloaded = 0;
    this.total = null;
    const unlisten = await api.onUpdateProgress((p) => {
      this.downloaded = p.downloaded;
      this.total = p.total;
    });
    try {
      await api.updateInstall(); // restarts the app on success
    } catch (e) {
      this.error = api.errorText(e);
      this.phase = "error";
    } finally {
      unlisten();
    }
  }

  close() {
    if (this.phase === "installing") return;
    this.open = false;
  }
}

export const updater = new Updater();
