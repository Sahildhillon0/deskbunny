<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import * as api from "../api";
  import {
    H,
    W,
    blankFrame,
    constrain,
    ellipsePoints,
    flipH,
    flipV,
    floodFill,
    frameFromB64,
    frameToB64,
    getPx,
    imageLuma,
    invertFrame,
    isBlank,
    linePoints,
    lumaToFrame,
    plot,
    rectPoints,
    setPx,
    shift,
    stamp,
    textPoints,
    textWidth,
    type Frame,
    type Point,
  } from "../drawing";
  import { OledRenderer } from "../oled";
  import { app } from "../state.svelte";
  import { DRAW_FRAMES, DRAW_MODES, FRAME_MS_MIN, type DrawMode, type Drawing, type FaceDef } from "../types";
  import Icon, { type IconName } from "./Icon.svelte";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";

  type Tool = "pen" | "eraser" | "line" | "rect" | "ellipse" | "fill" | "text" | "move";

  const TOOLS: { id: Tool; icon: IconName; label: string; key: string }[] = [
    { id: "pen", icon: "pencil", label: "Pen", key: "b" },
    { id: "eraser", icon: "eraser", label: "Eraser", key: "e" },
    { id: "line", icon: "line", label: "Line", key: "l" },
    { id: "rect", icon: "rect", label: "Rectangle", key: "r" },
    { id: "ellipse", icon: "ellipse", label: "Ellipse", key: "o" },
    { id: "fill", icon: "fill", label: "Fill", key: "f" },
    { id: "text", icon: "text", label: "Text", key: "t" },
    { id: "move", icon: "move", label: "Move", key: "m" },
  ];

  const TOOL_HINT: Record<Tool, string> = {
    pen: "Left button draws, right button erases.",
    eraser: "Drag to erase.",
    line: "Drag to draw a line. Shift snaps to 45°. Right button erases.",
    rect: "Drag to draw a box. Shift makes a square. Right button erases.",
    ellipse: "Drag to draw an ellipse. Shift makes a circle. Right button erases.",
    fill: "Click an area to fill it (or clear it, if it is lit).",
    text: "Type below, then click where the text should go. Right button erases.",
    move: "Drag to move the whole frame. Pixels pushed off the edge are lost.",
  };

  const MODE_LABEL: Record<DrawMode, string> = { replace: "Drawing only", over: "Over the face" };

  // ---------------------------------------------------------------- working copy

  const initial: Drawing = app.face.drawing
    ? $state.snapshot(app.face.drawing)
    : { mode: "replace", frameMs: 500, frames: [frameToB64(blankFrame())] };
  const initialJson = JSON.stringify(initial);

  // Frames are typed arrays (not deeply reactive): edits bump `rev`.
  let frames = $state.raw<Frame[]>(initial.frames.map(frameFromB64));
  let rev = $state(0);
  let cur = $state(0);
  let mode = $state<DrawMode>(initial.mode);
  let frameMs = $state(initial.frameMs);

  let tool = $state<Tool>("pen");
  let size = $state(1);
  let filled = $state(false);
  let text = $state("HELLO");
  let textScale = $state(2);
  let onion = $state(true);
  let showFace = $state(true);
  let confirmDiscard = $state(false);

  const drawing = (): Drawing => ({ mode, frameMs, frames: frames.map(frameToB64) });

  // ---------------------------------------------------------------- history

  type Snapshot = { frames: Frame[]; cur: number };
  const MAX_HISTORY = 100;
  let undoStack = $state.raw<Snapshot[]>([]);
  let redoStack = $state.raw<Snapshot[]>([]);

  const snapshot = (): Snapshot => ({ frames: frames.map((f) => f.slice()), cur });

  /** Call before every change. */
  function remember() {
    importing = null;
    undoStack = [...undoStack.slice(-(MAX_HISTORY - 1)), snapshot()];
    redoStack = [];
  }

  function restore(s: Snapshot) {
    frames = s.frames;
    cur = Math.min(s.cur, frames.length - 1);
    changed();
  }

  function undo() {
    const s = undoStack.at(-1);
    if (!s) return;
    redoStack = [...redoStack, snapshot()];
    undoStack = undoStack.slice(0, -1);
    restore(s);
  }

  function redo() {
    const s = redoStack.at(-1);
    if (!s) return;
    undoStack = [...undoStack, snapshot()];
    redoStack = redoStack.slice(0, -1);
    restore(s);
  }

  function changed() {
    rev++;
    paint();
  }

  /** Replace the current frame with f(current), as one undo step. */
  function edit(f: (fr: Frame) => Frame) {
    remember();
    frames = frames.map((fr, i) => (i === cur ? f(fr) : fr));
    changed();
  }

  // ---------------------------------------------------------------- frames

  function addFrame(copy: boolean) {
    if (frames.length >= DRAW_FRAMES) return;
    remember();
    const next = copy ? frames[cur].slice() : blankFrame();
    frames = [...frames.slice(0, cur + 1), next, ...frames.slice(cur + 1)];
    cur++;
    changed();
  }

  function deleteFrame() {
    if (frames.length <= 1) return;
    remember();
    frames = frames.filter((_, i) => i !== cur);
    cur = Math.min(cur, frames.length - 1);
    changed();
  }

  function selectFrame(i: number) {
    importing = null;
    cur = i;
    paint();
  }

  // ---------------------------------------------------------------- canvas

  let wrap: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D;
  const src = document.createElement("canvas");
  src.width = W;
  src.height = H;
  const srcCtx = src.getContext("2d")!;
  const img = srcCtx.createImageData(W, H);
  let scale = $state(6);

  /** Upright frame of the face alone, drawn faintly under an "over" drawing. */
  let faceFrame: Uint8Array | null = null;
  /** Pixels a drag (or a click of the text tool) would draw. */
  let preview: { pts: Point[]; on: boolean } | null = null;
  let hover = $state<Point | null>(null);

  const LIT = [223, 246, 255];
  const PREVIEW_ON = [140, 220, 255];
  const PREVIEW_OFF = [120, 40, 40];
  const FACE = [36, 52, 58];
  const ONION = [26, 40, 46];
  const OFF = [2, 3, 4];

  function relayout() {
    if (!wrap || !canvas) return;
    const s = Math.floor(Math.min((wrap.clientWidth - 16) / W, (wrap.clientHeight - 16) / H));
    scale = Math.max(2, Math.min(9, s));
    const dpr = window.devicePixelRatio || 1;
    canvas.style.width = `${W * scale}px`;
    canvas.style.height = `${H * scale}px`;
    canvas.width = Math.round(W * scale * dpr);
    canvas.height = Math.round(H * scale * dpr);
    paint();
  }

  // Coalesce repaints to one per display frame (pointer events can outrun it).
  let paintQueued = false;
  function paint() {
    if (paintQueued) return;
    paintQueued = true;
    requestAnimationFrame(() => {
      paintQueued = false;
      paintNow();
    });
  }

  function paintNow() {
    if (!ctx) return;
    const frame = frames[cur];
    const under = mode === "over" && showFace ? faceFrame : null;
    const prev = onion && frames.length > 1 ? frames[(cur + frames.length - 1) % frames.length] : null;
    const shape = new Map<number, boolean>();
    if (preview) for (const [x, y] of preview.pts) if (x >= 0 && x < W && y >= 0 && y < H) shape.set(y * W + x, preview.on);
    const d = img.data;
    for (let y = 0; y < H; y++)
      for (let x = 0; x < W; x++) {
        const i = y * W + x;
        const s = shape.get(i);
        let c = OFF;
        if (s !== undefined) c = s ? PREVIEW_ON : PREVIEW_OFF;
        else if (getPx(frame, x, y)) c = LIT;
        else if (under && getPx(under, x, y)) c = FACE;
        else if (prev && getPx(prev, x, y)) c = ONION;
        d[i * 4] = c[0];
        d[i * 4 + 1] = c[1];
        d[i * 4 + 2] = c[2];
        d[i * 4 + 3] = 255;
      }
    srcCtx.putImageData(img, 0, 0);
    const { width: bw, height: bh } = canvas;
    const px = bw / W;
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(src, 0, 0, bw, bh);
    if (px >= 4) {
      // Pixel grid, a little stronger every 8 rows (the SSD1306 pages) and columns.
      for (let x = 1; x < W; x++) {
        ctx.fillStyle = x % 8 ? "rgba(255,255,255,0.035)" : "rgba(255,255,255,0.08)";
        ctx.fillRect(Math.round(x * px), 0, 1, bh);
      }
      for (let y = 1; y < H; y++) {
        ctx.fillStyle = y % 8 ? "rgba(255,255,255,0.035)" : "rgba(255,255,255,0.08)";
        ctx.fillRect(0, Math.round(y * px), bw, 1);
      }
    }
    if (hover && !drag && (tool === "pen" || tool === "eraser")) {
      const o = Math.floor((size - 1) / 2);
      ctx.strokeStyle = "rgba(223,246,255,0.6)";
      ctx.lineWidth = Math.max(1, Math.round(px / 6));
      ctx.strokeRect((hover[0] - o) * px + 0.5, (hover[1] - o) * px + 0.5, size * px - 1, size * px - 1);
    }
  }

  // ---------------------------------------------------------------- pointer

  let drag: { start: Point; last: Point; on: boolean; base?: Frame } | null = null;

  function pointAt(e: PointerEvent): Point {
    const r = canvas.getBoundingClientRect();
    return [Math.floor(((e.clientX - r.left) / r.width) * W), Math.floor(((e.clientY - r.top) / r.height) * H)];
  }

  function shapePoints(a: Point, b: Point, shiftKey: boolean): Point[] {
    if (tool === "line") return linePoints(a, shiftKey ? constrain(a, b, false) : b);
    const end = shiftKey ? constrain(a, b, true) : b;
    return tool === "rect" ? rectPoints(a, end, filled) : ellipsePoints(a, end, filled);
  }

  /** Brush footprint of a shape's points, so previews match what gets drawn. */
  function brushed(pts: Point[]): Point[] {
    if (size === 1) return pts;
    const f = blankFrame();
    plot(f, pts, size, true);
    const out: Point[] = [];
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) if (getPx(f, x, y)) out.push([x, y]);
    return out;
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 && e.button !== 2) return;
    e.preventDefault();
    canvas.setPointerCapture(e.pointerId);
    const p = pointAt(e);
    const on = e.button === 0 && tool !== "eraser";
    if (tool === "fill") {
      edit((f) => {
        const out = f.slice();
        floodFill(out, p[0], p[1]);
        return out;
      });
      return;
    }
    if (tool === "text") {
      if (!text.trim()) return;
      edit((f) => {
        const out = f.slice();
        for (const [x, y] of textPoints(text, p[0], p[1], textScale)) setPx(out, x, y, on);
        return out;
      });
      return;
    }
    remember();
    frames = frames.map((f, i) => (i === cur ? f.slice() : f));
    drag = { start: p, last: p, on, base: tool === "move" ? frames[cur].slice() : undefined };
    if (tool === "pen" || tool === "eraser") {
      stamp(frames[cur], p[0], p[1], size, on);
      changed();
    } else {
      onPointerMove(e);
    }
  }

  function onPointerMove(e: PointerEvent) {
    const p = pointAt(e);
    if (!drag) {
      hover = p;
      preview = tool === "text" && text.trim() ? { pts: textPoints(text, p[0], p[1], textScale), on: true } : null;
      paint();
      return;
    }
    if (tool === "pen" || tool === "eraser") {
      plot(frames[cur], linePoints(drag.last, p), size, drag.on);
      drag.last = p;
      changed();
    } else if (tool === "move" && drag.base) {
      frames[cur].set(shift(drag.base, p[0] - drag.start[0], p[1] - drag.start[1]));
      changed();
    } else {
      preview = { pts: brushed(shapePoints(drag.start, p, e.shiftKey)), on: drag.on };
      paint();
    }
  }

  function onPointerUp(e: PointerEvent) {
    if (!drag) return;
    if (tool === "line" || tool === "rect" || tool === "ellipse") {
      plot(frames[cur], shapePoints(drag.start, pointAt(e), e.shiftKey), size, drag.on);
      preview = null;
    }
    drag = null;
    changed();
  }

  function onPointerLeave() {
    hover = null;
    if (!drag) preview = null;
    paint();
  }

  // ---------------------------------------------------------------- image import

  let fileInput: HTMLInputElement;
  /** A picture being imported: it is already in frame `frame`; the options re-convert it live. */
  let importing = $state<{ luma: Float32Array; name: string; frame: number } | null>(null);
  let threshold = $state(128);
  let dither = $state(true);
  let invertImg = $state(false);
  let importError = $state("");

  async function onFile(e: Event & { currentTarget: HTMLInputElement }) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = "";
    if (!file) return;
    importError = "";
    const url = URL.createObjectURL(file);
    try {
      const im = new Image();
      im.src = url;
      await im.decode();
      const luma = imageLuma(im);
      remember();
      importing = { luma, name: file.name, frame: cur };
    } catch {
      importError = `Could not read ${file.name} as an image. Try a PNG or JPEG.`;
    } finally {
      URL.revokeObjectURL(url);
    }
  }

  $effect(() => {
    if (!importing) return;
    const out = lumaToFrame(importing.luma, { threshold, dither, invert: invertImg });
    const at = importing.frame;
    untrack(() => {
      frames = frames.map((f, i) => (i === at ? out : f));
      changed();
    });
  });

  /** Keep the picture as it is now. */
  function keepImport() {
    importing = null;
  }

  /** Take the picture out again. */
  function cancelImport() {
    importing = null;
    undo();
  }

  // ---------------------------------------------------------------- previews (face under, live result)

  let previewCanvas: HTMLCanvasElement;
  let liveSim: number | null = null;
  let faceSim: number | null = null;

  const plainFace = (): FaceDef => {
    const f = $state.snapshot(app.face);
    delete f.drawing;
    return f;
  };

  async function refreshFaceFrame() {
    if (faceSim === null) return;
    // Upright and unlit-background, like the firmware composites it.
    await api.simSetFace(faceSim, { ...plainFace(), invert: false, rotate180: false });
    faceFrame = await api.simFrame(faceSim, 0);
    paint();
  }

  // Keep the live preview on the working copy (debounced: frames are 1.4 KB of base64 each).
  let pushTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    void rev;
    void mode;
    void frameMs;
    clearTimeout(pushTimer);
    pushTimer = setTimeout(() => {
      if (liveSim !== null) api.simSetFace(liveSim, { ...plainFace(), drawing: drawing() }).catch(() => {});
    }, 150);
  });

  $effect(() => {
    void mode;
    void showFace;
    void onion;
    paint();
  });

  // ---------------------------------------------------------------- keyboard and closing

  function onKey(e: KeyboardEvent) {
    const typing = e.target instanceof HTMLInputElement && e.target.type === "text";
    if (e.key === "Escape") {
      e.preventDefault();
      if (importing) cancelImport();
      else close();
      return;
    }
    if (typing) return;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "z") {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    } else if (mod && e.key.toLowerCase() === "y") {
      e.preventDefault();
      redo();
    } else if (!mod && !e.altKey) {
      const t = TOOLS.find((t) => t.key === e.key.toLowerCase());
      if (t) tool = t.id;
      else if (e.key >= "1" && e.key <= "4") size = Number(e.key);
    }
  }

  function close() {
    if (JSON.stringify(drawing()) !== initialJson && !confirmDiscard) {
      confirmDiscard = true;
      setTimeout(() => (confirmDiscard = false), 3000);
      return;
    }
    app.drawOpen = false;
  }

  function done() {
    app.face.drawing = drawing();
    app.drawOpen = false;
  }

  const allBlank = $derived.by(() => (void rev, frames.every(isBlank)));
  const curBlank = $derived.by(() => (void rev, isBlank(frames[cur])));
  const secs = (v: number) => `${(v / 1000).toFixed(v < 1000 ? 2 : 1)} s`;

  // ---------------------------------------------------------------- lifecycle

  /** Draws a frame into a small thumbnail canvas whenever it (or rev) changes. */
  function thumb(node: HTMLCanvasElement, arg: { frame: Frame; rev: number }) {
    const c = node.getContext("2d")!;
    const im = c.createImageData(W, H);
    const draw = ({ frame }: { frame: Frame; rev: number }) => {
      for (let y = 0; y < H; y++)
        for (let x = 0; x < W; x++) {
          const o = (y * W + x) * 4;
          const on = getPx(frame, x, y);
          im.data[o] = on ? LIT[0] : 6;
          im.data[o + 1] = on ? LIT[1] : 8;
          im.data[o + 2] = on ? LIT[2] : 9;
          im.data[o + 3] = 255;
        }
      c.putImageData(im, 0, 0);
    };
    draw(arg);
    return { update: draw };
  }

  onMount(() => {
    ctx = canvas.getContext("2d", { alpha: false })!;
    relayout();
    const ro = new ResizeObserver(relayout);
    ro.observe(wrap);

    const renderer = new OledRenderer(previewCanvas, { glow: 0.6, grid: false });
    renderer.resize(2);
    renderer.setContrast(app.face.contrast);
    let dead = false;
    let raf = 0;
    let busy = false;
    let last = 0;
    const t0 = performance.now();
    Promise.all([api.simCreate({ ...plainFace(), drawing: drawing() }, 0), api.simCreate(plainFace(), 0)])
      .then(([live, face]) => {
        if (dead) {
          api.simDrop(live);
          api.simDrop(face);
          return;
        }
        liveSim = live;
        faceSim = face;
        refreshFaceFrame();
      })
      .catch(() => {});
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      if (liveSim === null || busy || now - last < 1000 / 20) return;
      last = now;
      busy = true;
      api
        .simFrame(liveSim, now - t0)
        .then((fb) => renderer.draw(fb))
        .catch(() => {})
        .finally(() => (busy = false));
    };
    raf = requestAnimationFrame(loop);
    tick().then(() => canvas.focus());

    return () => {
      dead = true;
      ro.disconnect();
      cancelAnimationFrame(raf);
      clearTimeout(pushTimer);
      if (liveSim !== null) api.simDrop(liveSim);
      if (faceSim !== null) api.simDrop(faceSim);
    };
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="draw-title">
    <header class="top">
      <h1 id="draw-title">Draw on the screen</h1>
      <div class="modes" role="radiogroup" aria-label="How the drawing is shown">
        {#each DRAW_MODES as m (m)}
          <label class="chip" class:on={mode === m}>
            <input type="radio" name="draw-mode" value={m} bind:group={mode} class="sr-only" />
            {MODE_LABEL[m]}
          </label>
        {/each}
      </div>
      <span class="spacer"></span>
      <button class="btn ghost icon" aria-label="Close" onclick={close}><Icon name="close" /></button>
    </header>

    <div class="body">
      <div class="tools" role="toolbar" aria-label="Drawing tools" aria-orientation="vertical">
        {#each TOOLS as t (t.id)}
          <button
            class="tool"
            class:on={tool === t.id}
            aria-pressed={tool === t.id}
            title={`${t.label} (${t.key.toUpperCase()})`}
            aria-label={t.label}
            onclick={() => (tool = t.id)}
          >
            <Icon name={t.icon} />
          </button>
        {/each}
        <span class="sep"></span>
        {#each [1, 2, 3, 4] as s (s)}
          <button
            class="tool size"
            class:on={size === s}
            aria-pressed={size === s}
            title={`Brush ${s} px (${s})`}
            aria-label={`Brush ${s} pixels`}
            onclick={() => (size = s)}
          >
            <span style:width={`${2 + s * 2}px`} style:height={`${2 + s * 2}px`}></span>
          </button>
        {/each}
        {#if tool === "rect" || tool === "ellipse"}
          <span class="sep"></span>
          <button class="tool" class:on={filled} aria-pressed={filled} title="Filled shapes" aria-label="Filled shapes" onclick={() => (filled = !filled)}>
            <Icon name="fill" />
          </button>
        {/if}
      </div>

      <div class="center">
        <div class="wrap" bind:this={wrap}>
          <canvas
            bind:this={canvas}
            class:crosshair={tool !== "move"}
            tabindex="-1"
            aria-label="Drawing canvas, 128 by 64 pixels"
            onpointerdown={onPointerDown}
            onpointermove={onPointerMove}
            onpointerup={onPointerUp}
            onpointerleave={onPointerLeave}
            oncontextmenu={(e) => e.preventDefault()}
          ></canvas>
        </div>
        <p class="status">
          <span class="coords">{hover ? `x ${hover[0]}  y ${hover[1]}` : "128 × 64"}</span>
          <span>{importing ? "Adjust the picture on the right. Drawing on it keeps it." : TOOL_HINT[tool]}</span>
        </p>
        {#if tool === "text"}
          <div class="text-row">
            <input class="text-input" type="text" bind:value={text} maxlength="32" spellcheck="false" placeholder="Your text" aria-label="Text to place" />
            <div class="seg" role="radiogroup" aria-label="Text size">
              {#each [1, 2, 3, 4] as s (s)}
                <label class="chip small" class:on={textScale === s}>
                  <input type="radio" name="text-scale" value={s} bind:group={textScale} class="sr-only" />
                  {s}x
                </label>
              {/each}
            </div>
            <span class="muted">{textWidth(text, textScale)} px wide · A-Z, 0-9 and punctuation</span>
          </div>
        {/if}
      </div>

      <aside class="side">
        <section>
          <h2 class="label">Preview</h2>
          <canvas class="live" bind:this={previewCanvas} aria-label="Live preview of the screen"></canvas>
        </section>

        <section>
          <h2 class="label">Frames</h2>
          <div class="frames">
            {#each frames as f, i (i)}
              <button class="frame" class:on={i === cur} aria-pressed={i === cur} aria-label={`Frame ${i + 1}`} onclick={() => selectFrame(i)}>
                <canvas width={W} height={H} use:thumb={{ frame: f, rev: i === cur ? rev : 0 }}></canvas>
                <span>{i + 1}</span>
              </button>
            {/each}
          </div>
          <div class="row">
            <button class="btn small" onclick={() => addFrame(false)} disabled={frames.length >= DRAW_FRAMES} title="Add a blank frame">
              <Icon name="plus" /> New
            </button>
            <button class="btn small" onclick={() => addFrame(true)} disabled={frames.length >= DRAW_FRAMES} title="Copy this frame">
              <Icon name="duplicate" /> Copy
            </button>
            <button class="btn small ghost" onclick={deleteFrame} disabled={frames.length <= 1} aria-label="Delete this frame" title="Delete this frame">
              <Icon name="trash" />
            </button>
          </div>
          {#if frames.length > 1}
            <div class="opts">
              <Slider label="Each frame" bind:value={frameMs} min={FRAME_MS_MIN} max={2000} step={50} format={secs} />
              <Switch label="Show previous frame" bind:checked={onion} hint="Onion skin: the previous frame, faintly" />
            </div>
          {:else}
            <p class="muted">Add frames for a short animation (up to {DRAW_FRAMES}).</p>
          {/if}
          {#if mode === "over"}
            <div class="opts"><Switch label="Show the face" bind:checked={showFace} hint="Draw around the face; it is not part of the drawing" /></div>
          {/if}
        </section>

        <section>
          <h2 class="label">Edit</h2>
          <div class="grid4">
            <button class="btn small icon" onclick={undo} disabled={!undoStack.length} title="Undo (Ctrl+Z)" aria-label="Undo"><Icon name="undo" /></button>
            <button class="btn small icon" onclick={redo} disabled={!redoStack.length} title="Redo (Ctrl+Shift+Z)" aria-label="Redo"><Icon name="redo" /></button>
            <button class="btn small icon" onclick={() => edit(flipH)} title="Flip left-right" aria-label="Flip left-right"><Icon name="flipH" /></button>
            <button class="btn small icon" onclick={() => edit(flipV)} title="Flip upside down" aria-label="Flip upside down"><Icon name="flipV" /></button>
            <button class="btn small icon" onclick={() => edit((f) => shift(f, -1, 0))} title="Nudge left" aria-label="Nudge left"><Icon name="arrowLeft" /></button>
            <button class="btn small icon" onclick={() => edit((f) => shift(f, 1, 0))} title="Nudge right" aria-label="Nudge right"><Icon name="arrowRight" /></button>
            <button class="btn small icon" onclick={() => edit((f) => shift(f, 0, -1))} title="Nudge up" aria-label="Nudge up"><Icon name="arrowUp" /></button>
            <button class="btn small icon" onclick={() => edit((f) => shift(f, 0, 1))} title="Nudge down" aria-label="Nudge down"><Icon name="arrowDown" /></button>
          </div>
          <div class="row">
            <button class="btn small" onclick={() => edit(invertFrame)} title="Swap lit and dark pixels"><Icon name="invert" /> Invert</button>
            <button class="btn small" onclick={() => edit(() => blankFrame())} disabled={curBlank}><Icon name="trash" /> Clear</button>
          </div>
        </section>

        <section>
          <h2 class="label">Picture</h2>
          <input bind:this={fileInput} type="file" accept="image/*" class="sr-only" tabindex="-1" onchange={onFile} />
          {#if importing}
            <p class="muted file">{importing.name}</p>
            <div class="opts">
              <Slider label="Brightness cut" bind:value={threshold} min={1} max={254} />
              <Switch label="Dither" bind:checked={dither} hint="Dots for shades of grey" />
              <Switch label="Invert" bind:checked={invertImg} />
            </div>
            <div class="row">
              <button class="btn small primary" onclick={keepImport}><Icon name="check" /> Keep</button>
              <button class="btn small ghost" onclick={cancelImport}>Remove picture</button>
            </div>
          {:else}
            <button class="btn small" onclick={() => fileInput.click()}><Icon name="image" /> Import an image…</button>
            {#if importError}<p class="error" role="alert">{importError}</p>{/if}
          {/if}
        </section>
      </aside>
    </div>

    <footer class="actions">
      <span class="muted">
        {#if allBlank}Nothing drawn yet: the screen will be {mode === "replace" ? "dark" : "just the face"}.{:else}{frames.length}
          {frames.length === 1 ? "frame" : "frames"}{frames.length > 1 ? `, ${secs(frameMs)} each` : ""}. Flash it like any face.{/if}
      </span>
      <span class="spacer"></span>
      <button class="btn ghost" class:confirm={confirmDiscard} onclick={close}>{confirmDiscard ? "Discard changes?" : "Cancel"}</button>
      <button class="btn primary" onclick={done}><Icon name="check" /> Done</button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: 20px;
    /* Opaque, no backdrop blur: re-blurring the window on every pointer move is slow in WebKitGTK. */
    background: rgba(0, 0, 0, 0.9);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(1240px, 100%);
    height: min(820px, 100%);
    border: 1px solid var(--line-2);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: 0 24px 80px -20px rgba(0, 0, 0, 0.9);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 12px 10px 20px;
    border-bottom: 1px solid var(--line);
  }

  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .modes,
  .seg {
    display: flex;
    gap: 6px;
  }

  .chip {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-size: 12px;
    color: var(--text-2);
    cursor: pointer;
    white-space: nowrap;
  }

  .chip.small {
    height: 28px;
    padding: 0 9px;
  }

  .chip:hover {
    border-color: var(--line-2);
    color: var(--text);
  }

  .chip.on {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--accent);
  }

  .chip:has(input:focus-visible) {
    box-shadow: var(--focus);
  }

  .spacer {
    flex: 1;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 56px minmax(0, 1fr) 292px;
  }

  .tools {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 12px 0;
    border-right: 1px solid var(--line);
    overflow-y: auto;
  }

  .tool {
    display: grid;
    place-items: center;
    flex: none;
    width: 36px;
    height: 36px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: none;
    color: var(--text-2);
  }

  .tool:hover {
    background: var(--surface-3);
    color: var(--text);
  }

  .tool.on {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--accent);
  }

  .tool.size span {
    display: block;
    background: currentColor;
    border-radius: 1px;
  }

  .sep {
    flex: none;
    width: 24px;
    height: 1px;
    margin: 6px 0;
    background: var(--line-2);
  }

  .center {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 16px 20px 12px;
    gap: 10px;
  }

  .wrap {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
  }

  canvas {
    display: block;
  }

  .wrap canvas {
    border-radius: 4px;
    box-shadow: 0 0 0 1px var(--line-2), 0 0 0 6px #050708;
    touch-action: none;
    cursor: grab;
  }

  .wrap canvas.crosshair {
    cursor: crosshair;
  }

  .status {
    display: flex;
    gap: 16px;
    margin: 0;
    font-size: 12px;
    color: var(--text-3);
  }

  .coords {
    flex: none;
    width: 11ch;
    font-family: var(--mono);
    white-space: pre;
  }

  .text-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .text-input {
    width: 220px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--line-2);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-family: var(--mono);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .text-input:focus-visible {
    border-color: var(--accent-line);
  }

  .side {
    min-height: 0;
    overflow-y: auto;
    padding: 4px 18px 16px;
    border-left: 1px solid var(--line);
  }

  section {
    padding: 14px 0;
  }

  section + section {
    border-top: 1px solid var(--line);
  }

  h2 {
    margin: 0 0 10px;
  }

  .live {
    border-radius: 4px;
    box-shadow: 0 0 0 1px var(--line-2);
  }

  .frames {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-bottom: 10px;
  }

  .frame {
    position: relative;
    padding: 3px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
  }

  .frame:hover {
    border-color: var(--line-2);
  }

  .frame.on {
    border-color: var(--accent-line);
  }

  .frame canvas {
    width: 100%;
    height: auto;
    image-rendering: pixelated;
    border-radius: 2px;
  }

  .frame span {
    position: absolute;
    top: 5px;
    left: 7px;
    font-family: var(--mono);
    font-size: 10px;
    color: var(--text-3);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }

  .grid4 {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
  }

  .opts {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 12px;
  }

  .btn.small {
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
    gap: 6px;
  }

  .btn.small.icon {
    padding: 0;
  }

  .muted {
    margin: 0;
    font-size: 12px;
    color: var(--text-3);
  }

  .file {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    margin: 8px 0 0;
    font-size: 12px;
    color: var(--danger);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px 12px 20px;
    border-top: 1px solid var(--line);
  }

  .btn.primary {
    border-color: var(--accent-line);
    color: var(--accent);
  }

  .btn.primary:hover:not(:disabled) {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  .btn.confirm {
    color: var(--danger);
    border-color: rgba(242, 147, 138, 0.45);
    background: var(--danger-soft);
  }
</style>
