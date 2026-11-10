/* nixxa clock faces: 20 styles for the 128x64 SSD1306 page buffer.
 *
 * Pure C99 with no Arduino or ESP-IDF includes. Every pixel write goes through
 * px(), which clips to the panel, so no style can write outside the buffer.
 *
 * Time display: styles 2, 10 and 20 use 12-hour time (AM/PM, no leading zero
 * on the hour). The others use 24-hour HH:MM. When have_time is 0 each style
 * shows "--:--" in place of the time and draws no hands or progress.
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "nx_clock.h"
#include "nx_font5x7.h"

#define FB_W 128
#define FB_H 64

typedef struct {
  int h, m, s; /* sanitized 24-hour time */
  int set;     /* 0: clock not set yet, draw "--:--" */
} ctx_t;

/* sin(d) * 1000 for d = 0..90 degrees */
static const int16_t SIN_Q[91] = {
  0, 17, 35, 52, 70, 87, 105, 122, 139, 156,
  174, 191, 208, 225, 242, 259, 276, 292, 309, 326,
  342, 358, 375, 391, 407, 423, 438, 454, 469, 485,
  500, 515, 530, 545, 559, 574, 588, 602, 616, 629,
  643, 656, 669, 682, 695, 707, 719, 731, 743, 755,
  766, 777, 788, 799, 809, 819, 829, 839, 848, 857,
  866, 875, 883, 891, 899, 906, 914, 921, 927, 934,
  940, 946, 951, 956, 961, 966, 970, 974, 978, 982,
  985, 988, 990, 993, 995, 996, 998, 999, 999, 1000,
  1000,
};

/* ---- integer trig ------------------------------------------------------- */

static int rdiv(int v)
{
  return v >= 0 ? (v + 500) / 1000 : -((-v + 500) / 1000);
}

static int sin_deg(int d)
{
  d %= 360;
  if (d < 0) d += 360;
  if (d <= 90) return SIN_Q[d];
  if (d <= 180) return SIN_Q[180 - d];
  if (d <= 270) return -SIN_Q[d - 180];
  return -SIN_Q[360 - d];
}

static int cos_deg(int d)
{
  return sin_deg(d + 90);
}

/* Point at clock angle a (0 = 12 o'clock, clockwise) and radius r. */
static void polar(int cx, int cy, int r, int a, int *x, int *y)
{
  *x = cx + rdiv(r * sin_deg(a));
  *y = cy - rdiv(r * cos_deg(a));
}

/* ---- pixels and shapes -------------------------------------------------- */

static void px(uint8_t *fb, int x, int y, int on)
{
  if (x < 0 || x >= FB_W || y < 0 || y >= FB_H) return;
  if (on) fb[(y >> 3) * FB_W + x] |= (uint8_t)(1u << (y & 7));
  else fb[(y >> 3) * FB_W + x] &= (uint8_t)~(1u << (y & 7));
}

static void box(uint8_t *fb, int x, int y, int w, int h, int on)
{
  int i, j;
  for (j = 0; j < h; j++)
    for (i = 0; i < w; i++) px(fb, x + i, y + j, on);
}

static void frame(uint8_t *fb, int x, int y, int w, int h, int on)
{
  box(fb, x, y, w, 1, on);
  box(fb, x, y + h - 1, w, 1, on);
  box(fb, x, y, 1, h, on);
  box(fb, x + w - 1, y, 1, h, on);
}

/* Bresenham line. */
static void line(uint8_t *fb, int x0, int y0, int x1, int y1, int on)
{
  int dx = x1 > x0 ? x1 - x0 : x0 - x1;
  int dy = -(y1 > y0 ? y1 - y0 : y0 - y1);
  int sx = x0 < x1 ? 1 : -1;
  int sy = y0 < y1 ? 1 : -1;
  int err = dx + dy;
  for (;;) {
    int e2;
    px(fb, x0, y0, on);
    if (x0 == x1 && y0 == y1) break;
    e2 = 2 * err;
    if (e2 >= dy) {
      err += dy;
      x0 += sx;
    }
    if (e2 <= dx) {
      err += dx;
      y0 += sy;
    }
  }
}

