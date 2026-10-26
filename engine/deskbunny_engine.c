/*
 * DeskBunny face engine. Strict C99, integer only, no malloc, no stdio.
 * Runs on a 16 MHz ATmega328P (int is 16 bits there: every product that can
 * exceed 32767 is done in int32_t) and on the desktop simulator via FFI.
 *
 * Drawing model: every eye is a set of vertical spans, one per column. A base
 * shape (rounded rect, ellipse, heart) gives the top/bottom of each column,
 * then lids clip it (straight top lid, flat bottom lid, curved "happy" cut)
 * and a blink scales it towards the eye centre. Vertical spans map straight
 * onto SSD1306 pages, so a whole face is a few hundred byte writes.
 */
#include "deskbunny_engine.h"

#include <string.h>

/* ------------------------------------------------------------------------ */
/* Engine state usage (db_engine_t keeps its public layout):                 */
/*   last_ms        start of the current glance                              */
/*   look_cur_x/y   glance origin, 8.8 fixed point px                        */
/*   look_tgt_x/y   glance target, px                                        */
/*   blinking       ST_* bits below                                          */
/* ------------------------------------------------------------------------ */
#define ST_BLINK   0x01u /* a blink is in progress */
#define ST_DOUBLE  0x02u /* another blink follows this one */
#define ST_SECOND  0x04u /* the next blink is the second of a pair */
#define ST_STARTED 0x80u /* timers have been based on the first now_ms */

enum { SH_RRECT = 0, SH_ELLIPSE, SH_HEART, SH_SPIRAL };

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

/* '?' from the classic 5x7 font, column-major, LSB at the top. */
static const uint8_t k_glyph_q[5] = { 0x02, 0x01, 0x51, 0x09, 0x06 };

/* ------------------------------------------------------------------------ */
/* Small integer helpers                                                     */
/* ------------------------------------------------------------------------ */
static int16_t imin(int16_t a, int16_t b) { return a < b ? a : b; }
static int16_t imax(int16_t a, int16_t b) { return a > b ? a : b; }
static int16_t iabs(int16_t a) { return a < 0 ? (int16_t)-a : a; }

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

/* sqrt(r*r - d*d), 0 outside the circle. r, d in [0, 255]. */
static int16_t circ(int16_t r, int16_t d)
{
  int32_t v;
  d = iabs(d);
  if (d >= r) return 0;
  v = (int32_t)r * r - (int32_t)d * d;
  if (v > 65535) v = 65535;
  return (int16_t)isqrt16((uint16_t)v);
}

/* One turn = 64 steps; returns -127..127. */
static int16_t sin64(uint8_t a)
{
  uint8_t q;
  a &= 63;
  q = (uint8_t)(a & 15);
  if (a < 16) return k_sin_q[q];
  if (a < 32) return k_sin_q[16 - q];
  if (a < 48) return (int16_t)-k_sin_q[q];
  return (int16_t)-k_sin_q[16 - q];
}
static int16_t cos64(uint8_t a) { return sin64((uint8_t)(a + 16)); }

/* a * s / 127 for a sine value s, rounded towards zero. The reciprocal
 * multiply is exact for |a * s| <= 16762 and avoids a slow AVR division. */
static int16_t sscale(int16_t a, int16_t s)
{
  int32_t p = (int32_t)a * s;
  if (p < 0) return (int16_t)-(int16_t)((-p * 8257L) >> 20);
  return (int16_t)((p * 8257L) >> 20);
}

/* Smoothstep on 0..256. */
static int16_t ease(int16_t x)
{
  int32_t v;
  if (x <= 0) return 0;
  if (x >= 256) return 256;
  v = (int32_t)x * x * (768 - 2 * (int32_t)x);
  return (int16_t)(v >> 16);
}

/* Cheap stateless hash for jitter that must not consume the PRNG. */
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
static int16_t rng_signed(db_engine_t *e, int16_t m)
{
  if (m <= 0) return 0;
  return (int16_t)((int16_t)(rng_next(e) % (uint32_t)(2 * m + 1)) - m);
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
  static const char name[] = "DESKBUNNY";
  memset(out, 0, sizeof(*out));
  out->mood = DB_MOOD_NEUTRAL;
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
}

static uint8_t clamp_u8(uint8_t v, uint8_t lo, uint8_t hi)
{
  return v < lo ? lo : (v > hi ? hi : v);
}

