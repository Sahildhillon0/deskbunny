<script lang="ts" module>
  const PATHS = {
    play: "M5 3.5v9l7.5-4.5z",
    pause: "M4.5 3h2.5v10H4.5zM9 3h2.5v10H9z",
    refresh: "M13 8a5 5 0 1 1-1.46-3.54M13 2.5V5h-2.5",
    zoomIn: "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM14 14l-3.5-3.5M5 7h4M7 5v4",
    zoomOut: "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM14 14l-3.5-3.5M5 7h4",
    download: "M8 2v8M4.5 7 8 10.5 11.5 7M3 13.5h10",
    upload: "M8 10.5v-8M4.5 6 8 2.5 11.5 6M3 13.5h10",
    trash: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5",
    save: "M3.5 2.5h7l2 2v9h-9zM5.5 2.5v3h4v-3M5.5 13.5v-4h5v4",
    check: "M3 8.5 6.5 12 13 4.5",
    alert: "M8 2.5 14 13H2zM8 6.5v3M8 11.2v.3",
    usb: "M8 1.5v10M8 11.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3M8 9 4.5 7V5M8 7.5l3.5-2V4M3.5 3.5h2v1.5h-2zM10.5 2.5h2v1.5h-2z",
    bolt: "M9 1.5 3.5 9H8l-1 5.5L12.5 7H8z",
    plus: "M8 3v10M3 8h10",
  } as const;
  export type IconName = keyof typeof PATHS;
</script>

<script lang="ts">
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
  const filled = $derived(name === "play" || name === "pause" || name === "bolt");
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill={filled ? "currentColor" : "none"}
  stroke={filled ? "none" : "currentColor"}
  stroke-width="1.5"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={PATHS[name]} />
</svg>