/* Clock hand from the centre. thick: 2x2 brush instead of 1 px. */
static void hand(uint8_t *fb, int cx, int cy, int len, int a, int thick, int on)
{
  int x1, y1;
  polar(cx, cy, len, a, &x1, &y1);
  line(fb, cx, cy, x1, y1, on);
  if (thick) {
    line(fb, cx + 1, cy, x1 + 1, y1, on);
    line(fb, cx, cy + 1, x1, y1 + 1, on);
    line(fb, cx + 1, cy + 1, x1 + 1, y1 + 1, on);
  }
}

static void circle_pt8(uint8_t *fb, int cx, int cy, int x, int y, int on)
{
  px(fb, cx + x, cy + y, on);
  px(fb, cx - x, cy + y, on);
  px(fb, cx + x, cy - y, on);
  px(fb, cx - x, cy - y, on);
  px(fb, cx + y, cy + x, on);
  px(fb, cx - y, cy + x, on);
  px(fb, cx + y, cy - x, on);
  px(fb, cx - y, cy - x, on);
}

/* 1 px circle outline (midpoint algorithm). */
static void ring(uint8_t *fb, int cx, int cy, int r, int on)
{
  int x = 0, y = r, d = 1 - r;
  while (x <= y) {
    circle_pt8(fb, cx, cy, x, y, on);
    x++;
    if (d < 0) {
      d += 2 * x + 1;
    } else {
      y--;
      d += 2 * (x - y) + 1;
    }
  }
}

static void disc(uint8_t *fb, int cx, int cy, int r, int on)
{
  int x, y;
  for (y = -r; y <= r; y++)
    for (x = -r; x <= r; x++)
      if (x * x + y * y <= r * r + r) px(fb, cx + x, cy + y, on);
}

/* Arc over clock angles a0..a1 (inclusive, step), radii r0..r1. */
static void arc(uint8_t *fb, int cx, int cy, int r0, int r1, int a0, int a1, int step, int on)
{
  int a, r, x, y;
  for (a = a0; a <= a1; a += step)
    for (r = r0; r <= r1; r++) {
      polar(cx, cy, r, a, &x, &y);
      px(fb, x, y, on);
    }
}

/* Rounded rectangle [x0,x1) x [y0,y1) with corner radius r: inside test. */
static int in_rrect(int x, int y, int x0, int y0, int x1, int y1, int r)
{
  int ccx, ccy;
  if (x < x0 || x >= x1 || y < y0 || y >= y1) return 0;
  if (x < x0 + r && y < y0 + r) {
    ccx = x0 + r;
    ccy = y0 + r;
  } else if (x >= x1 - r && y < y0 + r) {
    ccx = x1 - 1 - r;
    ccy = y0 + r;
  } else if (x < x0 + r && y >= y1 - r) {
    ccx = x0 + r;
    ccy = y1 - 1 - r;
  } else if (x >= x1 - r && y >= y1 - r) {
    ccx = x1 - 1 - r;
    ccy = y1 - 1 - r;
  } else {
    return 1;
  }
  return (x - ccx) * (x - ccx) + (y - ccy) * (y - ccy) <= r * r;
}

/* ---- text --------------------------------------------------------------- */

/* 5x7 font, 6*scale px per character (1 column gap). */
static int str_w(const char *s, int scale)
{
  int n = (int)strlen(s);
  return n ? n * 6 * scale - scale : 0;
}

static int glyph_index(int ch)
{
  if (ch < NX_FONT_FIRST || ch > NX_FONT_LAST) ch = '?';
  return (ch - NX_FONT_FIRST) * 5;
}

/* Draws one glyph. Only lit font pixels are touched, so with on = 0 the glyph
 * is cut out of whatever is already lit. */
static void glyph(uint8_t *fb, int x, int y, int scale, int ch, int on)
{
  int col, row, idx = glyph_index(ch);
  for (col = 0; col < 5; col++) {
    uint8_t bits = NX_FONT5X7[idx + col];
    for (row = 0; row < 7; row++)
      if (bits & (1u << row)) box(fb, x + col * scale, y + row * scale, scale, scale, on);
  }
}

static void text(uint8_t *fb, int x, int y, int scale, const char *s, int on)
{
  for (; *s; s++) {
    glyph(fb, x, y, scale, (unsigned char)*s, on);
    x += 6 * scale;
  }
}

static void text_c(uint8_t *fb, int cx, int y, int scale, const char *s, int on)
{
  text(fb, cx - str_w(s, scale) / 2, y, scale, s, on);
}

