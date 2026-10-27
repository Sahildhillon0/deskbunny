<script lang="ts">
  import { onMount, tick } from "svelte";
  import * as api from "../api";
  import { updater } from "../updater.svelte";
  import Icon from "./Icon.svelte";

  let heading = $state<HTMLHeadingElement | null>(null);
  let dialog = $state<HTMLDivElement | null>(null);

  const mb = (n: number) => `${(n / 1_000_000).toFixed(1)} MB`;
  const percent = $derived(updater.total ? Math.min(100, Math.round((updater.downloaded / updater.total) * 100)) : null);

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      updater.close();
    } else if (e.key === "Tab" && dialog) {
      // Keep focus inside the dialog.
      const items = dialog.querySelectorAll<HTMLElement>("button:not(:disabled), [href]");
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

  onMount(async () => {
    await tick();
    heading?.focus();
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="update-title" bind:this={dialog}>
    <header class="top">
      <h1 id="update-title" tabindex="-1" bind:this={heading}>
        {#if updater.phase === "checking"}
          Checking for updates…
        {:else if updater.phase === "latest"}
          You're up to date
        {:else if updater.phase === "error"}
          Update problem
        {:else if updater.phase === "installing"}
          Updating to {updater.info?.version}
        {:else}
          DeskBunny {updater.info?.version} is out
        {/if}
      </h1>
      <button class="btn ghost icon" aria-label="Close" onclick={() => updater.close()} disabled={updater.phase === "installing"}>
        <Icon name="close" />
      </button>
    </header>

    <div class="body" aria-live="polite">
      {#if updater.phase === "checking"}
        <p class="muted"><span class="spinner" aria-hidden="true"></span> Looking at the latest release on GitHub.</p>
      {:else if updater.phase === "latest"}
        <p class="muted">
          <Icon name="check" /> Version {updater.current || updater.info?.currentVersion} is the newest release.
        </p>
      {:else if updater.phase === "error"}
        <p class="error" role="alert">{updater.error}</p>
      {:else if updater.info}
        <p class="versions">
          <span class="mono">{updater.info.currentVersion}</span>
          <span aria-hidden="true">→</span>
          <span class="mono new">{updater.info.version}</span>
          {#if updater.info.date}<span class="date">released {updater.info.date}</span>{/if}
        </p>
        {#if updater.phase === "installing"}
          <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={percent ?? undefined}>
            <span style:width={percent === null ? "100%" : `${percent}%`} class:indeterminate={percent === null}></span>
          </div>
          <p class="muted small">
            {updater.downloaded ? mb(updater.downloaded) : "Starting download"}{updater.total ? ` of ${mb(updater.total)}` : ""}.
            DeskBunny restarts when it's done.
          </p>
        {:else if updater.info.notes}
          <pre class="notes">{updater.info.notes}</pre>
        {/if}
      {/if}
    </div>

    <footer class="actions">
      {#if updater.phase === "available"}
        <button class="btn ghost" onclick={() => updater.close()}>Later</button>
        <button class="btn primary" onclick={() => updater.install()}><Icon name="download" /> Update now</button>
      {:else if updater.phase === "error"}
        <button class="btn ghost" onclick={() => api.updateOpenReleases().catch(() => {})}>
          <Icon name="external" /> Releases page
        </button>
        <button class="btn primary" onclick={() => updater.check()}><Icon name="refresh" /> Try again</button>
      {:else if updater.phase !== "installing"}
        <button class="btn primary" onclick={() => updater.close()} disabled={updater.phase === "checking"}>OK</button>
      {/if}
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
    animation: fade 160ms ease-out;
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(460px, 100%);
    max-height: min(560px, 100%);
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

  h1 {
    flex: 1;
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    outline: none;
  }

  .body {
    min-height: 0;
    overflow-y: auto;
    padding: 20px 24px;
  }

  p {
    margin: 0;
  }

  .muted {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    font-size: 13px;
  }

  .small {
    margin-top: 10px;
    font-size: 12px;
    color: var(--text-3);
  }

  .error {
    padding: 8px 12px;
    border: 1px solid rgba(242, 147, 138, 0.3);
    border-radius: var(--r-sm);
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 13px;
  }

  .versions {
    display: flex;
    align-items: baseline;
    gap: 10px;
    font-size: 14px;
    color: var(--text-2);
  }

  .mono {
    font-family: var(--mono);
  }

  .new {
    color: var(--accent);
  }

  .date {
    margin-left: auto;
    font-size: 12px;
    color: var(--text-3);
  }

  .notes {
    margin: 16px 0 0;
    padding: 12px;
    max-height: 220px;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-family: inherit;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-2);
    white-space: pre-wrap;
  }

  .bar {
    position: relative;
    height: 4px;
    margin-top: 18px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--line);
  }

  .bar span {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--accent);
    transition: width 120ms linear;
  }

  .bar span.indeterminate {
    width: 30% !important;
    animation: slide 1.1s ease-in-out infinite;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 1.5px solid var(--line-2);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 800ms linear infinite;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
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

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .backdrop,
    .dialog,
    .spinner,
    .bar span.indeterminate {
      animation: none;
    }
  }
</style>
