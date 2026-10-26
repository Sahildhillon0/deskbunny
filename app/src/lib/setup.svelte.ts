// Setup guide visibility. Opens by itself on first launch (App.svelte); the
// help button in the library header reopens it.

import * as api from "./api";

class SetupGuide {
  open = $state(false);

  async openIfFirstRun() {
    try {
      if ((await api.setupStatus()).firstRun) this.open = true;
    } catch {
      // A failed check must never block the app; the guide stays reachable from the header.
    }
  }

  show() {
    this.open = true;
  }

  /** Finishing and skipping both count: the guide is one click away afterwards. */
  async close() {
    this.open = false;
    await api.setupFinish().catch(() => {});
  }
}

export const setup = new SetupGuide();
