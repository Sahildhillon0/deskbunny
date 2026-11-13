<script lang="ts">
  // Live mode: the app keeps the board's port open and drives its screen
  // (the flashed face, designed screens, Claude, notifications), in rotation or
  // pinned. docs/ARCHITECTURE.md, "Live mode".
  import { onMount, tick } from "svelte";
  import * as api from "../api";
  import { OledRenderer } from "../oled";
  import { app } from "../state.svelte";
  import type {
    ClaudeView,
    Design,
    HooksStatus,
    LiveConfig,
    LiveStatus,
    Place,
    Port,
    ScreenCfg,
    ScreenRef,
    WeatherNow,
    UsageHarness,
    UsageNow,
  } from "../types";
  import Icon from "./Icon.svelte";
  import Switch from "./Switch.svelte";

  const CLAUDE_TEXT: Record<ClaudeView["state"], string> = {
    idle: "Idle",
    starting: "Starting",
    working: "Working",
    done: "Done",
    needsYou: "Needs you",
  };
  const STATE_TEXT: Record<LiveStatus["state"], string> = {
    off: "Off",
    paused: "Paused while flashing",
    connecting: "Connecting…",
    connected: "Connected",
    error: "Problem",
  };

  let cfg = $state<LiveConfig | null>(null);
  let status = $state<LiveStatus | null>(null);
  let designs = $state<Design[]>([]);
  /** Built-in designs, offered in "Add a screen" when not saved yet. */
  let templates = $state<Design[]>([]);
  let ports = $state<Port[]>([]);
  let error = $state<string | null>(null);
  let preview = $state<ScreenRef>({ kind: "design", design: "clock" });
  let heading = $state<HTMLHeadingElement | null>(null);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let hooks = $state<HooksStatus | null>(null);
  let harnesses = $state<UsageHarness[]>([]);
  let usage = $state<UsageNow | null>(null);
  const harness = $derived(harnesses.find((h) => h.id === cfg?.usage.harness) ?? null);
  let hooksBusy = $state(false);
  let claude = $state<ClaudeView | null>(null);
  let ignoreText = $state("");
  let weather = $state<WeatherNow | null>(null);
  let placeQuery = $state("");
  let places = $state<Place[]>([]);
  let searching = $state(false);

  const same = (a: ScreenRef | null | undefined, b: ScreenRef | null | undefined) =>
    !!a && !!b && a.kind === b.kind && (a.design ?? null) === (b.design ?? null);

  function screenName(s: ScreenRef): string {
    if (s.kind === "face") return "Face";
    if (s.kind === "claude") return "Claude";
    if (s.kind === "notify") return "Notification";
    return designs.find((d) => d.id === s.design)?.name ?? `(missing design "${s.design}")`;
  }

  function screenHint(s: ScreenRef): string {
    if (s.kind === "face") return "The face you flashed, animated by the board.";
    if (s.kind === "claude")
      return "Claude Code sessions. Joins the rotation while Claude is busy; takes over when it starts, finishes or needs you.";
    return "A designed screen, drawn by this app every second.";
  }

  /** Designs not in the rotation yet. */
  const unused = $derived(designs.filter((d) => !cfg?.screens.some((s) => s.kind === "design" && s.design === d.id)));
  /** Built-in designs this install does not have (new ones, or deleted ones). */
  const newTemplates = $derived(templates.filter((t) => !designs.some((d) => d.id === t.id)));

  const AQI_BAND_US = (a: number) => (a <= 50 ? "good" : a <= 100 ? "moderate" : a <= 150 ? "unhealthy for some" : a <= 200 ? "unhealthy" : a <= 300 ? "very unhealthy" : "hazardous");
  const AQI_BAND_IN = (a: number) => (a <= 50 ? "good" : a <= 100 ? "satisfactory" : a <= 200 ? "moderate" : a <= 300 ? "poor" : a <= 400 ? "very poor" : "severe");
  const AQI_BAND_EU = (a: number) => (a <= 20 ? "good" : a <= 40 ? "fair" : a <= 60 ? "moderate" : a <= 80 ? "poor" : a <= 100 ? "very poor" : "extremely poor");

  async function setHooks(install: boolean) {
    hooksBusy = true;
    try {
      hooks = await (install ? api.claudeHooksInstall() : api.claudeHooksRemove());
      error = null;
    } catch (e) {
      error = api.errorText(e);
    } finally {
      hooksBusy = false;
    }
  }

  async function apply(next: LiveConfig) {
    try {
      cfg = await api.liveSetConfig($state.snapshot(next) as LiveConfig);
      error = null;
    } catch (e) {
      error = api.errorText(e);
    }
  }

  function update(fn: (c: LiveConfig) => void) {
    if (!cfg) return;
    const next = $state.snapshot(cfg) as LiveConfig;
    fn(next);
    apply(next);
  }

  function move(i: number, d: -1 | 1) {
    update((c) => {
      const j = i + d;
      if (j < 0 || j >= c.screens.length) return;
      [c.screens[i], c.screens[j]] = [c.screens[j], c.screens[i]];
    });
  }

  async function addDesign(value: string) {
    if (!value) return;
    let id = value;
    if (value.startsWith("template:")) {
      // Save the built-in design first, then put it in the rotation.
      const t = templates.find((x) => x.id === value.slice(9));
      if (!t) return;
      try {
        designs = await api.designSave(structuredClone($state.snapshot(t) as Design));
      } catch (e) {
        error = api.errorText(e);
        return;
      }
      id = t.id;
    }
    update((c) => c.screens.push({ kind: "design", design: id, enabled: true, seconds: 15 }));
    preview = { kind: "design", design: id };
  }

  function removeScreen(i: number) {
    update((c) => c.screens.splice(i, 1));
  }

  async function pin(s: ScreenRef) {
    try {
      await api.livePin(same(status?.pinned, s) ? null : { kind: s.kind, design: s.design });
    } catch (e) {
      error = api.errorText(e);
    }
  }

  function edit(id: string | null) {
    const d = id ? designs.find((x) => x.id === id) : null;
    app.designing = d
      ? structuredClone($state.snapshot(d) as Design)
      : { id: `d-${Date.now().toString(36)}`, name: "My screen", elements: [] };
  }

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  function searchPlaces() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(async () => {
      if (placeQuery.trim().length < 2) {
        places = [];
        return;
      }
      searching = true;
      try {
        places = await api.weatherSearch(placeQuery);
        error = null;
      } catch (e) {
        error = api.errorText(e);
      } finally {
        searching = false;
      }
    }, 350);
  }

  function choosePlace(p: Place) {
    update((c) => (c.weather.place = p));
    places = [];
    placeQuery = "";
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      app.liveOpen = false;
    }
  }

  // Live toggle bound to the switch; changes apply at once.
  let enabled = $state(false);
  $effect(() => {
    if (cfg && enabled !== cfg.enabled) update((c) => (c.enabled = enabled));
  });

  onMount(() => {
    let dead = false;
    let unlisten: (() => void) | null = null;
    (async () => {
      try {
        const [c, s, p, d, cat, hs] = await Promise.all([
          api.liveGetConfig(),
          api.liveStatus(),
          api.listPorts(),
          api.designList(),
          api.designCatalog(),
          api.usageHarnesses(),
        ]);
        if (dead) return;
        harnesses = hs;
        ports = p;
        designs = d;
        templates = cat.templates;
        // First time: default to the first port.
        if (!c.port && p.length) c.port = p[0].path;
        cfg = c;
        enabled = c.enabled;
        status = s;
        ignoreText = c.notify.ignoreApps.join(", ");
        const firstDesign = c.screens.find((x) => x.kind === "design");
        if (firstDesign) preview = { kind: "design", design: firstDesign.design };
        hooks = await api.claudeHooksStatus().catch((e) => {
          error = api.errorText(e);
          return null;
        });
      } catch (e) {
        error = api.errorText(e);
      }
      unlisten = await api.onLiveStatus((s) => (status = s));
      if (dead) unlisten();
      await tick();
      heading?.focus();
    })();

    // Preview loop: Clawd animates, clocks tick.
    let renderer: OledRenderer | null = null;
    let busy = false;
    const slowTimer = setInterval(async () => {
      claude = await api.claudeView().catch(() => claude);
      weather = await api.weatherNow().catch(() => weather);
      usage = await api.usageNow().catch(() => usage);
    }, 1000);
    const timer = setInterval(async () => {
      if (!canvas || busy || preview.kind === "face") return;
      if (!renderer) {
        renderer = new OledRenderer(canvas, { glow: 0.6, grid: false });
        renderer.resize(2);
      }
      busy = true;
      try {
        renderer.draw(await api.livePreview($state.snapshot(preview) as ScreenRef, performance.now()));
      } catch {
        // preview is best effort
      } finally {
        busy = false;
      }
    }, 66);
    return () => {
      dead = true;
      unlisten?.();
      clearInterval(timer);
      clearInterval(slowTimer);
      if (searchTimer) clearTimeout(searchTimer);
    };
  });

  const rowKey = (s: ScreenCfg) => `${s.kind}:${s.design ?? ""}`;
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="live-title">
    <header class="top">
      <h1 id="live-title" tabindex="-1" bind:this={heading}>Live screens</h1>
      <button class="btn ghost icon" aria-label="Close" onclick={() => (app.liveOpen = false)}>
        <Icon name="close" />
      </button>
    </header>

    {#if !cfg}
      <div class="body"><p class="muted">{error ?? "Loading…"}</p></div>
    {:else}
      <div class="body">
        <p class="muted small">
          With live mode on, DeskBunny keeps the board's port open and rotates your screens: the face, clocks, weather,
          Claude's status and notifications. Close the window and it keeps running in the tray. Without the app the board
          shows its face after 30 s.
        </p>

        <div class="row">
          <Switch label="Live mode" bind:checked={enabled} />
          <label class="label" for="live-port">Port</label>
          <select
            id="live-port"
            class="select"
            value={cfg.port ?? ""}
            onchange={(e) => update((c) => (c.port = (e.currentTarget as HTMLSelectElement).value || null))}
          >
            {#if !ports.length}<option value="">No ports found</option>{/if}
            {#each ports as p (p.path)}
              <option value={p.path}>{p.path}{p.product ? ` · ${p.product}` : ""}</option>
            {/each}
          </select>
        </div>

        <div class="status" class:ok={status?.state === "connected"} class:bad={status?.state === "error"} aria-live="polite">
          <span class="dot" aria-hidden="true"></span>
          <span>{status ? STATE_TEXT[status.state] : "…"}</span>
          {#if status?.state === "connected"}
            <code>{status.banner}</code>
            {#if status.screen}<span class="muted small">· showing {screenName(status.screen)}</span>{/if}
            {#if !status.clawd}<span class="muted small">· no Clawd on this board, Claude uses the face</span>{/if}
          {:else if status?.message}
            <span class="detail">{status.message}</span>
          {/if}
        </div>

        <section class="group" aria-labelledby="screens-h">
          <h2 id="screens-h">Screens</h2>
          <ol class="screens">
            {#each cfg.screens as s, i (rowKey(s))}
              <li class:on={same(status?.screen, s) && status?.state === "connected"}>
                <input
                  type="checkbox"
                  aria-label={`Show ${screenName(s)} in the rotation`}
                  checked={s.enabled}
                  onchange={(e) => update((c) => (c.screens[i].enabled = (e.currentTarget as HTMLInputElement).checked))}
                />
                <button class="name" title={screenHint(s)} onclick={() => (preview = { kind: s.kind, design: s.design })} class:sel={same(preview, s)}>
                  {screenName(s)}
                </button>
                <label class="secs">
                  <input
                    type="number"
                    min="3"
                    max="600"
                    value={s.seconds}
                    onchange={(e) => update((c) => (c.screens[i].seconds = Number((e.currentTarget as HTMLInputElement).value) || 15))}
                  />
                  s
                </label>
                <button class="btn ghost small" disabled={i === 0} aria-label="Move up" onclick={() => move(i, -1)}>↑</button>
                <button class="btn ghost small" disabled={i === cfg.screens.length - 1} aria-label="Move down" onclick={() => move(i, 1)}>↓</button>
                {#if s.kind === "design"}
                  <button class="btn ghost small" aria-label={`Edit ${screenName(s)}`} title="Edit" onclick={() => edit(s.design ?? null)}><Icon name="pencil" /></button>
                  <button class="btn ghost small" aria-label={`Remove ${screenName(s)} from the rotation`} title="Remove from rotation" onclick={() => removeScreen(i)}><Icon name="close" /></button>
                {/if}
                <button class="btn small" class:primary={same(status?.pinned, s)} onclick={() => pin(s)}>
                  {same(status?.pinned, s) ? "Pinned" : "Pin"}
                </button>
              </li>
            {/each}
          </ol>
          <div class="row">
            {#if unused.length || newTemplates.length}
              <select class="select" aria-label="Add a design to the rotation" value="" onchange={(e) => {
                addDesign((e.currentTarget as HTMLSelectElement).value);
                (e.currentTarget as HTMLSelectElement).value = "";
              }}>
                <option value="" disabled>Add a screen…</option>
                {#if unused.length}
                  <optgroup label="Your designs">
                    {#each unused as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
                  </optgroup>
                {/if}
                {#if newTemplates.length}
                  <optgroup label="Built-in">
                    {#each newTemplates as t (t.id)}<option value={`template:${t.id}`}>{t.name}</option>{/each}
                  </optgroup>
                {/if}
              </select>
            {/if}
            <button class="btn small" onclick={() => edit(null)}><Icon name="plus" /> New design</button>
          </div>
        </section>

        <div class="preview">
          {#if preview.kind === "face"}
            <p class="muted small">The face screen is the face you flashed; the main preview shows it.</p>
          {:else}
            <canvas bind:this={canvas} aria-label={`${screenName(preview)} preview`}></canvas>
          {/if}
          <div class="tabs" role="tablist" aria-label="Preview">
            <button role="tab" class="btn small ghost" class:primary={preview.kind === "claude"} aria-selected={preview.kind === "claude"} onclick={() => (preview = { kind: "claude" })}>Claude</button>
            <button role="tab" class="btn small ghost" class:primary={preview.kind === "notify"} aria-selected={preview.kind === "notify"} onclick={() => (preview = { kind: "notify" })}>Notification</button>
          </div>
        </div>

        <section class="group" aria-labelledby="weather-h">
          <h2 id="weather-h">Weather and air quality</h2>
          <p class="muted small">
            From Open-Meteo (free, no account), every 15 minutes. Designs show it with <code>{"{TEMP}"}</code>,
            <code>{"{COND}"}</code>, <code>{"{AQI}"}</code>, <code>{"{AQICAT}"}</code>, <code>{"{PM25}"}</code>, the weather icon
            and more; the built-in "Weather" and "Air quality" screens use them.
          </p>
          <div class="row">
            <div class="place">
              <input
                class="input grow"
                placeholder={cfg.weather.place ? `${cfg.weather.place.name} · change city…` : "Search for your city"}
                bind:value={placeQuery}
                oninput={searchPlaces}
                aria-label="City"
              />
              {#if places.length}
                <ul class="places" role="listbox" aria-label="Cities">
                  {#each places as p (`${p.lat},${p.lon}`)}
                    <li><button role="option" aria-selected="false" onclick={() => choosePlace(p)}><b>{p.name}</b> <span class="muted">{p.detail}</span></button></li>
                  {/each}
                </ul>
              {/if}
            </div>
            <select
              class="select narrow"
              aria-label="Units"
              value={cfg.weather.units}
              onchange={(e) => update((c) => (c.weather.units = (e.currentTarget as HTMLSelectElement).value as "metric" | "imperial"))}
            >
              <option value="metric">°C, km/h</option>
              <option value="imperial">°F, mph</option>
            </select>
            <select
              class="select narrow"
              aria-label="Air quality index"
              value={cfg.weather.aqi}
              onchange={(e) => update((c) => (c.weather.aqi = (e.currentTarget as HTMLSelectElement).value as "us" | "eu" | "in"))}
            >
              <option value="us">US AQI</option>
              <option value="eu">European AQI</option>
              <option value="in">Indian AQI (CPCB)</option>
            </select>
          </div>
          <p class="muted small" aria-live="polite">
            {#if searching}Searching…
            {:else if !cfg.weather.place}No city chosen.
            {:else if weather?.weather}
              {cfg.weather.place.name}{cfg.weather.place.detail ? `, ${cfg.weather.place.detail}` : ""}: <b>{Math.round(weather.weather.temp)}°{weather.weather.unit}</b>,
              {weather.weather.humidity}% humidity{#if weather.weather.aqi !== null}, AQI <b>{weather.weather.aqi}</b>
                ({(weather.weather.aqiScale === "EU" ? AQI_BAND_EU : weather.weather.aqiScale === "IN" ? AQI_BAND_IN : AQI_BAND_US)(
                  weather.weather.aqi,
                )}){/if}{weather.ageSecs !== null
                ? ` · updated ${Math.round(weather.ageSecs / 60)} min ago`
                : ""}
            {:else if weather?.error}<span class="err">{weather.error}</span>
            {:else}Fetching…{/if}
          </p>
        </section>

        <section class="group" aria-labelledby="usage-h">
          <h2 id="usage-h">AI usage</h2>
          <p class="muted small">
            Your plan's 5-hour and weekly usage for the chosen tool. Designs show it with <code>{"{U5H}"}</code>,
            <code>{"{UWK}"}</code>, <code>{"{U5HIN}"}</code>, <code>{"{UWKIN}"}</code> and <code>{"{USRC}"}</code>; the built-in
            "AI usage" screen uses them.
          </p>
          <div class="row">
            <label class="label" for="usage-harness">Show usage for</label>
            <select
              id="usage-harness"
              class="select"
              value={cfg.usage.harness}
              onchange={(e) => update((c) => (c.usage.harness = (e.currentTarget as HTMLSelectElement).value))}
            >
              {#each harnesses as h (h.id)}<option value={h.id}>{h.name}</option>{/each}
            </select>
          </div>
          <p class="muted small" aria-live="polite">
            {#if usage && usage.ageSecs !== null}
              5-hour <b>{usage.usage.five ?? "--"}%</b> (resets in {usage.usage.fiveIn}), weekly
              <b>{usage.usage.week ?? "--"}%</b> (resets in {usage.usage.weekIn}) · reported {usage.ageSecs < 60
                ? "just now"
                : `${Math.round(usage.ageSecs / 60)} min ago`}
            {:else if harness}No report yet. {harness.setup}{/if}
          </p>
        </section>

        <section class="group" aria-labelledby="claude-h">
          <h2 id="claude-h">Claude Code</h2>
          <p class="muted small">
            Hooks tell DeskBunny when a Claude Code session starts, works, needs your permission or finishes. Each hook
            runs <code>curl</code> to this app and gives up after a second, so Claude never waits on it.
          </p>
          <div class="row">
            {#if hooks?.installed}
              <span class="ok-text"><Icon name="check" /> Hooks installed</span>
              <button class="btn small" disabled={hooksBusy} onclick={() => setHooks(false)}>Remove</button>
            {:else}
              <button class="btn small primary" disabled={hooksBusy || !hooks} onclick={() => setHooks(true)}>
                Install hooks
              </button>
            {/if}
            <span class="muted small" title={hooks?.command}>in <code>{hooks?.settingsPath ?? "~/.claude/settings.json"}</code></span>
          </div>
          <div class="row">
            <label class="label" for="claude-style">Show Claude as</label>
            <select
              id="claude-style"
              class="select"
              value={cfg.claudeStyle}
              onchange={(e) => update((c) => (c.claudeStyle = (e.currentTarget as HTMLSelectElement).value as LiveConfig["claudeStyle"]))}
            >
              <option value="clawd">Clawd (the crab)</option>
              <option value="face">The face</option>
            </select>
          </div>
          <div class="row">
            <Switch
              label="Stay on Claude while it works"
              hint="On: the board shows Claude from your prompt until it's done, then the rotation resumes. Off: Claude shows for a few seconds and then takes turns with the other screens."
              bind:checked={() => cfg?.claudeHold ?? true, (v) => update((c) => (c.claudeHold = v))}
            />
            {#if claude}
              <span class="muted small">
                Now: <b>{CLAUDE_TEXT[claude.state]}</b>{claude.sessions ? ` · ${claude.caption}` : ""}{claude.sessions > 1
                  ? ` · ${claude.sessions} sessions`
                  : ""}
              </span>
            {/if}
          </div>
        </section>

        <section class="group" aria-labelledby="notify-h">
          <h2 id="notify-h">Notifications</h2>
          <div class="row wrap">
            <Switch label="Show notifications" bind:checked={() => cfg?.notify.enabled ?? true, (v) => update((c) => (c.notify.enabled = v))} />
            <Switch label="Desktop notifications" bind:checked={() => cfg?.notify.desktop ?? true, (v) => update((c) => (c.notify.desktop = v))} />
            <Switch
              label="Show a line of the message"
              hint="Normally the board shows the app and the sender in big letters. With this on, a short sender is followed by two small lines of the message."
              bind:checked={() => cfg?.notify.showBody ?? false, (v) => update((c) => (c.notify.showBody = v))}
            />
          </div>
          <div class="row">
            <label class="secs">
              On screen
              <input
                type="number"
                min="2"
                max="60"
                value={cfg.notify.seconds}
                onchange={(e) => update((c) => (c.notify.seconds = Number((e.currentTarget as HTMLInputElement).value) || 8))}
              />
              s
            </label>
            <label class="label" for="ignore-apps">Ignore</label>
            <input
              id="ignore-apps"
              class="input grow"
              placeholder="App names, comma separated"
              bind:value={ignoreText}
              onchange={() =>
                update((c) => (c.notify.ignoreApps = ignoreText.split(",").map((a) => a.trim()).filter(Boolean)))}
            />
            <button class="btn small" onclick={() => api.liveTestNotification().catch((e) => (error = api.errorText(e)))}>Test</button>
          </div>
          <p class="muted small">
            Scripts can post too: <code>curl -H 'X-DeskBunny: 1' -d 'Backup done' http://127.0.0.1:47807/notify</code>
          </p>
        </section>

        {#if error}<p class="error" role="alert">{error}</p>{/if}
      </div>
    {/if}
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
    width: min(600px, 100%);
    /* The body scrolls; the dialog never grows past the window. */
    max-height: calc(100vh - 48px);
    min-height: 0;
    border: 1px solid var(--line-2);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: 0 24px 80px -20px rgba(0, 0, 0, 0.9);
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
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-height: 0;
    padding: 16px 24px 20px;
    overflow: auto;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .row .select {
    flex: 1;
    min-width: 0;
  }

  .row .select.narrow {
    flex: none;
    width: auto;
  }

  .row.wrap {
    flex-wrap: wrap;
  }

  .muted {
    margin: 0;
    color: var(--text-3);
  }

  .small {
    font-size: 12px;
  }

  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-2);
  }

  .status code {
    font-size: 12px;
    color: var(--text-3);
  }

  .status .detail,
  .err {
    flex-basis: 100%;
    font-size: 12px;
    color: var(--danger);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-3);
  }

  .status.ok .dot {
    background: #7fe0a0;
  }

  .status.bad .dot {
    background: var(--danger);
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }

  h2 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }

  .screens {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .screens li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
  }

  .screens li.on {
    border-color: var(--line-2);
  }

  .screens .name {
    flex: 1;
    min-width: 0;
    text-align: left;
    background: none;
    border: none;
    color: var(--text-2);
    font: inherit;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }

  .screens .name.sel {
    color: var(--text);
    font-weight: 600;
  }

  .secs {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-3);
  }

  .secs input {
    width: 56px;
  }

  .input,
  .secs input {
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
  }

  .input:focus,
  .secs input:focus {
    border-color: var(--line-2);
    outline: none;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .place {
    position: relative;
    flex: 1;
    display: flex;
    min-width: 0;
  }

  .places {
    position: absolute;
    top: 34px;
    left: 0;
    right: 0;
    z-index: 2;
    margin: 0;
    padding: 4px;
    list-style: none;
    border: 1px solid var(--line-2);
    border-radius: var(--r-sm);
    background: var(--surface);
    box-shadow: 0 12px 30px -10px rgba(0, 0, 0, 0.8);
  }

  .places button {
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: var(--r-sm);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }

  .places button:hover,
  .places button:focus {
    background: var(--bg);
    outline: none;
  }

  .ok-text {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: #7fe0a0;
  }

  code {
    font-size: 11px;
    word-break: break-all;
  }

  .preview {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    min-height: 160px;
  }

  .tabs {
    display: flex;
    gap: 6px;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
  }
</style>
