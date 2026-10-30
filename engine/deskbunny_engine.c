/*
 * DeskBunny engine, drone edition. Strict C99, integer only, no malloc, no stdio.
 * Runs on a 16 MHz ATmega328P (int is 16 bits there: every product that can
 * exceed 32767 is done in int32_t) and on the desktop simulator via FFI.
 *
 * Drawing model: a scene is a sky background plus one or more quadcopters.
 * The side-view drone is drawn as line segments through a small rotate/mirror
 * transform, so it can bank, flip and face either way; its rotor blades are
 * seen edge-on and their length follows the spin phase. The top-down drone is
 * four rotor rings on an X frame. Everything is lines, spans and circles.
 */
#include "deskbunny_engine.h"

#include <string.h>

/* ------------------------------------------------------------------------ */
/* Engine state usage (db_engine_t keeps its public layout):                 */
/*   last_ms        start of the current drift move                          */
/*   look_cur_x/y   drift origin, 8.8 fixed point px                         */
/*   look_tgt_x/y   drift target, px                                         */
/*   blinking       ST_STARTED only                                          */
/*   next_blink_ms, blink_start_ms: unused (the strobe is clock driven)      */
/* ------------------------------------------------------------------------ */
#define ST_STARTED 0x80u /* timers have been based on the first now_ms */

/* The magic is stored XOR-ed so the firmware image contains "DBFACE01"
 * exactly once (in DB_FACE_BLOB), as the flasher requires. */
#define MAGIC_X 0x5A
static const uint8_t k_magic_x[8] = {
  'D' ^ MAGIC_X, 'B' ^ MAGIC_X, 'F' ^ MAGIC_X, 'A' ^ MAGIC_X,
  'C' ^ MAGIC_X, 'E' ^ MAGIC_X, '0' ^ MAGIC_X, '1' ^ MAGIC_X
};

/* Quarter sine, 64 steps per turn, amplitude 127. */
static const uint8_t k_sin_q[17] = {
  0, 12, 25, 37, 49, 60, 71, 81, 90, 98, 106, 112, 117, 122, 125, 126, 127
};

/* ------------------------------------------------------------------------ */
/* Small integer helpers                                                     */
/* ------------------------------------------------------------------------ */
static int imin(int a, int b) { return a < b ? a : b; }
static int imax(int a, int b) { return a > b ? a : b; }
static int iabs(int a) { return a < 0 ? -a : a; }
static int iclamp(int v, int lo, int hi) { return v < lo ? lo : (v > hi ? hi : v); }

/* floor(sqrt(v)) for v < 65536. */
static uint16_t isqrt16(uint16_t v)
{
  uint16_t res = 0;
  uint16_t bit = 1u << 14;
  while (bit > v) bit >>= 2;
  while (bit != 0) {
    if (v >= (uint16_t)(res + bit)) {
      v = (uint16_t)(v - (res + bit));
      res = (uint16_t)((res >> 1) + bit);
    } else {
      res >>= 1;
    }
    bit >>= 2;
  }
  return res;
}

/* One turn = 64 steps; returns -127..127. */
static int sin64(uint8_t a)
{
  uint8_t q;
  a &= 63;
  q = (uint8_t)(a & 15);
  if (a < 16) return k_sin_q[q];
  if (a < 32) return k_sin_q[16 - q];
  if (a < 48) return -k_sin_q[q];
  return -k_sin_q[16 - q];
}
static int cos64(uint8_t a) { return sin64((uint8_t)(a + 16)); }

/* a * s / 127 for a sine value s, rounded towards zero. The reciprocal
 * multiply is exact for |a * s| <= 16762 and avoids a slow AVR division. */
static int sscale(int a, int s)
{
  int32_t p = (int32_t)a * s;
  if (p < 0) return -(int)((-p * 8257L) >> 20);
  return (int)((p * 8257L) >> 20);
}

/* Smoothstep on 0..256. */
static int ease(int x)
{
  int32_t v;
  if (x <= 0) return 0;
  if (x >= 256) return 256;
  v = (int32_t)x * x * (768 - 2 * (int32_t)x);
  return (int)(v >> 16);
}

/* ease() of p's progress through [a, b). */
static int ramp(int p, int a, int b)
{
  return ease((int)((int32_t)(p - a) * 256 / (b - a)));
}

/* Cheap stateless hash for scenery that must not consume the PRNG. */
static uint8_t hash8(uint32_t v)
{
  v ^= v >> 7;
  v *= 0x2C1B3C6DUL;
  v ^= v >> 12;
  v *= 0x297A2D39UL;
  v ^= v >> 15;
  return (uint8_t)v;
}

static uint32_t rng_next(db_engine_t *e)
{
  uint32_t x = e->rng;
  x ^= x << 13;
  x ^= x >> 17;
  x ^= x << 5;
  e->rng = x;
  return x;
}

/* Uniform-ish in [lo, hi]. */
static uint32_t rng_range(db_engine_t *e, uint32_t lo, uint32_t hi)
{
  if (hi <= lo) return lo;
  return lo + rng_next(e) % (hi - lo + 1u);
}

/* Uniform-ish in [-m, m]. */
static int rng_signed(db_engine_t *e, int m)
{
  if (m <= 0) return 0;
  return (int)(rng_next(e) % (uint32_t)(2 * m + 1)) - m;
}

/* ------------------------------------------------------------------------ */
/* CRC and blob codec                                                        */
/* ------------------------------------------------------------------------ */
uint16_t db_crc16(const uint8_t *data, size_t len)
{
  uint16_t crc = 0xFFFFu;
  uint8_t b;
  while (len--) {
    crc ^= (uint16_t)((uint16_t)*data++ << 8);
    for (b = 0; b < 8; b++) {
      if (crc & 0x8000u) crc = (uint16_t)((uint16_t)(crc << 1) ^ 0x1021u);
      else crc = (uint16_t)(crc << 1);
    }
  }
  return crc;
}

