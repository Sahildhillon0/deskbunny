/*
 * Host-side simulator for the phone app. Mirrors crates/deskbunny-core:
 * Engine::set_face / Engine::render (engine.rs) and Drawing::compose /
 * invert_rotate (drawing.rs). The drawing blob is decoded with the engine's
 * own db_art_frame, the firmware's decoder.
 */
#include "nixxa_engine.h"

#include <stdlib.h>
#include <string.h>

#include "deskbunny_engine.h"
#include "nx_clock.h"

struct nx_sim {
  db_engine_t eng;
  db_face_t face;
  uint32_t seed;
  uint8_t mode; /* DB_ART_OFF / DB_ART_REPLACE / DB_ART_OVER */
  uint8_t nframes;
  uint16_t frame_ms;
  uint8_t fb[DB_FB_SIZE];
  uint8_t frames[DB_ART_FRAMES_MAX][DB_FB_SIZE];
};

static void restart(nx_sim *s)
{
  db_face_t f = s->face;
  /* With a drawing the engine draws upright; invert/rotate180 are applied
   * after compositing, as the panel does on the board. */
  if (s->mode != DB_ART_OFF) f.flags &= (uint8_t)~(DB_FLAG_INVERT | DB_FLAG_ROTATE180);
  db_engine_init(&s->eng, &f, s->seed);
}

nx_sim *nx_sim_new(uint32_t seed)
{
  nx_sim *s = (nx_sim *)calloc(1, sizeof(nx_sim));
  if (!s) return NULL;
  s->seed = seed;
  db_face_default(&s->face);
  restart(s);
  return s;
}

void nx_sim_free(nx_sim *s) { free(s); }

int32_t nx_sim_set_face(nx_sim *s, const uint8_t *blob)
{
  db_face_t f;
  int rc = db_face_decode(blob, &f);
  if (rc != 0) return rc;
  s->face = f;
  restart(s);
  return 0;
}

/* db_art_read_t has no context argument: the blob being decoded. */
static _Thread_local const uint8_t *t_art;
static uint8_t art_read(uint16_t off) { return t_art[off]; }

static int32_t decode_drawing(nx_sim *s, const uint8_t *b, uint32_t len)
{
  uint16_t pos = DB_ART_HEADER, frames, ms;
  uint8_t i, fb[DB_FB_SIZE];
  if (!b || len == 0) return 0;
  if (len < DB_ART_HEADER || len > 0xFFFFu) return -1;
  if (memcmp(b, DB_ART_MAGIC, 8) != 0 || b[8] != DB_ART_VERSION) return -1;
  if (b[9] == DB_ART_OFF) return 0;
  if (b[9] != DB_ART_REPLACE && b[9] != DB_ART_OVER) return -1;
  frames = (uint16_t)(b[10] | (b[11] << 8));
  if (frames < 1 || frames > DB_ART_FRAMES_MAX) return -1;
  ms = (uint16_t)(b[12] | (b[13] << 8));
  /* Drawing::sanitized: 0 -> 500, else 50..10000. */
  ms = ms == 0 ? 500 : (ms < 50 ? 50 : (ms > 10000 ? 10000 : ms));

  memset(fb, 0, sizeof fb);
  t_art = b;
  for (i = 0; i < frames; i++) {
    if (b[9] == DB_ART_OVER) memset(fb, 0, sizeof fb);
    if (db_art_frame(art_read, (uint16_t)len, &pos, b[9] == DB_ART_OVER ? DB_ART_OR : DB_ART_XOR, fb) != 0) {
      t_art = NULL;
      return -1;
    }
    memcpy(s->frames[i], fb, DB_FB_SIZE);
  }
  t_art = NULL;
  s->mode = b[9];
  s->nframes = (uint8_t)frames;
  s->frame_ms = ms;
  return 1;
}

int32_t nx_sim_set_drawing(nx_sim *s, const uint8_t *blob, uint32_t len)
{
  int32_t rc;
  s->mode = DB_ART_OFF;
  s->nframes = 0;
  rc = decode_drawing(s, blob, len);
  if (rc != 1) s->mode = DB_ART_OFF;
  restart(s);
  return rc;
}

static uint8_t rev8(uint8_t b)
{
  b = (uint8_t)((b & 0xF0u) >> 4 | (b & 0x0Fu) << 4);
  b = (uint8_t)((b & 0xCCu) >> 2 | (b & 0x33u) << 2);
  b = (uint8_t)((b & 0xAAu) >> 1 | (b & 0x55u) << 1);
  return b;
}

const uint8_t *nx_sim_render(nx_sim *s, uint32_t t_ms)
{
  uint16_t i;
  db_engine_render(&s->eng, t_ms, s->fb);
  if (s->mode != DB_ART_OFF) {
    const uint8_t *fr = s->frames[(t_ms / (s->frame_ms ? s->frame_ms : 1u)) % s->nframes];
    if (s->mode == DB_ART_REPLACE) memcpy(s->fb, fr, DB_FB_SIZE);
    else for (i = 0; i < DB_FB_SIZE; i++) s->fb[i] |= fr[i];
    if (s->face.flags & DB_FLAG_ROTATE180) {
      for (i = 0; i < DB_FB_SIZE / 2; i++) {
        uint8_t a = s->fb[i];
        s->fb[i] = rev8(s->fb[DB_FB_SIZE - 1 - i]);
        s->fb[DB_FB_SIZE - 1 - i] = rev8(a);
      }
    }
    if (s->face.flags & DB_FLAG_INVERT) {
      for (i = 0; i < DB_FB_SIZE; i++) s->fb[i] = (uint8_t)~s->fb[i];
    }
  }
  return s->fb;
}

/* The clock module (firmware nx_clock.c) drawn for the phone preview. */
void nx_clock_preview(uint8_t style, uint8_t hour, uint8_t minute, uint8_t second, uint8_t *out_fb)
{
  nx_clock_time_t t;
  t.hour = hour;
  t.minute = minute;
  t.second = second;
  nx_clock_draw(out_fb, style, &t, 1);
}

const char *nx_clock_style_name(uint8_t style) { return nx_clock_name(style); }
