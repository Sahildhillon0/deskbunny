<script lang="ts">
  import { errorText } from "../api";
  import { app } from "../state.svelte";
  import { MOODS, NAME_MAX, OVERLAYS, sanitizeName, type Mood, type Overlay } from "../types";
  import Icon from "./Icon.svelte";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";

  const MOOD_LABEL: Record<Mood, string> = {
    neutral: "Watching",
    happy: "Happy",
    angry: "Angry",
    sad: "Sad",
    sleepy: "Sleepy",
    surprised: "Surprised",
    love: "Love",
    suspicious: "Suspicious",
    excited: "Excited",
    bored: "Bored",
    scared: "Scared",
    dizzy: "Dizzy",
  };

  const OVERLAY_LABEL: Record<Overlay, string> = {
    none: "None",
    hearts: "Hearts",
    zzz: "Zzz",
    sweat: "Sweat",
    anger: "Anger",
    tears: "Tears",
    sparkles: "Sparkle",
    question: "?",
  };

  const secs = (v: number) => `${(v / 10).toFixed(1)} s`;
  const px = (v: number) => `${v} px`;
  const signedPx = (v: number) => `${v > 0 ? "+" : ""}${v} px`;
  const hex = (v: number) => `0x${v.toString(16).toUpperCase().padStart(2, "0")}`;

  let status = $state<{ kind: "ok" | "error"; text: string } | null>(null);
  let confirmDelete = $state(false);
  let busy = $state(false);
  let fileInput: HTMLInputElement;
  let statusTimer: ReturnType<typeof setTimeout> | undefined;

  const f = $derived(app.face);
  const radiusMax = $derived(Math.floor(f.eyeW / 2));

  function say(kind: "ok" | "error", text: string) {
    status = { kind, text };
    clearTimeout(statusTimer);
    if (kind === "ok") statusTimer = setTimeout(() => (status = null), 2500);
  }

  // Keep dependent fields valid as their limits move.
  $effect(() => {
    if (f.eyeR > radiusMax) f.eyeR = radiusMax;
    if (f.blinkMax < f.blinkMin) f.blinkMax = f.blinkMin;
    if (f.lookMax < f.lookMin) f.lookMax = f.lookMin;
  });

  function onName(e: Event & { currentTarget: HTMLInputElement }) {
    const clean = sanitizeName(e.currentTarget.value);
    if (clean !== e.currentTarget.value) e.currentTarget.value = clean;
    app.face.name = clean;
  }

  async function save() {
    if (!f.name.trim()) return say("error", "Give the face a name first.");
    busy = true;
    try {
      const updating = !!app.savedMatch;
      await app.save();
      say("ok", updating ? `Updated ${f.name}` : `Saved ${f.name} to My faces`);
    } catch (e) {
      say("error", errorText(e));
    } finally {
      busy = false;
    }
  }

  async function remove() {
    const target = app.savedMatch;
    if (!target) return;
    if (!confirmDelete) {
      confirmDelete = true;
      setTimeout(() => (confirmDelete = false), 3000);
      return;
    }
    confirmDelete = false;
    busy = true;
    try {
      await app.remove(target.name);
      say("ok", `Deleted ${target.name}`);
    } catch (e) {
      say("error", errorText(e));
    } finally {
      busy = false;
    }
  }

  function exportJson() {
    const face = $state.snapshot(app.face);
    const blob = new Blob([JSON.stringify(face, null, 2) + "\n"], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${(face.name.trim() || "face").toLowerCase().replace(/\s+/g, "-")}.deskbunny.json`;
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    say("ok", `Exported ${a.download}`);
  }

  async function importJson(e: Event & { currentTarget: HTMLInputElement }) {
    const file = e.currentTarget.files?.[0];
    e.currentTarget.value = "";
    if (!file) return;
    try {
      const face = app.importJson(await file.text());
      say("ok", `Imported ${face.name}`);
    } catch {
      say("error", `${file.name} isn't a DeskBunny face (expected JSON).`);
    }
  }
</script>

<header class="head">
  <div class="name-field">
    <label class="label" for="face-name">Name</label>
    <input
      id="face-name"
      class="name"
      value={f.name}
      oninput={onName}
      maxlength={NAME_MAX}
      spellcheck="false"
      autocomplete="off"
      placeholder="NAME"
      aria-describedby="face-name-hint"
    />
    <span id="face-name-hint" class="count">{f.name.length}/{NAME_MAX}</span>
  </div>
</header>

