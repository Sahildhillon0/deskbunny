<script lang="ts">
  // Screen designer: place text (with {TOKENS}), boxes, lines, bars, an analog
  // clock, icons and hand-drawn pixels anywhere on the 128x64 panel. The preview
  // is rendered by the same Rust code that draws the board's frames.
  import { onMount, tick } from "svelte";
  import * as api from "../api";
  import { OledRenderer } from "../oled";
  import { app } from "../state.svelte";
  import type { Design, DesignCatalog, Element, ElementType } from "../types";
  import Icon from "./Icon.svelte";

  // 4x where the window has room for it next to both side panels.
  const SCALE = typeof window !== "undefined" && window.innerWidth < 1180 ? 3 : 4;

  // The design being edited: a working copy; nothing is saved until Save.
  let design = $state<Design>(structuredClone($state.snapshot(app.designing) as Design));
  let isNew = $state(false);
  let selected = $state<number | null>(null);
  let bounds = $state<[number, number, number, number][]>([]);
  let catalog = $state<DesignCatalog | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);
  let paint = $state<"off" | "draw" | "erase">("off");
  let canvas = $state<HTMLCanvasElement | null>(null);
  let heading = $state<HTMLInputElement | null>(null);
  let textInput = $state<HTMLInputElement | null>(null);
  let renderer: OledRenderer | null = null;
  let dirty = $state(false);

  const LABELS: Record<ElementType, string> = {
    text: "Text",
    rect: "Box",
    line: "Line",
    bar: "Bar",
    analog: "Analog",
    icon: "Icon",
    bitmap: "Drawing",
  };

  const el = $derived(selected === null ? null : (design.elements[selected] ?? null));

  function fresh(type: ElementType): Element {
    switch (type) {
      case "text":
        return { type, x: 64, y: 28, text: "{HH}:{MM}", font: "small", size: 2, align: "center", erase: false };
      case "rect":
        return { type, x: 10, y: 10, w: 40, h: 20, fill: false, erase: false };
      case "line":
        return { type, x1: 0, y1: 32, x2: 127, y2: 32, thick: 1 };
      case "bar":
        return { type, x: 4, y: 56, w: 120, h: 5, value: "{SS}", max: "59" };
      case "analog":
        return { type, cx: 32, cy: 32, r: 28, seconds: true };
      case "icon":
        return { type, x: 56, y: 24, icon: "{WICON}", size: 1 };
      case "bitmap":
        return { type, x: 48, y: 16, w: 32, h: 32, data: encodeBits(new Uint8Array(32 * 32), 32, 32) };
    }
  }

  // --- bitmap bits (row-major, MSB first, base64) -------------------------
  function decodeBits(data: string, w: number, h: number): Uint8Array {
    const out = new Uint8Array(w * h);
    let bytes: Uint8Array;
    try {
      bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
    } catch {
      return out;
    }
    for (let i = 0; i < w * h; i++) out[i] = (bytes[i >> 3] >> (7 - (i & 7))) & 1;
    return out;
  }

  function encodeBits(px: Uint8Array, w: number, h: number): string {
    const bytes = new Uint8Array(Math.ceil((w * h) / 8));
    for (let i = 0; i < w * h; i++) if (px[i]) bytes[i >> 3] |= 0x80 >> (i & 7);
    let s = "";
    for (const b of bytes) s += String.fromCharCode(b);
    return btoa(s);
  }

  /** Resize a drawing, keeping the pixels that still fit. */
  function resizeBitmap(e: Extract<Element, { type: "bitmap" }>, w: number, h: number) {
    w = Math.max(1, Math.min(128, Math.round(w)));
    h = Math.max(1, Math.min(64, Math.round(h)));
    const old = decodeBits(e.data, e.w, e.h);
    const px = new Uint8Array(w * h);
    for (let y = 0; y < Math.min(h, e.h); y++) for (let x = 0; x < Math.min(w, e.w); x++) px[y * w + x] = old[y * e.w + x];
    e.w = w;
    e.h = h;
    e.data = encodeBits(px, w, h);
  }

  // --- render -------------------------------------------------------------
  let pending = false;
  let again = false;
  async function redraw() {
    if (pending) {
      again = true;
      return;
    }
    pending = true;
    try {
      const r = await api.designRender($state.snapshot(design) as Design);
      bounds = r.bounds;
      if (canvas && !renderer) {
        renderer = new OledRenderer(canvas, { glow: 0.5, grid: true });
        renderer.resize(SCALE);
      }
      renderer?.draw(Uint8Array.from(r.frame));
    } catch (e) {
      error = api.errorText(e);
    } finally {
      pending = false;
      if (again) {
        again = false;
        redraw();
      }
    }
  }

  $effect(() => {
    // Track every field of the design.
    JSON.stringify(design);
    redraw();
  });

  // --- editing ------------------------------------------------------------
  function changed() {
    dirty = true;
  }

  function add(type: ElementType) {
    design.elements.push(fresh(type));
    selected = design.elements.length - 1;
    paint = type === "bitmap" ? "draw" : "off";
    changed();
  }

  function remove(i: number) {
    design.elements.splice(i, 1);
    selected = null;
    paint = "off";
    changed();
  }

  function move(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= design.elements.length) return;
    [design.elements[i], design.elements[j]] = [design.elements[j], design.elements[i]];
    selected = j;
    changed();
  }

  function insertToken(t: string) {
    if (!el || el.type !== "text") return;
    const input = textInput;
    const at = input?.selectionStart ?? el.text.length;
    el.text = el.text.slice(0, at) + `{${t}}` + el.text.slice(input?.selectionEnd ?? at);
    changed();
    tick().then(() => {
      input?.focus();
      const p = at + t.length + 2;
      input?.setSelectionRange(p, p);
    });
  }

  function label(e: Element): string {
    switch (e.type) {
      case "text":
        return e.text || "(empty)";
      case "icon":
        return `${LABELS.icon}: ${e.icon}`;
      case "bar":
        return `${LABELS.bar}: ${e.value} / ${e.max}`;
      default:
        return LABELS[e.type];
    }
  }

  /** Shift an element by (dx, dy) from a starting copy. */
  function placed(start: Element, dx: number, dy: number): Element {
    const e = structuredClone(start);
    if (e.type === "line") {
      e.x1 += dx;
      e.x2 += dx;
      e.y1 += dy;
      e.y2 += dy;
    } else if (e.type === "analog") {
      e.cx += dx;
      e.cy += dy;
    } else {
      e.x += dx;
      e.y += dy;
    }
    return e;
  }

  // --- pointer: select, drag, paint ----------------------------------------
  let drag: { i: number; px: number; py: number; start: Element } | null = null;

  function panelPoint(ev: PointerEvent): [number, number] {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    return [Math.floor(((ev.clientX - r.left) / r.width) * 128), Math.floor(((ev.clientY - r.top) / r.height) * 64)];
  }

  function hit(x: number, y: number): number | null {
    for (let i = bounds.length - 1; i >= 0; i--) {
      const [bx, by, bw, bh] = bounds[i];
      if (x >= bx - 1 && x <= bx + bw && y >= by - 1 && y <= by + bh) return i;
    }
    return null;
  }

  function paintAt(x: number, y: number) {
    if (!el || el.type !== "bitmap" || paint === "off") return false;
    const lx = x - el.x;
    const ly = y - el.y;
    if (lx < 0 || ly < 0 || lx >= el.w || ly >= el.h) return false;
    const px = decodeBits(el.data, el.w, el.h);
    const v = paint === "draw" ? 1 : 0;
    if (px[ly * el.w + lx] === v) return true;
    px[ly * el.w + lx] = v;
    el.data = encodeBits(px, el.w, el.h);
    changed();
    return true;
  }

  let painting = false;

  function down(ev: PointerEvent) {
    const [x, y] = panelPoint(ev);
    (ev.currentTarget as HTMLElement).setPointerCapture(ev.pointerId);
    if (paintAt(x, y)) {
      painting = true;
      return;
    }
    const i = hit(x, y);
    selected = i;
    if (i === null || design.elements[i].type !== "bitmap") paint = "off";
    if (i !== null) drag = { i, px: x, py: y, start: $state.snapshot(design.elements[i]) as Element };
  }

  function moveEv(ev: PointerEvent) {
    const [x, y] = panelPoint(ev);
    if (painting) {
      paintAt(x, y);
      return;
    }
    if (!drag) return;
    const dx = x - drag.px;
    const dy = y - drag.py;
    if (dx === 0 && dy === 0) return;
    design.elements[drag.i] = placed(drag.start, dx, dy);
    changed();
  }

  function up() {
    drag = null;
    painting = false;
  }

  function onKey(ev: KeyboardEvent) {
    const target = ev.target as HTMLElement;
    const typing = target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA";
    if (ev.key === "Escape") {
      ev.preventDefault();
      close();
      return;
    }
    if (typing || selected === null || !el) return;
    const step = ev.shiftKey ? 8 : 1;
    const d: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    if (d[ev.key]) {
      ev.preventDefault();
      design.elements[selected] = placed($state.snapshot(el) as Element, ...d[ev.key]);
      changed();
    } else if (ev.key === "Delete" || ev.key === "Backspace") {
      ev.preventDefault();
      remove(selected);
    }
  }

  // --- save / close --------------------------------------------------------
  async function save() {
    saving = true;
    try {
      await api.designSave($state.snapshot(design) as Design);
      dirty = false;
      isNew = false;
      error = null;
    } catch (e) {
      error = api.errorText(e);
    } finally {
      saving = false;
    }
  }

  async function del() {
    if (!confirm(`Delete the design "${design.name}"?`)) return;
    try {
      await api.designDelete(design.id);
      app.designing = null;
    } catch (e) {
      error = api.errorText(e);
    }
  }

  function close() {
    if (dirty && !confirm("Close without saving your changes?")) return;
    app.designing = null;
  }

  function fromTemplate(id: string) {
    const t = catalog?.templates.find((d) => d.id === id);
    if (!t) return;
    if (design.elements.length && !confirm(`Replace this design's elements with "${t.name}"?`)) return;
    design.elements = structuredClone(t.elements);
    selected = null;
    changed();
  }

  onMount(() => {
    api.designList().then((all) => (isNew = !all.some((d) => d.id === design.id)));
    api.designCatalog().then((c) => (catalog = c)).catch((e) => (error = api.errorText(e)));
    tick().then(() => heading?.focus());
    // Clock tokens change every second.
    const timer = setInterval(redraw, 1000);
    return () => clearInterval(timer);
  });

  const num = (v: string, lo: number, hi: number) => Math.max(lo, Math.min(hi, Math.round(Number(v) || 0)));
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Screen designer">
    <header class="top">
      <input
        class="name"
        bind:this={heading}
        bind:value={design.name}
        oninput={changed}
        maxlength="32"
        aria-label="Design name"
        placeholder="Design name"
      />
      {#if catalog}
        <select class="select small-select" aria-label="Start from a template" value="" onchange={(e) => {
          fromTemplate((e.currentTarget as HTMLSelectElement).value);
          (e.currentTarget as HTMLSelectElement).value = "";
        }}>
          <option value="" disabled>Start from…</option>
          {#each catalog.templates as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
        </select>
      {/if}
      {#if !isNew}
        <button class="btn ghost small" onclick={del}><Icon name="trash" /> Delete</button>
      {/if}
      <button class="btn small" onclick={close}>Close</button>
      <button class="btn primary small" disabled={saving || !design.name.trim()} onclick={save}>
        <Icon name="save" /> {dirty || isNew ? "Save" : "Saved"}
      </button>
    </header>

    <div class="body">
      <aside class="left">
        <h2>Add</h2>
        <div class="adds">
          {#each Object.keys(LABELS) as t (t)}
            <button class="btn small" onclick={() => add(t as ElementType)}><Icon name="plus" /> {LABELS[t as ElementType]}</button>
          {/each}
        </div>
        <h2>Elements</h2>
        <ol class="elements">
          {#each design.elements as e, i (i)}
            <li class:sel={selected === i}>
              <button class="pick" onclick={() => (selected = i)} title={label(e)}>{label(e)}</button>
              <button class="btn ghost tiny" disabled={i === 0} aria-label="Move back" onclick={() => move(i, -1)}>↑</button>
              <button class="btn ghost tiny" disabled={i === design.elements.length - 1} aria-label="Move forward" onclick={() => move(i, 1)}>↓</button>
            </li>
          {:else}
            <li class="muted small">Nothing yet. Add an element.</li>
          {/each}
        </ol>
        <p class="muted small">Later elements draw on top. Drag on the screen to move; arrow keys nudge (Shift = 8 px).</p>
      </aside>

      <main class="stage">
        <div
          class="panel"
          style:width={`${128 * SCALE}px`}
          style:height={`${64 * SCALE}px`}
          onpointerdown={down}
          onpointermove={moveEv}
          onpointerup={up}
          onpointercancel={up}
          class:painting={paint !== "off"}
          role="application"
          aria-label="Design canvas"
        >
          <canvas bind:this={canvas}></canvas>
          {#if selected !== null && bounds[selected]}
            {@const [bx, by, bw, bh] = bounds[selected]}
            <div
              class="sel-box"
              style:left={`${bx * SCALE - 2}px`}
              style:top={`${by * SCALE - 2}px`}
              style:width={`${bw * SCALE + 4}px`}
              style:height={`${bh * SCALE + 4}px`}
            ></div>
          {/if}
        </div>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
      </main>

      <aside class="right">
        {#if !el}
          <p class="muted small">Select an element to change it.</p>
          {#if catalog}
            <h2>Tokens</h2>
            <p class="muted small">Text elements fill these in live:</p>
            <dl class="tokens">
              {#each catalog.tokens as [k, v] (k)}<dt><code>{`{${k}}`}</code></dt><dd>{v}</dd>{/each}
            </dl>
          {/if}
        {:else}
          <h2>{LABELS[el.type]}</h2>
          <div class="props">
            {#if el.type === "text"}
              <label class="wide">Text
                <input class="input" bind:this={textInput} bind:value={el.text} oninput={changed} maxlength="80" spellcheck="false" />
              </label>
              <label>Font
                <select class="input" bind:value={el.font} onchange={() => {
                  if (el?.type === "text") el.size = el.font === "seg" ? 24 : 2;
                  changed();
                }}>
                  <option value="small">5x7 pixel</option>
                  <option value="seg">7-segment</option>
                </select>
              </label>
              <label>{el.font === "seg" ? "Height" : "Scale"}
                <input class="input" type="number" min={el.font === "seg" ? 10 : 1} max={el.font === "seg" ? 60 : 4} value={el.size}
                  onchange={(e) => { if (el?.type === "text") { el.size = num(e.currentTarget.value, el.font === "seg" ? 10 : 1, el.font === "seg" ? 60 : 4); changed(); } }} />
              </label>
              <label>Align
                <select class="input" bind:value={el.align} onchange={changed}>
                  <option value="left">Left</option><option value="center">Centre</option><option value="right">Right</option>
                </select>
              </label>
              <label class="check"><input type="checkbox" bind:checked={el.erase} onchange={changed} /> Black (on a filled box)</label>
              {#if catalog}
                <div class="wide token-chips" aria-label="Insert a token">
                  {#each catalog.tokens as [k, v] (k)}
                    <button class="chip" title={v} onclick={() => insertToken(k)}>{k}</button>
                  {/each}
                </div>
              {/if}
            {:else if el.type === "rect"}
              <label class="check"><input type="checkbox" bind:checked={el.fill} onchange={changed} /> Filled</label>
              <label class="check"><input type="checkbox" bind:checked={el.erase} onchange={changed} /> Black</label>
            {:else if el.type === "line"}
              {#each ["x1", "y1", "x2", "y2"] as k (k)}
                <label>{k}
                  <input class="input" type="number" value={el[k as "x1"]}
                    onchange={(e) => { if (el?.type === "line") { el[k as "x1"] = num(e.currentTarget.value, -64, 192); changed(); } }} />
                </label>
              {/each}
              <label>Thickness
                <input class="input" type="number" min="1" max="4" value={el.thick}
                  onchange={(e) => { if (el?.type === "line") { el.thick = num(e.currentTarget.value, 1, 4); changed(); } }} />
              </label>
            {:else if el.type === "bar"}
              <label>Value <input class="input" bind:value={el.value} oninput={changed} /></label>
              <label>Max <input class="input" bind:value={el.max} oninput={changed} /></label>
            {:else if el.type === "analog"}
              <label>Radius
                <input class="input" type="number" min="4" max="40" value={el.r}
                  onchange={(e) => { if (el?.type === "analog") { el.r = num(e.currentTarget.value, 4, 40); changed(); } }} />
              </label>
              <label class="check"><input type="checkbox" bind:checked={el.seconds} onchange={changed} /> Second hand</label>
            {:else if el.type === "icon"}
              <label>Icon
                <select class="input" bind:value={el.icon} onchange={changed}>
                  <option value={"{WICON}"}>Weather now</option>
                  {#each catalog?.icons ?? [] as n (n)}<option value={n}>{n}</option>{/each}
                </select>
              </label>
              <label>Size
                <select class="input" value={String(el.size)} onchange={(e) => { if (el?.type === "icon") { el.size = Number(e.currentTarget.value); changed(); } }}>
                  <option value="1">16 px</option><option value="2">32 px</option><option value="3">48 px</option>
                </select>
              </label>
            {:else if el.type === "bitmap"}
              <div class="wide paint-modes" role="radiogroup" aria-label="Paint">
                {#each [["off", "Move"], ["draw", "Draw"], ["erase", "Erase"]] as [m, t] (m)}
                  <button class="btn small" class:primary={paint === m} role="radio" aria-checked={paint === m} onclick={() => (paint = m as typeof paint)}>{t}</button>
                {/each}
              </div>
              <button class="btn small wide" onclick={() => { if (el?.type === "bitmap") { el.data = encodeBits(new Uint8Array(el.w * el.h), el.w, el.h); changed(); } }}>Clear drawing</button>
            {/if}

            {#if el.type === "text" || el.type === "icon" || el.type === "rect" || el.type === "bar" || el.type === "bitmap"}
              <label>x
                <input class="input" type="number" value={el.x} onchange={(e) => { if (el && "x" in el) { el.x = num(e.currentTarget.value, -128, 255); changed(); } }} />
              </label>
              <label>y
                <input class="input" type="number" value={el.y} onchange={(e) => { if (el && "y" in el) { el.y = num(e.currentTarget.value, -64, 127); changed(); } }} />
              </label>
            {/if}
            {#if el.type === "analog"}
              <label>x <input class="input" type="number" value={el.cx} onchange={(e) => { if (el?.type === "analog") { el.cx = num(e.currentTarget.value, -64, 192); changed(); } }} /></label>
              <label>y <input class="input" type="number" value={el.cy} onchange={(e) => { if (el?.type === "analog") { el.cy = num(e.currentTarget.value, -64, 128); changed(); } }} /></label>
            {/if}
            {#if el.type === "rect" || el.type === "bar" || el.type === "bitmap"}
              <label>Width
                <input class="input" type="number" min="1" max="128" value={el.w} onchange={(e) => {
                  if (!el) return;
                  if (el.type === "bitmap") resizeBitmap(el, Number(e.currentTarget.value), el.h);
                  else if (el.type === "rect" || el.type === "bar") el.w = num(e.currentTarget.value, 1, 128);
                  changed();
                }} />
              </label>
              <label>Height
                <input class="input" type="number" min="1" max="64" value={el.h} onchange={(e) => {
                  if (!el) return;
                  if (el.type === "bitmap") resizeBitmap(el, el.w, Number(e.currentTarget.value));
                  else if (el.type === "rect" || el.type === "bar") el.h = num(e.currentTarget.value, 1, 64);
                  changed();
                }} />
              </label>
            {/if}
          </div>
          <button class="btn ghost small danger" onclick={() => selected !== null && remove(selected)}><Icon name="trash" /> Remove element</button>
        {/if}
      </aside>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    padding: 16px;
    background: rgba(0, 0, 0, 0.78);
    backdrop-filter: blur(6px);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(1240px, 100%);
    height: min(760px, calc(100vh - 32px));
    min-height: 0;
    border: 1px solid var(--line-2);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: 0 24px 80px -20px rgba(0, 0, 0, 0.9);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 10px 16px;
    border-bottom: 1px solid var(--line);
  }

  .name {
    flex: 1;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 15px;
    font-weight: 600;
  }

  .name:hover,
  .name:focus {
    border-color: var(--line);
    outline: none;
  }

  .small-select {
    width: auto;
    height: 30px;
    font-size: 12px;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr) 260px;
    /* One row the height of the dialog, so the side panels scroll inside it. */
    grid-template-rows: minmax(0, 1fr);
  }

  .left,
  .right {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    min-width: 0;
    padding: 14px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .left {
    border-right: 1px solid var(--line);
  }

  .right {
    border-left: 1px solid var(--line);
  }

  h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .adds {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .adds .btn {
    justify-content: flex-start;
    min-width: 0;
    padding: 0 8px;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .elements {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .elements li {
    display: flex;
    align-items: center;
    gap: 2px;
    border-radius: var(--r-sm);
  }

  .elements li.sel {
    background: var(--bg);
    outline: 1px solid var(--line-2);
  }

  .pick {
    flex: 1;
    min-width: 0;
    padding: 6px 8px;
    border: none;
    background: none;
    color: var(--text-2);
    font: inherit;
    font-size: 12px;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }

  .sel .pick {
    color: var(--text);
  }

  .tiny {
    padding: 0 6px;
    height: 24px;
    font-size: 12px;
  }

  .stage {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 16px;
    overflow: auto;
    background: var(--bg);
  }

  .panel {
    position: relative;
    flex: none;
    cursor: default;
    touch-action: none;
    border-radius: 4px;
    box-shadow: 0 0 0 6px #0a0d10, 0 0 0 7px var(--line);
  }

  .panel.painting {
    cursor: crosshair;
  }

  .panel canvas {
    display: block;
  }

  .sel-box {
    position: absolute;
    border: 1px dashed #7fc8ff;
    pointer-events: none;
  }

  .props {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .props label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
    color: var(--text-3);
  }

  .props label.check {
    flex-direction: row;
    align-items: center;
    grid-column: 1 / -1;
    font-size: 12px;
    color: var(--text-2);
  }

  .wide {
    grid-column: 1 / -1;
  }

  .input {
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    min-width: 0;
  }

  .input:focus {
    border-color: var(--line-2);
    outline: none;
  }

  .token-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip {
    padding: 2px 6px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--bg);
    color: var(--text-2);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .chip:hover {
    border-color: var(--line-2);
    color: var(--text);
  }

  .paint-modes {
    display: flex;
    gap: 6px;
  }

  .tokens {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 10px;
    margin: 0;
    font-size: 12px;
  }

  .tokens dt {
    margin: 0;
  }

  .tokens dd {
    margin: 0;
    color: var(--text-3);
  }

  .muted {
    margin: 0;
    color: var(--text-3);
  }

  .small {
    font-size: 12px;
  }

  .danger {
    color: var(--danger);
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
  }
</style>
