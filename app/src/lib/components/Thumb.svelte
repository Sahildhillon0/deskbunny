<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import { OledRenderer } from "../oled";
  import { registerThumb, type Thumb } from "../thumbs";
  import type { FaceDef } from "../types";

  let { face, seed = 0 }: { face: FaceDef; seed?: number } = $props();

  let canvas: HTMLCanvasElement;
  let simId = $state<number | null>(null);
  let thumb: Thumb | null = null;
  let renderer: OledRenderer | null = null;

  onMount(() => {
    renderer = new OledRenderer(canvas, { glow: 0.6, grid: false });
    renderer.resize(1);
    renderer.setContrast(face.contrast);
    let unregister: (() => void) | null = null;
    let dead = false;

    const io = new IntersectionObserver(([entry]) => {
      if (thumb) thumb.visible = entry.isIntersecting;
    });
    io.observe(canvas);

    api.simCreate($state.snapshot(face), seed).then((id) => {
      if (dead) {
        api.simDrop(id);
        return;
      }
      simId = id;
      [thumb, unregister] = registerThumb(id, renderer!);
      const r = canvas.getBoundingClientRect();
      thumb.visible = r.bottom > 0 && r.top < window.innerHeight;
    });

    return () => {
      dead = true;
      io.disconnect();
      unregister?.();
      if (simId !== null) api.simDrop(simId);
    };
  });

  // Saved faces can be overwritten in place; keep the thumbnail in step.
  $effect(() => {
    const f = $state.snapshot(face);
    renderer?.setContrast(f.contrast);
    if (simId !== null) api.simSetFace(simId, f);
  });
</script>

<canvas bind:this={canvas} class="thumb" aria-hidden="true"></canvas>

<style>
  .thumb {
    display: block;
    width: 128px;
    height: 64px;
    image-rendering: pixelated;
    border-radius: 3px;
    background: #020304;
  }
</style>