/* Force every field into the documented range. Identity on valid faces. */
static void face_sanitize(db_face_t *f)
{
  uint8_t i, end = 0;
  if (f->mood >= DB_MOOD_COUNT) f->mood = DB_MOOD_NEUTRAL;
  if (f->overlay >= DB_OVERLAY_COUNT) f->overlay = DB_OVERLAY_NONE;
  f->eye_w = clamp_u8(f->eye_w, 8, 56);
  f->eye_h = clamp_u8(f->eye_h, 8, 56);
  if (f->eye_r > f->eye_w / 2) f->eye_r = (uint8_t)(f->eye_w / 2);
  if (f->eye_gap > 40) f->eye_gap = 40;
  f->speed = clamp_u8(f->speed, 1, 10);
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

static void vspan(uint8_t *fb, int16_t x, int16_t y0, int16_t y1, uint8_t on)
{
  uint8_t *p;
  int16_t page, last;
  if (x < 0 || x >= DB_W) return;
  if (y0 > y1) { int16_t t = y0; y0 = y1; y1 = t; }
  if (y1 < 0 || y0 >= DB_H) return;
  if (y0 < 0) y0 = 0;
  if (y1 > DB_H - 1) y1 = DB_H - 1;
  page = (int16_t)(y0 >> 3);
  last = (int16_t)(y1 >> 3);
  p = fb + page * DB_W + x;
  if (page == last) {
    put(p, (uint8_t)((0xFFu << (y0 & 7)) & (0xFFu >> (7 - (y1 & 7)))), on);
    return;
  }
  put(p, (uint8_t)(0xFFu << (y0 & 7)), on);
  for (page++, p += DB_W; page < last; page++, p += DB_W) *p = on ? 0xFFu : 0x00u;
  put(p, (uint8_t)(0xFFu >> (7 - (y1 & 7))), on);
}

static void pset(uint8_t *fb, int16_t x, int16_t y, uint8_t on)
{
  if (x < 0 || x >= DB_W || y < 0 || y >= DB_H) return;
  put(fb + (y >> 3) * DB_W + x, (uint8_t)(1u << (y & 7)), on);
}

static void line(uint8_t *fb, int16_t x0, int16_t y0, int16_t x1, int16_t y1, uint8_t on)
{
  int16_t dx = iabs((int16_t)(x1 - x0)), dy = (int16_t)-iabs((int16_t)(y1 - y0));
  int16_t sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
  int16_t err = (int16_t)(dx + dy), e2;
  for (;;) {
    pset(fb, x0, y0, on);
    if (x0 == x1 && y0 == y1) break;
    e2 = (int16_t)(2 * err);
    if (e2 >= dy) { err = (int16_t)(err + dy); x0 = (int16_t)(x0 + sx); }
    if (e2 <= dx) { err = (int16_t)(err + dx); y0 = (int16_t)(y0 + sy); }
  }
}

/* 2 px thick line (adds a copy one pixel to the right and below). */
static void line2(uint8_t *fb, int16_t x0, int16_t y0, int16_t x1, int16_t y1, uint8_t on)
{
  line(fb, x0, y0, x1, y1, on);
  line(fb, (int16_t)(x0 + 1), y0, (int16_t)(x1 + 1), y1, on);
  line(fb, x0, (int16_t)(y0 + 1), x1, (int16_t)(y1 + 1), on);
}

static void fill_circle(uint8_t *fb, int16_t cx, int16_t cy, int16_t r, uint8_t on)
{
  int16_t dx, h;
  if (r < 0) return;
  for (dx = (int16_t)-r; dx <= r; dx++) {
    /* r*r + r gives rounder small circles than r*r */
    h = (int16_t)isqrt16((uint16_t)((int32_t)r * r + r - (int32_t)dx * dx));
    vspan(fb, (int16_t)(cx + dx), (int16_t)(cy - h), (int16_t)(cy + h), on);
  }
}

/* Four-pointed sparkle (astroid |x|^.5 + |y|^.5 <= s^.5). */
static void astroid(uint8_t *fb, int16_t cx, int16_t cy, int16_t s, uint8_t on)
{
  int16_t dx, rs = (int16_t)isqrt16((uint16_t)(s * 256));
  for (dx = (int16_t)-s; dx <= s; dx++) {
    int16_t a = (int16_t)(rs - (int16_t)isqrt16((uint16_t)(iabs(dx) * 256)));
    int16_t h = (int16_t)((a * a + 128) / 256);
    vspan(fb, (int16_t)(cx + dx), (int16_t)(cy - h), (int16_t)(cy + h), on);
  }
}

/* ------------------------------------------------------------------------ */
/* Eyes                                                                      */
/* ------------------------------------------------------------------------ */
typedef struct eye {
  int16_t x0, y0, w, h, r;
  int16_t lid_l, lid_m, lid_r; /* top lid depth (px from eye top): left edge, middle, right edge */
  int16_t lid_bot;      /* flat lower lid depth, px */
  int16_t arc;          /* happy cut: lift of the lower curve at the centre, px */
  int16_t open;         /* 0 = closed .. 256 = open */
  uint8_t shape;
} eye_t;

static void draw_eye(uint8_t *fb, const eye_t *e, uint8_t on)
{
  int16_t i, w = e->w, h = e->h, rr = e->r;
  int16_t c2 = (int16_t)(2 * e->y0 + h - 1); /* doubled vertical centre */
  int16_t yb = (int16_t)(e->y0 + h - 1);
  int16_t mid = (int16_t)((w - 1) / 2);
  int32_t lid8, lid_step;
  uint16_t ky = 0, karc = 0;
  int16_t arc_a = (int16_t)(w + w / 4);
  /* heart, in half pixels: bumps of radius hr centred hc from the middle */
  int16_t hc = (int16_t)(w * 9 / 20), hr = (int16_t)(w - w * 9 / 20);
  int16_t hb = (int16_t)(2 * h - 1);
  uint16_t hslope = 0;

  if (w < 2 || h < 2) return;
  if (rr > w / 2) rr = (int16_t)(w / 2);
  if (rr > h / 2) rr = (int16_t)(h / 2);
  if (rr < 0) rr = 0;
  if (e->shape == SH_ELLIPSE) ky = (uint16_t)(((uint32_t)h << 8) / (uint32_t)w);
  if (e->shape == SH_HEART && hb > hr) hslope = (uint16_t)(((uint32_t)(hb - hr) << 8) / (uint32_t)w);
  if (e->arc > 0) karc = (uint16_t)(((uint32_t)e->arc << 8) / (uint32_t)arc_a);
  /* the top lid is two straight segments: left edge -> middle -> right edge */
  lid8 = (int32_t)e->lid_l * 256 + 128;
  lid_step = mid > 0 ? ((int32_t)(e->lid_m - e->lid_l) * 256) / mid : 0;

  for (i = 0; i < w; i++, lid8 += lid_step) {
    if (i == mid) {
      lid8 = (int32_t)e->lid_m * 256 + 128;
      lid_step = ((int32_t)(e->lid_r - e->lid_m) * 256) / (w - 1 - mid);
    }
    int16_t d = iabs((int16_t)(2 * i - (w - 1))); /* half px from the middle */
    int16_t ti = 0, bi = 0, top, bot;
    if (e->shape == SH_RRECT) {
      int16_t edge = (int16_t)(w - d);
      if (edge < 2 * rr) {
        int16_t ins2 = (int16_t)(2 * rr - circ((int16_t)(2 * rr), (int16_t)(2 * rr - edge)));
        ti = bi = (int16_t)((ins2 + 1) / 2);
      }
    } else if (e->shape == SH_ELLIPSE) {
      int16_t hh2 = (int16_t)(((uint32_t)ky * (uint16_t)circ(w, d)) >> 8);
      ti = bi = (int16_t)((h - hh2 + 1) / 2);
    } else { /* SH_HEART */
      int16_t dd = (int16_t)(d - hc), s = circ(hr, dd);
      int16_t topy = iabs(dd) <= hr ? (int16_t)(hr - s) : hr;
      int16_t boty = (int16_t)(hr + s);
      int16_t tri = (int16_t)(hb - (int16_t)(((uint32_t)hslope * (uint16_t)d) >> 8));
      if (tri > boty) boty = tri;
      if (iabs(dd) > hr && d > hc) continue;
      ti = (int16_t)(topy / 2);
      bi = (int16_t)((hb - boty) / 2);
    }
    top = (int16_t)(e->y0 + ti);
    bot = (int16_t)(yb - bi);
    if (lid8 > 256) top = imax(top, (int16_t)(e->y0 + (int16_t)(lid8 >> 8)));
    if (e->lid_bot > 0) bot = imin(bot, (int16_t)(yb - e->lid_bot));
    if (karc && d < arc_a) {
      int16_t lift = (int16_t)(((uint32_t)karc * (uint16_t)circ(arc_a, d)) >> 8);
      bot = imin(bot, (int16_t)(yb - lift));
    }
    if (top > bot) continue;
    if (e->open < 256) {
      int16_t o = e->open < 0 ? 0 : e->open;
      int16_t t2 = (int16_t)(c2 + (int16_t)((int32_t)(2 * top - c2) * o / 256));
      int16_t b2 = (int16_t)(c2 + (int16_t)((int32_t)(2 * bot - c2) * o / 256));
      top = (int16_t)(t2 >= 0 ? t2 / 2 : (t2 - 1) / 2);
      bot = (int16_t)(b2 >= 0 ? (b2 + 1) / 2 : b2 / 2);
      if (bot - top < 1) {
        /* closed: a 2 px line, trimmed at rounded ends */
        if (o < 48 && ti * 3 > h) continue;
        bot = (int16_t)(top + 1);
      }
    }
    vspan(fb, (int16_t)(e->x0 + i), top, bot, on);
  }
}

static void draw_spiral(uint8_t *fb, int16_t cx, int16_t cy, int16_t rad, uint8_t rot, int8_t dir)
{
  int16_t k, px = cx, py = cy;
  for (k = 1; k <= 40; k++) {
    uint8_t a = (uint8_t)(rot + dir * k * 4);
    int16_t r = (int16_t)(((int32_t)rad * k * 205) >> 13); /* rad * k / 40 */
    int16_t x = (int16_t)(cx + sscale(r, cos64(a)));
    int16_t y = (int16_t)(cy + sscale(r, sin64(a)));
    line2(fb, px, py, x, y, 1);
    px = x;
    py = y;
  }
}

/* ------------------------------------------------------------------------ */
/* Timers                                                                    */
/* ------------------------------------------------------------------------ */
static uint32_t blink_dur(const db_engine_t *e)
{
  uint32_t d = 100u + 250u / e->face.speed;
  if (e->face.mood == DB_MOOD_SLEEPY) d *= 3u;
  return d;
}

static uint32_t glance_dur(const db_engine_t *e)
{
  uint32_t d = 120u + 400u / e->face.speed;
  switch (e->face.mood) {
  case DB_MOOD_SUSPICIOUS: case DB_MOOD_SLEEPY: case DB_MOOD_BORED: d *= 2u; break;
  case DB_MOOD_SCARED: case DB_MOOD_EXCITED: d /= 2u; break;
  default: break;
  }
  return d;
}

static uint32_t blink_interval(db_engine_t *e)
{
  uint32_t lo = (uint32_t)(e->face.blink_min ? e->face.blink_min : 1) * 100u;
  uint32_t hi = (uint32_t)e->face.blink_max * 100u;
  return rng_range(e, lo, hi < lo ? lo : hi);
}

static uint32_t look_interval(db_engine_t *e)
{
  uint32_t lo = (uint32_t)(e->face.look_min ? e->face.look_min : 1) * 100u;
  uint32_t hi = (uint32_t)e->face.look_max * 100u;
  uint32_t v = rng_range(e, lo, hi < lo ? lo : hi);
  switch (e->face.mood) {
  case DB_MOOD_SLEEPY: case DB_MOOD_BORED: v *= 2u; break;
  case DB_MOOD_SCARED: v /= 3u; break;
  case DB_MOOD_EXCITED: v /= 2u; break;
  default: break;
  }
  return v < 150u ? 150u : v;
}

/* Current glance position, 8.8 fixed point. */
static void look_pos(const db_engine_t *e, uint32_t now, int16_t *x, int16_t *y)
{
  uint32_t el = now - e->last_ms, d = glance_dur(e);
  int32_t tx = (int32_t)e->look_tgt_x * 256, ty = (int32_t)e->look_tgt_y * 256;
  int16_t s;
  if ((int32_t)el < 0 || el >= d) { *x = (int16_t)tx; *y = (int16_t)ty; return; }
  s = ease((int16_t)(el * 256u / d));
  *x = (int16_t)(e->look_cur_x + (tx - e->look_cur_x) * s / 256);
  *y = (int16_t)(e->look_cur_y + (ty - e->look_cur_y) * s / 256);
}

static void pick_target(db_engine_t *e)
{
  int16_t lx = imin(e->face.look_x, 60), ly = imin(e->face.look_y, 30);
  int16_t tx, ty;
  uint8_t r = (uint8_t)(rng_next(e) & 7u);
  switch (e->face.mood) {
  case DB_MOOD_SUSPICIOUS:
    if (lx < 8) lx = 8;
    tx = e->look_tgt_x > 0 ? (int16_t)-lx : lx;
    if (r < 2) tx = 0;
    ty = 0;
    break;
  case DB_MOOD_SLEEPY:
    tx = rng_signed(e, (int16_t)(lx / 2));
    ty = (int16_t)(ly / 2 + (int16_t)rng_range(e, 0, (uint32_t)(ly / 2)));
    break;
  case DB_MOOD_BORED:
    tx = (int16_t)(lx / 2 + (int16_t)rng_range(e, 0, (uint32_t)(lx / 2)));
    if (r & 1) tx = (int16_t)-tx;
    ty = (int16_t)(ly / 2 + (int16_t)rng_range(e, 0, (uint32_t)(ly / 2)));
    break;
  case DB_MOOD_SCARED:
    tx = rng_signed(e, lx);
    ty = rng_signed(e, ly);
    break;
  default:
    if (r < 3) { tx = 0; ty = 0; } /* look back at you */
    else { tx = rng_signed(e, lx); ty = rng_signed(e, ly); }
    break;
  }
  e->look_tgt_x = (int8_t)tx;
  e->look_tgt_y = (int8_t)ty;
}

static void update(db_engine_t *e, uint32_t now)
{
  const db_face_t *f = &e->face;
  uint8_t lookable = (uint8_t)((f->flags & DB_FLAG_LOOK) || f->mood == DB_MOOD_SUSPICIOUS);
  if (f->mood == DB_MOOD_DIZZY) lookable = 0;

  if (!(e->blinking & ST_STARTED)) {
    e->blinking = ST_STARTED;
    e->next_blink_ms = now + blink_interval(e) / 2u;
    e->next_look_ms = now + look_interval(e) / 2u;
    e->last_ms = now - 100000u;
    e->look_cur_x = e->look_cur_y = 0;
    e->look_tgt_x = e->look_tgt_y = 0;
  }

  /* blink */
  if (f->flags & DB_FLAG_BLINK) {
    if (e->blinking & ST_BLINK) {
      uint32_t el = now - e->blink_start_ms;
      if ((int32_t)el < 0 || el >= blink_dur(e)) {
        e->blinking &= (uint8_t)~ST_BLINK;
        if (e->blinking & ST_DOUBLE) {
          e->blinking = (uint8_t)((e->blinking & ~ST_DOUBLE) | ST_SECOND);
          e->next_blink_ms = now + 90u;
        } else {
          e->next_blink_ms = now + blink_interval(e);
        }
      }
    }
    if (!(e->blinking & ST_BLINK)) {
      int32_t d = (int32_t)(e->next_blink_ms - now);
      if (d > 26000L) {
        e->next_blink_ms = now + blink_interval(e); /* clock went backwards */
      } else if (d <= 0) {
        e->blinking |= ST_BLINK;
        e->blink_start_ms = now;
        if (e->blinking & ST_SECOND) e->blinking &= (uint8_t)~ST_SECOND;
        else if ((rng_next(e) & 3u) == 0) e->blinking |= ST_DOUBLE;
      }
    }
  } else {
    e->blinking &= (uint8_t)~(ST_BLINK | ST_DOUBLE | ST_SECOND);
  }

  /* glance */
  if (lookable) {
    int32_t d = (int32_t)(e->next_look_ms - now);
    if (d > 80000L) {
      e->next_look_ms = now + look_interval(e);
    } else if (d <= 0) {
      int16_t cx, cy, dist;
      look_pos(e, now, &cx, &cy);
      e->look_cur_x = cx;
      e->look_cur_y = cy;
      pick_target(e);
      e->last_ms = now;
      e->next_look_ms = now + look_interval(e);
      dist = (int16_t)(iabs((int16_t)(e->look_tgt_x - cx / 256)) + iabs((int16_t)(e->look_tgt_y - cy / 256)));
      /* big glances often come with a blink, like real eyes */
      if ((f->flags & DB_FLAG_BLINK) && !(e->blinking & ST_BLINK) && dist > 10 &&
          (rng_next(e) % 3u) == 0) {
        e->next_blink_ms = now;
      }
    }
  } else {
    e->look_cur_x = e->look_cur_y = 0;
    e->look_tgt_x = e->look_tgt_y = 0;
  }
}

/* 0 = open .. 256 = closed */
static int16_t blink_amount(const db_engine_t *e, uint32_t now)
{
  uint32_t el, d, c, h;
  if (!(e->blinking & ST_BLINK)) return 0;
  el = now - e->blink_start_ms;
  d = blink_dur(e);
  c = d * 2u / 5u;
  h = d / 5u;
  if ((int32_t)el < 0 || el >= d) return 0;
  if (el < c) {
    int32_t x = (int32_t)(el * 256u / c);
    return (int16_t)(x * x / 256);
  }
  if (el < c + h) return 256;
  return (int16_t)(256 - ease((int16_t)((el - c - h) * 256u / (d - c - h))));
}

/* ------------------------------------------------------------------------ */
/* Mouth and overlays                                                        */
/* ------------------------------------------------------------------------ */
/* Curved 2 px stroke: dir +1 smile (U), -1 frown. */
static void mouth_arc(uint8_t *fb, int16_t cx, int16_t cy, int16_t hw, int16_t hh, int8_t dir)
{
  int16_t dx, prev = cy, y;
  for (dx = (int16_t)-hw; dx <= hw; dx++) {
    int16_t dy = (int16_t)((int32_t)hh * circ(hw, dx) / hw);
    y = (int16_t)(cy + dir * dy);
    if (dx == -hw) prev = y;
    vspan(fb, (int16_t)(cx + dx), imin(prev, y), (int16_t)(imax(prev, y) + 1), 1);
    prev = y;
  }
}

static void mouth_wave(uint8_t *fb, int16_t cx, int16_t cy, int16_t hw, uint8_t ph)
{
  int16_t dx, prev = cy, y;
  for (dx = (int16_t)-hw; dx <= hw; dx++) {
    y = (int16_t)(cy + sscale(2, sin64((uint8_t)(dx * 8 + ph))));
    if (dx == -hw) prev = y;
    vspan(fb, (int16_t)(cx + dx), imin(prev, y), (int16_t)(imax(prev, y) + 1), 1);
    prev = y;
  }
}

static void draw_mouth(uint8_t *fb, uint8_t mood, int16_t cx, int16_t cy, uint32_t ac)
{
  int16_t dx;
  switch (mood) {
  case DB_MOOD_HAPPY: mouth_arc(fb, cx, (int16_t)(cy - 2), 8, 4, 1); break;
  case DB_MOOD_ANGRY: mouth_arc(fb, cx, (int16_t)(cy + 2), 6, 2, -1); break;
  case DB_MOOD_SAD: mouth_arc(fb, cx, (int16_t)(cy + 2), 6, 3, -1); break;
  case DB_MOOD_SLEEPY:
  case DB_MOOD_SURPRISED: {
    int16_t r = mood == DB_MOOD_SLEEPY ? (int16_t)(2 + (sin64((uint8_t)(ac >> 9)) > 0)) : 4;
    fill_circle(fb, cx, cy, r, 1);
    fill_circle(fb, cx, cy, (int16_t)(r - 2), 0);
    break;
  }
  case DB_MOOD_LOVE:
    mouth_arc(fb, (int16_t)(cx - 3), (int16_t)(cy - 1), 3, 2, 1);
    mouth_arc(fb, (int16_t)(cx + 3), (int16_t)(cy - 1), 3, 2, 1);
    break;
  case DB_MOOD_SUSPICIOUS: vspan(fb, cx, cy, (int16_t)(cy + 1), 1);
    for (dx = 0; dx <= 8; dx++) vspan(fb, (int16_t)(cx - 2 + dx), cy, (int16_t)(cy + 1), 1);
    break;
  case DB_MOOD_EXCITED:
    for (dx = -7; dx <= 7; dx++) {
      int16_t dy = (int16_t)((int32_t)5 * circ(7, dx) / 7);
      vspan(fb, (int16_t)(cx + dx), (int16_t)(cy - 2), (int16_t)(cy - 2 + imax(dy, 1)), 1);
    }
    break;
  case DB_MOOD_BORED:
    for (dx = -6; dx <= 6; dx++) vspan(fb, (int16_t)(cx + dx), cy, (int16_t)(cy + 1), 1);
    break;
  case DB_MOOD_SCARED: mouth_wave(fb, cx, cy, 8, (uint8_t)(ac >> 5)); break;
  case DB_MOOD_DIZZY: mouth_wave(fb, cx, cy, 7, (uint8_t)(ac >> 6)); break;
  default: mouth_arc(fb, cx, (int16_t)(cy - 1), 6, 3, 1); break;
  }
}

/* Small heart via the eye renderer. */
static void mini_heart(uint8_t *fb, int16_t cx, int16_t cy, int16_t w)
{
  eye_t h;
  memset(&h, 0, sizeof(h));
  h.shape = SH_HEART;
  h.w = (int16_t)(w + 2);
  h.h = (int16_t)(w * 7 / 8 + 2);
  h.x0 = (int16_t)(cx - h.w / 2);
  h.y0 = (int16_t)(cy - h.h / 2);
  h.open = 256;
  draw_eye(fb, &h, 0); /* black halo */
  h.w = w;
  h.h = (int16_t)(w * 7 / 8);
  h.x0 = (int16_t)(cx - w / 2);
  h.y0 = (int16_t)(cy - h.h / 2);
  draw_eye(fb, &h, 1);
}

/* Tear/sweat drop: round bottom of radius r at (cx, cy), tip pointing up. */
static void drop(uint8_t *fb, int16_t cx, int16_t cy, int16_t r, uint8_t on)
{
  int16_t dx;
  for (dx = (int16_t)-r; dx <= r; dx++) {
    int16_t b = circ((int16_t)(r + 1), dx);
    int16_t t = (int16_t)(cy - r * 2 + iabs(dx) * 2);
    vspan(fb, (int16_t)(cx + dx), imin(t, cy), (int16_t)(cy + b - 1), on);
  }
}

static void zee(uint8_t *fb, int16_t x, int16_t y, int16_t s, uint8_t on)
{
  line(fb, x, y, (int16_t)(x + s), y, on);
  line(fb, (int16_t)(x + s), y, x, (int16_t)(y + s), on);
  line(fb, x, (int16_t)(y + s), (int16_t)(x + s), (int16_t)(y + s), on);
}

static void anger(uint8_t *fb, int16_t cx, int16_t cy, int16_t s, uint8_t on)
{
  int8_t sx, sy;
  for (sx = -1; sx <= 1; sx += 2) {
    for (sy = -1; sy <= 1; sy += 2) {
      int16_t gx = (int16_t)(cx + sx * 2), gy = (int16_t)(cy + sy * 2);
      line2(fb, gx, (int16_t)(gy + sy * (s - 2)), gx, gy, on);
      line2(fb, gx, gy, (int16_t)(gx + sx * (s - 2)), gy, on);
    }
  }
}

static void question(uint8_t *fb, int16_t x, int16_t y, uint8_t on)
{
  int16_t c, r;
  for (c = 0; c < 5; c++) {
    for (r = 0; r < 7; r++) {
      if (k_glyph_q[c] & (1u << r)) {
        int16_t px = (int16_t)(x + c * 2), py = (int16_t)(y + r * 2);
        if (on) {
          vspan(fb, px, py, (int16_t)(py + 1), 1);
          vspan(fb, (int16_t)(px + 1), py, (int16_t)(py + 1), 1);
        } else {
          int16_t k;
          for (k = -1; k <= 2; k++) vspan(fb, (int16_t)(px + k), (int16_t)(py - 1), (int16_t)(py + 2), 0);
        }
      }
    }
  }
}

/* Geometry the overlays anchor to. */
typedef struct layout {
  int16_t xl, xr, w, y0, h; /* left eye x, right eye x (== xl for cyclops) */
} layout_t;

static void draw_overlay(uint8_t *fb, uint8_t ov, uint32_t ac, const layout_t *L)
{
  int16_t k;
  switch (ov) {
  case DB_OVERLAY_HEARTS:
    for (k = 0; k < 3; k++) {
      uint32_t p = (ac + (uint32_t)k * 5461u) & 16383u; /* 0..16383 over ~3.3 s */
      int16_t y = (int16_t)(66 - (int16_t)(p * 72u / 16384u));
      int16_t x = (int16_t)((k == 1 ? 9 : (k == 0 ? 118 : 110)) +
                            sscale(3, sin64((uint8_t)(p >> 8))));
      int16_t s = (int16_t)(p < 12000u ? 9 : 9 - (int16_t)((p - 12000u) / 800u));
      if (s >= 3) mini_heart(fb, x, y, s);
    }
    break;
  case DB_OVERLAY_ZZZ:
    for (k = 0; k < 3; k++) {
      uint32_t p = (ac + (uint32_t)k * 5461u) & 16383u;
      int16_t q = (int16_t)(p >> 8); /* 0..63 */
      int16_t s = (int16_t)(3 + q * 5 / 64);
      int16_t x = (int16_t)(100 + q * 20 / 64), y = (int16_t)(22 - q * 22 / 64);
      int16_t dx, dy;
      if (q > 58) continue;
      for (dx = -1; dx <= 1; dx++)
        for (dy = -1; dy <= 1; dy++) zee(fb, (int16_t)(x + dx), (int16_t)(y + dy), s, 0);
      zee(fb, x, y, s, 1);
    }
    break;
  case DB_OVERLAY_SWEAT: {
    uint32_t p = ac & 8191u; /* ~1.6 s */
    int16_t x = (int16_t)(L->xr + L->w + 4), y;
    if (x > 122) x = 122;
    y = (int16_t)(L->y0 + 4 + (int16_t)(p * 14u / 8192u));
    if (p < 7000u) {
      drop(fb, x, y, 4, 0);
      drop(fb, x, y, 3, 1);
      fill_circle(fb, (int16_t)(x - 1), y, 0, 0); /* shine */
    }
    break;
  }
  case DB_OVERLAY_ANGER: {
    int16_t s = (int16_t)(5 + sscale(2, iabs(sin64((uint8_t)(ac >> 6)))));
    int16_t cx = (int16_t)imin((int16_t)(L->xr + L->w + 2), 117), cy = (int16_t)imax((int16_t)(L->y0 - 2), 9);
    int16_t dx;
    for (dx = (int16_t)-(s + 1); dx <= s + 1; dx++)
      vspan(fb, (int16_t)(cx + dx), (int16_t)(cy - s - 1), (int16_t)(cy + s + 1), 0);
    anger(fb, cx, cy, s, 1);
    break;
  }
  case DB_OVERLAY_TEARS:
    for (k = 0; k < 2; k++) {
      uint32_t p = (ac + (uint32_t)k * 4096u) & 8191u;
      int16_t x = k == 0 ? (int16_t)(L->xl + L->w / 4) : (int16_t)(L->xr + L->w * 3 / 4);
      int16_t y0 = (int16_t)(L->y0 + L->h + 2);
      int16_t t = (int16_t)(p >> 5); /* 0..255 */
      int16_t y = (int16_t)(y0 + (int16_t)((int32_t)t * t * (68 - y0) / 65536));
      if (L->xl == L->xr && k == 1) break;
      drop(fb, x, y, 4, 0);
      drop(fb, x, y, 3, 1);
    }
    break;
  case DB_OVERLAY_SPARKLES: {
    int16_t pos[4][2];
    pos[0][0] = (int16_t)(L->xl - 7);        pos[0][1] = (int16_t)(L->y0 + 3);
    pos[1][0] = (int16_t)(L->xr + L->w + 6); pos[1][1] = (int16_t)(L->y0 + 1);
    pos[2][0] = (int16_t)(L->xr + L->w + 4); pos[2][1] = (int16_t)(L->y0 + L->h - 3);
    pos[3][0] = (int16_t)(L->xl - 5);        pos[3][1] = (int16_t)(L->y0 + L->h - 6);
    for (k = 0; k < 4; k++) {
      uint8_t p = (uint8_t)(((ac >> 6) + (uint32_t)k * 23u) & 63u);
      int16_t s = (int16_t)(p < 32 ? 1 + sin64((uint8_t)p) * 5 / 127 : 0);
      int16_t x = imax(4, imin(pos[k][0], 123)), y = imax(4, imin(pos[k][1], 59));
      if (s <= 0) continue;
      /* skip sparkles that would land on an eye */
      if (y + 5 >= L->y0 && y - 5 < L->y0 + L->h &&
          ((x + 5 >= L->xl && x - 5 < L->xl + L->w) || (x + 5 >= L->xr && x - 5 < L->xr + L->w)))
        continue;
      astroid(fb, x, y, (int16_t)(s + 2), 0);
      astroid(fb, x, y, s, 1);
    }
    break;
  }
  case DB_OVERLAY_QUESTION: {
    int16_t y = (int16_t)(3 + sscale(2, sin64((uint8_t)(ac >> 7))));
    int16_t x = imin((int16_t)(L->xr + L->w + 2), 116);
    question(fb, x, y, 0);
    question(fb, x, y, 1);
    break;
  }
  default: break;
  }
}

/* ------------------------------------------------------------------------ */
/* Moods                                                                     */
/* ------------------------------------------------------------------------ */
typedef struct mood_eye {
  int16_t w, h, r, dx, dy;
  int16_t lid_in, lid_out; /* px at the nose side / outer side */
  int16_t lid_bot, arc;
  int16_t pdx, pdy;        /* pupil offset (SHINE_RING) */
  uint8_t shape, shine;
} mood_eye_t;

/* mood_eye_t.shine */
enum { SHINE_NONE = 0, SHINE_DOT, SHINE_TWO, SHINE_STAR, SHINE_RING };

/* side: 0 screen-left eye, 1 screen-right eye, 2 cyclops */
static void mood_eye(const db_engine_t *e, uint32_t now, uint32_t ac, uint8_t side, mood_eye_t *m)
{
  const db_face_t *f = &e->face;
  int16_t W = f->eye_w, H = f->eye_h;
  memset(m, 0, sizeof(*m));
  m->w = W;
  m->h = H;
  m->r = f->eye_r;
  m->shape = SH_RRECT;
  switch (f->mood) {
  case DB_MOOD_HAPPY:
    m->arc = (int16_t)(H * 11 / 16);
    m->dy = (int16_t)(-1 - sscale(2, iabs(sin64((uint8_t)(ac >> 7)))));
    break;
  case DB_MOOD_ANGRY:
    m->lid_in = (int16_t)(H * 9 / 16);
    m->lid_out = (int16_t)(H * 1 / 16);
    m->dy = 2;
    break;
  case DB_MOOD_SAD:
    m->lid_out = (int16_t)(H * 8 / 16);
    m->lid_in = 0;
    m->h = (int16_t)(H * 15 / 16);
    m->dy = 4;
    break;
  case DB_MOOD_SLEEPY: {
    int16_t droop = (int16_t)((int32_t)H * (9 * 127 + 2 * sin64((uint8_t)(ac >> 9))) / (16 * 127));
    m->lid_in = (int16_t)(droop - H / 16);
    m->lid_out = (int16_t)(droop + H / 16);
    m->lid_bot = (int16_t)(H * 2 / 16);
    m->dy = 3;
    break;
  }
  case DB_MOOD_SURPRISED: {
    int16_t s = (int16_t)(imax(W, H) * 9 / 8);
    m->w = m->h = imin(s, 58);
    m->shape = SH_ELLIPSE;
    m->dy = -2;
    m->shine = SHINE_DOT;
    break;
  }
  case DB_MOOD_LOVE: {
    uint8_t p = (uint8_t)((ac >> 6) & 63u);
    int16_t beat = p < 6 ? (int16_t)p : (p < 12 ? (int16_t)(12 - p) : 0);
    if (p >= 14 && p < 18) beat = (int16_t)(beat + 2 - iabs((int16_t)(p - 16)));
    m->shape = SH_HEART;
    m->w = (int16_t)(W + W / 6 + (beat * 2) / 3 * 2);
    m->h = (int16_t)((m->w * 7) / 8);
    m->dy = (int16_t)(-1 - beat / 3);
    m->shine = SHINE_DOT;
    break;
  }
  case DB_MOOD_SUSPICIOUS:
    if (side == 0 || side == 2) {
      m->lid_in = m->lid_out = (int16_t)(H * 7 / 16);
      m->lid_bot = (int16_t)(H * 3 / 16);
    } else {
      m->lid_in = (int16_t)(H * 3 / 16);
      m->lid_out = (int16_t)(H * 1 / 16);
    }
    break;
  case DB_MOOD_EXCITED:
    m->w = imin((int16_t)(W + W / 8), 60);
    m->h = imin((int16_t)(H + H / 8), 58);
    m->dy = (int16_t)(1 - sscale(4, iabs(sin64((uint8_t)(ac >> 6)))));
    m->shine = SHINE_STAR;
    break;
  case DB_MOOD_BORED:
    m->lid_in = m->lid_out = (int16_t)(H * 8 / 16);
    m->lid_bot = (int16_t)(H * 1 / 16);
    m->dy = 3;
    break;
  case DB_MOOD_SCARED: {
    uint8_t j = hash8(now / 45u);
    /* wide white rings with tiny trembling pupils */
    m->w = (int16_t)(W * 7 / 8);
    m->h = H;
    m->shape = SH_ELLIPSE;
    m->dx = (int16_t)((j & 3u) == 3u ? 0 : (int16_t)(j & 3u) - 1);
    m->pdx = (int16_t)(((j >> 2) & 3u) == 3u ? 0 : (int16_t)((j >> 2) & 3u) - 1);
    m->pdy = (int16_t)((int16_t)((j >> 4) & 1u) - 1);
    m->shine = SHINE_RING;
    break;
  }
  case DB_MOOD_DIZZY:
    m->shape = SH_SPIRAL;
    m->dx = sscale(3, cos64((uint8_t)(ac >> 7)));
    m->dy = sscale(3, sin64((uint8_t)(ac >> 7)));
    break;
  default:
    break;
  }
}

static void render_eye(uint8_t *fb, const mood_eye_t *m, int16_t x0, int16_t y0,
                       uint8_t side, int16_t open, uint32_t ac)
{
  eye_t ey;
  if (m->shape == SH_SPIRAL) {
    int16_t rad = (int16_t)(imin(m->w, m->h) / 2 - 1);
    draw_spiral(fb, (int16_t)(x0 + m->w / 2), (int16_t)(y0 + m->h / 2), rad,
                (uint8_t)(ac >> 6), (int8_t)(side == 1 ? -1 : 1));
    return;
  }
  ey.x0 = x0;
  ey.y0 = y0;
  ey.w = m->w;
  ey.h = m->h;
  ey.r = m->r;
  ey.shape = m->shape;
  /* the nose side is the right edge of the left eye and vice versa; a
   * cyclops has its "nose side" in the middle, so angry/sad lids form a V */
  if (side == 2) {
    ey.lid_l = ey.lid_r = m->lid_out;
    ey.lid_m = (int16_t)(m->lid_out + (m->lid_in - m->lid_out) * 2 / 3);
  } else {
    ey.lid_l = side == 0 ? m->lid_out : m->lid_in;
    ey.lid_r = side == 0 ? m->lid_in : m->lid_out;
    ey.lid_m = (int16_t)((ey.lid_l + ey.lid_r) / 2);
  }
  ey.lid_bot = m->lid_bot;
  ey.arc = m->arc;
  ey.open = open;
  draw_eye(fb, &ey, 1);
  if (m->shine == SHINE_RING) {
    int16_t t = imax(3, (int16_t)(m->w / 10));
    ey.x0 = (int16_t)(x0 + t);
    ey.y0 = (int16_t)(y0 + t);
    ey.w = (int16_t)(m->w - 2 * t);
    ey.h = (int16_t)(m->h - 2 * t);
    draw_eye(fb, &ey, 0);
    if (open > 160)
      fill_circle(fb, (int16_t)(x0 + m->w / 2 + m->pdx), (int16_t)(y0 + m->h / 2 + m->pdy),
                  imax(2, (int16_t)(m->w / 9)), 1);
  } else if (m->shine && open > 200) {
    int16_t r = imax(2, (int16_t)(m->w / 9));
    int16_t sx = (int16_t)(x0 + m->w * 3 / 10), sy = (int16_t)(y0 + m->h * 3 / 10);
    if (m->shape == SH_HEART) { sy = (int16_t)(y0 + m->h / 4); r = imax(1, (int16_t)(m->w / 12)); }
    if (m->shine == SHINE_STAR) {
      astroid(fb, (int16_t)(sx + 1), (int16_t)(sy + 1), imax(4, (int16_t)(m->w / 5)), 0);
    } else {
      fill_circle(fb, sx, sy, r, 0);
    }
    if (m->shine >= SHINE_TWO)
      fill_circle(fb, (int16_t)(x0 + m->w * 13 / 20), (int16_t)(y0 + m->h * 3 / 5), imax(1, (int16_t)(r / 2)), 0);
  }
}

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
  e->rng = seed ^ ((uint32_t)e->face.seed * 0x9E3779B1UL) ^ 0xA5F1523DUL;
  if (e->rng == 0) e->rng = 0x6D2B79F5UL;
  for (i = 0; i < 4; i++) (void)rng_next(e);
}

