<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import { app } from "../state.svelte";
  import type { Board, FlashProgress, FlashReport, FlashStage, Port } from "../types";
  import Icon from "./Icon.svelte";
  import { debugLog } from "../debuglog.svelte";

  const STAGE_LABEL: Record<FlashStage, string> = {
    reset: "Resetting board",
    sync: "Talking to bootloader",
    erase: "Erasing",
    write: "Writing",
    verify: "Verifying",
    done: "Done",
  };

  type Phase =
    | { kind: "idle" }
    | { kind: "flashing"; progress: FlashProgress }
    | { kind: "success"; board: string; report: FlashReport }
    | { kind: "error"; message: string; hint: string };

  let ports = $state<Port[]>([]);
  let boards = $state<Board[]>([]);
  let boardsError = $state<string | null>(null);
  let portPath = $state<string>("");
  let userBoard = $state<string | null>(null);
  let scanning = $state(false);
  let phase = $state<Phase>({ kind: "idle" });

  const port = $derived(ports.find((p) => p.path === portPath) ?? null);
  const boardId = $derived(
    userBoard ?? (port?.guessedBoard && boards.some((b) => b.id === port.guessedBoard) ? port.guessedBoard : boards[0]?.id ?? ""),
  );
  const board = $derived(boards.find((b) => b.id === boardId) ?? null);
  const flashing = $derived(phase.kind === "flashing");
  const canFlash = $derived(!!port && !!board && !flashing);

  function portLabel(p: Port) {
    const what = p.product || (p.vid ? `USB ${p.vid}:${p.pid ?? "????"}` : "Serial port");
    return `${p.path} · ${what}`;
  }

  async function refreshPorts() {
    if (scanning || flashing) return;
    scanning = true;
    try {
      const list = await api.listPorts();
      ports = list;
      if (!list.some((p) => p.path === portPath)) {
        selectPort((list.find((p) => p.guessedBoard) ?? list[0])?.path ?? "");
      }
    } catch {
      ports = [];
      selectPort("");
    } finally {
      scanning = false;
    }
  }

  function selectPort(path: string) {
    if (path !== portPath) userBoard = null;
    portPath = path;
  }

  function hintFor(msg: string): string {
    if (/firmware/i.test(msg) && /(bundl|not found|missing|no such)/i.test(msg))
      return "This build doesn't include the firmware images. Run firmware/build.sh (needs arduino-cli), or use a release build.";
    if (/permission denied|access is denied|EACCES/i.test(msg))
      return "DeskBunny isn't allowed to use this USB device yet. Open the setup guide (the ? button, top left) and follow \"Prepare your computer\".";
    if (/already running/i.test(msg)) return "Wait for the current upload to finish, then try again.";
    if (/busy|in use|already open/i.test(msg))
      return "Another program has the port open. Close any serial monitor or Arduino IDE, then retry.";
    if (/bootloader|sync|no response|timed? ?out/i.test(msg))
      return "Check the board choice, then try again. Pressing the board's reset button just as you click Flash can help.";
    return "Unplug the board, plug it back in, and try again.";
  }

  async function doFlash() {
    if (!port || !board || flashing) return;
    const face = $state.snapshot(app.face);
    const target = { path: port.path, board };
    phase = { kind: "flashing", progress: { stage: "reset", percent: 0, message: "Starting" } };
    try {
      const report = await api.flash(target.path, target.board.id, face);
      phase = { kind: "success", board: target.board.name, report };
    } catch (e) {
      const message = api.errorText(e);
      phase = { kind: "error", message, hint: hintFor(message) };
    }
    refreshPorts();
  }

  onMount(() => {
    api
      .listBoards()
      .then((b) => {
        boards = b;
        if (!b.length) boardsError = "No boards available. The firmware manifest may be missing from this build.";
      })
      .catch((e) => (boardsError = api.errorText(e)));
    refreshPorts();

    let unlisten: (() => void) | null = null;
    let dead = false;
    api.onFlashProgress((p) => {
      if (phase.kind === "flashing") phase = { kind: "flashing", progress: p };
    }).then((u) => (dead ? u() : (unlisten = u)));

    // Poll for plugged/unplugged boards only while the window has focus.
    const timer = setInterval(() => {
      if (document.hasFocus() && !document.hidden) refreshPorts();
    }, 2000);
    const onFocus = () => refreshPorts();
    window.addEventListener("focus", onFocus);

    return () => {
      dead = true;
      unlisten?.();
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  });
</script>

<div class="bar">
  <div class="field port">
    <label class="label" for="port-select">Port</label>
    <div class="inline">
      {#if ports.length}
        <select
          id="port-select"
          class="select"
          value={portPath}
          onchange={(e) => selectPort(e.currentTarget.value)}
          disabled={flashing}
        >
          {#each ports as p (p.path)}
            <option value={p.path}>{portLabel(p)}</option>
          {/each}
        </select>
      {:else}
        <div id="port-select" class="empty-port" role="status">
          <Icon name="usb" />
          Plug in your DeskBunny via USB
        </div>
      {/if}
      <button
        class="btn icon"
        class:spin={scanning}
        onclick={refreshPorts}
        disabled={flashing}
        aria-label="Rescan ports"
        title="Rescan ports"
      >
        <Icon name="refresh" />
      </button>
    </div>
  </div>

  <div class="field board">
    <label class="label" for="board-select">
      Board
      {#if port?.guessedBoard && port.guessedBoard === boardId}<span class="detected">· detected</span>{/if}
    </label>
    <select
      id="board-select"
      class="select"
      value={boardId}
      onchange={(e) => (userBoard = e.currentTarget.value)}
      disabled={flashing || !boards.length}
    >
      {#each boards as b (b.id)}
        <option value={b.id}>{b.name}</option>
      {/each}
    </select>
  </div>

  <div class="status" aria-live="polite">
    {#if boardsError}
      <div class="msg error">
        <Icon name="alert" />
        <div>
          <strong>Firmware not available.</strong>
          <span class="detail">{boardsError}</span>
        </div>
      </div>
    {:else if phase.kind === "flashing"}
      {@const p = phase.progress}
      <div class="progress-wrap">
        <div class="progress-top">
          <span>{STAGE_LABEL[p.stage] ?? p.stage}<span class="sub">{p.message && p.message !== STAGE_LABEL[p.stage] ? ` · ${p.message}` : ""}</span></span>
          <span class="pct">{Math.round(p.percent)}%</span>
        </div>
        <div
          class="progress"
          role="progressbar"
          aria-label="Flash progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(p.percent)}
        >
          <div class="fill" style:width={`${Math.max(2, Math.min(100, p.percent))}%`}></div>
        </div>
      </div>
    {:else if phase.kind === "success"}
      {@const r = phase.report}
      <div class="msg" class:ok={r.deviceVerified} class:warn={!r.deviceVerified}>
        <Icon name={r.deviceVerified ? "check" : "alert"} />
        <div>
          <strong class="one-line" title={`${r.faceName} is on your ${phase.board}`}>
            {r.deviceVerified ? `${r.faceName} is on your ${phase.board}` : `Flashed ${r.faceName} to your ${phase.board}`}
          </strong>
          {#if r.deviceInfo}
            <code class="device-line">{r.deviceInfo}</code>
          {/if}
          {#each r.warnings as w, i (i)}
            <span class="detail">{w}</span>
          {/each}
          {#if !r.deviceInfo && !r.warnings.length}
            <span class="detail">The board didn't answer the check. If the face is showing, you're set.</span>
          {/if}
        </div>
        <button class="btn ghost small" onclick={() => (phase = { kind: "idle" })}>Dismiss</button>
      </div>
    {:else if phase.kind === "error"}
      <div class="msg error">
        <Icon name="alert" />
        <div>
          <strong>Flash failed</strong>
          <span class="message">{phase.message}</span>
          <span class="detail">{phase.hint}</span>
        </div>
        <div class="error-actions">
          <button class="btn ghost small" onclick={() => debugLog.show()}>Show log</button>
          <button class="btn ghost small" onclick={() => (phase = { kind: "idle" })}>Dismiss</button>
        </div>
      </div>
    {:else if port && board}
      <p class="ready">
        Ready to flash <b>{app.face.name || "UNTITLED"}</b> to {board.name} on <code>{port.path}</code>
      </p>
    {:else}
      <p class="ready muted">Connect a board to flash this face.</p>
    {/if}
  </div>

  <button class="flash-btn" onclick={doFlash} disabled={!canFlash || !!boardsError}>
    <Icon name="bolt" />
    {flashing ? "Flashing…" : "Flash to DeskBunny"}
  </button>
</div>

<style>
  .bar {
    display: grid;
    grid-template-columns: minmax(200px, 280px) minmax(160px, 200px) minmax(0, 1fr) auto;
    align-items: end;
    gap: 16px;
    padding: 12px 20px 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .inline {
    display: flex;
    gap: 6px;
    min-width: 0;
  }

  .inline .select {
    flex: 1;
  }

  .select {
    width: 100%;
  }

  .empty-port {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 10px;
    border: 1px dashed var(--line-2);
    border-radius: var(--r-sm);
    font-size: 13px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
  }

  .empty-port :global(svg) {
    color: var(--text-3);
    flex: none;
  }

  .spin :global(svg) {
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Ready, progress and a clean success fit in 56px; errors and warnings grow the bar. */
  .status {
    min-width: 0;
    min-height: 56px;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
  }

  .ready {
    padding-bottom: 7px;
  }

  .progress-wrap {
    padding-bottom: 6px;
  }

  .detected {
    margin-left: 4px;
    color: var(--accent);
    opacity: 0.7;
    text-transform: none;
    letter-spacing: 0.02em;
    font-weight: 500;
  }

  .one-line {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ready {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ready b {
    font-family: var(--mono);
    font-weight: 600;
    color: var(--text);
    letter-spacing: 0.04em;
  }

  .ready code,
  .device-line {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text);
  }

  .muted {
    color: var(--text-3);
  }

  .progress-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .progress-top {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
    white-space: nowrap;
  }

  .progress-top > span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sub {
    color: var(--text-3);
  }

  .pct {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }

  .progress {
    height: 4px;
    border-radius: 2px;
    background: #1c1c1c;
    overflow: hidden;
  }

  .fill {
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    box-shadow: 0 0 10px rgba(223, 246, 255, 0.6);
    transition: width 180ms ease-out;
  }

  .msg {
    min-height: 56px;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 8px 8px 12px;
    border: 1px solid var(--line);
    border-radius: var(--r);
    font-size: 13px;
  }

  .msg > :global(svg) {
    flex: none;
    margin-top: 2px;
  }

  .msg > div {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .msg strong {
    font-weight: 600;
  }

  .msg.ok {
    border-color: rgba(168, 230, 200, 0.25);
    background: rgba(168, 230, 200, 0.05);
  }

  .msg.ok > :global(svg) {
    color: var(--ok);
  }

  .msg.warn {
    border-color: rgba(232, 200, 122, 0.3);
    background: rgba(232, 200, 122, 0.05);
  }

  .msg.warn > :global(svg) {
    color: #e8c87a;
  }

  .msg.error {
    border-color: rgba(242, 147, 138, 0.3);
    background: var(--danger-soft);
  }

  .error-actions {
    display: flex;
    gap: 4px;
    flex: none;
  }

  .msg.error > :global(svg),
  .msg.error strong {
    color: var(--danger);
  }

  .message {
    color: var(--text);
    font-size: 12px;
  }

  .detail {
    color: var(--text-2);
    font-size: 12px;
  }

  .btn.small {
    height: 26px;
    padding: 0 8px;
    font-size: 12px;
  }

  .flash-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 20px;
    border: 1px solid var(--accent);
    border-radius: var(--r);
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
    box-shadow: 0 0 24px -4px rgba(223, 246, 255, 0.45);
    transition: box-shadow 160ms, background-color 160ms, opacity 160ms;
  }

  .flash-btn:hover:not(:disabled) {
    background: #fff;
    box-shadow: 0 0 32px -2px rgba(223, 246, 255, 0.6);
  }

  .flash-btn:disabled {
    background: var(--surface-3);
    border-color: var(--line-2);
    color: var(--text-3);
    box-shadow: none;
  }

  .flash-btn :global(svg) {
    width: 16px;
    height: 16px;
  }
</style>
