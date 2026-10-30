/*
 * Host tests for the DeskBunny engine. Build and run with `make test`
 * (gcc and clang, ASan + UBSan). Exit code 0 means every check passed.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../deskbunny_engine.h"
#include "../../firmware/deskbunny/db_default_blob.h"

static int g_checks, g_fails;

#define CHECK(cond)                                                        \
  do {                                                                     \
    g_checks++;                                                            \
    if (!(cond)) {                                                         \
      g_fails++;                                                           \
      fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #cond); \
    }                                                                      \
  } while (0)

static uint32_t g_rng = 12345u;
static uint32_t rnd(void)
{
  g_rng ^= g_rng << 13;
  g_rng ^= g_rng >> 17;
  g_rng ^= g_rng << 5;
  return g_rng;
}

static int face_eq(const db_face_t *a, const db_face_t *b)
{
  return a->mood == b->mood && strcmp(a->name, b->name) == 0 && a->eye_w == b->eye_w &&
         a->eye_h == b->eye_h && a->eye_r == b->eye_r && a->eye_gap == b->eye_gap &&
         a->eye_y == b->eye_y && a->flags == b->flags && a->blink_min == b->blink_min &&
         a->blink_max == b->blink_max && a->look_min == b->look_min &&
         a->look_max == b->look_max && a->look_x == b->look_x && a->look_y == b->look_y &&
         a->speed == b->speed && a->contrast == b->contrast && a->overlay == b->overlay &&
         a->seed == b->seed && a->cycle == b->cycle && a->cycle_secs == b->cycle_secs &&
         a->cycle_count == b->cycle_count && a->cycle_flags == b->cycle_flags &&
         memcmp(a->cycle_moods, b->cycle_moods, DB_CYCLE_MAX) == 0;
}

/* A random face inside the documented ranges. */
static void random_valid_face(db_face_t *f)
{
  static const char chars[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
  int i, n;
  memset(f, 0, sizeof(*f));
  f->mood = (uint8_t)(rnd() % DB_MOOD_COUNT);
  n = (int)(rnd() % 13);
  for (i = 0; i < n; i++) f->name[i] = chars[rnd() % (sizeof(chars) - 1)];
  f->eye_w = (uint8_t)(8 + rnd() % 49);
  f->eye_h = (uint8_t)(8 + rnd() % 49);
  f->eye_r = (uint8_t)(rnd() % (f->eye_w / 2u + 1u));
  f->eye_gap = (uint8_t)(rnd() % 41);
  f->eye_y = (int8_t)((int)(rnd() % 41) - 20);
  f->flags = (uint8_t)(rnd() & 0x7F);
  f->blink_min = (uint8_t)(1 + rnd() % 40);
  f->blink_max = (uint8_t)(f->blink_min + rnd() % 60);
  f->look_min = (uint8_t)(1 + rnd() % 40);
  f->look_max = (uint8_t)(f->look_min + rnd() % 60);
  f->look_x = (uint8_t)(rnd() % 40);
  f->look_y = (uint8_t)(rnd() % 20);
  f->speed = (uint8_t)(1 + rnd() % 10);
  f->contrast = (uint8_t)rnd();
  f->overlay = (uint8_t)(rnd() % DB_OVERLAY_COUNT);
  f->seed = (uint8_t)rnd();
  f->cycle = (uint8_t)(rnd() % DB_CYCLE_COUNT);
  f->cycle_secs = (uint8_t)(1 + rnd() % 255);
  f->cycle_count = (uint8_t)(rnd() % (DB_CYCLE_MAX + 1));
  f->cycle_flags = (uint8_t)(rnd() & DB_CYCLE_FLAG_OVERLAY);
  for (i = 0; i < f->cycle_count; i++) f->cycle_moods[i] = (uint8_t)(rnd() % DB_MOOD_COUNT);
}

static void test_crc(void)
{
  const uint8_t v[] = { '1', '2', '3', '4', '5', '6', '7', '8', '9' };
  CHECK(db_crc16(v, sizeof(v)) == 0x29B1);
  CHECK(db_crc16(v, 0) == 0xFFFF);
}

static void test_codec(void)
{
  db_face_t a, b, def;
  uint8_t blob[DB_BLOB_SIZE], bad[DB_BLOB_SIZE];
  int i;
  db_face_default(&def);

  db_face_encode(&def, blob);
  CHECK(memcmp(blob, DB_MAGIC, 8) == 0);
  CHECK(blob[8] == DB_BLOB_VERSION);
  CHECK(db_crc16(blob, 62) == (uint16_t)(blob[62] | (blob[63] << 8)));
  CHECK(blob[38] == DB_CYCLE_OFF && blob[39] == 10 && blob[40] == 0 && blob[41] == DB_CYCLE_FLAG_OVERLAY);
  for (i = 42; i < 62; i++) CHECK(blob[i] == 0);
  CHECK(db_face_decode(blob, &b) == 0);
  CHECK(face_eq(&def, &b));

  /* a blob from before the cycle fields (zeros there) decodes with the cycle off */
  memcpy(bad, blob, sizeof(bad));
  memset(bad + 38, 0, 24);
  bad[62] = (uint8_t)(db_crc16(bad, 62) & 0xFF);
  bad[63] = (uint8_t)(db_crc16(bad, 62) >> 8);
  CHECK(db_face_decode(bad, &b) == 0);
  CHECK(b.cycle == DB_CYCLE_OFF && b.cycle_secs == 10 && b.cycle_count == 0);

  for (i = 0; i < 2000; i++) {
    random_valid_face(&a);
    db_face_encode(&a, blob);
    memset(&b, 0xAA, sizeof(b));
    CHECK(db_face_decode(blob, &b) == 0);
    CHECK(face_eq(&a, &b));
  }

  /* bad magic */
  random_valid_face(&a);
  db_face_encode(&a, blob);
  memcpy(bad, blob, sizeof(bad));
  bad[0] = 'X';
  memset(&b, 0xAA, sizeof(b));
  CHECK(db_face_decode(bad, &b) == -1);
  CHECK(face_eq(&b, &def));
  /* bad version (CRC recomputed so only the version is wrong) */
  memcpy(bad, blob, sizeof(bad));
  bad[8] = 2;
  bad[62] = (uint8_t)(db_crc16(bad, 62) & 0xFF);
  bad[63] = (uint8_t)(db_crc16(bad, 62) >> 8);
  memset(&b, 0xAA, sizeof(b));
  CHECK(db_face_decode(bad, &b) == -2);
  CHECK(face_eq(&b, &def));
  /* bad CRC: any single bit flip in the payload */
  for (i = 9 * 8; i < 64 * 8; i += 7) {
    memcpy(bad, blob, sizeof(bad));
    bad[i / 8] ^= (uint8_t)(1u << (i % 8));
    memset(&b, 0xAA, sizeof(b));
    CHECK(db_face_decode(bad, &b) == -3);
    CHECK(face_eq(&b, &def));
  }

  /* out-of-range fields with a valid CRC are clamped, not trusted */
  memcpy(bad, blob, sizeof(bad));
  bad[9] = 200;  /* mood */
  bad[22] = 255; /* eye_w */
  bad[23] = 0;   /* eye_h */
  bad[24] = 200; /* eye_r */
  bad[25] = 99;  /* gap */
  bad[34] = 0;   /* speed */
  bad[36] = 99;  /* overlay */
  bad[38] = 9;   /* cycle */
  bad[40] = 2;   /* cycle_count */
  bad[41] = 0xFF; /* cycle_flags */
  bad[42] = 3;
  bad[43] = 77;  /* unknown mood in the list */
  bad[44] = 5;   /* past cycle_count */
  bad[10] = 'a';
  bad[11] = '!';
  bad[62] = (uint8_t)(db_crc16(bad, 62) & 0xFF);
  bad[63] = (uint8_t)(db_crc16(bad, 62) >> 8);
  CHECK(db_face_decode(bad, &b) == 0);
  CHECK(b.mood == DB_MOOD_HOVER && b.overlay == DB_OVERLAY_NONE);
  CHECK(b.eye_w == 56 && b.eye_h == 8 && b.eye_r == 28 && b.eye_gap == 40 && b.speed == 1);
  CHECK(b.name[0] == 'A' && b.name[1] == ' ');
  CHECK(b.cycle == DB_CYCLE_OFF && b.cycle_flags == DB_CYCLE_FLAG_OVERLAY);
  CHECK(b.cycle_moods[0] == 3 && b.cycle_moods[1] == 0 && b.cycle_moods[2] == 0);

  /* name: 12 chars max, always NUL-terminated */
  memcpy(bad, blob, sizeof(bad));
  memset(bad + 10, 'Z', 12);
  bad[62] = (uint8_t)(db_crc16(bad, 62) & 0xFF);
  bad[63] = (uint8_t)(db_crc16(bad, 62) >> 8);
  CHECK(db_face_decode(bad, &b) == 0);
  CHECK(strlen(b.name) == 12);
}

static void test_firmware_blob(void)
{
  static const uint8_t fw[DB_BLOB_SIZE] = DB_DEFAULT_BLOB_INIT;
  db_face_t def, b;
  uint8_t blob[DB_BLOB_SIZE];
  db_face_default(&def);
  db_face_encode(&def, blob);
  CHECK(memcmp(fw, blob, DB_BLOB_SIZE) == 0);
  CHECK(db_face_decode(fw, &b) == 0);
  CHECK(face_eq(&b, &def));
}

static void test_determinism(void)
{
  uint8_t fa[DB_FB_SIZE], fb2[DB_FB_SIZE];
  int m, ov, differs = 0;
  for (m = 0; m < DB_MOOD_COUNT; m++) {
    for (ov = 0; ov < DB_OVERLAY_COUNT; ov += 3) {
      db_face_t f;
      db_engine_t a, b, c;
      uint32_t t;
      db_face_default(&f);
      f.mood = (uint8_t)m;
      f.overlay = (uint8_t)(ov ? ov : m % DB_OVERLAY_COUNT);
      f.flags = (uint8_t)(DB_FLAG_BLINK | DB_FLAG_LOOK | DB_FLAG_BREATHE | DB_FLAG_MOUTH);
      db_engine_init(&a, &f, 42);
      db_engine_init(&b, &f, 42);
      db_engine_init(&c, &f, 43);
      for (t = 0; t <= 10000; t += 33) {
        uint8_t fc[DB_FB_SIZE];
        db_engine_render(&a, t, fa);
        db_engine_render(&b, t, fb2);
        db_engine_render(&c, t, fc);
        if (memcmp(fa, fb2, DB_FB_SIZE) != 0) {
          CHECK(0 && "same seed, different frame");
          break;
        }
        if (memcmp(fa, fc, DB_FB_SIZE) != 0) differs = 1;
      }
    }
  }
  CHECK(differs); /* a different seed changes the behaviour somewhere */
}

static int popcount_fb(const uint8_t *fb)
{
  int i, n = 0;
  for (i = 0; i < DB_FB_SIZE; i++) {
    uint8_t v = fb[i];
    while (v) { n += v & 1; v >>= 1; }
  }
  return n;
}

static int px(const uint8_t *fb, int x, int y) { return (fb[(y >> 3) * DB_W + x] >> (y & 7)) & 1; }

static void test_display_flags(void)
{
  db_face_t f;
  db_engine_t a, b;
  uint8_t plain[DB_FB_SIZE], other[DB_FB_SIZE];
  int i, x, y, ok;
  db_face_default(&f);
  f.flags |= DB_FLAG_MOUTH;
  f.overlay = DB_OVERLAY_GRID;
  db_engine_init(&a, &f, 9);
  f.flags |= DB_FLAG_INVERT;
  db_engine_init(&b, &f, 9);
  db_engine_render(&a, 1234, plain);
  db_engine_render(&b, 1234, other);
  ok = 1;
  for (i = 0; i < DB_FB_SIZE; i++) ok &= (other[i] == (uint8_t)~plain[i]);
  CHECK(ok);

  f.flags = (uint8_t)((f.flags & ~DB_FLAG_INVERT) | DB_FLAG_ROTATE180);
  db_engine_init(&b, &f, 9);
  db_engine_render(&b, 1234, other);
  ok = 1;
  for (y = 0; y < DB_H; y++)
    for (x = 0; x < DB_W; x++) ok &= (px(plain, x, y) == px(other, DB_W - 1 - x, DB_H - 1 - y));
  CHECK(ok);
  CHECK(popcount_fb(plain) > 500); /* the scene is actually drawn */
}

/* Every scene draws something and the scenes differ from each other. */
static void test_moods_distinct(void)
{
  static uint8_t frames[DB_MOOD_COUNT][DB_FB_SIZE];
  int m, k;
  for (m = 0; m < DB_MOOD_COUNT; m++) {
    db_face_t f;
    db_engine_t e;
    db_face_default(&f);
    f.mood = (uint8_t)m;
    f.flags = 0; /* static pose */
    db_engine_init(&e, &f, 1);
    db_engine_render(&e, 5500, frames[m]); /* flip mid-roll, race on screen */
    CHECK(popcount_fb(frames[m]) > 100);
  }
  for (m = 0; m < DB_MOOD_COUNT; m++)
    for (k = m + 1; k < DB_MOOD_COUNT; k++) CHECK(memcmp(frames[m], frames[k], DB_FB_SIZE) != 0);
}

/* The strobe lights the nav lights twice every blink_min, and only then. */
static void test_strobe(void)
{
  db_face_t f;
  db_engine_t a, b;
  uint8_t fa[DB_FB_SIZE], fb[DB_FB_SIZE];
  db_face_default(&f);
  f.flags = 0;
  f.blink_min = 20;
  db_engine_init(&b, &f, 3);
  f.flags = DB_FLAG_BLINK;
  db_engine_init(&a, &f, 3);
  db_engine_render(&a, 2030, fa); /* first flash of the period at 2000 */
  db_engine_render(&b, 2030, fb);
  CHECK(popcount_fb(fa) > popcount_fb(fb));
  db_engine_render(&a, 2190, fa); /* second flash */
  db_engine_render(&b, 2190, fb);
  CHECK(popcount_fb(fa) > popcount_fb(fb));
  db_engine_render(&a, 2500, fa); /* dark */
  db_engine_render(&b, 2500, fb);
  CHECK(memcmp(fa, fb, DB_FB_SIZE) == 0);
}

/* The scene cycle: scenes change on schedule, in order or at random, the
 * drones climb out of view at each change and the sky follows the scene. */
static void test_cycle(void)
{
  db_face_t f;
  db_engine_t e, r;
  uint8_t fb[DB_FB_SIZE], open_fb[DB_FB_SIZE];
  uint32_t t;
  int seen[DB_MOOD_COUNT], k, changes = 0, repeats = 0;
  uint8_t last;
  db_face_default(&f);
  f.flags = 0; /* no strobe, no drift, no shadow: only the drones are drawn */
  f.cycle = DB_CYCLE_SEQUENCE;
  f.cycle_secs = 2;
  f.cycle_count = 3;
  f.cycle_moods[0] = DB_MOOD_FLIP;
  f.cycle_moods[1] = DB_MOOD_SWARM;
  f.cycle_moods[2] = DB_MOOD_PATROL;
  f.cycle_flags = 0;
  db_engine_init(&e, &f, 5);
  CHECK(e.face.mood == DB_MOOD_FLIP);
  db_engine_render(&e, 0, fb);
  db_engine_render(&e, 1000, open_fb);
  CHECK(e.face.mood == DB_MOOD_FLIP);
  db_engine_render(&e, 1990, fb); /* climbing out for the change at 2000 */
  CHECK(popcount_fb(fb) * 4 < popcount_fb(open_fb));
  db_engine_render(&e, 2000, fb);
  CHECK(e.face.mood == DB_MOOD_SWARM);
  db_engine_render(&e, 3000, fb);
  CHECK(popcount_fb(fb) > 150); /* back in view */
  db_engine_render(&e, 4000, fb);
  CHECK(e.face.mood == DB_MOOD_PATROL);
  db_engine_render(&e, 6000, fb);
  CHECK(e.face.mood == DB_MOOD_FLIP); /* wraps */
  db_engine_render(&e, 13000, fb);  /* far behind: one step, then back on schedule */
  CHECK(e.face.mood == DB_MOOD_SWARM);
  CHECK(e.cycle_at == 15000u);

  /* with the flag, the sky follows the scene */
  f.cycle_flags = DB_CYCLE_FLAG_OVERLAY;
  db_engine_init(&e, &f, 5);
  CHECK(e.face.overlay == DB_OVERLAY_NONE);
  db_engine_render(&e, 0, fb);
  db_engine_render(&e, 2000, fb);
  CHECK(e.face.overlay == DB_OVERLAY_STARS);
  db_engine_render(&e, 4000, fb);
  CHECK(e.face.overlay == DB_OVERLAY_CITY);

  /* and stays put without it */
  f.cycle_flags = 0;
  f.overlay = DB_OVERLAY_RAIN;
  db_engine_init(&e, &f, 5);
  db_engine_render(&e, 0, fb);
  db_engine_render(&e, 2000, fb);
  CHECK(e.face.overlay == DB_OVERLAY_RAIN);

  /* random over all scenes: covers them, never repeats back to back */
  db_face_default(&f);
  f.cycle = DB_CYCLE_RANDOM;
  f.cycle_secs = 1;
  db_engine_init(&r, &f, 77);
  memset(seen, 0, sizeof(seen));
  last = r.face.mood;
  for (t = 0; t < 400000u; t += 250) {
    db_engine_render(&r, t, fb);
    if (r.face.mood != last) changes++;
    else if (t % 1000u == 0 && t) repeats++;
    last = r.face.mood;
    seen[last] = 1;
  }
  for (k = 0; k < DB_MOOD_COUNT; k++) CHECK(seen[k]);
  CHECK(changes > 390 && repeats == 0);

  /* a one-scene list never gets stuck looking for a different scene */
  f.cycle_count = 1;
  f.cycle_moods[0] = DB_MOOD_CHARGE;
  db_engine_init(&r, &f, 1);
  for (t = 0; t < 5000u; t += 100) db_engine_render(&r, t, fb);
  CHECK(r.face.mood == DB_MOOD_CHARGE);
}

/* Render through an exact 1024-byte heap buffer (ASan flags any overrun)
 * plus guard bytes around a stack copy, with hostile faces and clocks. */
static void test_bounds(void)
{
  int iter;
  uint8_t *fb = (uint8_t *)malloc(DB_FB_SIZE);
  uint8_t guarded[DB_FB_SIZE + 64];
  CHECK(fb != NULL);
  if (!fb) return;
  memset(guarded, 0x5A, sizeof(guarded));
  for (iter = 0; iter < 400; iter++) {
    db_face_t f;
    db_engine_t e;
    uint32_t t, base;
    int k;
    if (iter < 200) {
      /* raw random struct, never sanitised by decode */
      uint8_t *p = (uint8_t *)&f;
      size_t i;
      for (i = 0; i < sizeof(f); i++) p[i] = (uint8_t)rnd();
    } else {
      random_valid_face(&f);
      if (iter & 1) { f.eye_w = 56; f.eye_h = 56; f.eye_gap = 40; f.look_x = 255; f.look_y = 255; }
      f.eye_y = (int8_t)((iter & 2) ? 127 : -128);
      f.flags = (uint8_t)(0x7F);
    }
    db_engine_init(&e, &f, (uint32_t)iter);
    base = (iter % 3 == 0) ? 0xFFFFF000u : (uint32_t)rnd(); /* covers millis() wrap */
    for (k = 0, t = base; k < 300; k++, t += 7 + (rnd() % 60)) {
      if (k == 150) t = base; /* clock goes backwards */
      db_engine_render(&e, t, fb);
      db_engine_render(&e, t, guarded + 32);
    }
  }
  for (iter = 0; iter < 32; iter++) {
    CHECK(guarded[iter] == 0x5A);
    CHECK(guarded[32 + DB_FB_SIZE + iter] == 0x5A);
  }
  free(fb);
}

int main(void)
{
  test_crc();
  test_codec();
  test_firmware_blob();
  test_determinism();
  test_display_flags();
  test_moods_distinct();
  test_strobe();
  test_cycle();
  test_bounds();
  printf("sizeof(db_engine_t) = %u (host), sizeof(db_face_t) = %u\n",
         (unsigned)sizeof(db_engine_t), (unsigned)sizeof(db_face_t));
  CHECK(sizeof(db_engine_t) <= 96);
  printf("%d checks, %d failures\n", g_checks, g_fails);
  return g_fails ? 1 : 0;
}
