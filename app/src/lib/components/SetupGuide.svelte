<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import * as api from "../api";
  import { setup } from "../setup.svelte";
  import type { Board, Port, SetupCheck, SetupStatus } from "../types";
  import Icon from "./Icon.svelte";

  const STEPS = ["Welcome", "Prepare your computer", "Plug in your DeskBunny", "All set"] as const;

  let step = $state(0);
  let status = $state<SetupStatus | null>(null);
  let statusError = $state<string | null>(null);
  let checking = $state(false);
  /** Per-action result: running, or the message it finished with. */
  let runs = $state<Record<string, { running: boolean; ok?: boolean; message?: string }>>({});
  let copied = $state<string | null>(null);

  let ports = $state<Port[]>([]);
  let boards = $state<Board[]>([]);
  let probe = $state<{ port: string; running: boolean; line?: string | null; error?: string } | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  let heading = $state<HTMLHeadingElement | null>(null);
  let dialog = $state<HTMLDivElement | null>(null);

  const os = $derived(status?.os ?? "linux");
  const todo = $derived(status?.checks.filter((c) => c.state === "action").length ?? 0);
  const found = $derived(ports.find((p) => p.guessedBoard) ?? ports.find((p) => p.vid));

  const boardName = (id?: string) => boards.find((b) => b.id === id)?.name;

  async function refresh() {
    checking = true;
    statusError = null;
    try {
      status = await api.setupStatus();
    } catch (e) {
      statusError = api.errorText(e);
    } finally {
      checking = false;
    }
  }

  async function runAction(check: SetupCheck) {
    if (!check.action) return;
    const id = check.action.id;
    runs[id] = { running: true };
    try {
      runs[id] = { running: false, ok: true, message: await api.setupRun(id) };
      await refresh();
    } catch (e) {
      runs[id] = { running: false, ok: false, message: api.errorText(e) };
    }
  }

  async function copy(text: string, key: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = key;
      setTimeout(() => copied === key && (copied = null), 1600);
    } catch {
      copied = null;
    }
  }

  async function pollPorts() {
    try {
      ports = await api.listPorts();
    } catch {
      ports = [];
    }
  }

  async function checkBoard(port: Port) {
    probe = { port: port.path, running: true };
    try {
      probe = { port: port.path, running: false, line: await api.deviceInfo(port.path) };
    } catch (e) {
      probe = { port: port.path, running: false, error: api.errorText(e) };
    }
  }

  async function go(to: number) {
    step = Math.max(0, Math.min(STEPS.length - 1, to));
    await tick();
    heading?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      setup.close();
    } else if (e.key === "Tab" && dialog) {
      // Keep focus inside the dialog.
      const items = dialog.querySelectorAll<HTMLElement>(
        "button:not(:disabled), [href], summary, [tabindex]:not([tabindex='-1'])",
      );
      if (!items.length) return;
      const first = items[0];
      const last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  // Port polling only while the plug-in step is visible.
  $effect(() => {
    if (step === 2) {
      pollPorts();
      pollTimer = setInterval(pollPorts, 1500);
    }
    return () => {
      if (pollTimer) clearInterval(pollTimer);
      pollTimer = null;
    };
  });

  onMount(() => {
    refresh();
    api.listBoards().then((b) => (boards = b)).catch(() => {});
    heading?.focus();
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  const STATE_LABEL: Record<SetupCheck["state"], string> = {
    ok: "Ready",
    action: "To do",
    warn: "Heads up",
    info: "Good to know",
  };
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="setup-title"
    bind:this={dialog}
  >
    <header class="top">
      <ol class="steps" aria-label="Setup steps">
        {#each STEPS as label, i}
          <li class:done={i < step} class:current={i === step} aria-current={i === step ? "step" : undefined}>
            <span class="num">{#if i < step}<Icon name="check" size={12} />{:else}{i + 1}{/if}</span>
            <span class="step-label">{label}</span>
          </li>
        {/each}
      </ol>
      <button class="btn ghost icon" aria-label="Close setup guide" onclick={() => setup.close()}>
        <Icon name="close" />
      </button>
    </header>

    <div class="body">
      {#if step === 0}
        <div class="welcome">
          <div class="eyes" aria-hidden="true"><span></span><span></span></div>
          <h1 id="setup-title" tabindex="-1" bind:this={heading}>Welcome to DeskBunny</h1>
          <p class="lead">
            A one-minute setup so this computer can flash faces onto your DeskBunny. No Arduino IDE needed.
          </p>
          <ul class="needs">
            <li><Icon name="usb" /> Your DeskBunny: an Arduino with the 0.96" OLED wired up</li>
            <li><Icon name="bolt" /> A USB cable that carries data. Some phone cables only charge.</li>
          </ul>
          {#if status}
            <p class="os">Setting up for <strong>{status.osLabel}</strong></p>
          {/if}
        </div>
      {:else if step === 1}
        <h1 id="setup-title" tabindex="-1" bind:this={heading}>Prepare your computer</h1>
        <p class="lead">
          {#if !status && !statusError}
            Checking what this computer needs…
          {:else if todo}
            {todo === 1 ? "One thing" : `${todo} things`} to sort out. Each takes a click; your system asks for permission where needed.
          {:else}
            Everything DeskBunny needs is in place.
          {/if}
        </p>

        {#if statusError}
          <p class="banner error" role="alert">Couldn't run the checks: {statusError}</p>
        {/if}

        <ul class="checks">
          {#each status?.checks ?? [] as c (c.id)}
            {@const run = c.action ? runs[c.action.id] : undefined}
            <li class="check {c.state}">
              <span class="badge" title={STATE_LABEL[c.state]}>
                {#if c.state === "ok"}<Icon name="check" size={14} />
                {:else if c.state === "warn"}<Icon name="alert" size={14} />
                {:else if c.state === "info"}<Icon name="info" size={14} />
                {:else}<span class="dot"></span>{/if}
              </span>
              <div class="check-main">
                <div class="check-head">
                  <span class="check-title">{c.title}</span>
                  <span class="chip">{STATE_LABEL[c.state]}</span>
                </div>
                <p class="detail">{c.detail}</p>

                {#if c.action || c.link}
                  <div class="row">
                    {#if c.action && c.state !== "ok"}
                      <button class="btn primary" disabled={run?.running} onclick={() => runAction(c)}>
                        {#if run?.running}<span class="spinner" aria-hidden="true"></span>{c.action.id === "install_ch340_driver" ? "Downloading, then asking permission…" : "Waiting for permission…"}{:else}{c.action.label}{/if}
                      </button>
                    {/if}
                    {#if c.link}
                      <button class="btn" onclick={() => api.setupOpenLink(c.link!.id)}>
                        {c.link.label}<Icon name="external" size={14} />
                      </button>
                    {/if}
                  </div>
                {/if}

                {#if run?.message}
                  <p class="banner" class:error={!run.ok} class:success={run.ok} role="status">{run.message}</p>
                {/if}

                {#if c.commands?.length && c.state !== "ok"}
                  <details class="manual" open={run?.ok === false}>
                    <summary>Prefer the terminal? Run it yourself</summary>
                    <div class="code">
                      <pre>{c.commands.join("\n")}</pre>
                      <button class="btn ghost copy" onclick={() => copy(c.commands!.join("\n"), c.id)}>
                        {#if copied === c.id}<Icon name="check" size={14} />Copied{:else}<Icon name="copy" size={14} />Copy{/if}
                      </button>
                    </div>
                  </details>
                {/if}
              </div>
            </li>
          {/each}
        </ul>

        <button class="btn ghost recheck" disabled={checking} onclick={refresh}>
          <Icon name="refresh" size={14} />{checking ? "Checking…" : "Check again"}
        </button>
      {:else if step === 2}
        <h1 id="setup-title" tabindex="-1" bind:this={heading}>Plug in your DeskBunny</h1>
        <p class="lead">Connect the board with a USB cable. DeskBunny spots it automatically.</p>

        {#if found}
          <div class="found" role="status">
            <span class="badge ok"><Icon name="check" size={16} /></span>
            <div>
              <p class="found-title">
                Found {boardName(found.guessedBoard) ?? found.product ?? "a USB serial device"}
              </p>
              <p class="detail mono">{found.path}{#if found.vid} · {found.vid}:{found.pid}{/if}</p>
            </div>
            <button class="btn" disabled={probe?.running} onclick={() => checkBoard(found!)}>
              {probe?.running ? "Checking…" : "Check what's on it"}
            </button>
          </div>
          {#if probe && !probe.running && probe.port === found.path}
            <p class="banner" class:error={!!probe.error} class:success={!!probe.line} role="status">
              {#if probe.error}
                {probe.error}
              {:else if probe.line}
                Already running DeskBunny: <span class="mono">{probe.line}</span>
              {:else}
                It's not running DeskBunny yet. You'll flash it in the next step.
              {/if}
            </p>
          {/if}
          {#if !found.guessedBoard}
            <p class="hint">
              DeskBunny can't tell which board this is (Nano clones look like any USB serial adapter). You'll pick the board
              by hand next to the Flash button.
            </p>
          {/if}
          {#if ports.length > 1}
            <p class="hint">{ports.length} serial ports are connected; you can choose another one next to the Flash button.</p>
          {/if}
        {:else}
          <div class="waiting" role="status">
            <span class="pulse" aria-hidden="true"></span>
            <p>Waiting for a board…</p>
          </div>
          <ul class="tips">
            <li>Use a data cable and plug straight into the computer, not a hub or monitor port.</li>
            {#if os === "linux"}
              <li>If you just allowed USB access, unplug the board and plug it back in.</li>
            {:else if os === "macos"}
              <li>If macOS asks "Allow accessory to connect?", choose Allow.</li>
            {:else}
              <li>Windows may take a few seconds to install a driver the first time. Watch for a notification.</li>
            {/if}
            <li>Nano clone not showing up? Go back a step and look at the CH340 note.</li>
          </ul>
        {/if}
      {:else}
        <div class="welcome">
          <div class="eyes happy" aria-hidden="true"><span></span><span></span></div>
          <h1 id="setup-title" tabindex="-1" bind:this={heading}>You're all set</h1>
          <ol class="howto">
            <li><span class="n">1</span><span>Pick a drone scene on the left, or tweak one in the editor.</span></li>
            <li><span class="n">2</span><span>Check the port and board at the bottom. DeskBunny picks them when it can.</span></li>
            <li>
              <span class="n">3</span><span>Press <strong>Flash to DeskBunny</strong>. Your board keeps the scene, even unplugged from the computer.</span>
            </li>
          </ol>
          <p class="hint">Need this again? It's under the <Icon name="help" size={13} /> button next to the DeskBunny name.</p>
        </div>
      {/if}
    </div>

    <footer class="bottom">
      {#if step < STEPS.length - 1}
        <button class="btn ghost" onclick={() => setup.close()}>Skip setup</button>
      {:else}
        <span></span>
      {/if}
      <div class="nav">
        {#if step > 0}
          <button class="btn" onclick={() => go(step - 1)}>Back</button>
        {/if}
        {#if step < STEPS.length - 1}
          <button class="btn primary" onclick={() => go(step + 1)}>
            {step === 0 ? "Get started" : step === 2 && !found ? "Continue without a board" : "Continue"}
          </button>
        {:else}
          <button class="btn primary" onclick={() => setup.close()}>Start flying</button>
        {/if}
      </div>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(6px);
    animation: fade 160ms ease-out;
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(680px, 100%);
    max-height: min(640px, 100%);
    border: 1px solid var(--line-2);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow:
      0 24px 80px -20px rgba(0, 0, 0, 0.9),
      0 0 0 1px rgba(223, 246, 255, 0.03);
    animation: rise 200ms ease-out;
  }

  .top {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 12px 12px 24px;
    border-bottom: 1px solid var(--line);
  }

  .steps {
    display: flex;
    gap: 20px;
    flex: 1;
    margin: 0;
    padding: 0;
    list-style: none;
    min-width: 0;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
  }

  .steps li.current {
    color: var(--text);
  }

  .steps li.done {
    color: var(--text-2);
  }

  .num {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border: 1px solid var(--line-2);
    border-radius: 50%;
    font-family: var(--mono);
    font-size: 11px;
  }

  .current .num {
    border-color: var(--accent-line);
    color: var(--accent);
    box-shadow: 0 0 10px -2px rgba(223, 246, 255, 0.35);
  }

  .done .num {
    border-color: transparent;
    background: var(--surface-3);
    color: var(--ok);
  }

  .step-label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .steps li:not(.current) .step-label {
    display: none;
  }

  .body {
    flex: 1;
    overflow-y: auto;
    padding: 28px 32px 24px;
  }

  h1 {
    margin: 0 0 8px;
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  h1:focus {
    outline: none;
  }

  .lead {
    margin: 0 0 20px;
    color: var(--text-2);
  }

  .welcome {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding-top: 8px;
  }

  .welcome .lead {
    max-width: 440px;
  }

  /* Two OLED-white eyes that blink, echoing the device. */
  .eyes {
    display: flex;
    gap: 14px;
    margin-bottom: 20px;
    padding: 18px 24px;
    border: 1px solid var(--line-2);
    border-radius: var(--r);
    background: #000;
  }

  .eyes span {
    width: 30px;
    height: 30px;
    border-radius: 8px;
    background: var(--accent);
    box-shadow: 0 0 14px rgba(223, 246, 255, 0.45);
    animation: blink 4.2s infinite;
  }

  .eyes.happy span {
    height: 16px;
    margin-top: 6px;
    border-radius: 16px 16px 4px 4px;
    animation: none;
  }

  .needs {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0 0 20px;
    padding: 0;
    list-style: none;
    text-align: left;
  }

  .needs li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border: 1px solid var(--line);
    border-radius: var(--r);
    background: var(--surface-2);
    font-size: 13px;
    color: var(--text-2);
  }

  .needs :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .os {
    margin: 0;
    font-size: 12px;
    color: var(--text-3);
  }

  .os strong {
    color: var(--text-2);
    font-weight: 500;
  }

  .checks {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
  }

  .check {
    display: flex;
    gap: 14px;
    padding: 14px 16px;
    border: 1px solid var(--line);
    border-radius: var(--r);
    background: var(--surface-2);
  }

  .check.action {
    border-color: var(--accent-line);
  }

  .badge {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--surface-3);
    color: var(--text-2);
  }

  .check.ok .badge,
  .badge.ok {
    color: var(--ok);
    background: rgba(168, 230, 200, 0.1);
  }

  .check.warn .badge {
    color: var(--warn);
    background: rgba(240, 201, 135, 0.1);
  }

  .check.action .badge {
    background: var(--accent-soft);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 8px rgba(223, 246, 255, 0.6);
  }

  .check-main {
    flex: 1;
    min-width: 0;
  }

  .check-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .check-title {
    font-weight: 600;
  }

  .chip {
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--surface-3);
    font-size: 11px;
    color: var(--text-3);
  }

  .check.action .chip {
    color: var(--accent);
    background: var(--accent-soft);
  }

  .check.ok .chip {
    color: var(--ok);
  }

  .check.warn .chip {
    color: var(--warn);
  }

  .detail {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
  }

  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-ink);
    font-weight: 600;
  }

  .btn.primary:hover:not(:disabled) {
    background: #fff;
    border-color: #fff;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid rgba(10, 20, 24, 0.25);
    border-top-color: var(--accent-ink);
    border-radius: 50%;
    animation: spin 700ms linear infinite;
  }

  .banner {
    margin: 12px 0 0;
    padding: 8px 12px;
    border: 1px solid var(--line-2);
    border-radius: var(--r-sm);
    background: var(--surface-3);
    font-size: 13px;
    color: var(--text-2);
  }

  .banner.success {
    border-color: rgba(168, 230, 200, 0.3);
    background: rgba(168, 230, 200, 0.06);
    color: var(--ok);
  }

  .banner.error {
    border-color: rgba(242, 147, 138, 0.3);
    background: var(--danger-soft);
    color: var(--danger);
  }

  .manual {
    margin-top: 12px;
  }

  .manual summary {
    width: fit-content;
    font-size: 12px;
    color: var(--text-3);
    cursor: pointer;
    border-radius: 4px;
  }

  .manual summary:hover {
    color: var(--text-2);
  }

  .code {
    position: relative;
    margin-top: 8px;
  }

  pre {
    margin: 0;
    padding: 12px 84px 12px 12px;
    max-height: 180px;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: #000;
    font-family: var(--mono);
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--text-2);
    white-space: pre;
  }

  .copy {
    position: absolute;
    top: 6px;
    right: 6px;
    height: 26px;
    font-size: 12px;
  }

  .recheck {
    font-size: 12px;
  }

  .found {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 16px;
    border: 1px solid rgba(168, 230, 200, 0.28);
    border-radius: var(--r);
    background: rgba(168, 230, 200, 0.05);
  }

  .found > div {
    flex: 1;
    min-width: 0;
  }

  .found .badge {
    width: 32px;
    height: 32px;
  }

  .found-title {
    margin: 0 0 2px;
    font-weight: 600;
  }

  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }

  .hint {
    margin: 12px 0 0;
    font-size: 12px;
    color: var(--text-3);
  }

  .hint :global(svg) {
    vertical-align: -2px;
  }

  .waiting {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 20px;
    border: 1px dashed var(--line-2);
    border-radius: var(--r);
    color: var(--text-2);
  }

  .waiting p {
    margin: 0;
  }

  .pulse {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.6s ease-in-out infinite;
  }

  .tips {
    margin: 16px 0 0;
    padding-left: 18px;
    font-size: 13px;
    color: var(--text-2);
  }

  .tips li + li {
    margin-top: 6px;
  }

  .howto {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 16px 0 20px;
    padding: 0;
    list-style: none;
    text-align: left;
    max-width: 460px;
  }

  .howto li {
    display: flex;
    gap: 12px;
    align-items: baseline;
    color: var(--text-2);
  }

  .howto .n {
    display: grid;
    place-items: center;
    flex: none;
    width: 22px;
    height: 22px;
    border: 1px solid var(--accent-line);
    border-radius: 50%;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--accent);
    transform: translateY(-1px);
  }

  .howto strong {
    color: var(--text);
  }

  .bottom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-top: 1px solid var(--line);
  }

  .nav {
    display: flex;
    gap: 8px;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.99);
    }
  }

  @keyframes blink {
    0%,
    92%,
    100% {
      transform: scaleY(1);
    }
    95% {
      transform: scaleY(0.1);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
      box-shadow: 0 0 0 0 rgba(223, 246, 255, 0.4);
    }
    50% {
      opacity: 1;
      box-shadow: 0 0 0 8px rgba(223, 246, 255, 0);
    }
  }
</style>