void db_engine_render(db_engine_t *e, uint32_t now_ms, uint8_t fb[DB_FB_SIZE])
{
  const db_face_t *f = &e->face;
  uint32_t ac = now_ms * f->speed; /* animation clock: 5 ticks per ms at speed 5 */
  int16_t lx8, ly8, lx, ly, by = 0, open, cx, cy;
  mood_eye_t ml, mr;
  layout_t L;
  uint8_t cyclops = (uint8_t)((f->flags & DB_FLAG_CYCLOPS) != 0);

  update(e, now_ms);
  memset(fb, 0, DB_FB_SIZE);

  look_pos(e, now_ms, &lx8, &ly8);
  lx = (int16_t)((lx8 + (lx8 >= 0 ? 128 : -128)) / 256);
  ly = (int16_t)((ly8 + (ly8 >= 0 ? 128 : -128)) / 256);
  if (f->flags & DB_FLAG_BREATHE) by = (int16_t)((int32_t)sin64((uint8_t)(ac >> 8)) * 5 / 254);
  open = (int16_t)(256 - blink_amount(e, now_ms));
  if (f->mood == DB_MOOD_DIZZY) open = 256;

  cx = (int16_t)(DB_W / 2 + lx);
  cy = (int16_t)(DB_H / 2 + f->eye_y + ly + by);

  if (cyclops) {
    mood_eye(e, now_ms, ac, 2, &ml);
    L.xl = L.xr = (int16_t)(cx + ml.dx - ml.w / 2);
    L.y0 = (int16_t)(cy + ml.dy - ml.h / 2);
    L.w = ml.w;
    L.h = ml.h;
    render_eye(fb, &ml, L.xl, L.y0, 2, open, ac);
  } else {
    int16_t gap = f->eye_gap, total;
    mood_eye(e, now_ms, ac, 0, &ml);
    mood_eye(e, now_ms, ac, 1, &mr);
    if (ml.shape != SH_SPIRAL) {
      /* parallax: the eye on the side we look towards grows a little */
      int16_t dh = (int16_t)(lx * 3 / 16);
      ml.h = (int16_t)(ml.h - dh);
      mr.h = (int16_t)(mr.h + dh);
      if (ml.shape == SH_ELLIPSE && ml.shine != SHINE_RING) { ml.w = (int16_t)(ml.w - dh); mr.w = (int16_t)(mr.w + dh); }
    }
    total = (int16_t)(ml.w + mr.w + gap);
    L.xl = (int16_t)(cx + ml.dx - total / 2);
    L.xr = (int16_t)(L.xl + ml.w + gap);
    L.y0 = (int16_t)(cy + ml.dy - ml.h / 2);
    L.w = ml.w;
    L.h = ml.h;
    render_eye(fb, &ml, L.xl, L.y0, 0, open, ac);
    render_eye(fb, &mr, L.xr, (int16_t)(cy + mr.dy - mr.h / 2), 1, open, ac);
  }

  if (f->flags & DB_FLAG_MOUTH) {
    int16_t my = (int16_t)(DB_H / 2 + f->eye_y + by + ly / 2 + f->eye_h / 2 + 7 + ml.dy / 2);
    draw_mouth(fb, f->mood, (int16_t)(DB_W / 2 + lx / 2 + ml.dx), imin(my, 60), ac);
  }

  draw_overlay(fb, f->overlay, ac, &L);

  if (f->flags & DB_FLAG_INVERT) fb_invert(fb);
  if (f->flags & DB_FLAG_ROTATE180) fb_rotate180(fb);
}