/* Dot-matrix text: each font pixel is a 2x2 dot, 2 px gap, 22 px per char. */
static int dots_w(const char *s)
{
  int n = (int)strlen(s);
  return n ? n * 22 - 4 : 0;
}

static void dots_text(uint8_t *fb, int x, int y, const char *s)
{
  for (; *s; s++) {
    int col, row, idx = glyph_index((unsigned char)*s);
    for (col = 0; col < 5; col++) {
      uint8_t bits = NX_FONT5X7[idx + col];
      for (row = 0; row < 7; row++)
        if (bits & (1u << row)) box(fb, x + col * 4, y + row * 4, 2, 2, 1);
    }
    x += 22;
  }
}

/* ---- time helpers ------------------------------------------------------- */

static void two(char *o, int v)
{
  o[0] = (char)('0' + (v / 10) % 10);
  o[1] = (char)('0' + v % 10);
}

/* "HH:MM" (24 h) or "H:MM" (12 h, no leading zero). o needs 8 bytes. */
static void fmt_hm(char *o, const ctx_t *c, int h12)
{
  int h = c->h;
  int n = 0;
  if (h12) {
    h %= 12;
    if (h == 0) h = 12;
    if (h >= 10) o[n++] = '1';
    o[n++] = (char)('0' + h % 10);
  } else {
    o[n++] = (char)('0' + h / 10);
    o[n++] = (char)('0' + h % 10);
  }
  o[n++] = ':';
  o[n++] = (char)('0' + c->m / 10);
  o[n++] = (char)('0' + c->m % 10);
  o[n] = 0;
}

/* Time text, or "--:--" when the clock is not set. */
static void hm(char *o, const ctx_t *c, int h12)
{
  if (c->set) fmt_hm(o, c, h12);
  else strcpy(o, "--:--");
}

/* Two-digit seconds, or "--". o needs 3 bytes. */
static void sec2(char *o, const ctx_t *c)
{
  if (c->set) two(o, c->s);
  else strcpy(o, "--");
}

static int hour_deg(const ctx_t *c)
{
  return (c->h % 12) * 30 + c->m / 2;
}

static int min_deg(const ctx_t *c)
{
  return c->m * 6;
}

static int sec_deg(const ctx_t *c)
{
  return c->s * 6;
}

/* Percent text "42%", or "--%". o needs 5 bytes. */
static void pct_str(char *o, const ctx_t *c, int p)
{
  int n = 0;
  if (!c->set) {
    strcpy(o, "--%");
    return;
  }
  if (p >= 10) o[n++] = (char)('0' + p / 10);
  o[n++] = (char)('0' + p % 10);
  o[n++] = '%';
  o[n] = 0;
}

/* ---- styles ------------------------------------------------------------- */

/* 1: big 24 h digits, seconds small below. */
static void s_big24(uint8_t *fb, const ctx_t *c)
{
  char b[8], sb[4];
  hm(b, c, 0);
  text_c(fb, 64, 18, 4, b, 1);
  sec2(sb, c);
  text_c(fb, 64, 52, 1, sb, 1);
}

/* 2: big 12 h digits, AM/PM, twelve hour dots (passed hours solid). */
static void s_big12(uint8_t *fb, const ctx_t *c)
{
  char b[8];
  int i, h12 = c->h % 12;
  if (h12 == 0) h12 = 12;
  hm(b, c, 1);
  text_c(fb, 64, 8, 3, b, 1);
  if (c->set) {
    text_c(fb, 64, 34, 1, c->h < 12 ? "AM" : "PM", 1);
    for (i = 0; i < 12; i++) {
      int x = 8 + i * 10;
      if (i < h12) box(fb, x, 52, 3, 3, 1);
      else box(fb, x + 1, 53, 1, 1, 1);
    }
  }
}

/* 3: 24 h digits, seconds number, seconds progress bar along the bottom. */
static void s_bar_sec(uint8_t *fb, const ctx_t *c)
{
  char b[8], sb[4];
  hm(b, c, 0);
  text_c(fb, 64, 10, 3, b, 1);
  sec2(sb, c);
  text_c(fb, 64, 36, 1, sb, 1);
  frame(fb, 2, 54, 124, 8, 1);
  if (c->set) box(fb, 3, 55, c->s * 122 / 60, 6, 1);
}