void db_face_default(db_face_t *out)
{
  static const char name[] = "DESKDRONE";
  memset(out, 0, sizeof(*out));
  out->mood = DB_MOOD_HOVER;
  memcpy(out->name, name, sizeof(name));
  out->eye_w = 36;
  out->eye_h = 36;
  out->eye_r = 8;
  out->eye_gap = 16;
  out->eye_y = 0;
  out->flags = DB_FLAG_BLINK | DB_FLAG_LOOK | DB_FLAG_BREATHE;
  out->blink_min = 20;
  out->blink_max = 60;
  out->look_min = 15;
  out->look_max = 50;
  out->look_x = 18;
  out->look_y = 8;
  out->speed = 5;
  out->contrast = 0xCF;
  out->overlay = DB_OVERLAY_NONE;
  out->seed = 0;
  out->cycle = DB_CYCLE_OFF;
  out->cycle_secs = 10;
  out->cycle_count = 0;
  out->cycle_flags = DB_CYCLE_FLAG_OVERLAY;
}

static uint8_t clamp_u8(uint8_t v, uint8_t lo, uint8_t hi)
{
  return v < lo ? lo : (v > hi ? hi : v);
}

/* Force every field into the documented range. Identity on valid faces. */
static void face_sanitize(db_face_t *f)
{
  uint8_t i, end = 0;
  if (f->mood >= DB_MOOD_COUNT) f->mood = DB_MOOD_HOVER;
  if (f->overlay >= DB_OVERLAY_COUNT) f->overlay = DB_OVERLAY_NONE;
  f->eye_w = clamp_u8(f->eye_w, 8, 56);
  f->eye_h = clamp_u8(f->eye_h, 8, 56);
  if (f->eye_r > f->eye_w / 2) f->eye_r = (uint8_t)(f->eye_w / 2);
  if (f->eye_gap > 40) f->eye_gap = 40;
  f->speed = clamp_u8(f->speed, 1, 10);
  if (f->cycle >= DB_CYCLE_COUNT) f->cycle = DB_CYCLE_OFF;
  if (f->cycle_secs == 0) f->cycle_secs = 10;
  if (f->cycle_count > DB_CYCLE_MAX) f->cycle_count = DB_CYCLE_MAX;
  f->cycle_flags &= DB_CYCLE_FLAG_OVERLAY;
  for (i = 0; i < DB_CYCLE_MAX; i++) {
    if (i >= f->cycle_count || f->cycle_moods[i] >= DB_MOOD_COUNT) f->cycle_moods[i] = DB_MOOD_HOVER;
  }
  for (i = 0; i < 12; i++) {
    char c = f->name[i];
    if (end || c == '\0') { end = 1; f->name[i] = '\0'; continue; }
    if (c >= 'a' && c <= 'z') c = (char)(c - 'a' + 'A');
    if (!((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == ' ')) c = ' ';
    f->name[i] = c;
  }
  f->name[12] = '\0';
}

void db_face_encode(const db_face_t *face, uint8_t blob[DB_BLOB_SIZE])
{
  uint8_t i;
  uint16_t crc;
  memset(blob, 0, DB_BLOB_SIZE);
  for (i = 0; i < 8; i++) blob[i] = (uint8_t)(k_magic_x[i] ^ MAGIC_X);
  blob[8] = DB_BLOB_VERSION;
  blob[9] = face->mood;
  for (i = 0; i < 12 && face->name[i] != '\0'; i++) blob[10 + i] = (uint8_t)face->name[i];
  blob[22] = face->eye_w;
  blob[23] = face->eye_h;
  blob[24] = face->eye_r;
  blob[25] = face->eye_gap;
  blob[26] = (uint8_t)face->eye_y;
  blob[27] = face->flags;
  blob[28] = face->blink_min;
  blob[29] = face->blink_max;
  blob[30] = face->look_min;
  blob[31] = face->look_max;
  blob[32] = face->look_x;
  blob[33] = face->look_y;
  blob[34] = face->speed;
  blob[35] = face->contrast;
  blob[36] = face->overlay;
  blob[37] = face->seed;
  blob[38] = face->cycle;
  blob[39] = face->cycle_secs;
  blob[40] = face->cycle_count;
  blob[41] = face->cycle_flags;
  for (i = 0; i < DB_CYCLE_MAX; i++) blob[42 + i] = face->cycle_moods[i];
  crc = db_crc16(blob, 62);
  blob[62] = (uint8_t)(crc & 0xFFu);
  blob[63] = (uint8_t)(crc >> 8);
}

int db_face_decode(const uint8_t blob[DB_BLOB_SIZE], db_face_t *out)
{
  uint8_t i;
  uint16_t crc;
  for (i = 0; i < 8; i++) {
    if ((uint8_t)(blob[i] ^ MAGIC_X) != k_magic_x[i]) { db_face_default(out); return -1; }
  }
  if (blob[8] != DB_BLOB_VERSION) { db_face_default(out); return -2; }
  crc = db_crc16(blob, 62);
  if (blob[62] != (uint8_t)(crc & 0xFFu) || blob[63] != (uint8_t)(crc >> 8)) {
    db_face_default(out);
    return -3;
  }
  memset(out, 0, sizeof(*out));
  out->mood = blob[9];
  for (i = 0; i < 12; i++) out->name[i] = (char)blob[10 + i];
  out->name[12] = '\0';
  out->eye_w = blob[22];
  out->eye_h = blob[23];
  out->eye_r = blob[24];
  out->eye_gap = blob[25];
  out->eye_y = (int8_t)blob[26];
  out->flags = blob[27];
  out->blink_min = blob[28];
  out->blink_max = blob[29];
  out->look_min = blob[30];
  out->look_max = blob[31];
  out->look_x = blob[32];
  out->look_y = blob[33];
  out->speed = blob[34];
  out->contrast = blob[35];
  out->overlay = blob[36];
  out->seed = blob[37];
  out->cycle = blob[38];
  out->cycle_secs = blob[39];
  out->cycle_count = blob[40];
  out->cycle_flags = blob[41];
  for (i = 0; i < DB_CYCLE_MAX; i++) out->cycle_moods[i] = blob[42 + i];
  face_sanitize(out);
  return 0;
}

/* ------------------------------------------------------------------------ */
/* Framebuffer primitives (all clip to the screen)                           */
/* ------------------------------------------------------------------------ */
static void put(uint8_t *p, uint8_t m, uint8_t on)
{
  if (on) *p |= m;
  else *p &= (uint8_t)~m;
}

static void vspan(uint8_t *fb, int x, int y0, int y1, uint8_t on)
{
  uint8_t *p;
  int page, last;
  if (x < 0 || x >= DB_W) return;
  if (y0 > y1) { int t = y0; y0 = y1; y1 = t; }
  if (y1 < 0 || y0 >= DB_H) return;
  if (y0 < 0) y0 = 0;
  if (y1 > DB_H - 1) y1 = DB_H - 1;
  page = y0 >> 3;
  last = y1 >> 3;
  p = fb + page * DB_W + x;
  if (page == last) {
    put(p, (uint8_t)((0xFFu << (y0 & 7)) & (0xFFu >> (7 - (y1 & 7)))), on);
    return;
  }
  put(p, (uint8_t)(0xFFu << (y0 & 7)), on);
  for (page++, p += DB_W; page < last; page++, p += DB_W) *p = on ? 0xFFu : 0x00u;
  put(p, (uint8_t)(0xFFu >> (7 - (y1 & 7))), on);
}

static void pset(uint8_t *fb, int x, int y, uint8_t on)
{
  if (x < 0 || x >= DB_W || y < 0 || y >= DB_H) return;
  put(fb + (y >> 3) * DB_W + x, (uint8_t)(1u << (y & 7)), on);
}

static void line(uint8_t *fb, int x0, int y0, int x1, int y1, uint8_t on)
{
  int dx = iabs(x1 - x0), dy = -iabs(y1 - y0);
  int sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
  int err = dx + dy, e2;
  for (;;) {
    pset(fb, x0, y0, on);
    if (x0 == x1 && y0 == y1) break;
    e2 = 2 * err;
    if (e2 >= dy) { err += dy; x0 += sx; }
    if (e2 <= dx) { err += dx; y0 += sy; }
  }
}

static void fill_circle(uint8_t *fb, int cx, int cy, int r, uint8_t on)
{
  int dx, h;
  if (r < 0) return;
  for (dx = -r; dx <= r; dx++) {
    /* r*r + r gives rounder small circles than r*r */
    h = (int)isqrt16((uint16_t)((int32_t)r * r + r - (int32_t)dx * dx));
    vspan(fb, cx + dx, cy - h, cy + h, on);
  }
}

/* Midpoint circle outline. */
static void ring(uint8_t *fb, int cx, int cy, int r)
{
  int x = r, y = 0, err = 1 - r;
  while (x >= y) {
    pset(fb, cx + x, cy + y, 1); pset(fb, cx + y, cy + x, 1);
    pset(fb, cx - y, cy + x, 1); pset(fb, cx - x, cy + y, 1);
    pset(fb, cx - x, cy - y, 1); pset(fb, cx - y, cy - x, 1);
    pset(fb, cx + y, cy - x, 1); pset(fb, cx + x, cy - y, 1);
    y++;
    if (err < 0) {
      err += 2 * y + 1;
    } else {
      x--;
      err += 2 * (y - x) + 1;
    }
  }
}

/* ------------------------------------------------------------------------ */
/* Drones                                                                    */
/* ------------------------------------------------------------------------ */
#define OPT_LIGHT 0x01u /* nav lights lit (strobe flash) */
#define OPT_CAM   0x02u /* camera gimbal */
#define OPT_STILL 0x04u /* rotors stopped */

/* Rotate by (c, s) = (cos, sin) x 127 about (cx, cy) after mirroring x by m. */
typedef struct xf {
  int cx, cy, c, s, m;
} xf_t;

static void xf_pt(const xf_t *t, int x, int y, int *ox, int *oy)
{
  x *= t->m;
  *ox = t->cx + sscale(x, t->c) - sscale(y, t->s);
  *oy = t->cy + sscale(x, t->s) + sscale(y, t->c);
}

static void xline(uint8_t *fb, const xf_t *t, int x0, int y0, int x1, int y1)
{
  int ax, ay, bx, by;
  xf_pt(t, x0, y0, &ax, &ay);
  xf_pt(t, x1, y1, &bx, &by);
  line(fb, ax, ay, bx, by, 1);
}

static int body_h(int s) { return imax(1, s / 14); }
static int leg_h(int s) { return imax(2, s / 9); }

/* Centre to the bottom of the feet. */
static int drone_foot(int s) { return body_h(s) + leg_h(s); }

/* Side view of a quadcopter centred on (cx, cy), s px from rotor to rotor,
 * nose towards +x (m = 1) or -x (m = -1). ang = roll in 64ths of a turn
 * (positive dips the right-hand side). */
static void drone_side(uint8_t *fb, int cx, int cy, int s, int m, uint8_t ang, uint32_t spin, uint8_t opt)
{
  xf_t t;
  int bw = s / 4, bh = body_h(s), aw = s / 2, lg = leg_h(s), pw, x, y, k;
  t.cx = cx;
  t.cy = cy;
  t.c = cos64(ang);
  t.s = sin64(ang);
  t.m = m;
  /* body, filled along both axes so a rotated body has no holes */
  for (y = -bh; y <= bh; y++) xline(fb, &t, -bw, y, bw, y);
  for (x = -bw; x <= bw; x++) xline(fb, &t, x, -bh, x, bh);
  xline(fb, &t, -bw / 2, -bh - 1, bw / 2, -bh - 1); /* canopy */
  xf_pt(&t, bw - 2, 0, &x, &y);
  pset(fb, x, y, 0); /* nose window */
  /* arms, motors, rotors, legs */
  xline(fb, &t, -aw, -bh, aw, -bh);
  for (k = -1; k <= 1; k += 2) {
    xline(fb, &t, k * aw, -bh, k * aw, -bh - 2);
    /* a blade seen edge-on: its length swings as it turns */
    if (opt & OPT_STILL) pw = s / 6;
    else pw = 1 + sscale(s / 5, iabs(cos64((uint8_t)(spin + (uint32_t)(k + 1) * 11u))));
    xline(fb, &t, k * aw - pw, -bh - 3, k * aw + pw, -bh - 3);
    xline(fb, &t, k * bw / 2, bh, k * (bw / 2 + lg), bh + lg);
    xline(fb, &t, k * (bw / 2 + lg) - 2, bh + lg, k * (bw / 2 + lg) + 2, bh + lg);
    if (opt & OPT_LIGHT) {
      xf_pt(&t, k * aw, -bh + 2, &x, &y);
      fill_circle(fb, x, y, 1, 1);
    }
  }
  if (opt & OPT_CAM) {
    for (y = bh + 1; y <= bh + 3; y++) xline(fb, &t, bw / 3, y, bw / 3 + 3, y);
    xf_pt(&t, bw / 3 + 4, bh + 2, &x, &y);
    pset(fb, x, y, 1); /* lens */
  }
}

/* Seen from above: X frame, four rotor rings with counter-rotating blades.
 * yaw 0 points the nose at +x. */
static void drone_top(uint8_t *fb, int cx, int cy, int s, uint8_t yaw, uint32_t spin, uint8_t opt)
{
  int len = imin(s * 3 / 5, 26), r = imax(2, len / 3), k, ex, ey, bx, by;
  for (k = 0; k < 4; k++) {
    uint8_t a = (uint8_t)(yaw + 8 + 16 * k), b;
    ex = cx + sscale(len, cos64(a));
    ey = cy + sscale(len, sin64(a));
    line(fb, cx, cy, ex, ey, 1);
    line(fb, cx + 1, cy, ex + 1, ey, 1);
    vspan(fb, ex, ey - r + 1, ey + r - 1, 0); /* clear the hub area under the ring */
    ring(fb, ex, ey, r);
    b = (uint8_t)((k & 1) ? spin : 0u - spin);
    bx = sscale(r - 1, cos64(b));
    by = sscale(r - 1, sin64(b));
    line(fb, ex - bx, ey - by, ex + bx, ey + by, 1);
    if (opt & OPT_LIGHT) fill_circle(fb, ex, ey, 1, 1);
  }
  k = imax(2, len / 4);
  fill_circle(fb, cx, cy, k, 1);
  pset(fb, cx + sscale(k - 1, cos64(yaw)), cy + sscale(k - 1, sin64(yaw)), 0); /* heading mark */
  if (opt & OPT_CAM) fill_circle(fb, cx + sscale(k + 2, cos64(yaw)), cy + sscale(k + 2, sin64(yaw)), 1, 1);
}

/* Dotted shadow on the bottom row, smaller the higher the drone. */
static void shadow(uint8_t *fb, int cx, int cy, int s)
{
  int alt = 63 - cy, hw, x;
  if (alt < 0) return;
  hw = imax(2, s / 2 - alt / 3);
  for (x = -hw; x <= hw; x += 2) pset(fb, cx + x, 63, 1);
}

static void pad(uint8_t *fb, int cx, int s)
{
  int hw = s / 2 + 4;
  line(fb, cx - hw, 62, cx + hw, 62, 1);
  line(fb, cx - hw + 2, 63, cx + hw - 2, 63, 1);
}

/* Battery gauge, top left: bars 0..4, the next bar blinks while charging. */
static void battery(uint8_t *fb, int bars, uint8_t blink)
{
  int i, x;
  for (x = 1; x <= 25; x++) vspan(fb, x, 0, 12, 0); /* keep the sky out of it */
  line(fb, 3, 2, 21, 2, 1);
  line(fb, 3, 10, 21, 10, 1);
  vspan(fb, 3, 2, 10, 1);
  vspan(fb, 21, 2, 10, 1);
  vspan(fb, 22, 5, 7, 1);
  vspan(fb, 23, 5, 7, 1);
  for (i = 0; i < 4; i++) {
    if (i > bars || (i == bars && !blink)) continue;
    for (x = 5 + i * 4; x < 8 + i * 4; x++) vspan(fb, x, 4, 8, 1);
  }
}

/* Radio mast with a blinking beacon; clears a margin so it reads in front. */
static void mast(uint8_t *fb, uint32_t ac)
{
  vspan(fb, 62, 22, 63, 0);
  vspan(fb, 65, 22, 63, 0);
  fill_circle(fb, 63, 25, 3, 0);
  vspan(fb, 63, 28, 63, 1);
  vspan(fb, 64, 28, 63, 1);
  fill_circle(fb, 63, 25, ((ac >> 9) & 1u) ? 2 : 1, 1);
}

/* ------------------------------------------------------------------------ */
/* Sky backgrounds (stateless, driven by the animation clock)                */
/* ------------------------------------------------------------------------ */
static void cloud(uint8_t *fb, int x, int y, int r)
{
  fill_circle(fb, x, y, r, 1);
  fill_circle(fb, x - r, y + 2, r - 1, 1);
  fill_circle(fb, x + r, y + 2, r - 1, 1);
  fill_circle(fb, x, y, r - 1, 0);
  fill_circle(fb, x - r, y + 2, r - 2, 0);
  fill_circle(fb, x + r, y + 2, r - 2, 0);
}

static void sky(uint8_t *fb, uint8_t ov, uint32_t ac, uint8_t salt)
{
  int i, x, y;
  uint32_t sc, wx;
  switch (ov) {
  case DB_OVERLAY_CLOUDS:
    for (i = 0; i < 3; i++) {
      sc = (ac >> 9) * (uint32_t)(i + 2) / 3u + (uint32_t)i * 67u + salt;
      cloud(fb, 150 - (int)(sc % 190u), 6 + ((i * 9) % 14), 4 + (i & 1));
    }
    break;
  case DB_OVERLAY_STARS:
    for (i = 0; i < 18; i++) {
      x = hash8((uint32_t)i * 7u + salt) >> 1;
      y = hash8((uint32_t)i * 13u + 101u + salt) % 44;
      if (hash8((ac >> 10) + (uint32_t)i * 31u) < 40) continue; /* twinkle */
      pset(fb, x, y, 1);
      if ((i & 3) == 0) {
        pset(fb, x - 1, y, 1);
        pset(fb, x + 1, y, 1);
        pset(fb, x, y - 1, 1);
        pset(fb, x, y + 1, 1);
      }
    }
    break;
  case DB_OVERLAY_RAIN:
    for (i = 0; i < 16; i++) {
      sc = (ac >> 5) * (uint32_t)(2 + (i & 1)) / 2u + hash8((uint32_t)i + salt);
      y = (int)(sc % 72u) - 4;
      x = (hash8((uint32_t)i * 29u + 7u) + (76 - y) / 3) % DB_W;
      line(fb, x, y, x - 1, y + 3, 1);
    }
    break;
  case DB_OVERLAY_CITY:
    sc = ac >> 9;
    for (x = 0; x < DB_W; x++) {
      uint32_t b;
      int lx, top;
      wx = (uint32_t)x + sc;
      b = wx / 12u;
      lx = (int)(wx % 12u);
      top = 55 - (hash8(b + salt) % 14);
      if (lx == 11) continue;
      if (lx == 0 || lx == 10) {
        vspan(fb, x, top, 63, 1);
        continue;
      }
      pset(fb, x, top, 1);
      if (lx == 1 || lx == 5 || lx == 9) continue;
      for (y = top + 3; y < 62; y += 4) {
        if (hash8(b * 32u + (uint32_t)y + (lx > 5 ? 7u : 0u)) & 1u) {
          pset(fb, x, y, 1);
          pset(fb, x, y + 1, 1);
        }
      }
    }
    break;
  case DB_OVERLAY_HILLS:
    sc = ac >> 9;
    for (x = 0; x < DB_W; x++) {
      wx = (uint32_t)x + sc;
      y = 55 + sscale(4, sin64((uint8_t)(wx >> 1))) + sscale(2, sin64((uint8_t)((wx * 3u) >> 1)));
      pset(fb, x, y, 1);
      if (wx % 9u == 0) pset(fb, x, y + 3, 1);
      if (wx % 13u == 4) pset(fb, x, y + 6, 1);
    }
    break;
  case DB_OVERLAY_WIND:
    for (i = 0; i < 6; i++) {
      int len = 6 + (hash8((uint32_t)i + 40u) & 7);
      sc = (ac >> 5) * (uint32_t)(3 + i % 3) / 3u + hash8((uint32_t)i * 17u + salt);
      x = 140 - (int)(sc % 180u);
      y = 4 + i * 10 + (hash8((uint32_t)i) & 3);
      line(fb, x, y, x + len, y, 1);
      if (i & 1) {
        pset(fb, x + len + 1, y - 1, 1);
        pset(fb, x + len + 1, y - 2, 1);
        pset(fb, x + len, y - 3, 1);
      }
    }
    break;
  case DB_OVERLAY_GRID:
    line(fb, 0, 44, DB_W - 1, 44, 1);
    for (i = -4; i <= 4; i++) line(fb, 64 + i * 6, 45, 64 + i * 34, 63, 1);
    for (i = 0; i < 4; i++) {
      int p = (int)(((ac >> 6) + (uint32_t)i * 64u) & 255u);
      y = 45 + (int)((int32_t)p * p / 3449);
      for (x = (y & 1); x < DB_W; x += 2) pset(fb, x, y, 1);
    }
    break;
  default:
    break;
  }
}

/* ------------------------------------------------------------------------ */
/* Drift (the face engine's glance, now the drone moving around)             */
/* ------------------------------------------------------------------------ */
static uint32_t drift_dur(const db_engine_t *e) { return 400u + 2000u / e->face.speed; }

static uint32_t drift_interval(db_engine_t *e)
{
  uint32_t lo = (uint32_t)(e->face.look_min ? e->face.look_min : 1) * 100u;
  uint32_t hi = (uint32_t)e->face.look_max * 100u;
  uint32_t v = rng_range(e, lo, hi < lo ? lo : hi);
  return v < 150u ? 150u : v;
}

/* Current drift offset, 8.8 fixed point. */
static void drift_pos(const db_engine_t *e, uint32_t now, int *x, int *y)
{
  uint32_t el = now - e->last_ms, d = drift_dur(e);
  int32_t tx = (int32_t)e->look_tgt_x * 256, ty = (int32_t)e->look_tgt_y * 256;
  int s;
  if ((int32_t)el < 0) { *x = e->look_cur_x; *y = e->look_cur_y; return; }
  if (el >= d) { *x = (int)tx; *y = (int)ty; return; }
  s = ease((int)(el * 256u / d));
  *x = (int)(e->look_cur_x + (tx - e->look_cur_x) * s / 256);
  *y = (int)(e->look_cur_y + (ty - e->look_cur_y) * s / 256);
}

static void pick_target(db_engine_t *e)
{
  int lx = imin(e->face.look_x, 60), ly = imin(e->face.look_y, 30);
  if ((rng_next(e) & 7u) < 2) { /* back to the middle */
    e->look_tgt_x = 0;
    e->look_tgt_y = 0;
    return;
  }
  e->look_tgt_x = (int8_t)rng_signed(e, lx);
  e->look_tgt_y = (int8_t)rng_signed(e, ly);
}

/* ------------------------------------------------------------------------ */
/* Scene cycle                                                               */
/* ------------------------------------------------------------------------ */
#define CYCLE_FADE_MS 300u /* drones climb out this long before a change, drop in after */

static uint8_t mood_overlay(uint8_t mood)
{
  switch (mood) {
  case DB_MOOD_PATROL: case DB_MOOD_DELIVERY: return DB_OVERLAY_CITY;
  case DB_MOOD_TAKEOFF: return DB_OVERLAY_HILLS;
  case DB_MOOD_ORBIT: case DB_MOOD_CHARGE: return DB_OVERLAY_CLOUDS;
  case DB_MOOD_RACE: return DB_OVERLAY_WIND;
  case DB_MOOD_SCAN: return DB_OVERLAY_GRID;
  case DB_MOOD_SWARM: return DB_OVERLAY_STARS;
  default: return DB_OVERLAY_NONE;
  }
}

static uint8_t cycle_len(const db_face_t *f) { return f->cycle_count ? f->cycle_count : (uint8_t)DB_MOOD_COUNT; }

static uint8_t cycle_entry(const db_face_t *f, uint8_t i) { return f->cycle_count ? f->cycle_moods[i] : i; }

static uint32_t cycle_period(const db_face_t *f) { return (uint32_t)f->cycle_secs * 1000u; }

static void set_mood(db_engine_t *e, uint8_t mood)
{
  e->face.mood = mood;
  if (e->face.cycle_flags & DB_CYCLE_FLAG_OVERLAY) e->face.overlay = mood_overlay(mood);
}

static void cycle_next(db_engine_t *e)
{
  const db_face_t *f = &e->face;
  uint8_t n = cycle_len(f), m;
  if (f->cycle == DB_CYCLE_SEQUENCE) {
    e->cycle_idx = (uint8_t)((e->cycle_idx + 1u) % n);
    m = cycle_entry(f, e->cycle_idx);
  } else {
    uint8_t i = (uint8_t)(rng_next(e) % n), k;
    m = cycle_entry(f, i);
    /* step past the current scene; gives up when every entry is the same */
    for (k = 1; m == f->mood && k < n; k++) m = cycle_entry(f, (uint8_t)((i + k) % n));
  }
  set_mood(e, m);
}

static void cycle_update(db_engine_t *e, uint32_t now)
{
  uint32_t period = cycle_period(&e->face);
  int32_t d;
  if (e->face.cycle == DB_CYCLE_OFF) return;
  d = (int32_t)(e->cycle_at - now);
  if (d > (int32_t)period) {
    e->cycle_at = now + period; /* clock went backwards */
  } else if (d <= 0) {
    cycle_next(e);
    e->cycle_at += period;
    if ((int32_t)(e->cycle_at - now) <= 0) e->cycle_at = now + period; /* fell far behind */
  }
}

/* 0 = in place .. 256 = gone off the top: the climb that hides a scene change */
static int cycle_amount(const db_engine_t *e, uint32_t now)
{
  uint32_t period = cycle_period(&e->face), to = e->cycle_at - now, since;
  if (e->face.cycle == DB_CYCLE_OFF || to > period) return 0;
  since = period - to;
  if (to < CYCLE_FADE_MS) return ease((int)((CYCLE_FADE_MS - to) * 256u / CYCLE_FADE_MS));
  if (since < CYCLE_FADE_MS) return 256 - ease((int)(since * 256u / CYCLE_FADE_MS));
  return 0;
}

static void update(db_engine_t *e, uint32_t now)
{
  if (!(e->blinking & ST_STARTED)) {
    e->blinking = ST_STARTED;
    e->next_look_ms = now + drift_interval(e) / 2u;
    e->last_ms = now - 100000u;
    e->cycle_at = now + cycle_period(&e->face);
    e->look_cur_x = e->look_cur_y = 0;
    e->look_tgt_x = e->look_tgt_y = 0;
  }
  if (e->face.flags & DB_FLAG_LOOK) {
    int32_t d = (int32_t)(e->next_look_ms - now);
    if (d > 80000L) {
      e->next_look_ms = now + drift_interval(e); /* clock went backwards */
    } else if (d <= 0) {
      int cx, cy;
      drift_pos(e, now, &cx, &cy);
      e->look_cur_x = (int16_t)cx;
      e->look_cur_y = (int16_t)cy;
      pick_target(e);
      e->last_ms = now;
      e->next_look_ms = now + drift_interval(e);
    }
  } else {
    e->look_cur_x = e->look_cur_y = 0;
    e->look_tgt_x = e->look_tgt_y = 0;
  }
}

/* Nav lights: two short flashes every blink_min x 100 ms. */
static uint8_t strobe_on(const db_face_t *f, uint32_t now)
{
  uint32_t ph;
  if (!(f->flags & DB_FLAG_BLINK)) return 0;
  ph = now % ((uint32_t)(f->blink_min ? f->blink_min : 1) * 100u);
  return (uint8_t)(ph < 60u || (ph >= 160u && ph < 220u));
}

/* ------------------------------------------------------------------------ */
/* Scenes                                                                    */
/* ------------------------------------------------------------------------ */
typedef struct scene {
  uint8_t *fb;
  const db_face_t *f;
  uint32_t ac;   /* animation clock */
  uint32_t spin; /* rotor phase */
  int s;         /* drone span */
  int x, y;      /* home position: centre + altitude + drift + bob - climb-out */
  int tilt;      /* bank from the drift, 64ths of a turn */
  int lift;      /* climb-out px during a scene change */
  uint8_t opt;   /* OPT_* from the flags */
} scene_t;

static void drone(const scene_t *S, int x, int y, int s, int m, int ang, uint8_t opt)
{
  if (S->f->flags & DB_FLAG_MOUTH) shadow(S->fb, x, y, s);
  drone_side(S->fb, x, y, s, m, (uint8_t)ang, S->spin, (uint8_t)(S->opt | opt));
}

/* Pad scenes: 0..1023 phase -> altitude 0..256 above the pad. */
static void pad_scene(const scene_t *S, int alt, uint8_t opt)
{
  int gy = 61 - drone_foot(S->s);
  int x = DB_W / 2 + (S->x - DB_W / 2) * alt / 256;
  int y = gy - S->lift + (S->y + S->lift - gy) * alt / 256;
  pad(S->fb, DB_W / 2, S->s);
  drone(S, x, y, S->s, 1, S->tilt * alt / 256, opt);
}

static void scene_draw(const scene_t *S)
{
  uint8_t *fb = S->fb;
  uint32_t ac = S->ac;
  int s = S->s, x = S->x, y = S->y, a, p, k;
  uint8_t ph;

  switch (S->f->mood) {
  case DB_MOOD_PATROL:
    ph = (uint8_t)(ac >> 9);
    a = cos64(ph); /* velocity */
    drone(S, x + sscale((DB_W - s) / 2 - 2, sin64(ph)), y, s, a < 0 ? -1 : 1, iclamp(S->tilt + sscale(3, a), -4, 4), 0);
    break;

  case DB_MOOD_TAKEOFF:
    p = (int)((ac >> 6) & 1023u);
    if (p < 192) a = 0;
    else if (p < 384) a = ramp(p, 192, 384);
    else if (p < 640) a = 256;
    else if (p < 832) a = 256 - ramp(p, 640, 832);
    else a = 0;
    pad_scene(S, a, (p < 128 || p > 900) ? OPT_STILL : 0);
    break;

  case DB_MOOD_CHARGE:
    p = (int)((ac >> 6) & 1023u);
    if (p < 704) a = 0;
    else if (p < 800) a = ramp(p, 704, 800);
    else if (p < 896) a = 256;
    else a = 256 - ramp(p, 896, 1000);
    k = p < 704 ? p * 4 / 704 : 4 - (p - 704) / 80;
    battery(fb, k, (uint8_t)(p >= 704 || ((ac >> 8) & 1u)));
    pad_scene(S, a, p < 680 ? OPT_STILL : 0);
    break;

  case DB_MOOD_FLIP:
    p = (int)((ac >> 5) & 1023u);
    a = p < 768 ? 0 : ramp(p, 768, 1000) / 4; /* 0..64: one full roll */
    drone(S, x, y - sscale(10, sin64((uint8_t)(a / 2))), s, 1, S->tilt + a, 0);
    break;

  case DB_MOOD_ORBIT: {
    int sn, ds;
    ph = (uint8_t)(ac >> 9);
    sn = sin64(ph); /* > 0: the near half of the circle */
    ds = imax(8, s * (176 + sscale(80, sn)) / 256);
    if (sn > 0) mast(fb, ac);
    drone(S, x + sscale(44, cos64(ph)), y + sscale(6, sn), ds, sn < 0 ? 1 : -1, sscale(-3, sn), 0);
    if (sn <= 0) mast(fb, ac);
    break;
  }

  case DB_MOOD_FORMATION: {
    int ws = imax(8, s * 2 / 3), dx = imin(S->f->eye_gap + ws, DB_W / 2 - ws / 2 - 1);
    for (k = -1; k <= 1; k++) {
      int bob = sscale(2, sin64((uint8_t)((ac >> 8) + (uint32_t)(k + 1) * 21u)));
      drone(S, x + k * dx, y - 6 + iabs(k) * 12 + bob, ws, 1, S->tilt, 0);
    }
    break;
  }

  case DB_MOOD_DELIVERY: {
    int th, bx, by;
    ph = (uint8_t)(ac >> 9);
    x += sscale(14, sin64(ph));
    y -= 10;
    th = sscale(10, sin64((uint8_t)(ph - 10))); /* the parcel swings behind */
    bx = x + sscale(14, sin64((uint8_t)th));
    by = y + body_h(s) + sscale(14, cos64((uint8_t)th));
    line(fb, x, y + body_h(s), bx, by, 1);
    for (k = -4; k <= 3; k++) vspan(fb, bx + k, by, by + 6, 1);
    vspan(fb, bx, by + 1, by + 5, 0);
    line(fb, bx - 3, by + 3, bx + 2, by + 3, 0);
    drone(S, x, y, s, 1, S->tilt + sscale(2, cos64(ph)), 0);
    break;
  }

  case DB_MOOD_RACE:
    x = (int)((ac >> 5) % 256u) - 64;
    y += sscale(6, sin64((uint8_t)(ac >> 8)));
    for (k = 0; k < 3; k++) {
      int x1 = x - s / 2 - 4;
      line(fb, x1 - 8 - k * 5 - (k & 1) * 6, y - 4 + k * 4, x1, y - 4 + k * 4, 1);
    }
    drone(S, x, y, s, 1, 4, 0);
    break;

  case DB_MOOD_SCAN: {
    int sx, top;
    y -= 14;
    sx = x + sscale(34, sin64((uint8_t)(ac >> 9)));
    top = y + drone_foot(s);
    line(fb, x - 1, top, sx - 9, 62, 1);
    line(fb, x + 1, top, sx + 9, 62, 1);
    for (k = -10; k <= 10; k++)
      if ((k + (int)((ac >> 6) & 1u)) & 1) pset(fb, sx + k, 63, 1);
    drone(S, x, y, s, sx >= x ? 1 : -1, S->tilt, OPT_CAM);
    break;
  }

  case DB_MOOD_SWARM: {
    int ss = imax(8, s / 3), ax = DB_W / 2 - ss / 2 - 2, ay = DB_H / 2 - ss / 3 - 4;
    for (k = 0; k < 7; k++) {
      uint8_t px = (uint8_t)((ac >> (8 + (k & 1))) + (uint32_t)k * 23u + S->f->seed);
      uint8_t py = (uint8_t)((ac >> (9 - (k % 3 == 0))) + (uint32_t)k * 41u);
      a = cos64(px);
      drone(S, DB_W / 2 + sscale(ax, sin64(px)), DB_H / 2 + sscale(ay, sin64(py)) - S->lift,
            ss, a < 0 ? -1 : 1, sscale(3, a), 0);
    }
    break;
  }

  case DB_MOOD_TOPDOWN:
    drone_top(fb, x, y, s, (uint8_t)(ac >> 10), ac >> 5, S->opt);
    break;

  default: /* hover */
    drone(S, x, y, s, 1, S->tilt, 0);
    break;
  }
}

/* ------------------------------------------------------------------------ */
/* Display flags                                                             */
/* ------------------------------------------------------------------------ */
static void fb_invert(uint8_t *fb)
{
  uint16_t i;
  for (i = 0; i < DB_FB_SIZE; i++) fb[i] = (uint8_t)~fb[i];
}

static uint8_t rev8(uint8_t b)
{
  b = (uint8_t)(((b & 0xF0u) >> 4) | ((b & 0x0Fu) << 4));
  b = (uint8_t)(((b & 0xCCu) >> 2) | ((b & 0x33u) << 2));
  b = (uint8_t)(((b & 0xAAu) >> 1) | ((b & 0x55u) << 1));
  return b;
}

static void fb_rotate180(uint8_t *fb)
{
  uint16_t i, j;
  for (i = 0, j = DB_FB_SIZE - 1; i < j; i++, j--) {
    uint8_t t = rev8(fb[i]);
    fb[i] = rev8(fb[j]);
    fb[j] = t;
  }
}

/* ------------------------------------------------------------------------ */
/* Public engine API                                                         */
/* ------------------------------------------------------------------------ */
void db_engine_init(db_engine_t *e, const db_face_t *face, uint32_t seed)
{
  uint8_t i;
  memset(e, 0, sizeof(*e));
  e->face = *face;
  face_sanitize(&e->face);
  if (e->face.cycle == DB_CYCLE_SEQUENCE) set_mood(e, cycle_entry(&e->face, 0));
  else if (e->face.cycle == DB_CYCLE_RANDOM) set_mood(e, e->face.mood);
  e->rng = seed ^ ((uint32_t)e->face.seed * 0x9E3779B1UL) ^ 0xA5F1523DUL;
  if (e->rng == 0) e->rng = 0x6D2B79F5UL;
  for (i = 0; i < 4; i++) (void)rng_next(e);
}

void db_engine_render(db_engine_t *e, uint32_t now_ms, uint8_t fb[DB_FB_SIZE])
{
  const db_face_t *f = &e->face;
  scene_t S;
  int dx, dy, px, py, bob = 0;

  update(e, now_ms);
  cycle_update(e, now_ms);
  memset(fb, 0, DB_FB_SIZE);

  S.fb = fb;
  S.f = f;
  S.ac = now_ms * f->speed; /* animation clock: 5 ticks per ms at speed 5 */
  S.spin = S.ac >> 3;
  S.s = f->eye_w;
  S.lift = cycle_amount(e, now_ms) * 60 / 256;
  S.opt = (uint8_t)((strobe_on(f, now_ms) ? OPT_LIGHT : 0u) | ((f->flags & DB_FLAG_CYCLOPS) ? OPT_CAM : 0u));

  sky(fb, f->overlay, S.ac, f->seed);

  drift_pos(e, now_ms, &dx, &dy);
  drift_pos(e, now_ms - 40u, &px, &py);
  if (f->flags & DB_FLAG_BREATHE) bob = sscale(3, sin64((uint8_t)(S.ac >> 8)));
  S.x = DB_W / 2 + (dx + (dx >= 0 ? 128 : -128)) / 256;
  S.y = DB_H / 2 + iclamp(f->eye_y, -24, 24) + (dy + (dy >= 0 ? 128 : -128)) / 256 + bob - S.lift;
  S.tilt = iclamp((dx - px) / 64, -3, 3); /* bank into the move */

  scene_draw(&S);

  if (f->flags & DB_FLAG_INVERT) fb_invert(fb);
  if (f->flags & DB_FLAG_ROTATE180) fb_rotate180(fb);
}
