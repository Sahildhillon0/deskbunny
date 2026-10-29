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
    close: "M4 4l8 8M12 4l-8 8",
    help: "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM6.2 6.2a1.9 1.9 0 1 1 2.6 1.8c-.5.2-.8.6-.8 1.1v.4M8 11.3v.2",
    info: "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 7.5v4M8 4.8v.2",
    copy: "M5.5 5.5h7v8h-7zM3.5 10.5v-8h7",
    external: "M9.5 2.5h4v4M13.5 2.5 7.5 8.5M11.5 9.5v4h-9v-9h4",
    pencil: "M10.5 2.5l3 3-8 8H2.5v-3zM9 4l3 3",
    eraser: "M6 13.5h7.5M2.8 9.7l6.4-6.4a1 1 0 0 1 1.4 0l2.1 2.1a1 1 0 0 1 0 1.4L7 12.5H5.6L2.8 9.7zM5.5 7l3.5 3.5",
    line: "M3 13 13 3",
    rect: "M2.5 4h11v8h-11z",
    ellipse: "M8 12.5c3 0 5.5-2 5.5-4.5S11 3.5 8 3.5 2.5 5.5 2.5 8s2.5 4.5 5.5 4.5z",
    fill: "M7 2.5 12.5 8 8 12.5 2.5 7zM2.5 7h10M13.5 11c0 1-.5 2-1 2s-1-1-1-2l1-1.5z",
    text: "M3 4V2.5h10V4M8 2.5v11M6 13.5h4",
    move: "M8 1.5v13M1.5 8h13M8 1.5 6 3.5M8 1.5l2 2M8 14.5l-2-2M8 14.5l2-2M1.5 8l2-2M1.5 8l2 2M14.5 8l-2-2M14.5 8l-2 2",
    undo: "M5.5 3 2.5 6l3 3M2.5 6h7a4 4 0 0 1 0 8h-2",
    redo: "M10.5 3l3 3-3 3M13.5 6h-7a4 4 0 0 0 0 8h2",
    flipH: "M8 1.5v13M6 4 2 8l4 4zM10 4l4 4-4 4z",
    flipV: "M1.5 8h13M4 6l4-4 4 4zM4 10l4 4 4-4z",
    invert: "M8 14.5a6.5 6.5 0 1 0 0-13zM8 14.5a6.5 6.5 0 1 1 0-13",
    image: "M2.5 3h11v10h-11zM2.5 11l3.5-3.5 3 3 2-2 2.5 2.5M10.5 6.2v.1",
    duplicate: "M5.5 5.5h8v8h-8zM2.5 10.5v-8h8",
    arrowUp: "M8 13V3M4 7l4-4 4 4",
    arrowDown: "M8 3v10M4 9l4 4 4-4",
    arrowLeft: "M13 8H3M7 4 3 8l4 4",
    arrowRight: "M3 8h10M9 4l4 4-4 4",
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
