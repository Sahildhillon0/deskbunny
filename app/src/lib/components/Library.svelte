<script lang="ts">
  import { app, type FaceSource } from "../state.svelte";
  import type { FaceDef } from "../types";
  import Thumb from "./Thumb.svelte";
  import Icon from "./Icon.svelte";
  import { setup } from "../setup.svelte";
  import { updater } from "../updater.svelte";

  const MOOD_LABEL: Record<string, string> = {
    hover: "Hover",
    patrol: "Patrol",
    takeoff: "Take off",
    flip: "Flip",
    orbit: "Orbit",
    formation: "Formation",
    delivery: "Delivery",
    race: "Race",
    scan: "Scan",
    swarm: "Swarm",
    topdown: "Top view",
    charge: "Charge",
  };
</script>

{#snippet card(kind: FaceSource["kind"], face: FaceDef, i: number)}
  {@const selected = app.isSelected(kind, face.name)}
  <li>
    <button
      class="card"
      class:selected
      aria-pressed={selected}
      onclick={() => app.select(kind, face)}
    >
      <Thumb {face} seed={i + (kind === "saved" ? 100 : 0)} />
      <span class="meta">
        <span class="name">{face.name || "UNTITLED"}</span>
        <span class="mood">
          {MOOD_LABEL[face.mood] ?? face.mood}{#if face.overlay !== "none"}<span class="dot">·</span>{face.overlay}{/if}
        </span>
      </span>
      {#if selected && app.dirty}
        <span class="edited" title="Edited, not saved">edited</span>
      {/if}
    </button>
  </li>
{/snippet}

<header class="brand">
  <svg class="logo" viewBox="0 0 24 24" aria-hidden="true">
    <rect x="2" y="6" width="20" height="13" rx="3" fill="none" stroke="currentColor" stroke-width="1.5" />
    <rect x="6.5" y="10" width="4" height="5" rx="1.2" fill="currentColor" />
    <rect x="13.5" y="10" width="4" height="5" rx="1.2" fill="currentColor" />
    <path d="M8 6 6.5 2.5M16 6l1.5-3.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
  </svg>
  <span class="title">DeskBunny</span>
  <button
    class="btn ghost icon update"
    class:has-update={updater.phase === "available"}
    title={updater.phase === "available" ? `Update to ${updater.info?.version}` : "Check for updates"}
    aria-label={updater.phase === "available" ? `Update available: ${updater.info?.version}` : "Check for updates"}
    onclick={() => (updater.phase === "available" ? (updater.open = true) : updater.check())}
  >
    <Icon name="download" />
  </button>
  <button class="btn ghost icon help" title="Setup guide" aria-label="Open the setup guide" onclick={() => setup.show()}>
    <Icon name="help" />
  </button>
</header>

<div class="scroll">
  <section aria-labelledby="lib-presets">
    <h2 id="lib-presets" class="label">Presets</h2>
    <ul class="list">
      {#each app.presets as face, i (face.name)}
        {@render card("preset", face, i)}
      {/each}
    </ul>
  </section>

  <section aria-labelledby="lib-saved">
    <h2 id="lib-saved" class="label">My drones</h2>
    {#if app.saved.length}
      <ul class="list">
        {#each app.saved as face, i (face.name)}
          {@render card("saved", face, i)}
        {/each}
      </ul>
    {:else}
      <p class="empty">Tweak a preset, name it, and press <em>Save</em>. It shows up here.</p>
    {/if}
  </section>
</div>

<style>
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 56px;
    padding: 0 20px;
    border-bottom: 1px solid var(--line);
    flex: none;
  }

  .logo {
    width: 22px;
    height: 22px;
    color: var(--accent);
    filter: drop-shadow(0 0 6px rgba(223, 246, 255, 0.35));
  }

  .update {
    position: relative;
    margin-left: auto;
  }

  .update.has-update {
    color: var(--accent);
  }

  .update.has-update::after {
    content: "";
    position: absolute;
    top: 6px;
    right: 6px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px rgba(223, 246, 255, 0.6);
  }

  .help {
    margin-right: -8px;
  }

  .title {
    font-weight: 600;
    font-size: 15px;
    letter-spacing: 0.01em;
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 16px 12px 24px;
  }

  section + section {
    margin-top: 24px;
  }

  h2 {
    margin: 0 8px 8px;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .card {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 8px;
    border: 1px solid transparent;
    border-radius: var(--r);
    background: transparent;
    text-align: left;
    transition: background-color 120ms, border-color 120ms;
  }

  .card:hover {
    background: var(--surface-2);
  }

  .card.selected {
    background: var(--surface-2);
    border-color: var(--accent-line);
    box-shadow: 0 0 18px -6px rgba(223, 246, 255, 0.18);
  }

  .card :global(.thumb) {
    flex: none;
    outline: 1px solid var(--line);
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .name {
    font-family: var(--mono);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mood {
    font-size: 12px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dot {
    margin: 0 4px;
    color: var(--text-3);
  }

  .edited {
    position: absolute;
    top: 6px;
    right: 8px;
    font-size: 10px;
    color: var(--text-3);
    letter-spacing: 0.04em;
  }

  .empty {
    margin: 0 8px;
    padding: 12px;
    border: 1px dashed var(--line-2);
    border-radius: var(--r);
    color: var(--text-3);
    font-size: 12px;
  }

  .empty em {
    font-style: normal;
    color: var(--text-2);
  }
</style>
