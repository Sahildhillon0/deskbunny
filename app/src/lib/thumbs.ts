// One shared, throttled loop for every library thumbnail. Only cards that are
// on screen render, at most THUMB_FPS, and nothing runs while the window is hidden.

import { simFrame } from "./api";
import type { OledRenderer } from "./oled";

const THUMB_FPS = 12;

export interface Thumb {
  simId: number;
  renderer: OledRenderer;
  visible: boolean;
}

const thumbs = new Set<Thumb & { busy: boolean }>();
const t0 = performance.now();
let raf = 0;
let last = 0;

function tick(now: number) {
  raf = requestAnimationFrame(tick);
  if (document.hidden || now - last < 1000 / THUMB_FPS - 2) return;
  last = now;
  const t = now - t0;
  for (const th of thumbs) {
    if (!th.visible || th.busy) continue;
    th.busy = true;
    simFrame(th.simId, t)
      .then((fb) => th.renderer.draw(fb))
      .catch(() => {})
      .finally(() => (th.busy = false));
  }
}

function start() {
  if (!raf && thumbs.size && !document.hidden) raf = requestAnimationFrame(tick);
}

function stop() {
  cancelAnimationFrame(raf);
  raf = 0;
}

document.addEventListener("visibilitychange", () => (document.hidden ? stop() : start()));

/** Registers a thumbnail; returns the live record (toggle `visible`) and an unregister fn. */
export function registerThumb(simId: number, renderer: OledRenderer): [Thumb, () => void] {
  const th = { simId, renderer, visible: false, busy: false };
  thumbs.add(th);
  start();
  return [
    th,
    () => {
      thumbs.delete(th);
      if (!thumbs.size) stop();
    },
  ];
}