/* 4: analog dial on the left (radius 28), digital time on the right. */
static void s_analog_digital(uint8_t *fb, const ctx_t *c)
{
  const int cx = 30, cy = 32;
  char b[8], sb[4];
  int i, x0, y0, x1, y1;
  ring(fb, cx, cy, 28, 1);
  for (i = 0; i < 12; i++) {
    polar(cx, cy, 24, i * 30, &x0, &y0);
    polar(cx, cy, 27, i * 30, &x1, &y1);
    line(fb, x0, y0, x1, y1, 1);
  }
  if (c->set) {
    hand(fb, cx, cy, 14, hour_deg(c), 1, 1);
    hand(fb, cx, cy, 21, min_deg(c), 0, 1);
  }
  disc(fb, cx, cy, 1, 1);
  hm(b, c, 0);
  text(fb, 66, 18, 2, b, 1);
  sec2(sb, c);
  text(fb, 66, 38, 1, sb, 1);
}

/* 5: full-height analog dial, major ticks, minor dots, bold hands. */
static void s_full_analog(uint8_t *fb, const ctx_t *c)
{
  const int cx = 64, cy = 32;
  int i, x0, y0, x1, y1;
  ring(fb, cx, cy, 31, 1);
  for (i = 0; i < 60; i++) {
    if (i % 5 == 0) {
      polar(cx, cy, 26, i * 6, &x0, &y0);
      polar(cx, cy, 30, i * 6, &x1, &y1);
      line(fb, x0, y0, x1, y1, 1);
    } else {
      polar(cx, cy, 29, i * 6, &x0, &y0);
      px(fb, x0, y0, 1);
    }
  }
  if (c->set) {
    hand(fb, cx, cy, 16, hour_deg(c), 1, 1);
    hand(fb, cx, cy, 26, min_deg(c), 0, 1);
  }
  disc(fb, cx, cy, 2, 1);
}

/* 6: minimal analog: 2x2 hour dots on a 30 px circle, thin 1 px hands. */
static void s_minimal(uint8_t *fb, const ctx_t *c)
{
  const int cx = 64, cy = 32;
  int i, x, y;
  for (i = 0; i < 12; i++) {
    polar(cx, cy, 30, i * 30, &x, &y);
    box(fb, x, y, 2, 2, 1);
  }
  if (c->set) {
    hand(fb, cx, cy, 16, hour_deg(c), 0, 1);
    hand(fb, cx, cy, 26, min_deg(c), 0, 1);
  }
}

/* 7: analog dial with a sweeping second hand (with tail) and a filled hub. */
static void s_sweep(uint8_t *fb, const ctx_t *c)
{
  const int cx = 64, cy = 32;
  int i, x0, y0, x1, y1;
  ring(fb, cx, cy, 30, 1);
  for (i = 0; i < 12; i++) {
    polar(cx, cy, 26, i * 30, &x0, &y0);
    polar(cx, cy, 29, i * 30, &x1, &y1);
    line(fb, x0, y0, x1, y1, 1);
  }
  if (c->set) {
    hand(fb, cx, cy, 15, hour_deg(c), 1, 1);
    hand(fb, cx, cy, 24, min_deg(c), 0, 1);
    hand(fb, cx, cy, 28, sec_deg(c), 0, 1);
    polar(cx, cy, 8, sec_deg(c) + 180, &x0, &y0);
    line(fb, cx, cy, x0, y0, 1);
  }
  disc(fb, cx, cy, 3, 1);
}

/* Roman label centred on the point at clock angle a, radius r. */
static void roman_label(uint8_t *fb, int cx, int cy, int r, int a, const char *s)
{
  int x, y;
  polar(cx, cy, r, a, &x, &y);
  text(fb, x - str_w(s, 1) / 2, y - 3, 1, s, 1);
}

