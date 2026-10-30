<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import { OledRenderer, OLED_H, OLED_W } from "../oled";
  import { app } from "../state.svelte";
  import Icon from "./Icon.svelte";

  const TARGET_FPS = 30;
  const SPEEDS = [0.5, 1, 2] as const;
  const PINS = ["GND", "VCC", "D0", "D1", "RES", "DC", "CS"];

  let stage: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let renderer: OledRenderer | null = null;
  let simId = $state<number | null>(null);

  let playing = $state(true);
  let speed = $state<number>(1);
  let zoom = $state(false);
  let fps = $state(0);
  let simSeconds = $state(0);
  let scale = $state(3);
  let error = $state<string | null>(null);

  let needsFrame = true;

  function fitScale(w: number, h: number) {
    // Room taken by the module's PCB, header and padding around the glass.
    const [cw, ch] = zoom ? [48, 32] : [136, 168];
    const s = Math.floor(Math.min((w - cw) / OLED_W, (h - ch) / OLED_H));
    return Math.max(1, Math.min(zoom ? 12 : 6, s));
  }

  function relayout() {
    if (!stage || !renderer) return;
    scale = fitScale(stage.clientWidth, stage.clientHeight);
    renderer.resize(scale);
  }

  onMount(() => {
    renderer = new OledRenderer(canvas, { glow: 1, grid: true });
    renderer.setContrast(app.face.contrast);
    relayout();
    const ro = new ResizeObserver(relayout);
    ro.observe(stage);

    let dead = false;
    api
      .simCreate($state.snapshot(app.face), 0)
      .then((id) => {
        if (dead) api.simDrop(id);
        else simId = id;
      })
      .catch((e) => (error = api.errorText(e)));

    let simT = 0;
    let lastNow = performance.now();
    let lastReq = 0;
    let inFlight = false;
    let frames = 0;
    let fpsSince = lastNow;
    let raf = 0;

    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      const dt = Math.min(250, now - lastNow);
      lastNow = now;
      if (playing) simT += dt * speed;

      if (now - fpsSince >= 500) {
        fps = Math.round((frames * 1000) / (now - fpsSince));
        frames = 0;
        fpsSince = now;
        simSeconds = simT / 1000;
      }

      // One request in flight at a time; never queue frames behind a slow backend.
      // Paused under the drawing editor: it would compete for the main thread and IPC.
      if (simId === null || inFlight || app.drawOpen || !(playing || needsFrame)) return;
      if (now - lastReq < 1000 / TARGET_FPS - 2) return;
      inFlight = true;
      lastReq = now;
      needsFrame = false;
      api
        .simFrame(simId, simT)
        .then((fb) => {
          renderer?.draw(fb);
          frames++;
          error = null;
        })
        .catch((e) => (error = api.errorText(e)))
        .finally(() => (inFlight = false));
    };
    raf = requestAnimationFrame(loop);

    return () => {
      dead = true;
      cancelAnimationFrame(raf);
      ro.disconnect();
      if (simId !== null) api.simDrop(simId);
    };
  });

  // Every edit goes straight to the engine.
  $effect(() => {
    const face = $state.snapshot(app.face);
    renderer?.setContrast(face.contrast);
    if (simId === null) return;
    api
      .simSetFace(simId, face)
      .then(() => (needsFrame = true))
      .catch((e) => (error = api.errorText(e)));
  });

  $effect(() => {
    void zoom;
    relayout();
  });
</script>