<div class="scroll">
  <section>
    <h2 class="label" id="mood-label">Mood</h2>
    <div class="chips cols-3" role="radiogroup" aria-labelledby="mood-label">
      {#each MOODS as m (m)}
        <label class="chip" class:on={f.mood === m}>
          <input type="radio" name="mood" value={m} bind:group={app.face.mood} class="sr-only" />
          {MOOD_LABEL[m]}
        </label>
      {/each}
    </div>
  </section>

  <section>
    <h2 class="label" id="overlay-label">Overlay</h2>
    <div class="chips cols-4" role="radiogroup" aria-labelledby="overlay-label">
      {#each OVERLAYS as o (o)}
        <label class="chip" class:on={f.overlay === o} title={o === "question" ? "Question mark" : undefined}>
          <input type="radio" name="overlay" value={o} bind:group={app.face.overlay} class="sr-only" />
          {OVERLAY_LABEL[o]}
        </label>
      {/each}
    </div>
  </section>

  <section>
    <h2 class="label">Eyes</h2>
    <div class="stack">
      <Slider label="Width" bind:value={app.face.eyeW} min={8} max={56} format={px} />
      <Slider label="Height" bind:value={app.face.eyeH} min={8} max={56} format={px} />
      <Slider label="Corner radius" bind:value={app.face.eyeR} min={0} max={radiusMax} format={px} />
      <Slider label="Gap" bind:value={app.face.eyeGap} min={0} max={40} format={px} />
      <Slider label="Vertical offset" bind:value={app.face.eyeY} min={-20} max={20} format={signedPx} />
    </div>
  </section>

  <section>
    <h2 class="label">Behaviour</h2>
    <div class="switches">
      <Switch label="Blink" bind:checked={app.face.blink} />
      <Switch label="Watching" bind:checked={app.face.lookAround} hint="Glance around now and then" />
      <Switch label="Breathe" bind:checked={app.face.breathe} />
      <Switch label="Mouth" bind:checked={app.face.mouth} />
      <Switch label="Cyclops" bind:checked={app.face.cyclops} hint="One eye" />
      <Switch label="Invert" bind:checked={app.face.invert} />
      <Switch label="Rotate 180°" bind:checked={app.face.rotate180} hint="For a module mounted upside down" />
    </div>
  </section>

  <section>
    <h2 class="label">Timing</h2>
    <div class="stack">
      <Slider label="Speed" bind:value={app.face.speed} min={1} max={10} format={(v) => `${v} / 10`} />
      <div class="pair">
        <Slider label="Blink every" bind:value={app.face.blinkMin} min={5} max={200} format={secs} />
        <Slider label="to" bind:value={app.face.blinkMax} min={app.face.blinkMin} max={200} format={secs} />
      </div>
      <div class="pair">
        <Slider label="Glance every" bind:value={app.face.lookMin} min={5} max={200} format={secs} />
        <Slider label="to" bind:value={app.face.lookMax} min={app.face.lookMin} max={200} format={secs} />
      </div>
      <div class="pair">
        <Slider label="Glance X" bind:value={app.face.lookX} min={0} max={40} format={px} />
        <Slider label="Glance Y" bind:value={app.face.lookY} min={0} max={20} format={px} />
      </div>
    </div>
  </section>

  <section>
    <h2 class="label">Display</h2>
    <div class="stack">
      <Slider label="Contrast" bind:value={app.face.contrast} min={0} max={255} format={hex} />
    </div>
  </section>
</div>

<footer class="actions">
  <div class="row">
    <button class="btn primary" onclick={save} disabled={busy || !f.name.trim() || (app.source?.kind === "saved" && !app.dirty)}>
      <Icon name="save" />
      {app.savedMatch ? "Update" : "Save"}
    </button>
    {#if app.savedMatch}
      <button class="btn danger" class:confirm={confirmDelete} onclick={remove} disabled={busy}>
        <Icon name="trash" />
        {confirmDelete ? "Confirm" : "Delete"}
      </button>
    {/if}
    <span class="spacer"></span>
    <button class="btn ghost icon" onclick={() => fileInput.click()} title="Import face JSON" aria-label="Import face JSON">
      <Icon name="upload" />
    </button>
    <button class="btn ghost icon" onclick={exportJson} title="Export face JSON" aria-label="Export face JSON">
      <Icon name="download" />
    </button>
    <input bind:this={fileInput} type="file" accept=".json,application/json" onchange={importJson} hidden />
  </div>
  <p class="status" class:error={status?.kind === "error"} aria-live="polite">{status?.text ?? ""}</p>
</footer>

<style>
  .head {
    flex: none;
    display: flex;
    align-items: center;
    height: 56px;
    padding: 0 20px;
    border-bottom: 1px solid var(--line);
  }

  .name-field {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
  }

  .name {
    flex: 1;
    min-width: 0;
    height: 32px;
    padding: 0 48px 0 10px;
    border: 1px solid var(--line-2);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-family: var(--mono);
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .name:hover {
    border-color: #353535;
  }

  .name:focus-visible {
    border-color: var(--accent-line);
    box-shadow: 0 0 0 3px rgba(223, 246, 255, 0.08);
  }

  .name::placeholder {
    color: var(--text-3);
  }

  .count {
    position: absolute;
    right: 10px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text-3);
    pointer-events: none;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px 20px 24px;
  }

  section {
    padding: 16px 0;
  }

  section + section {
    border-top: 1px solid var(--line);
  }

  h2 {
    margin: 0 0 12px;
  }

  .chips {
    display: grid;
    gap: 6px;
  }

  .cols-3 {
    grid-template-columns: repeat(3, 1fr);
  }

  .cols-4 {
    grid-template-columns: repeat(4, 1fr);
  }

  .chip {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 32px;
    padding: 0 6px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-size: 12px;
    color: var(--text-2);
    cursor: pointer;
    white-space: nowrap;
    transition: border-color 120ms, color 120ms, background-color 120ms;
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

  .stack {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  .switches {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .actions {
    flex: none;
    padding: 12px 20px 8px;
    border-top: 1px solid var(--line);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .spacer {
    flex: 1;
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

  .status {
    min-height: 18px;
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--text-3);
  }

  .status.error {
    color: var(--danger);
  }
</style>
