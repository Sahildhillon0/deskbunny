<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./lib/state.svelte";
  import { isTauri } from "./lib/api";
  import Library from "./lib/components/Library.svelte";
  import Simulator from "./lib/components/Simulator.svelte";
  import Editor from "./lib/components/Editor.svelte";
  import FlashBar from "./lib/components/FlashBar.svelte";
  import SetupGuide from "./lib/components/SetupGuide.svelte";
  import UpdateDialog from "./lib/components/UpdateDialog.svelte";
  import DrawingEditor from "./lib/components/DrawingEditor.svelte";
  import { setup } from "./lib/setup.svelte";
  import { updater } from "./lib/updater.svelte";

  onMount(() => {
    app.load();
    setup.openIfFirstRun();
    updater.checkOnLaunch();
  });
</script>

<div class="shell">
  <aside class="library" aria-label="Drone library">
    <Library />
  </aside>

  <main class="stage">
    {#if app.loadError}
      <p class="load-error" role="alert">Couldn't load drones: {app.loadError}</p>
    {/if}
    <Simulator />
    {#if !isTauri}
      <p class="mock-note">Browser preview with a mock backend. Run the Tauri app for the real engine.</p>
    {/if}
  </main>

  <aside class="editor" aria-label="Drone editor">
    <Editor />
  </aside>

  <footer class="flash">
    <FlashBar />
  </footer>
</div>

{#if app.drawOpen}
  <DrawingEditor />
{:else if setup.open}
  <SetupGuide />
{:else if updater.open}
  <UpdateDialog />
{/if}

<style>
  .shell {
    display: grid;
    grid-template-columns: 272px minmax(0, 1fr) 336px;
    grid-template-rows: minmax(0, 1fr) auto;
    grid-template-areas:
      "library stage editor"
      "flash flash flash";
    height: 100vh;
    min-width: 960px;
    min-height: 640px;
  }

  .library {
    grid-area: library;
    border-right: 1px solid var(--line);
    background: var(--surface);
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .stage {
    grid-area: stage;
    position: relative;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background:
      radial-gradient(ellipse 70% 55% at 50% 45%, rgba(223, 246, 255, 0.035), transparent 70%),
      var(--bg);
  }

  .editor {
    grid-area: editor;
    border-left: 1px solid var(--line);
    background: var(--surface);
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .flash {
    grid-area: flash;
    border-top: 1px solid var(--line);
    background: var(--surface);
  }

  .load-error {
    margin: 16px 24px 0;
    padding: 8px 12px;
    border: 1px solid rgba(242, 147, 138, 0.3);
    border-radius: var(--r-sm);
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 13px;
  }

  .mock-note {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 12px;
    margin: 0;
    text-align: center;
    font-size: 11px;
    color: var(--text-3);
    pointer-events: none;
  }
</style>
