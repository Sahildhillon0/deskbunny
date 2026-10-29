<script lang="ts">
  import { onMount, tick } from "svelte";
  import * as api from "../api";
  import { debugLog, formatTime, logText } from "../debuglog.svelte";
  import Icon from "./Icon.svelte";

  let list = $state<HTMLDivElement | null>(null);
  let heading = $state<HTMLHeadingElement | null>(null);
  let follow = $state(true);
  let copied = $state(false);
  let scanning = $state(false);
  let copyError = $state("");

  // Keep the newest line in view unless the user scrolled up to read.
  $effect(() => {
    void debugLog.lines.length;
    if (follow && list) tick().then(() => list && (list.scrollTop = list.scrollHeight));
  });

  function onScroll() {
    if (!list) return;
    follow = list.scrollHeight - list.scrollTop - list.clientHeight < 24;
  }

  async function copy() {
    copyError = "";
    let version = "?";
    try {
      version = await api.appVersion();
    } catch {
      /* keep "?" */
    }
    const header = `DeskBunny ${version}\n${navigator.userAgent}\n\n`;
    try {
      await navigator.clipboard.writeText(header + logText(debugLog.lines));
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch (e) {
      copyError = `Could not copy: ${api.errorText(e)}. Select the lines and press Ctrl+C instead.`;
    }
  }

  async function scan() {
    scanning = true;
    follow = true;
    try {
      await debugLog.scan();
    } finally {
      scanning = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      debugLog.close();
    }
  }

  onMount(async () => {
    await tick();
    heading?.focus();
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="debug-title">
    <header class="top">
      <h1 id="debug-title" tabindex="-1" bind:this={heading}>Debug log</h1>
      <span class="count">{debugLog.lines.length} lines</span>
      <button class="btn ghost icon" aria-label="Close" onclick={() => debugLog.close()}>
        <Icon name="close" />
      </button>
    </header>

    <p class="intro">
      Everything the flasher did since DeskBunny started: ports, USB devices and their drivers, and every error. If
      flashing fails, press <em>Scan USB devices</em>, try the flash again, then <em>Copy</em> and send the log.
    </p>

    <!-- svelte-ignore a11y_no_noninteractive_tabindex (scrollable: keyboard users need focus to scroll) -->
    <div class="lines" bind:this={list} onscroll={onScroll} role="log" aria-live="off" tabindex="0">
      {#each debugLog.lines as e, i (i)}
        <div class="line {e.level}">
          <span class="t">{formatTime(e.t)}</span>
          <span class="lvl">{e.level}</span>
          <span class="msg">{e.msg}</span>
        </div>
      {:else}
        <p class="empty">Nothing logged yet.</p>
      {/each}
    </div>

    {#if copyError}<p class="error" role="alert">{copyError}</p>{/if}

    <footer class="actions">
      <button class="btn ghost" onclick={() => debugLog.clear()} disabled={!debugLog.lines.length}>
        <Icon name="trash" /> Clear
      </button>
      <span class="spacer"></span>
      <button class="btn ghost" onclick={scan} disabled={scanning}>
        <Icon name="usb" /> {scanning ? "Scanning…" : "Scan USB devices"}
      </button>
      <button class="btn primary" onclick={copy} disabled={!debugLog.lines.length}>
        <Icon name={copied ? "check" : "copy"} /> {copied ? "Copied" : "Copy"}
      </button>
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
    padding: 24px;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(6px);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(880px, 100%);
    height: min(640px, 100%);
    border: 1px solid var(--line-2);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: 0 24px 80px -20px rgba(0, 0, 0, 0.9);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 12px 12px 24px;
    border-bottom: 1px solid var(--line);
  }

  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    outline: none;
  }

  .count {
    flex: 1;
    font-size: 12px;
    color: var(--text-3);
  }

  .intro {
    margin: 0;
    padding: 12px 24px;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-2);
  }

  .lines {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0 16px;
    padding: 8px 0;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-family: var(--mono);
    font-size: 11.5px;
    line-height: 1.5;
    user-select: text;
  }

  .line {
    display: grid;
    grid-template-columns: 9ch 5.5ch minmax(0, 1fr);
    gap: 10px;
    padding: 0 12px;
    color: var(--text-2);
  }

  .t {
    color: var(--text-3);
    white-space: pre;
  }

  .lvl {
    color: var(--text-3);
    text-transform: uppercase;
  }

  .msg {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .line.warn .lvl,
  .line.warn .msg {
    color: var(--warn);
  }

  .line.error .lvl,
  .line.error .msg {
    color: var(--danger);
  }

  .line.debug .msg,
  .line.trace .msg {
    color: var(--text-3);
  }

  .empty {
    margin: 0;
    padding: 12px;
    color: var(--text-3);
  }

  .error {
    margin: 10px 16px 0;
    padding: 8px 12px;
    border-radius: var(--r-sm);
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 12px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
    margin-top: 12px;
    border-top: 1px solid var(--line);
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
</style>