/* 8: roman dial: XII III VI IX as text, small dots for the other hours. */
static void s_roman(uint8_t *fb, const ctx_t *c)
{
  const int cx = 64, cy = 32;
  int i, x, y;
  roman_label(fb, cx, cy, 26, 0, "XII");
  roman_label(fb, cx, cy, 26, 90, "III");
  roman_label(fb, cx, cy, 26, 180, "VI");
  roman_label(fb, cx, cy, 26, 270, "IX");
  for (i = 0; i < 12; i++) {
    if (i % 3 == 0) continue;
    polar(cx, cy, 29, i * 30, &x, &y);
    px(fb, x, y, 1);
  }
  if (c->set) {
    hand(fb, cx, cy, 11, hour_deg(c), 1, 1);
    hand(fb, cx, cy, 17, min_deg(c), 0, 1);
  }
  disc(fb, cx, cy, 1, 1);
}

/* 9: BCD binary, four columns (H tens, H units, M tens, M units), 4 rows each
 * with bit 3 on top. Lit cells are solid, unlit cells are outlined. */
static void s_binary(uint8_t *fb, const ctx_t *c)
{
  int vals[4];
  int col, row;
  vals[0] = c->h / 10;
  vals[1] = c->h % 10;
  vals[2] = c->m / 10;
  vals[3] = c->m % 10;
  for (col = 0; col < 4; col++) {
    int x = 16 + col * 28;
    if (!c->set) {
      box(fb, x, 30, 12, 2, 1);
      continue;
    }
    for (row = 0; row < 4; row++) {
      int y = 4 + row * 14;
      int bit = 3 - row;
      if ((vals[col] >> bit) & 1) box(fb, x, y, 12, 12, 1);
      else frame(fb, x, y, 12, 12, 1);
    }
  }
}

static const char *const HOUR_WORDS[12] = {
  "TWELVE", "ONE", "TWO", "THREE", "FOUR", "FIVE",
  "SIX", "SEVEN", "EIGHT", "NINE", "TEN", "ELEVEN"
};

/* Minutes past (or to) the hour, in steps of 5. Index = minutes / 5. */
static const char *const MINUTE_WORDS[7] = {
  "", "FIVE", "TEN", "QUARTER", "TWENTY", "TWENTY FIVE", "HALF"
};

/* 10: word clock, rounded to 5 minutes, 12 h. Five lines of 5x7 text. */
static void s_words(uint8_t *fb, const ctx_t *c)
{
  int mr, hh, disp;
  const char *mw;
  const char *pt;

  if (!c->set) {
    text_c(fb, 64, 4, 1, "IT IS", 1);
    text_c(fb, 64, 28, 1, "--:--", 1);
    text_c(fb, 64, 40, 1, "NOT SET", 1);
    return;
  }

  mr = ((c->m + 2) / 5) * 5;
  hh = c->h % 12;
  disp = c->h;
  if (mr == 60) {
    mr = 0;
    disp = (disp + 1) % 24;
    hh = (hh + 1) % 12;
  }
  if (mr == 0) {
    mw = "";
    pt = "O'CLOCK";
  } else if (mr <= 30) {
    mw = MINUTE_WORDS[mr / 5];
    pt = "PAST";
  } else {
    mw = MINUTE_WORDS[(60 - mr) / 5];
    pt = "TO";
    hh = (hh + 1) % 12;
    disp = (disp + 1) % 24;
  }

  text_c(fb, 64, 4, 1, "IT IS", 1);
  if (mw[0]) text_c(fb, 64, 16, 1, mw, 1);
  text_c(fb, 64, 28, 1, pt, 1);
  text_c(fb, 64, 40, 1, HOUR_WORDS[hh], 1);
  text_c(fb, 64, 52, 1, disp < 12 ? "AM" : "PM", 1);
}

/* 11: ruled digits: dashed rules above and below, 24 h digits, seconds. */
static void s_ruled(uint8_t *fb, const ctx_t *c)
{
  char b[8], sb[8];
  int x;
  for (x = 4; x < 124; x += 8) {
    box(fb, x, 8, 5, 1, 1);
    box(fb, x, 56, 5, 1, 1);
  }
  hm(b, c, 0);
  text_c(fb, 64, 16, 3, b, 1);
  strcpy(sb, "SEC ");
  sec2(sb + 4, c);
  text_c(fb, 64, 44, 1, sb, 1);
}

/* One split-flap tile: lit card with the glyph cut out and a split line. */
static void flap(uint8_t *fb, int x, int y, int ch)
{
  box(fb, x, y, 24, 30, 1);
  box(fb, x, y + 15, 24, 1, 0);
  glyph(fb, x + 4, y + 4, 3, ch, 0);
}