<div class="stage" bind:this={stage}>
  <div class="device" class:zoom>
    {#if !zoom}
      <div class="header" aria-hidden="true">
        {#each PINS as pin (pin)}
          <span class="pin"><i></i>{pin}</span>
        {/each}
      </div>
      <i class="hole tl"></i><i class="hole tr"></i><i class="hole bl"></i><i class="hole br"></i>
    {/if}
    <div class="glass" role="img" aria-label={`Live preview of ${app.face.name || "the drones"} on the 128 by 64 OLED`}>
      <canvas bind:this={canvas} aria-hidden="true"></canvas>
      <span class="sheen" aria-hidden="true"></span>
    </div>
    {#if !zoom}
      <div class="silk" aria-hidden="true">0.96" OLED · 128×64 · SSD1306</div>
    {/if}
  </div>
  {#if error}
    <p class="sim-error" role="alert">{error}</p>
  {/if}
</div>

<div class="controls" role="toolbar" aria-label="Simulator controls">
  <button
    class="btn icon"
    onclick={() => (playing = !playing)}
    aria-label={playing ? "Pause" : "Play"}
    title={playing ? "Pause" : "Play"}
  >
    <Icon name={playing ? "pause" : "play"} />
  </button>

  <div class="seg" role="radiogroup" aria-label="Simulation speed">
    {#each SPEEDS as s (s)}
      <label class:on={speed === s}>
        <input type="radio" name="sim-speed" value={s} bind:group={speed} class="sr-only" />
        {s}×
      </label>
    {/each}
  </div>

  <button
    class="btn icon"
    class:active={zoom}
    onclick={() => (zoom = !zoom)}
    aria-pressed={zoom}
    aria-label="Zoom to pixels"
    title={zoom ? "Show module" : "Zoom to pixels"}
  >
    <Icon name={zoom ? "zoomOut" : "zoomIn"} />
  </button>

  <span class="readout" aria-live="off">
    <span>{fps} fps</span>
    <span class="sep"></span>
    <span>{simSeconds.toFixed(1)} s</span>
    <span class="sep"></span>
    <span>{scale}×</span>
  </span>
</div>

<style>
  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 24px;
  }

  /* The module: matte black PCB, 7-pin header, four mounting holes. */
  .device {
    position: relative;
    padding: 44px 32px 36px;
    border-radius: 12px;
    background:
      radial-gradient(120% 90% at 30% 0%, rgba(255, 255, 255, 0.035), transparent 60%),
      linear-gradient(180deg, #0e1012, #08090a);
    border: 1px solid #1c1e20;
    box-shadow:
      0 1px 0 rgba(255, 255, 255, 0.04) inset,
      0 30px 60px -20px rgba(0, 0, 0, 0.9),
      0 0 0 1px rgba(0, 0, 0, 0.8);
  }

  .device.zoom {
    padding: 0;
    border-radius: 6px;
    background: transparent;
    border-color: transparent;
    box-shadow: none;
  }

  .header {
    position: absolute;
    top: 10px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    gap: 6px;
  }

  .pin {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    width: 26px;
    font-family: var(--mono);
    font-size: 8px;
    letter-spacing: 0.04em;
    color: #6d7377;
  }

  .pin i {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: radial-gradient(circle, #050505 30%, #2c2f31 34%, #45494c 60%, #1d1f21 64%);
  }

  .hole {
    position: absolute;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #000;
    box-shadow: 0 0 0 2px #1a1c1e;
  }

  .hole.tl { top: 10px; left: 10px; }
  .hole.tr { top: 10px; right: 10px; }
  .hole.bl { bottom: 10px; left: 10px; }
  .hole.br { bottom: 10px; right: 10px; }

  .glass {
    position: relative;
    padding: 10px 10px 18px;
    border-radius: 4px;
    background: #030405;
    box-shadow:
      0 0 0 1px #000,
      0 0 0 2px #1b1d1f,
      0 2px 12px rgba(0, 0, 0, 0.8) inset;
  }

  .zoom .glass {
    padding: 0;
    border-radius: 2px;
    box-shadow: 0 0 0 1px #141414;
  }

  canvas {
    display: block;
    image-rendering: pixelated;
  }

  .sheen {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
    background: linear-gradient(155deg, rgba(255, 255, 255, 0.045) 0%, rgba(255, 255, 255, 0.01) 32%, transparent 33%);
  }

  .silk {
    position: absolute;
    bottom: 12px;
    left: 0;
    right: 0;
    text-align: center;
    font-family: var(--mono);
    font-size: 9px;
    letter-spacing: 0.08em;
    color: #4f5559;
  }

  .sim-error {
    margin: 0;
    max-width: 480px;
    font-size: 12px;
    color: var(--danger);
    text-align: center;
  }

  .controls {
    flex: none;
    align-self: center;
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 40px;
    padding: 6px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--surface);
  }

  .btn.active {
    border-color: var(--accent-line);
    color: var(--accent);
  }

  .seg {
    display: flex;
    height: 32px;
    padding: 2px;
    border: 1px solid var(--line-2);
    border-radius: var(--r-sm);
    background: var(--bg);
  }

  .seg label {
    display: flex;
    align-items: center;
    padding: 0 10px;
    border-radius: 4px;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text-2);
    cursor: pointer;
    transition: background-color 120ms, color 120ms;
  }

  .seg label:hover {
    color: var(--text);
  }

  .seg label.on {
    background: var(--surface-3);
    color: var(--accent);
  }

  .seg label:has(input:focus-visible) {
    box-shadow: var(--focus);
  }

  .readout {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 4px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .readout span:first-child {
    min-width: 6ch;
    text-align: right;
  }

  .sep {
    width: 1px;
    height: 12px;
    background: var(--line-2);
  }
</style>
