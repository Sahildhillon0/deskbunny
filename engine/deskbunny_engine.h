/*
 * DeskBunny face engine: public API.
 * Shared verbatim by the Arduino firmware and the desktop simulator (via FFI).
 * C99, no malloc, no stdio. Contract: docs/ARCHITECTURE.md.
 */
#ifndef DESKBUNNY_ENGINE_H
#define DESKBUNNY_ENGINE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DB_W 128
#define DB_H 64
#define DB_FB_SIZE 1024 /* SSD1306 page layout: fb[(y>>3)*128 + x], bit (y&7) */
#define DB_BLOB_SIZE 64
#define DB_MAGIC "DBFACE01"
#define DB_BLOB_VERSION 1

enum db_mood {
  DB_MOOD_NEUTRAL = 0,
  DB_MOOD_HAPPY,
  DB_MOOD_ANGRY,
  DB_MOOD_SAD,
  DB_MOOD_SLEEPY,
  DB_MOOD_SURPRISED,
  DB_MOOD_LOVE,
  DB_MOOD_SUSPICIOUS,
  DB_MOOD_EXCITED,
  DB_MOOD_BORED,
  DB_MOOD_SCARED,
  DB_MOOD_DIZZY,
  DB_MOOD_COUNT
};

enum db_overlay {
  DB_OVERLAY_NONE = 0,
  DB_OVERLAY_HEARTS,
  DB_OVERLAY_ZZZ,
  DB_OVERLAY_SWEAT,
  DB_OVERLAY_ANGER,
  DB_OVERLAY_TEARS,
  DB_OVERLAY_SPARKLES,
  DB_OVERLAY_QUESTION,
  DB_OVERLAY_COUNT
};

#define DB_FLAG_BLINK      0x01
#define DB_FLAG_LOOK       0x02
#define DB_FLAG_BREATHE    0x04
#define DB_FLAG_CYCLOPS    0x08
#define DB_FLAG_INVERT     0x10
#define DB_FLAG_ROTATE180  0x20
#define DB_FLAG_MOUTH      0x40

/* Decoded face (field meanings and ranges: docs/ARCHITECTURE.md). */
typedef struct db_face {
  uint8_t mood;
  char name[13]; /* NUL-terminated */
  uint8_t eye_w, eye_h, eye_r, eye_gap;
  int8_t eye_y;
  uint8_t flags;
  uint8_t blink_min, blink_max; /* x100 ms */
  uint8_t look_min, look_max;   /* x100 ms */
  uint8_t look_x, look_y;
  uint8_t speed;
  uint8_t contrast;
  uint8_t overlay;
  uint8_t seed;
} db_face_t;

/* Opaque-ish runtime state. Callers allocate it; only the engine touches fields.
 * Must stay <= 96 bytes on AVR. */
typedef struct db_engine {
  db_face_t face;
  uint32_t rng;
  uint32_t last_ms;
  uint32_t next_blink_ms;
  uint32_t blink_start_ms;
  uint32_t next_look_ms;
  int16_t look_cur_x, look_cur_y; /* fixed point 8.8 */
  int8_t look_tgt_x, look_tgt_y;
  uint8_t blinking;
  uint8_t pad[3];
} db_engine_t;

uint16_t db_crc16(const uint8_t *data, size_t len);

/* Fill with the built-in default (neutral, blink + look + breathe). */
void db_face_default(db_face_t *out);

/* Decode a 64-byte blob. Returns 0 on success; negative on bad magic (-1),
 * version (-2) or CRC (-3). On failure *out is set to db_face_default(). */
int db_face_decode(const uint8_t blob[DB_BLOB_SIZE], db_face_t *out);

/* Encode a face into a 64-byte blob, including magic and CRC. */
void db_face_encode(const db_face_t *face, uint8_t blob[DB_BLOB_SIZE]);

void db_engine_init(db_engine_t *e, const db_face_t *face, uint32_t seed);

/* Render the frame for absolute time now_ms (monotonic, starts near 0).
 * Clears and fully redraws fb. Deterministic for the same init + time sequence.
 * DB_FLAG_INVERT and DB_FLAG_ROTATE180 are applied to fb here. */
void db_engine_render(db_engine_t *e, uint32_t now_ms, uint8_t fb[DB_FB_SIZE]);

#ifdef __cplusplus
}
#endif

#endif /* DESKBUNNY_ENGINE_H */