/* 12: split-flap display, four tiles with a colon between the pairs. */
static void s_split(uint8_t *fb, const ctx_t *c)
{
  static const int xs[4] = {8, 35, 70, 97};
  int d[4] = {0, 0, 0, 0};
  int i;
  if (c->set) {
    d[0] = c->h / 10;
    d[1] = c->h % 10;
    d[2] = c->m / 10;
    d[3] = c->m % 10;
  }
  for (i = 0; i < 4; i++) flap(fb, xs[i], 17, c->set ? '0' + d[i] : '-');
  box(fb, 62, 26, 3, 3, 1);
  box(fb, 62, 36, 3, 3, 1);
}

/* 13: stacked: hour on top, minute below, both large, 24 h. */
static void s_stacked(uint8_t *fb, const ctx_t *c)
{
  char hb[3], mb[3];
  if (c->set) {
    two(hb, c->h);
    two(mb, c->m);
  } else {
    strcpy(hb, "--");
    strcpy(mb, "--");
  }
  hb[2] = 0;
  mb[2] = 0;
  text_c(fb, 64, 2, 4, hb, 1);
  text_c(fb, 64, 34, 4, mb, 1);
}

/* 14: 24 h digits over a day-progress bar with 6-hour ticks and percent. */
static void s_day_bar(uint8_t *fb, const ctx_t *c)
{
  char b[8], pb[5];
  int k, mod = c->h * 60 + c->m;
  hm(b, c, 0);
  text_c(fb, 64, 8, 3, b, 1);
  frame(fb, 4, 40, 120, 8, 1);
  if (c->set) box(fb, 5, 41, mod * 118 / 1440, 6, 1);
  for (k = 0; k <= 4; k++) box(fb, 4 + k * 30, 49, 1, 4, 1);
  pct_str(pb, c, mod * 100 / 1440);
  text_c(fb, 64, 56, 1, pb, 1);
}

/* 15: semicircle sunrise-to-sunset arc: left = 00:00, top = 12:00, right = 24:00. */
static void s_day_arc(uint8_t *fb, const ctx_t *c)
{
  const int cx = 64, cy = 56, R = 40;
  char b[8];
  int a, x, y;
  for (a = 270; a <= 450; a += 6) {
    polar(cx, cy, R, a, &x, &y);
    px(fb, x, y, 1);
  }
  if (c->set) {
    int p = (c->h * 60 + c->m) * 180 / 1440;
    arc(fb, cx, cy, R - 2, R, 270, 270 + p, 1, 1);
    polar(cx, cy, R - 1, 270 + p, &x, &y);
    disc(fb, x, y, 2, 1);
  }
  hm(b, c, 0);
  text_c(fb, 64, 30, 2, b, 1);
}

/* 16: dot-matrix digits: 2x2 dots on a 4 px pitch (2 px gap). */
static void s_dot_matrix(uint8_t *fb, const ctx_t *c)
{
  char b[8], sb[4];
  hm(b, c, 0);
  dots_text(fb, (FB_W - dots_w(b)) / 2, 19, b);
  sec2(sb, c);
  text_c(fb, 64, 52, 1, sb, 1);
}

/* 17: two bars (12 hour segments, 60 minute steps) around the time text. */
static void s_bars(uint8_t *fb, const ctx_t *c)
{
  char b[8];
  int k;
  frame(fb, 3, 6, 122, 10, 1);
  frame(fb, 3, 46, 122, 10, 1);
  if (c->set) {
    box(fb, 4, 7, ((c->h % 12) * 60 + c->m) * 120 / 720, 8, 1);
    box(fb, 4, 47, c->m * 120 / 60, 8, 1);
    for (k = 1; k < 12; k++) box(fb, 4 + k * 10, 7, 1, 8, 0);
    for (k = 1; k < 4; k++) box(fb, 4 + k * 30, 47, 1, 8, 0);
  }
  hm(b, c, 0);
  text_c(fb, 64, 25, 2, b, 1);
}

