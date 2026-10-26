// Draws a 1024-byte SSD1306 page-layout framebuffer onto a canvas the way the
// real 0.96" white module looks: crisp integer-scaled pixels, a faint dot
// matrix, and a soft bloom underneath.

export const OLED_W = 128;
export const OLED_H = 64;

const LIT: [number, number, number] = [223, 246, 255]; // #dff6ff
const PANEL = "#020304";
const CELL = "#030506";

function makeCanvas(w: number, h: number): [HTMLCanvasElement, CanvasRenderingContext2D] {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d");
  if (!ctx) throw new Error("2D canvas unavailable");
  return [c, ctx];
}

export interface OledOptions {
  /** Bloom strength 0..1. */
  glow: number;
  /** Draw the dot-matrix gaps and unlit cells when pixels are large enough. */
  grid: boolean;
}

export class OledRenderer {
  private ctx: CanvasRenderingContext2D;
  private src: HTMLCanvasElement;
  private srcCtx: CanvasRenderingContext2D;
  private img: ImageData;
  private half: [HTMLCanvasElement, CanvasRenderingContext2D];
  private quarter: [HTMLCanvasElement, CanvasRenderingContext2D];
  private panel: HTMLCanvasElement | null = null;
  private gaps: HTMLCanvasElement | null = null;
  private last: Uint8Array | null = null;
  private brightness = 1;

  constructor(
    private canvas: HTMLCanvasElement,
    private opts: OledOptions,
  ) {
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("2D canvas unavailable");
    this.ctx = ctx;
    [this.src, this.srcCtx] = makeCanvas(OLED_W, OLED_H);
    this.img = this.srcCtx.createImageData(OLED_W, OLED_H);
    this.half = makeCanvas(OLED_W / 2, OLED_H / 2);
    this.quarter = makeCanvas(OLED_W / 4, OLED_H / 4);
  }

  /** Integer CSS scale; the backing store follows devicePixelRatio. */
  resize(scale: number) {
    const dpr = window.devicePixelRatio || 1;
    const cssW = OLED_W * scale;
    const cssH = OLED_H * scale;
    this.canvas.style.width = `${cssW}px`;
    this.canvas.style.height = `${cssH}px`;
    const bw = Math.round(cssW * dpr);
    const bh = Math.round(cssH * dpr);
    if (this.canvas.width !== bw || this.canvas.height !== bh) {
      this.canvas.width = bw;
      this.canvas.height = bh;
    }
    this.buildGrid();
    if (this.last) this.paint();
  }

  /** SSD1306 contrast register (0..255) mapped to a perceptual brightness. */
  setContrast(contrast: number) {
    this.brightness = 0.55 + 0.45 * Math.min(1, Math.max(0, contrast / 255));
    if (this.last) this.paint();
  }

  draw(fb: Uint8Array) {
    this.last = fb;
    const d = this.img.data;
    for (let y = 0; y < OLED_H; y++) {
      const row = (y >> 3) * OLED_W;
      const bit = 1 << (y & 7);
      for (let x = 0; x < OLED_W; x++) {
        const o = (y * OLED_W + x) * 4;
        if (fb[row + x] & bit) {
          d[o] = LIT[0];
          d[o + 1] = LIT[1];
          d[o + 2] = LIT[2];
          d[o + 3] = 255;
        } else {
          d[o + 3] = 0;
        }
      }
    }
    this.srcCtx.putImageData(this.img, 0, 0);
    this.paint();
  }

  private buildGrid() {
    const { width: bw, height: bh } = this.canvas;
    const px = bw / OLED_W;
    this.panel = this.gaps = null;
    if (!this.opts.grid || px < 3) return;
    const gap = Math.max(1, Math.round(px * 0.1));
    const [panel, pctx] = makeCanvas(bw, bh);
    const [gaps, gctx] = makeCanvas(bw, bh);
    pctx.fillStyle = PANEL;
    pctx.fillRect(0, 0, bw, bh);
    pctx.fillStyle = CELL;
    gctx.fillStyle = `rgba(0,0,0,${px >= 6 ? 0.65 : 0.4})`;
    for (let x = 0; x < OLED_W; x++) {
      const x0 = Math.round(x * px);
      gctx.fillRect(x0, 0, gap, bh);
      for (let y = 0; y < OLED_H; y++) {
        const y0 = Math.round(y * px);
        pctx.fillRect(x0 + gap, y0 + gap, Math.round((x + 1) * px) - x0 - gap, Math.round((y + 1) * px) - y0 - gap);
      }
    }
    for (let y = 0; y < OLED_H; y++) gctx.fillRect(0, Math.round(y * px), bw, gap);
    this.panel = panel;
    this.gaps = gaps;
  }

  private paint() {
    const { ctx } = this;
    const { width: bw, height: bh } = this.canvas;
    ctx.globalCompositeOperation = "source-over";
    ctx.globalAlpha = 1;
    if (this.panel) {
      ctx.drawImage(this.panel, 0, 0);
    } else {
      ctx.fillStyle = PANEL;
      ctx.fillRect(0, 0, bw, bh);
    }

    if (this.opts.glow > 0) {
      // Downsample in two steps so the bloom is an average, not a sample.
      const [half, hctx] = this.half;
      const [quarter, qctx] = this.quarter;
      hctx.clearRect(0, 0, half.width, half.height);
      hctx.imageSmoothingEnabled = true;
      hctx.drawImage(this.src, 0, 0, half.width, half.height);
      qctx.clearRect(0, 0, quarter.width, quarter.height);
      qctx.imageSmoothingEnabled = true;
      qctx.drawImage(half, 0, 0, quarter.width, quarter.height);

      ctx.imageSmoothingEnabled = true;
      ctx.globalCompositeOperation = "lighter";
      const pad = bw / OLED_W;
      ctx.globalAlpha = 0.7 * this.opts.glow * this.brightness;
      ctx.drawImage(quarter, -pad * 2, -pad * 2, bw + pad * 4, bh + pad * 4);
      ctx.globalAlpha = 0.45 * this.opts.glow * this.brightness;
      ctx.drawImage(half, -pad, -pad, bw + pad * 2, bh + pad * 2);
    }

    ctx.globalCompositeOperation = "source-over";
    ctx.imageSmoothingEnabled = false;
    ctx.globalAlpha = this.brightness;
    ctx.drawImage(this.src, 0, 0, bw, bh);
    ctx.globalAlpha = 1;
    if (this.gaps) ctx.drawImage(this.gaps, 0, 0);
  }
}