/* 18: concentric rings, outer = minutes (60 steps), inner = 12 h, time inside. */
static void s_rings(uint8_t *fb, const ctx_t *c)
{
  const int cx = 64, cy = 32;
  char b[8];
  int i, x, y;
  for (i = 0; i < 60; i++) {
    polar(cx, cy, 30, i * 6, &x, &y);
    px(fb, x, y, 1);
  }
  for (i = 0; i < 12; i++) {
    polar(cx, cy, 21, i * 30, &x, &y);
    px(fb, x, y, 1);
  }
  if (c->set) {
    arc(fb, cx, cy, 29, 30, 0, min_deg(c), 1, 1);
    arc(fb, cx, cy, 20, 21, 0, ((c->h % 12) * 60 + c->m) / 2, 1, 1);
  }
  hm(b, c, 0);
  text_c(fb, cx, 28, 1, b, 1);
}

/* 19: corner clock: time top-left, seconds sweep ring fills the rest. */
static void s_corner(uint8_t *fb, const ctx_t *c)
{
  const int cx = 96, cy = 38;
  char b[8], sb[4];
  int a, x, y;
  hm(b, c, 0);
  text(fb, 2, 2, 2, b, 1);
  sec2(sb, c);
  text(fb, 2, 20, 1, sb, 1);
  for (a = 0; a < 360; a += 6) {
    polar(cx, cy, 24, a, &x, &y);
    px(fb, x, y, 1);
  }
  if (c->set) arc(fb, cx, cy, 19, 24, 0, sec_deg(c), 1, 1);
  disc(fb, cx, cy, 2, 1);
}

/* 20: bold outline: rounded 2 px frame, 12 h digits, AM/PM, rule. */
static void s_bold(uint8_t *fb, const ctx_t *c)
{
  char b[8];
  int x, y;
  for (y = 0; y < FB_H; y++)
    for (x = 0; x < FB_W; x++)
      if (in_rrect(x, y, 2, 2, 126, 62, 8) && !in_rrect(x, y, 4, 4, 124, 60, 6)) px(fb, x, y, 1);
  hm(b, c, 1);
  text_c(fb, 64, 12, 3, b, 1);
  text_c(fb, 64, 40, 1, c->set ? (c->h < 12 ? "AM" : "PM") : "--", 1);
  box(fb, 24, 52, 80, 1, 1);
}

/* ---- public API --------------------------------------------------------- */

static const char *const STYLE_NAMES[NX_CLOCK_STYLES] = {
  "Big digits",
  "Big 12h",
  "Digital bar",
  "Analog + digital",
  "Full analog",
  "Minimal analog",
  "Sweep analog",
  "Roman dial",
  "Binary",
  "Word clock",
  "Ruled digits",
  "Split-flap",
  "Stacked",
  "Day bar",
  "Day arc",
  "Dot matrix",
  "Hour-minute bars",
  "Rings",
  "Corner sweep",
  "Bold outline"
};

static uint8_t clamp_style(uint8_t style)
{
  return (style < 1 || style > NX_CLOCK_STYLES) ? 1 : style;
}

void nx_clock_draw(uint8_t fb[1024], uint8_t style, const nx_clock_time_t *t, int have_time)
{
  ctx_t c;

  if (!fb) return;
  memset(fb, 0, 1024);

  c.set = (have_time && t) ? 1 : 0;
  c.h = t ? t->hour % 24 : 0;
  c.m = t ? t->minute % 60 : 0;
  c.s = t ? t->second % 60 : 0;

  switch (clamp_style(style)) {
  case 1: s_big24(fb, &c); break;
  case 2: s_big12(fb, &c); break;
  case 3: s_bar_sec(fb, &c); break;
  case 4: s_analog_digital(fb, &c); break;
  case 5: s_full_analog(fb, &c); break;
  case 6: s_minimal(fb, &c); break;
  case 7: s_sweep(fb, &c); break;
  case 8: s_roman(fb, &c); break;
  case 9: s_binary(fb, &c); break;
  case 10: s_words(fb, &c); break;
  case 11: s_ruled(fb, &c); break;
  case 12: s_split(fb, &c); break;
  case 13: s_stacked(fb, &c); break;
  case 14: s_day_bar(fb, &c); break;
  case 15: s_day_arc(fb, &c); break;
  case 16: s_dot_matrix(fb, &c); break;
  case 17: s_bars(fb, &c); break;
  case 18: s_rings(fb, &c); break;
  case 19: s_corner(fb, &c); break;
  default: s_bold(fb, &c); break;
  }
}

const char *nx_clock_name(uint8_t style)
{
  return STYLE_NAMES[clamp_style(style) - 1];
}
